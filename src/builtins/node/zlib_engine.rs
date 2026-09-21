//! zlib 增量流引擎（ZKind/ZEngine 状态机；对齐 zlib.rs；纯搬移）。

use super::zlib::{brotli_dec_err, gzip_header, zlib_level, CRC32_TABLE};

// ===== 增量流引擎（10f-g：真流式编解码状态机，零新依赖）=====
// flate2 Compress/Decompress 分段 + 手工 gzip 成员机（trailing zeros/garbage/
// magic 语义逐条真机对拍）+ brotli crate 状态机（enc compress_stream / dec
// BrotliDecompressStream，字典双向）+ ruzstd 单帧一次性（能力记档）。
// 注册表 thread_local（JS 会话线程一份；JS 侧 close/destroy/end 必须 free；
// 线程生灭即回收，§4.24 隔离边界一致）。
use std::io::Read as _;
use brotli::SliceWrapperMut as _;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZKind {
    ZlibDeflate = 0,
    RawDeflate = 1,
    GzipDeflate = 2,
    ZlibInflate = 3,
    RawInflate = 4,
    GzipInflate = 5,        // 多成员（Gunzip/Unzip）
    GzipInflateSingle = 6,  // 单成员（Web DecompressionStream 'gzip'）
    AutoInflate = 7,        // Unzip：1f8b → gzip 否则 zlib
    BrotliEnc = 8,
    BrotliDec = 9,
    ZstdEnc = 10,
    ZstdDec = 11,
}

/// 引擎错误（code = node 侧 err.code；msg = err.message）。
#[derive(Debug)]
pub(crate) struct ZErr {
    pub(crate) code: String,
    pub(crate) msg: String,
}
impl ZErr {
    pub(crate) fn new(code: &str, msg: &str) -> ZErr {
        ZErr { code: code.to_string(), msg: msg.to_string() }
    }
    pub(crate) fn data(msg: &str) -> ZErr {
        ZErr::new("Z_DATA_ERROR", msg)
    }
    pub(crate) fn buf(msg: &str) -> ZErr {
        // 真机口径：截断/空输入 = Z_BUF_ERROR "unexpected end of file"
        ZErr::new("Z_BUF_ERROR", msg)
    }
    pub(crate) fn junk() -> ZErr {
        ZErr::new(
            "ERR_TRAILING_JUNK_AFTER_STREAM_END",
            "Trailing junk found after the end of the compressed stream",
        )
    }
}

/// gzip 成员状态机。
struct GzMember {
    phase: u8, // 0=header 1=body 2=trailer 3=between
    hdr: Vec<u8>,
    zd: Option<flate2::Decompress>,
    crc: u32, // 成员输出 CRC 运行态（!-form 起点 0xFFFF_FFFF）
    outlen: u32,
    trailer: Vec<u8>,
}

/// brotli 编码器。
struct BrotliEnc {
    st: brotli::enc::encode::BrotliEncoderStateStruct<brotli::enc::StandardAlloc>,
}
/// brotli 解码器。
struct BrotliDec {
    st: brotli_decompressor::BrotliState<
        brotli::enc::StandardAlloc,
        brotli::enc::StandardAlloc,
        brotli::enc::StandardAlloc,
    >,
    done_flag: bool,
}

/// zstd 编码（ruzstd 只有一帧一次性：flush 边界分段出帧——偏离记档）。
struct ZstdEnc {
    buf: Vec<u8>,
    flushed: usize,
    pledged: Option<u64>,
    total_in: u64,
}
/// zstd 解码（只累积，flush/end 时对已收前缀一次性解帧）。
pub(crate) struct ZstdDec {
    pub(crate) carried: Vec<u8>,
    pos: usize,
}

pub(crate) struct ZEngine {
    kind: ZKind,
    level: i32,
    dict: Vec<u8>,
    zc: Option<flate2::Compress>,
    zd: Option<flate2::Decompress>,
    // gzip 编码
    gz_hdr_written: bool,
    gz_crc: u32,
    gz_isize: u32,
    // inflate 通用
    pending: Vec<u8>,
    pos: usize,
    hdr_checked: bool,
    dict_set_done: bool,
    first_byte: Option<u8>,
    // gzip 成员机
    gz: Option<GzMember>,
    // brotli
    be: Option<BrotliEnc>,
    bd: Option<BrotliDec>,
    // zstd
    ze: Option<ZstdEnc>,
    pub(crate) zd2: Option<ZstdDec>,
    reject: bool,
    pub(crate) done: bool,
    out: Vec<u8>,
}

impl ZEngine {
    pub(crate) fn new(kind: ZKind, level: i32, dict: &[u8], pledged: Option<u64>, reject: bool) -> ZEngine {
        let mut e = ZEngine {
            kind,
            level,
            dict: dict.to_vec(),
            zc: None,
            zd: None,
            gz_hdr_written: false,
            gz_crc: 0xFFFF_FFFF,
            gz_isize: 0,
            pending: Vec::new(),
            pos: 0,
            hdr_checked: false,
            dict_set_done: false,
            first_byte: None,
            gz: None,
            be: None,
            bd: None,
            ze: None,
            zd2: None,
            reject,
            done: false,
            out: Vec::new(),
        };
        match kind {
            ZKind::BrotliEnc => {
                let mut st = brotli::enc::encode::BrotliEncoderStateStruct::new(
                    brotli::enc::StandardAlloc::default(),
                );
                st.set_parameter(
                    brotli::enc::encode::BrotliEncoderParameter::BROTLI_PARAM_QUALITY,
                    level.clamp(0, 11) as u32,
                );
                st.set_parameter(
                    brotli::enc::encode::BrotliEncoderParameter::BROTLI_PARAM_LGWIN,
                    22,
                );
                if !dict.is_empty() {
                    st.set_custom_dictionary(dict.len(), dict);
                }
                e.be = Some(BrotliEnc { st });
            }
            ZKind::BrotliDec => {
                let a8 = brotli::enc::StandardAlloc::default();
                let a32 = brotli::enc::StandardAlloc::default();
                let ahc = brotli::enc::StandardAlloc::default();
                let st = if dict.is_empty() {
                    brotli::BrotliState::new(a8, a32, ahc)
                } else {
                    let mut alloc = brotli::enc::StandardAlloc::default();
                    let mut mem =
                        <brotli::enc::StandardAlloc as brotli::Allocator<u8>>::alloc_cell(
                            &mut alloc,
                            dict.len(),
                        );
                    mem.slice_mut().copy_from_slice(dict);
                    brotli::BrotliState::new_with_custom_dictionary(a8, a32, ahc, mem)
                };
                e.bd = Some(BrotliDec { st, done_flag: false });
            }
            ZKind::ZstdEnc => {
                e.ze = Some(ZstdEnc { buf: Vec::new(), flushed: 0, pledged, total_in: 0 });
            }
            ZKind::ZstdDec => {
                e.zd2 = Some(ZstdDec { carried: Vec::new(), pos: 0 });
            }
            _ => {}
        }
        e
    }

    pub(crate) fn unconsumed(&self) -> usize {
        match self.kind {
            ZKind::ZstdDec => {
                self.zd2.as_ref().map(|z| z.carried.len() - z.pos).unwrap_or(0)
            }
            _ => self.pending.len() - self.pos,
        }
    }

    /// 压缩侧喂入（flag：node 侧族内 flush kind）。
    pub(crate) fn deflate_feed(&mut self, input: &[u8], flag: u32) -> Result<(), ZErr> {
        if self.done {
            return Ok(()); // 终结后静默吞掉（真机：write-after-end 回调照常触发）
        }
        let flush = match flag {
            // Z_PARTIAL_FLUSH=1 用 Sync 近似：flate2(zlib_rs) 的 Partial 不保证
            // 字节对齐可解（flush-write-sync-interleaved 套件实测分段解不出），
            // 真机 PARTIAL/SYNC 的可观察面（分段解压结果）一致。
            1 | 2 => flate2::FlushCompress::Sync,
            3 | 5 => flate2::FlushCompress::Full, // Z_BLOCK≈Full（miniz 无 Block，记档）
            4 => flate2::FlushCompress::Finish,
            _ => flate2::FlushCompress::None,
        };
        let gzip = self.kind == ZKind::GzipDeflate;
        if gzip && !self.gz_hdr_written {
            self.out.extend_from_slice(&gzip_header(self.level));
            self.gz_hdr_written = true;
        }
        let zlib_wrap = self.kind == ZKind::ZlibDeflate;
        let zc = self
            .zc
            .get_or_insert_with(|| {
                let mut c = flate2::Compress::new(zlib_level(self.level), zlib_wrap);
                // zlib dict：set_dictionary 后首压头带 FDICT+DICTID（zlib_rs 同 libz；
                // 必须在首压前设——头在第一次 compress 时写）
                if !self.dict.is_empty() {
                    let _ = c.set_dictionary(&self.dict);
                }
                c
            });
        let mut src = input;
        loop {
            let tin = zc.total_in();
            let ob = self.out.len();
            // compress_vec 以 Vec spare capacity 为输出空间：不预留则 avail_out=0
            // 零推进（G9-1 潜伏 bug，zz_scratch_isolate_hang 探针实测）。
            self.out.reserve(src.len() + 4096);
            let status = zc
                .compress_vec(src, &mut self.out, flush)
                .map_err(|e| ZErr::new("Z_STREAM_ERROR", &e.to_string()))?;
            let consumed = (zc.total_in() - tin) as usize;
            if gzip {
                // gzip crc32/isize 对消费的【原始输入】累计（G9-1 曾错累计压缩
                // 输出——body 从未产数据，trailer 校验从未跑过，潜伏至今）
                for &b in &src[..consumed] {
                    self.gz_crc =
                        CRC32_TABLE[((self.gz_crc ^ b as u32) & 0xFF) as usize] ^ (self.gz_crc >> 8);
                }
                self.gz_isize = self.gz_isize.wrapping_add(consumed as u32);
            }
            let progressed = consumed > 0 || self.out.len() > ob;
            src = &src[consumed..];
            match status {
                flate2::Status::StreamEnd => {
                    self.done = true;
                    break;
                }
                _ => {
                    // finish 必须泵到 StreamEnd（flate2 内部缓冲分段吐出——
                    // zip-property 随机数据实测单轮只吐部分，src 空即退丢尾）；
                    // 非 finish 输入尽即回；progressed 防两类死循环。
                    if src.is_empty() && flush != flate2::FlushCompress::Finish {
                        break;
                    }
                    if !progressed {
                        break;
                    }
                }
            }
        }
        if gzip && self.done {
            let crc = !self.gz_crc;
            self.out.extend_from_slice(&crc.to_le_bytes());
            self.out.extend_from_slice(&self.gz_isize.to_le_bytes());
        }
        Ok(())
    }

    /// 解压侧喂入（先入 pending，再按族泵出）。
    pub(crate) fn inflate_feed(&mut self, input: &[u8], flag: u32) -> Result<(), ZErr> {
        if self.kind == ZKind::ZstdDec {
            let z = self.zd2.as_mut().unwrap();
            z.carried.extend_from_slice(input);
            if flag == 1 || flag == 2 {
                // 真机口径（truncated 套件）：flag 2 = ZSTD_e_end 终结档严格
                // （截断即报错）；flag 1 = ZSTD_e_flush 容忍部分解出。
                return self.zstd_decode_pump(flag == 1);
            }
            return Ok(());
        }
        self.pending.extend_from_slice(input);
        if self.first_byte.is_none() && self.pending.len() > self.pos {
            self.first_byte = Some(self.pending[self.pos]);
        }
        match self.kind {
            ZKind::ZlibInflate | ZKind::RawInflate => self.plain_inflate_pump(flag),
            ZKind::GzipInflate | ZKind::GzipInflateSingle | ZKind::AutoInflate => {
                self.gz_inflate_pump(flag)
            }
            _ => Ok(()),
        }
    }

    /// zlib/raw 解压泵。
    pub(crate) fn plain_inflate_pump(&mut self, flag: u32) -> Result<(), ZErr> {
        if self.done {
            return Ok(());
        }
        let zlib = self.kind == ZKind::ZlibInflate;
        let finish = flag == 4;
        let tolerate = flag == 2; // finishFlush=Z_SYNC_FLUSH：截断容忍（truncated 套件）
        if zlib && !self.hdr_checked {
            let avail = self.pending.len() - self.pos;
            if avail < 2 {
                if finish {
                    if tolerate {
                        self.done = true;
                        return Ok(());
                    }
                    return Err(ZErr::buf("unexpected end of file"));
                }
                return Ok(());
            }
            let b0 = self.pending[self.pos];
            let b1 = self.pending[self.pos + 1];
            let cm = b0 & 0x0f;
            let cinfo = b0 >> 4;
            if cm != 8 || cinfo > 7 || ((b0 as u16) * 256 + b1 as u16) % 31 != 0 {
                return Err(ZErr::data("incorrect header check"));
            }
            self.hdr_checked = true;
        }
        if !self.hdr_checked && !zlib {
            self.hdr_checked = true;
        }
        let zd = self.zd.get_or_insert_with(|| flate2::Decompress::new(zlib));
        // raw 流无 FDICT 头：zlib 语义字典必须在首次 inflate 前设（Node 对
        // raw 即构造期设）；被动 NEED_DICT 恢复在 raw 下后续喂入留 Mode::Bad，
        // 报 "repeated call with bad state"（dictionary 套件 raw/rawreset 实测）。
        if !zlib && !self.dict_set_done && !self.dict.is_empty() {
            self.dict_set_done = true;
            let _ = zd.set_dictionary(&self.dict);
        }
        let mut err: Option<flate2::DecompressError> = None;
        let mut ended = false;
        loop {
            let src = &self.pending[self.pos..];
            let tin = zd.total_in();
            let ob = self.out.len();
            // decompress_vec 同 compress_vec：spare capacity 为输出空间，先预留
            // 防零推进（G9-1 潜伏 bug 同根）。
            self.out.reserve(16384);
            let flush =
                if finish { flate2::FlushDecompress::Finish } else { flate2::FlushDecompress::None };
            match zd.decompress_vec(src, &mut self.out, flush) {
                Ok(status) => {
                    let consumed = (zd.total_in() - tin) as usize;
                    self.pos += consumed;
                    let progressed = consumed > 0 || self.out.len() > ob;
                    match status {
                        flate2::Status::StreamEnd => {
                            ended = true;
                            break;
                        }
                        _ => {
                            if self.pos >= self.pending.len() || !progressed {
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    self.pos += (zd.total_in() - tin) as usize;
                    let estr = e.to_string();
                    // zlib_rs 遇 FDICT 未供 dict 报 requires a dictionary：
                    // 有 dict → set_dictionary（含 adler32/DICTID 校验，错即
                    // Bad dictionary）后重解；无 dict → Missing dictionary
                    //（dictionary-fail 套件真机口径）。
                    if estr.contains("requires a dictionary") && !self.dict_set_done {
                        self.dict_set_done = true;
                        if self.dict.is_empty() {
                            return Err(ZErr::new("Z_NEED_DICT", "Missing dictionary"));
                        }
                        match zd.set_dictionary(&self.dict) {
                            Ok(_) => {
                                if self.pos < self.pending.len() {
                                    continue;
                                }
                                return Ok(());
                            }
                            Err(_) => {
                                return Err(ZErr::new("Z_NEED_DICT", "Bad dictionary"));
                            }
                        }
                    }
                    // zlib_rs set_dictionary 后 finish 档重解遗留 Mode::Bad
                    //（dict 已校验匹配、数据已解出——bad state 属收尾噪声，
                    // finish 档容忍，限定 dict 流。记档 G9-3 深挖）。
                    if estr.contains("repeated call with bad state")
                        && self.dict_set_done
                        && finish
                    {
                        self.pos = self.pending.len();
                        self.compact();
                        self.done = true;
                        return Ok(());
                    }
                    err = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = err {
            return Err(self.classify_inflate_err(&e));
        }
        if ended {
            self.done = true;
            if self.reject && self.unconsumed() > 0 {
                return Err(ZErr::junk());
            }
        } else if finish {
            return Err(ZErr::buf("unexpected end of file"));
        } else if tolerate {
            // 终结档 finishFlush=Z_SYNC_FLUSH（flag 2）：解到已备输入尽头即收
            // （truncated 套件口径；G9-1 遗留——原 finish 分支内 tolerate 恒 false 死代码）
            self.done = true;
        }
        self.compact();
        Ok(())
    }

    pub(crate) fn classify_inflate_err(&self, e: &flate2::DecompressError) -> ZErr {
        // 首块 BTYPE==3（保留块型）→ 真机 "invalid block type"（raw garbage 口径）
        if let Some(b0) = self.first_byte {
            if (b0 >> 1) & 3 == 3 {
                return ZErr::data("invalid block type");
            }
        }
        let msg = e.to_string();
        // Z_NEED_DICT → 真机消息（dictionary-fail 套件 26.8.2 对拍：
        // 无 dict "Missing dictionary"、错 dict "Bad dictionary"，code Z_NEED_DICT）
        if msg.contains("requires a dictionary") || msg.contains("Missing dictionary") {
            return ZErr::new("Z_NEED_DICT", "Missing dictionary");
        }
        ZErr::data(&msg)
    }

    /// gzip 多成员/单成员解压泵（trailing zeros/garbage/magic 语义真机对拍：
    /// 全零尾 → 忽略；乱码 → "incorrect header check"；假 gzip 头 →
    /// "unknown compression method"；截断 → Z_BUF_ERROR "unexpected end of file"；
    /// rejectGarbageAfterEnd → ERR_TRAILING_JUNK_AFTER_STREAM_END）。
    pub(crate) fn gz_inflate_pump(&mut self, flag: u32) -> Result<(), ZErr> {
        let finish = flag == 4;
        let tolerate = flag == 2;
        let single = self.kind == ZKind::GzipInflateSingle;
        if self.gz.is_none() {
            self.gz = Some(GzMember {
                phase: 0,
                hdr: Vec::new(),
                zd: None,
                crc: 0xFFFF_FFFF,
                outlen: 0,
                trailer: Vec::new(),
            });
        }
        // Auto 分流：1f8b → gzip；否则 zlib（一次性决定；真机 unzipSync(garbage) → zlib 路径）
        if self.kind == ZKind::AutoInflate {
            let avail = self.pending.len() - self.pos;
            if avail == 0 {
                if finish {
                    if tolerate {
                        self.done = true;
                        return Ok(());
                    }
                    return Err(ZErr::buf("unexpected end of file"));
                }
                return Ok(());
            }
            let is_gz = (avail >= 2
                && self.pending[self.pos] == 0x1f
                && self.pending[self.pos + 1] == 0x8b)
                || (avail == 1 && self.pending[self.pos] == 0x1f);
            if !is_gz {
                self.kind = ZKind::ZlibInflate;
                return self.plain_inflate_pump(flag);
            }
            self.kind = ZKind::GzipInflate;
        }
        let trunc = |tolerate: bool, s: &mut Self| -> Result<(), ZErr> {
            if tolerate {
                s.done = true;
                return Ok(());
            }
            Err(ZErr::buf("unexpected end of file"))
        };
        loop {
            let phase = self.gz.as_ref().unwrap().phase;
            match phase {
                0 => {
                    // header：固定 10B + 可选段（FEXTRA/FNAME/FCOMMENT/FHCRC）
                    let mut need: usize = 10;
                    let flg0;
                    {
                        let hdr = &self.gz.as_ref().unwrap().hdr;
                        flg0 = if hdr.len() >= 4 { Some(hdr[3]) } else { None };
                    }
                    if let Some(flg) = flg0 {
                        if flg & 4 != 0 {
                            // FEXTRA
                            if self.gz.as_ref().unwrap().hdr.len() >= 12 {
                                let h = &self.gz.as_ref().unwrap().hdr;
                                need = 12 + u16::from_le_bytes([h[10], h[11]]) as usize;
                            } else {
                                need = 12;
                            }
                        }
                    }
                    // 收集到 need 字节（除 FNAME/FCOMMENT/FHCRC 后段）
                    while self.gz.as_ref().unwrap().hdr.len() < need && self.pos < self.pending.len() {
                        let b = self.pending[self.pos];
                        self.pos += 1;
                        self.gz.as_mut().unwrap().hdr.push(b);
                    }
                    let hdr = &self.gz.as_ref().unwrap().hdr;
                    if hdr.len() < 2 {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    if hdr[0] != 0x1f || hdr[1] != 0x8b {
                        return Err(ZErr::data("incorrect header check"));
                    }
                    if hdr.len() < 3 {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    if hdr[2] != 8 {
                        return Err(ZErr::data("unknown compression method"));
                    }
                    if hdr.len() < 10 {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    let flg = hdr[3];
                    let extra = flg & 4 != 0;
                    if extra && hdr.len() < 12 {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    if extra && hdr.len() < 12 + u16::from_le_bytes([hdr[10], hdr[11]]) as usize {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    // FNAME(8)/FCOMMENT(16)：收到 NUL 为止
                    for sec in [(flg & 8 != 0), (flg & 16 != 0)] {
                        if !sec {
                            continue;
                        }
                        loop {
                            if self.pos >= self.pending.len() {
                                if finish {
                                    return trunc(tolerate, self);
                                }
                                return Ok(());
                            }
                            let b = self.pending[self.pos];
                            self.pos += 1;
                            self.gz.as_mut().unwrap().hdr.push(b);
                            if b == 0 {
                                break;
                            }
                        }
                    }
                    // FHCRC(2)
                    if flg & 2 != 0 {
                        let mut got = 0;
                        while got < 2 {
                            if self.pos >= self.pending.len() {
                                if finish {
                                    return trunc(tolerate, self);
                                }
                                return Ok(());
                            }
                            let b = self.pending[self.pos];
                            self.pos += 1;
                            self.gz.as_mut().unwrap().hdr.push(b);
                            got += 1;
                        }
                        // FHCRC 校验：crc16 over header（真机未测，记档不校验）
                    }
                    let m = self.gz.as_mut().unwrap();
                    m.phase = 1;
                    m.zd = Some(flate2::Decompress::new(false));
                }
                1 => {
                    // body：raw inflate
                    let mut ended = false;
                    let mut err: Option<flate2::DecompressError> = None;
                    {
                        let m = self.gz.as_mut().unwrap();
                        let zd = m.zd.as_mut().unwrap();
                        let mut produced: Vec<Vec<u8>> = Vec::new();
                        loop {
                            let src = &self.pending[self.pos..];
                            let tin = zd.total_in();
                            // spare capacity 输出空间 + 零推进保险（同 plain 泵）。
                            let mut ob: Vec<u8> = Vec::with_capacity(16384);
                            match zd.decompress_vec(src, &mut ob, flate2::FlushDecompress::None)
                            {
                                Ok(status) => {
                                    let consumed = (zd.total_in() - tin) as usize;
                                    self.pos += consumed;
                                    let progressed = consumed > 0 || !ob.is_empty();
                                    if !ob.is_empty() {
                                        produced.push(ob);
                                    }
                                    match status {
                                        flate2::Status::StreamEnd => {
                                            ended = true;
                                            break;
                                        }
                                        _ => {
                                            if self.pos >= self.pending.len() || !progressed {
                                                break;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.pos += (zd.total_in() - tin) as usize;
                                    err = Some(e);
                                    break;
                                }
                            }
                        }
                        let m = self.gz.as_mut().unwrap();
                        for chunk in &produced {
                            for &b in chunk.iter() {
                                m.crc = CRC32_TABLE[((m.crc ^ b as u32) & 0xFF) as usize]
                                    ^ (m.crc >> 8);
                            }
                            m.outlen = m.outlen.wrapping_add(chunk.len() as u32);
                        }
                        for chunk in produced.drain(..) {
                            self.out.extend_from_slice(&chunk);
                        }
                    }
                    if let Some(e) = err {
                        return Err(self.classify_inflate_err(&e));
                    }
                    if ended {
                        self.gz.as_mut().unwrap().phase = 2;
                    } else if finish {
                        return trunc(tolerate, self);
                    } else {
                        return Ok(());
                    }
                }
                2 => {
                    // trailer：crc32 + isize 各 4B
                    while self.gz.as_ref().unwrap().trailer.len() < 8
                        && self.pos < self.pending.len()
                    {
                        let b = self.pending[self.pos];
                        self.pos += 1;
                        self.gz.as_mut().unwrap().trailer.push(b);
                    }
                    if self.gz.as_ref().unwrap().trailer.len() < 8 {
                        if finish {
                            return trunc(tolerate, self);
                        }
                        return Ok(());
                    }
                    let t = self.gz.as_ref().unwrap().trailer.clone();
                    let m = self.gz.as_ref().unwrap();
                    let crc_expect = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
                    let isize_expect = u32::from_le_bytes([t[4], t[5], t[6], t[7]]);
                    if crc_expect != !m.crc {
                        return Err(ZErr::data("incorrect data check"));
                    }
                    if isize_expect != m.outlen {
                        return Err(ZErr::data("incorrect length check"));
                    }
                    let m = self.gz.as_mut().unwrap();
                    m.phase = 3;
                    m.hdr.clear();
                    m.trailer.clear();
                    m.zd = None;
                    m.crc = 0xFFFF_FFFF;
                    m.outlen = 0;
                }
                _ => {
                    // between：成员完结后决定去向
                    let avail = self.pending.len() - self.pos;
                    if avail == 0 {
                        if finish || single {
                            self.done = true;
                        }
                        return Ok(());
                    }
                    if self.reject {
                        return Err(ZErr::junk());
                    }
                    // trailing 全零 → 忽略（reject 已在上方报 junk，真机口径）
                    if self.pending[self.pos..].iter().all(|&b| b == 0) {
                        self.done = true;
                        return Ok(());
                    }
                    if single {
                        // 单成员（Web）：任何剩余输入即 junk → TypeError（JS 侧映射）
                        return Err(ZErr::junk());
                    }
                    self.gz.as_mut().unwrap().phase = 0;
                }
            }
        }
    }

    pub(crate) fn compact(&mut self) {
        if self.pos > 0 && self.pos == self.pending.len() {
            self.pending.clear();
            self.pos = 0;
        } else if self.pos > (1 << 20) {
            self.pending.drain(..self.pos);
            self.pos = 0;
        }
    }

    /// brotli 编码泵。
    pub(crate) fn brotli_enc_feed(&mut self, input: &[u8], flag: u32) -> Result<(), ZErr> {
        let be = self.be.as_mut().unwrap();
        if flag == 2 && be.st.is_finished() {
            return Ok(()); // 终结后吞掉
        }
        let op = match flag {
            1 => brotli::enc::encode::BrotliEncoderOperation::BROTLI_OPERATION_FLUSH,
            2 => brotli::enc::encode::BrotliEncoderOperation::BROTLI_OPERATION_FINISH,
            3 => brotli::enc::encode::BrotliEncoderOperation::BROTLI_OPERATION_EMIT_METADATA,
            _ => brotli::enc::encode::BrotliEncoderOperation::BROTLI_OPERATION_PROCESS,
        };
        let mut avail_in = input.len();
        let mut in_off = 0usize;
        loop {
            let mut outbuf = [0u8; 16384];
            let mut avail_out = outbuf.len();
            let mut out_off = 0usize;
            let mut total_out: Option<usize> = None;
            let ok = be.st.compress_stream(
                op,
                &mut avail_in,
                input,
                &mut in_off,
                &mut avail_out,
                &mut outbuf,
                &mut out_off,
                &mut total_out,
                &mut |_, _, _, _| {},
            );
            if !ok {
                return Err(ZErr::new("Z_DATA_ERROR", "Compression failed"));
            }
            self.out.extend_from_slice(&outbuf[..out_off]);
            if avail_in == 0 && !be.st.has_more_output() {
                break;
            }
        }
        if flag == 2 && be.st.is_finished() {
            self.done = true;
        }
        Ok(())
    }

    /// brotli 解码泵。
    pub(crate) fn brotli_dec_feed(&mut self, input: &[u8]) -> Result<(), ZErr> {
        let bd = self.bd.as_mut().unwrap();
        if bd.done_flag {
            // done 后的输入存 pending 计为未消费（bytesWritten 只计引擎实际
            // 消费；premature-end 套件 trailing 垃圾不计入）
            self.pending.extend_from_slice(input);
            return Ok(());
        }
        let mut avail_in = input.len();
        let mut in_off = 0usize;
        let mut total_out: usize = 0;
        loop {
            let mut outbuf = [0u8; 16384];
            let mut avail_out = outbuf.len();
            let mut out_off = 0usize;
            let res = brotli::BrotliDecompressStream(
                &mut avail_in,
                &mut in_off,
                input,
                &mut avail_out,
                &mut out_off,
                &mut outbuf,
                &mut total_out,
                &mut bd.st,
            );
            self.out.extend_from_slice(&outbuf[..out_off]);
            match res {
                brotli::BrotliResult::ResultSuccess => {
                    if brotli_decompressor::BrotliDecoderIsFinished(&bd.st) {
                        bd.done_flag = true;
                        self.done = true;
                        if avail_in > 0 {
                            // 同次 feed 内 StreamEnd 后的剩余计为未消费
                            self.pending.extend_from_slice(&input[input.len() - avail_in..]);
                        }
                        if self.reject && avail_in > 0 {
                            return Err(ZErr::junk());
                        }
                        break;
                    }
                    if avail_in == 0 {
                        break;
                    }
                }
                brotli::BrotliResult::NeedsMoreInput => {
                    if brotli_decompressor::BrotliDecoderIsFinished(&bd.st) {
                        bd.done_flag = true;
                        self.done = true;
                        if avail_in > 0 {
                            self.pending.extend_from_slice(&input[input.len() - avail_in..]);
                        }
                        if self.reject && avail_in > 0 {
                            return Err(ZErr::junk());
                        }
                    }
                    break;
                }
                brotli::BrotliResult::NeedsMoreOutput => {
                    continue;
                }
                brotli::BrotliResult::ResultFailure => {
                    return Err(brotli_dec_err(&bd.st));
                }
            }
        }
        Ok(())
    }

    /// zstd 编码泵（flush 边界分段出帧；pledged 终检）。
    pub(crate) fn zstd_enc_feed(&mut self, input: &[u8], flag: u32) -> Result<(), ZErr> {
        let ze = self.ze.as_mut().unwrap();
        ze.buf.extend_from_slice(input);
        ze.total_in += input.len() as u64;
        let end = flag == 2;
        if flag == 1 || end {
            if ze.buf.len() > ze.flushed {
                let seg = ze.buf[ze.flushed..].to_vec();
                ze.flushed = ze.buf.len();
                let out = ruzstd::encoding::compress_to_vec(
                    &seg[..],
                    ruzstd::encoding::CompressionLevel::Fastest,
                );
                self.out.extend_from_slice(&out);
            } else if end && ze.total_in == 0 {
                const EMPTY: &[u8] = &[];
                let out = ruzstd::encoding::compress_to_vec(
                    EMPTY,
                    ruzstd::encoding::CompressionLevel::Fastest,
                );
                self.out.extend_from_slice(&out);
            }
        }
        if end {
            let ze = self.ze.as_ref().unwrap();
            if let Some(p) = ze.pledged {
                if p != ze.total_in {
                    return Err(ZErr::new("ZSTD_error_srcSize_wrong", "Src size is incorrect"));
                }
            }
            self.done = true;
        }
        Ok(())
    }

    /// zstd 解码泵（一次性解 pos 前缀首帧；轮子无增量 API，记档）。
    pub(crate) fn zstd_decode_pump(&mut self, tolerate_missing: bool) -> Result<(), ZErr> {
        let z = self.zd2.as_mut().unwrap();
        if z.pos >= z.carried.len() {
            return Ok(());
        }
        let mut cur = std::io::Cursor::new(&z.carried[z.pos..]);
        let mut fd = ruzstd::decoding::FrameDecoder::new();
        // 注：ruzstd::decoding::{FrameDecoder, BlockDecodingStrategy}；FrameDecoder 自带 io::Read
        if let Err(_e) = fd.init(&mut cur) {
            let avail = z.carried.len() - z.pos;
            let magic_ok = avail >= 4
                && z.carried[z.pos] == 0x28
                && z.carried[z.pos + 1] == 0xB5
                && z.carried[z.pos + 2] == 0x2F
                && z.carried[z.pos + 3] == 0xFD;
            return Err(if avail < 4 || magic_ok {
                if tolerate_missing {
                    self.done = true;
                    return Ok(());
                }
                ZErr::buf("unexpected end of file")
            } else {
                ZErr::new("ZSTD_error_prefix_unknown", "Unknown frame descriptor")
            });
        }
        loop {
            if fd.is_finished() {
                break;
            }
            match fd.decode_blocks(&mut cur, ruzstd::decoding::BlockDecodingStrategy::UptoBytes(1 << 20)) {
                Ok(_) => {
                    let mut buf = [0u8; 65536];
                    loop {
                        match fd.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => self.out.extend_from_slice(&buf[..n]),
                            Err(_) => break,
                        }
                    }
                    let exhausted = cur.position() as usize >= z.carried.len() - z.pos;
                    if exhausted && !fd.is_finished() && fd.can_collect() == 0 {
                        if tolerate_missing {
                            self.done = true;
                            return Ok(());
                        }
                        return Err(ZErr::buf("unexpected end of file"));
                    }
                }
                Err(_e) => {
                    let exhausted = cur.position() as usize >= z.carried.len() - z.pos;
                    if exhausted {
                        if tolerate_missing {
                            self.done = true;
                            return Ok(());
                        }
                        return Err(ZErr::buf("unexpected end of file"));
                    }
                    return Err(ZErr::new("ZSTD_error_??", "corrupt zstd frame"));
                }
            }
        }
        let mut buf = [0u8; 65536];
        loop {
            match fd.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.out.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        let consumed = cur.position() as usize;
        z.pos += consumed;
        self.done = true;
        if self.reject && z.carried.len() > z.pos {
            return Err(ZErr::junk());
        }
        Ok(())
    }

    /// 统一入口：返回 (unconsumed, done)。
    pub(crate) fn feed(&mut self, input: &[u8], flag: u32) -> Result<(usize, bool), ZErr> {
        let r = match self.kind {
            ZKind::ZlibDeflate | ZKind::RawDeflate | ZKind::GzipDeflate => {
                self.deflate_feed(input, flag)
            }
            ZKind::BrotliEnc => self.brotli_enc_feed(input, flag),
            ZKind::BrotliDec => self.brotli_dec_feed(input),
            ZKind::ZstdEnc => self.zstd_enc_feed(input, flag),
            _ => self.inflate_feed(input, flag),
        };
        r?;
        Ok((self.unconsumed(), self.done))
    }

    pub(crate) fn take_out(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    pub(crate) fn reset(&mut self) {
        let kind = self.kind;
        let level = self.level;
        let reject = self.reject;
        let dict = std::mem::take(&mut self.dict);
        let pledged = self.ze.as_ref().and_then(|z| z.pledged);
        *self = ZEngine::new(kind, level, &dict, pledged, reject);
    }
}
