//! WinterJS.media 底座：音频解码/放音（symphonia/rodio）+ AV1 编码
//! （rav1e，手写 IVF）+ MP4 demux（shiguredo_mp4）。
//! 约定：元信息走 JSON 桥；PCM 走 Float32Array（f32 交错），像素/包走 Uint8Array。
//! 音频解码全量进内存（大文件走流式另案，见 caps）；动画/多轨只取首音频轨；
//! mp4 mux 不在本轮（demux 先行，见 §16-6 跟进项）。

use std::collections::HashMap;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::{LazyLock, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};

use mozjs::context::JSContext;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use mozjs::typedarray::{CreateWith, Float32, TypedArray, Uint8};

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};
use crate::builtins::crypto::common::set_rval_bytes;

/// PCM 上限（f32 总数；stereo 44.1k 约 12 分钟，超限报 RangeError，流式另案）。
const MAX_SAMPLES: u64 = 134_217_728;
/// 视频单边上限 + 总像素上限（rav1e 本来就慢，大帧另案）。
const MAX_VDIM: u32 = 4096;
const MAX_VPIXELS: u64 = 16_777_216;
/// 视频帧数上限。
const MAX_FRAMES: usize = 256;
/// mp4 样本 walk 上限（元信息轮；超限报 RangeError）。
const MAX_MP4_SAMPLES: usize = 1_000_000;

/// 内存可寻址源（symphonia 只要 File 的 MediaSource；内存走 newtype 包装）。
struct MemSrc {
    cur: Cursor<Vec<u8>>,
}

impl Read for MemSrc {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.cur.read(buf)
    }
}

impl Seek for MemSrc {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.cur.seek(pos)
    }
}

impl symphonia::core::io::MediaSource for MemSrc {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.cur.get_ref().len() as u64)
    }
}

/// 音频容器嗅探（symphonia probe 自带嗅探；hint 只在显式格式时给扩展名）。
fn audio_ext(fmt: &str) -> Option<&'static str> {
    Some(match fmt.trim().to_ascii_lowercase().as_str() {
        "mp3" => "mp3",
        "wav" => "wav",
        "flac" => "flac",
        "ogg" | "vorbis" => "ogg",
        "m4a" | "aac" => "m4a",
        "aiff" | "aif" => "aiff",
        "caf" => "caf",
        "alac" => "m4a",
        "mka" | "mkv" => "mka",
        _ => return None,
    })
}

/// 容器魔数（info 无显式格式时的 format 名；probe 本体不报容器名）。
fn sniff_audio(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 4 {
        return None;
    }
    if bytes.starts_with(b"fLaC") {
        return Some("flac");
    }
    if bytes.starts_with(b"OggS") {
        return Some("ogg");
    }
    if bytes.starts_with(b"RIFF") {
        return Some("wav");
    }
    if bytes.starts_with(b"FORM") {
        return Some("aiff");
    }
    if bytes.starts_with(b"caff") {
        return Some("caf");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("mp4");
    }
    if bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0 {
        return Some("mp3");
    }
    if bytes[0] == b'I' && bytes[1] == b'D' && bytes[2] == b'3' {
        return Some("mp3");
    }
    None
}

fn codec_name(id: symphonia::core::codecs::audio::AudioCodecId) -> &'static str {
    use symphonia::core::codecs::audio::well_known;
    match id {
        well_known::CODEC_ID_MP3 => "mp3",
        well_known::CODEC_ID_MP1 | well_known::CODEC_ID_MP2 => "mp123",
        well_known::CODEC_ID_AAC => "aac",
        well_known::CODEC_ID_VORBIS => "vorbis",
        well_known::CODEC_ID_OPUS => "opus",
        well_known::CODEC_ID_FLAC => "flac",
        well_known::CODEC_ID_ALAC => "alac",
        well_known::CODEC_ID_PCM_S16LE
        | well_known::CODEC_ID_PCM_S16BE
        | well_known::CODEC_ID_PCM_S24LE
        | well_known::CODEC_ID_PCM_S32LE
        | well_known::CODEC_ID_PCM_F32LE
        | well_known::CODEC_ID_PCM_U8 => "pcm",
        _ => "unknown",
    }
}

fn arg_bytes(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<Vec<u8>> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    view_bytes(cx, frame.arg(i), what)
}

fn set_rval_string(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

fn view_f32(cx: &mut JSContext, v: JSVal, what: &str) -> Option<Vec<f32>> {
    if !v.is_object() {
        report_error(cx, &format!("TypeError: {what} requires a Float32Array"));
        return None;
    }
    // SAFETY: is_object 已判定；from/as_slice_safe 均为 safe API
    let obj: *mut JSObject = v.to_object();
    let Ok(arr) = TypedArray::<Float32, *mut JSObject>::from(obj) else {
        report_error(cx, &format!("TypeError: {what} requires a Float32Array"));
        return None;
    };
    if arr.is_shared() {
        report_error(cx, &format!("TypeError: {what} does not accept SharedArrayBuffer views yet"));
        return None;
    }
    match arr.as_slice_safe(cx.no_gc()) {
        Some(s) => Some(s.to_vec()),
        None => {
            report_error(cx, &format!("TypeError: {what} view is detached"));
            None
        }
    }
}

fn set_rval_f32(cx: &mut JSContext, frame: &Frame, out: &[f32]) -> bool {
    rooted!(&in(cx) let mut obj: *mut JSObject = std::ptr::null_mut());
    // SAFETY: realm 内创建 Float32Array；obj 为 rooted 出参（§6 边界）
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(out.as_ptr() as *const u8, out.len() * 4)
    };
    let ok = unsafe {
        TypedArray::<Uint8, *mut JSObject>::create(cx, CreateWith::Slice(bytes), obj.handle_mut())
    };
    if ok.is_err() || obj.is_null() {
        report_error(cx, "RangeError: cannot allocate output");
        return false;
    }
    // Uint8 视图盖在 f32 缓冲上：JS 侧经 new Float32Array(buf) 重解释（见 prelude）。
    frame.set_rval(mozjs::jsval::ObjectValue(obj.get()));
    true
}

struct Decoded {
    format: String,
    codec: String,
    sample_rate: u32,
    channels: u32,
    title: Option<String>,
    artist: Option<String>,
    samples: Vec<f32>,
}

/// symphonia 全量解码（首音频轨；f32 交错；DecodeError 跳包，其余错即停）。
fn decode_all(bytes: &[u8], ext: Option<&str>) -> Result<Decoded, String> {
    let sniffed = if ext.is_none() { sniff_audio(bytes) } else { None };
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::{MetadataOptions, StandardTag};
    let mss = MediaSourceStream::new(
        Box::new(MemSrc { cur: Cursor::new(bytes.to_vec()) }),
        Default::default(),
    );
    let mut hint = Hint::new();
    if let Some(e) = ext {
        hint.with_extension(e);
    }
    let meta_opts: MetadataOptions = Default::default();
    let fmt_opts: FormatOptions = Default::default();
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, fmt_opts, meta_opts)
        .map_err(|e| format!("unsupported audio format: {e}"))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "no audio track".to_string())?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .ok_or_else(|| "missing codec parameters".to_string())?;
    let codec = match params {
        symphonia::core::codecs::CodecParameters::Audio(a) => codec_name(a.codec).to_string(),
        symphonia::core::codecs::CodecParameters::Video(_) => "video".to_string(),
        symphonia::core::codecs::CodecParameters::Subtitle(_) => "subtitle".to_string(),
        _ => "unknown".to_string(),
    };
    let audio = params
        .audio()
        .ok_or_else(|| "not an audio stream".to_string())?;
    let sample_rate = audio.sample_rate.unwrap_or(0);
    let channels = audio.channels.as_ref().map(|c| c.count() as u32).unwrap_or(0);
    if sample_rate == 0 || channels == 0 {
        return Err("missing sample rate or channel count".to_string());
    }
    let dec_opts: AudioDecoderOptions = Default::default();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(audio, &dec_opts)
        .map_err(|e| format!("unsupported codec: {e}"))?;
    let (mut title, mut artist) = (None, None);
    // skip_to_latest 为空即无元数据（current 同空，无需二次借用）。
    if let Some(rev) = format.metadata().skip_to_latest() {
        for tag in rev.media.tags.iter() {
            match tag.std {
                Some(StandardTag::TrackTitle(_)) if title.is_none() => {
                    title = Some(tag.raw.value.to_string())
                }
                Some(StandardTag::Artist(_)) if artist.is_none() => {
                    artist = Some(tag.raw.value.to_string())
                }
                _ => {}
            }
        }
    }
    let mut samples: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(format!("demux failed: {e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                if (samples.len() as u64) + (buf.samples_interleaved() as u64) > MAX_SAMPLES {
                    return Err("audio exceeds 134M samples (streaming decode deferred)".to_string());
                }
                samples.resize(samples.len() + buf.samples_interleaved(), 0.0);
                let n = samples.len();
                let from = n - buf.samples_interleaved();
                buf.copy_to_slice_interleaved(&mut samples[from..]);
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => {}
            Err(e) => return Err(format!("decode failed: {e}")),
        }
    }
    Ok(Decoded {
        format: ext.or(sniffed).unwrap_or("unknown").to_string(),
        codec,
        sample_rate,
        channels,
        title,
        artist,
        samples,
    })
}

/// `__wjs_media_audio_info(bytes, format?)` → JSON（不解包，只读头）。
pub unsafe extern "C" fn media_audio_info(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let what = "WinterJS.media.audioInfo";
    let Some(bytes) = arg_bytes(&mut cx, &frame, 0, what) else {
        return false;
    };
    if bytes.is_empty() {
        report_error(&mut cx, &format!("TypeError: {what} requires non-empty bytes"));
        return false;
    }
    let ext = if frame.argc() > 1 {
        let s = value_to_string(&mut cx, frame.arg(1));
        if s.is_empty() || s == "undefined" {
            None
        } else {
            Some(audio_ext(&s).unwrap_or("bin"))
        }
    } else {
        None
    };
    match decode_all(&bytes, ext) {
        Ok(d) => {
            let json = serde_json::json!({
                "format": d.format,
                "codec": d.codec,
                "sampleRate": d.sample_rate,
                "channels": d.channels,
                "duration": d.samples.len() as f64 / (d.sample_rate as f64 * d.channels as f64),
                "title": d.title,
                "artist": d.artist,
            })
            .to_string();
            set_rval_string(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: {e}"));
            false
        }
    }
}

/// `__wjs_media_audio_pcm(bytes, format?)` → Float32Array（f32 交错全量）。
pub unsafe extern "C" fn media_audio_pcm(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let what = "WinterJS.media.decodeAudio";
    let Some(bytes) = arg_bytes(&mut cx, &frame, 0, what) else {
        return false;
    };
    if bytes.is_empty() {
        report_error(&mut cx, &format!("TypeError: {what} requires non-empty bytes"));
        return false;
    }
    let ext = if frame.argc() > 1 {
        let s = value_to_string(&mut cx, frame.arg(1));
        if s.is_empty() || s == "undefined" {
            None
        } else {
            Some(audio_ext(&s).unwrap_or("bin"))
        }
    } else {
        None
    };
    match decode_all(&bytes, ext) {
        Ok(d) => set_rval_f32(&mut cx, &frame, &d.samples),
        Err(e) => {
            let is_range = e.contains("exceeds");
            report_error(&mut cx, &format!("{}: {e}", if is_range { "RangeError" } else { "TypeError" }));
            false
        }
    }
}

/// 放音台（rodio OutputStream + Player 常驻；id 递增；stop 即 drop）。
/// 纯 Rust 侧状态，不碰 JSAPI（4.78 finalize 纪律：只做簿记）。
static PLAYERS: LazyLock<Mutex<HashMap<u32, (rodio::MixerDeviceSink, rodio::Player)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_PLAY_ID: AtomicU32 = AtomicU32::new(1);

/// `__wjs_media_play(pcm, sampleRate, channels, volume?)` → id（u32）。
/// 非阻塞：cpal 线程放音，native 即返。
pub unsafe extern "C" fn media_play(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let what = "WinterJS.media.play";
    if frame.argc() < 3 {
        report_error(&mut cx, &format!("TypeError: {what} requires (samples, sampleRate, channels)"));
        return false;
    }
    let Some(samples) = view_f32(&mut cx, frame.arg(0), what) else {
        return false;
    };
    let (rate, ch) = (
        value_to_string(&mut cx, frame.arg(1)).parse::<u32>().unwrap_or(0),
        value_to_string(&mut cx, frame.arg(2)).parse::<u32>().unwrap_or(0),
    );
    if rate == 0 || !(1..=32).contains(&ch) {
        report_error(&mut cx, &format!("RangeError: {what} needs sampleRate>0 and 1..32 channels"));
        return false;
    }
    let volume: f32 = if frame.argc() > 3 {
        value_to_string(&mut cx, frame.arg(3)).parse::<f32>().unwrap_or(f32::NAN)
    } else {
        1.0
    };
    if !volume.is_finite() || volume < 0.0 {
        report_error(&mut cx, &format!("RangeError: {what} volume must be a finite number >= 0"));
        return false;
    }
    if samples.len() as u64 > MAX_SAMPLES {
        report_error(&mut cx, &format!("RangeError: {what} exceeds 134M samples"));
        return false;
    }
    // 回落链保留（open_default_sink 先默认设备再逐个试），drop 提示关掉
    // （stop 即 drop，不刷用户 stderr 通道）。
    let mut stream = match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(s) => s,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: no audio output device: {e}"));
            return false;
        }
    };
    stream.log_on_drop(false);
    let player = rodio::Player::connect_new(stream.mixer());
    player.set_volume(volume);
    // ch/rate 已在上方位校验非零，NonZero 恒成立（无 unwrap panic 面）。
    let ch_nz = std::num::NonZeroU16::new(ch as u16).unwrap_or(std::num::NonZeroU16::MIN);
    let rate_nz = std::num::NonZeroU32::new(rate).unwrap_or(std::num::NonZeroU32::MIN);
    player.append(rodio::buffer::SamplesBuffer::new(ch_nz, rate_nz, samples));
    let id = NEXT_PLAY_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut m) = PLAYERS.lock() {
        m.insert(id, (stream, player));
    }
    set_rval_string(&mut cx, &frame, &id.to_string());
    // set_rval_string 返回字符串形 id；JS 侧 Number() 化（u32 上溢无忧：2^32 次播放）。
    true
}

/// `__wjs_media_stop(id)` → true（未知 id 即 false，不报错）。
pub unsafe extern "C" fn media_stop(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = if frame.argc() > 0 {
        value_to_string(&mut cx, frame.arg(0)).parse::<u32>().unwrap_or(0)
    } else {
        0
    };
    let gone = PLAYERS.lock().map(|mut m| m.remove(&id).is_some()).unwrap_or(false);
    set_rval_string(&mut cx, &frame, if gone { "true" } else { "false" });
    true
}

/// RGBA8 → YUV420 BT.601 全值域（rav1e 要 420 平面；奇数维上游 pad，本面先拦偶数）。
fn rgba_to_yuv420(px: &[u8], w: usize, h: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut y = vec![0u8; w * h];
    let mut u = vec![0u8; (w / 2) * (h / 2)];
    let mut v = vec![0u8; (w / 2) * (h / 2)];
    for j in 0..h {
        for i in 0..w {
            let o = (j * w + i) * 4;
            let (r, g, b) = (px[o] as f32, px[o + 1] as f32, px[o + 2] as f32);
            y[j * w + i] = (0.299 * r + 0.587 * g + 0.114 * b).round().clamp(0.0, 255.0) as u8;
            if i % 2 == 0 && j % 2 == 0 {
                // 2x2 均值（边界偶数维已保证）。
                let mut su = 0.0;
                let mut sv = 0.0;
                for dj in 0..2 {
                    for di in 0..2 {
                        let q = ((j + dj) * w + (i + di)) * 4;
                        let (rr, gg, bb) = (px[q] as f32, px[q + 1] as f32, px[q + 2] as f32);
                        su += -0.168736 * rr - 0.331264 * gg + 0.5 * bb;
                        sv += 0.5 * rr - 0.418688 * gg - 0.081312 * bb;
                    }
                }
                let k = (j / 2) * (w / 2) + (i / 2);
                u[k] = ((su / 4.0) + 128.0).round().clamp(0.0, 255.0) as u8;
                v[k] = ((sv / 4.0) + 128.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    (y, u, v)
}

/// 手写 IVF 容器（32 字节头 + 每包 12 字节头；不另引 ivf 轮子）。
fn mux_ivf(packets: &[Vec<u8>], w: u16, h: u16, fps_num: u32, fps_den: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"DKIF");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(b"AV01");
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&fps_num.to_le_bytes());
    out.extend_from_slice(&fps_den.to_le_bytes());
    out.extend_from_slice(&(packets.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for (i, p) in packets.iter().enumerate() {
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        out.extend_from_slice(&(i as u64).to_le_bytes());
        out.extend_from_slice(p);
    }
    out
}

/// `__wjs_media_video_encode(width, height, framesConcat, count, optionsJson)` → IVF Uint8Array。
pub unsafe extern "C" fn media_video_encode(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let what = "WinterJS.media.videoEncode";
    if frame.argc() < 4 {
        report_error(&mut cx, &format!("TypeError: {what} requires (width, height, frames, count)"));
        return false;
    }
    let (w, h) = (
        value_to_string(&mut cx, frame.arg(0)).parse::<u32>().unwrap_or(0),
        value_to_string(&mut cx, frame.arg(1)).parse::<u32>().unwrap_or(0),
    );
    let Some(px) = view_bytes(&mut cx, frame.arg(2), what) else {
        return false;
    };
    let count = value_to_string(&mut cx, frame.arg(3)).parse::<usize>().unwrap_or(0);
    if w < 16 || h < 16 || w > MAX_VDIM || h > MAX_VDIM || w % 2 != 0 || h % 2 != 0 {
        report_error(&mut cx, &format!("RangeError: {what} needs even dimensions within 16..{MAX_VDIM}"));
        return false;
    }
    if count == 0 || count > MAX_FRAMES {
        report_error(&mut cx, &format!("RangeError: {what} needs 1..{MAX_FRAMES} frames"));
        return false;
    }
    if (w as u64) * (h as u64) * (count as u64) > MAX_VPIXELS {
        report_error(&mut cx, &format!("RangeError: {what} exceeds 16M pixels total"));
        return false;
    }
    if px.len() as u64 != (w as u64) * (h as u64) * 4 * (count as u64) {
        report_error(&mut cx, &format!("RangeError: {what} data length must equal width*height*4*count (RGBA8)"));
        return false;
    }
    let opts: serde_json::Value = if frame.argc() > 4 {
        let s = value_to_string(&mut cx, frame.arg(4));
        if s.is_empty() || s == "undefined" {
            serde_json::Value::Null
        } else {
            match serde_json::from_str(&s) {
                Ok(v) => v,
                Err(_) => {
                    report_error(&mut cx, &format!("TypeError: {what} options must be an object"));
                    return false;
                }
            }
        }
    } else {
        serde_json::Value::Null
    };
    let opt = |k: &str| opts.get(k).unwrap_or(&serde_json::Value::Null).clone();
    let speed = match opt("speed") {
        serde_json::Value::Null => 8,
        serde_json::Value::Number(n) => {
            let f = n.as_f64().unwrap_or(-1.0);
            if f.fract() != 0.0 || !(0.0..=10.0).contains(&f) {
                report_error(&mut cx, &format!("RangeError: {what} speed must be an integer within 0..10"));
                return false;
            }
            f as u8
        }
        _ => {
            report_error(&mut cx, &format!("RangeError: {what} speed must be an integer within 0..10"));
            return false;
        }
    };
    let quant = match opt("quantizer") {
        serde_json::Value::Null => 100,
        serde_json::Value::Number(n) => {
            let f = n.as_f64().unwrap_or(-1.0);
            if f.fract() != 0.0 || !(0.0..=255.0).contains(&f) {
                report_error(&mut cx, &format!("RangeError: {what} quantizer must be an integer within 0..255"));
                return false;
            }
            f as usize
        }
        _ => {
            report_error(&mut cx, &format!("RangeError: {what} quantizer must be an integer within 0..255"));
            return false;
        }
    };
    let fps = match opt("fps") {
        serde_json::Value::Null => 30,
        serde_json::Value::Number(n) => {
            let f = n.as_f64().unwrap_or(0.0);
            if f.fract() != 0.0 || !(1.0..=120.0).contains(&f) {
                report_error(&mut cx, &format!("RangeError: {what} fps must be an integer within 1..120"));
                return false;
            }
            f as u32
        }
        _ => {
            report_error(&mut cx, &format!("RangeError: {what} fps must be an integer within 1..120"));
            return false;
        }
    };
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8);
    let enc = rav1e::prelude::EncoderConfig {
        width: w as usize,
        height: h as usize,
        quantizer: quant,
        speed_settings: rav1e::prelude::SpeedSettings::from_preset(speed),
        ..Default::default()
    };
    let cfg = rav1e::prelude::Config::new()
        .with_encoder_config(enc)
        .with_threads(threads);
    let mut ctx = match cfg.new_context::<u8>() {
        Ok(c) => c,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: bad encoder config: {e}"));
            return false;
        }
    };
    let stride = (w as usize) * (h as usize) * 4;
    for n in 0..count {
        let (y, u, v) = rgba_to_yuv420(&px[n * stride..(n + 1) * stride], w as usize, h as usize);
        let mut f = ctx.new_frame();
        f.planes[0].copy_from_raw_u8(&y, w as usize, 1);
        f.planes[1].copy_from_raw_u8(&u, w as usize / 2, 1);
        f.planes[2].copy_from_raw_u8(&v, w as usize / 2, 1);
        if ctx.send_frame(f).is_err() {
            report_error(&mut cx, &format!("TypeError: {what} frame rejected"));
            return false;
        }
    }
    ctx.flush();
    let mut packets: Vec<Vec<u8>> = Vec::new();
    loop {
        match ctx.receive_packet() {
            Ok(p) => packets.push(p.data),
            Err(rav1e::prelude::EncoderStatus::LimitReached) => break,
            Err(rav1e::prelude::EncoderStatus::Encoded) => {}
            Err(rav1e::prelude::EncoderStatus::NeedMoreData) => {
                report_error(&mut cx, &format!("TypeError: {what} encoder stalled"));
                return false;
            }
            Err(e) => {
                report_error(&mut cx, &format!("TypeError: {what} encode failed: {e:?}"));
                return false;
            }
        }
    }
    let ivf = mux_ivf(&packets, w as u16, h as u16, fps, 1);
    if !set_rval_bytes(&mut cx, &frame, &ivf) {
        return false;
    }
    true
}

fn track_kind_s(k: &shiguredo_mp4::TrackKind) -> &'static str {
    use shiguredo_mp4::TrackKind;
    match k {
        TrackKind::Audio => "audio",
        TrackKind::Video => "video",
        _ => "other",
    }
}

fn sample_entry_s(e: &shiguredo_mp4::boxes::SampleEntry) -> &'static str {
    use shiguredo_mp4::boxes::SampleEntry;
    match e {
        SampleEntry::Avc1(_) => "avc1",
        SampleEntry::Hev1(_) | SampleEntry::Hvc1(_) => "hevc",
        SampleEntry::Vp08(_) => "vp8",
        SampleEntry::Vp09(_) => "vp9",
        SampleEntry::Av01(_) => "av1",
        SampleEntry::Opus(_) => "opus",
        SampleEntry::Mp4a(_) => "mp4a",
        SampleEntry::Flac(_) => "flac",
        _ => "other",
    }
}

struct Mp4Walk {
    tracks_json: Vec<serde_json::Value>,
    samples: Vec<(u32, u64, u32, usize, u64, usize)>,
}

fn walk_mp4(bytes: &[u8]) -> Result<Mp4Walk, String> {
    use shiguredo_mp4::demux::{Input, Mp4FileDemuxer};
    let mut demuxer = Mp4FileDemuxer::new();
    let mut guard = 0usize;
    while let Some(req) = demuxer.required_input() {
        guard += 1;
        if guard > 10_000 {
            return Err("demuxer stalled".to_string());
        }
        let start = req.position as usize;
        // size None = 要到末尾（fmp4 初始化段等）。
        let end = req.size.map(|s| start.saturating_add(s)).unwrap_or(bytes.len()).min(bytes.len());
        if start >= bytes.len() {
            return Err("truncated mp4".to_string());
        }
        demuxer.handle_input(Input { position: req.position, data: &bytes[start..end] });
    }
    // 先确认有轨（借用即还），再 walk 样本，最后重借组 JSON。
    demuxer.tracks().map_err(|e| format!("no tracks: {e}"))?;
    let mut codecs: HashMap<u32, &'static str> = HashMap::new();
    let mut counts: HashMap<u32, usize> = HashMap::new();
    let mut totals: HashMap<u32, usize> = HashMap::new();
    let mut samples = Vec::new();
    let mut n = 0usize;
    loop {
        if n >= MAX_MP4_SAMPLES {
            return Err("mp4 exceeds 1M samples".to_string());
        }
        match demuxer.next_sample() {
            Ok(Some(s)) => {
                n += 1;
                let tid = s.track.track_id;
                *counts.entry(tid).or_insert(0) += 1;
                *totals.entry(tid).or_insert(0) += s.data_size;
                if codecs.get(&tid).is_none() {
                    if let Some(e) = s.sample_entry {
                        codecs.insert(tid, sample_entry_s(e));
                    }
                }
                samples.push((tid, s.timestamp, s.duration, s.data_size, s.data_offset, n - 1));
            }
            Ok(None) => break,
            Err(e) => return Err(format!("demux failed: {e}")),
        }
    }
    let tracks_json = demuxer
        .tracks()
        .map_err(|e| format!("no tracks: {e}"))?
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.track_id,
                "kind": track_kind_s(&t.kind),
                "codec": codecs.get(&t.track_id).copied().unwrap_or("unknown"),
                "timescale": t.timescale.get(),
                "duration": t.duration,
                "sampleCount": counts.get(&t.track_id).copied().unwrap_or(0),
                "totalBytes": totals.get(&t.track_id).copied().unwrap_or(0),
            })
        })
        .collect();
    Ok(Mp4Walk { tracks_json, samples })
}

/// `__wjs_media_mp4info(bytes)` → `{tracks:[...]}` JSON。
pub unsafe extern "C" fn media_mp4info(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let what = "WinterJS.media.mp4Info";
    let Some(bytes) = arg_bytes(&mut cx, &frame, 0, what) else {
        return false;
    };
    if bytes.is_empty() {
        report_error(&mut cx, &format!("TypeError: {what} requires non-empty bytes"));
        return false;
    }
    match walk_mp4(&bytes) {
        Ok(w) => {
            let json = serde_json::json!({ "tracks": w.tracks_json }).to_string();
            set_rval_string(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            let is_range = e.contains("exceeds");
            report_error(&mut cx, &format!("{}: {e}", if is_range { "RangeError" } else { "TypeError" }));
            false
        }
    }
}

/// `__wjs_media_mp4samples(bytes, trackId?, limit?)` → `[{index,timestamp,duration,size}]` JSON。
pub unsafe extern "C" fn media_mp4samples(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let what = "WinterJS.media.mp4Samples";
    let Some(bytes) = arg_bytes(&mut cx, &frame, 0, what) else {
        return false;
    };
    if bytes.is_empty() {
        report_error(&mut cx, &format!("TypeError: {what} requires non-empty bytes"));
        return false;
    }
    let track: Option<u32> = if frame.argc() > 1 {
        let s = value_to_string(&mut cx, frame.arg(1));
        if s.is_empty() || s == "undefined" {
            None
        } else {
            Some(s.parse::<u32>().unwrap_or(u32::MAX))
        }
    } else {
        None
    };
    let limit: usize = if frame.argc() > 2 {
        value_to_string(&mut cx, frame.arg(2)).parse::<usize>().unwrap_or(1000)
    } else {
        1000
    };
    if limit == 0 || limit > 100_000 {
        report_error(&mut cx, &format!("RangeError: {what} limit must be within 1..100000"));
        return false;
    }
    match walk_mp4(&bytes) {
        Ok(w) => {
            let list: Vec<serde_json::Value> = w
                .samples
                .iter()
                .filter(|(tid, ..)| track.is_none_or(|t| *tid == t))
                .take(limit)
                .map(|(tid, ts, dur, size, _off, idx)| {
                    serde_json::json!({
                        "index": idx,
                        "track": tid,
                        "timestamp": ts,
                        "duration": dur,
                        "size": size,
                    })
                })
                .collect();
            set_rval_string(&mut cx, &frame, &serde_json::Value::Array(list).to_string());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: {e}"));
            false
        }
    }
}

/// `__wjs_media_mp4sample(bytes, trackId, index)` → Uint8Array（样本字节）。
pub unsafe extern "C" fn media_mp4sample(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let what = "WinterJS.media.mp4Sample";
    if frame.argc() < 3 {
        report_error(&mut cx, &format!("TypeError: {what} requires (bytes, trackId, index)"));
        return false;
    }
    let Some(bytes) = arg_bytes(&mut cx, &frame, 0, what) else {
        return false;
    };
    if bytes.is_empty() {
        report_error(&mut cx, &format!("TypeError: {what} requires non-empty bytes"));
        return false;
    }
    let track = value_to_string(&mut cx, frame.arg(1)).parse::<u32>().unwrap_or(u32::MAX);
    let index = value_to_string(&mut cx, frame.arg(2)).parse::<usize>().unwrap_or(usize::MAX);
    match walk_mp4(&bytes) {
        Ok(w) => {
            let hit = w
                .samples
                .iter()
                .filter(|(tid, ..)| *tid == track)
                .nth(index);
            match hit {
                Some((_, _, _, size, off, _)) => {
                    let (s, e) = (*off as usize, (*off as usize).saturating_add(*size));
                    if e > bytes.len() || s >= e {
                        report_error(&mut cx, &format!("RangeError: {what} sample out of range"));
                        return false;
                    }
                    if !set_rval_bytes(&mut cx, &frame, &bytes[s..e]) {
                        return false;
                    }
                    true
                }
                None => {
                    report_error(&mut cx, &format!("RangeError: {what} no such sample"));
                    false
                }
            }
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: {e}"));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_ext_map() {
        assert_eq!(audio_ext("mp3"), Some("mp3"));
        assert_eq!(audio_ext("m4a"), Some("m4a"));
        assert_eq!(audio_ext("ogg"), Some("ogg"));
        assert_eq!(audio_ext("flac"), Some("flac"));
        assert_eq!(audio_ext("xyz"), None);
        assert_eq!(audio_ext(""), None);
    }

    #[test]
    fn yuv420_shapes() {
        // 2x2 红：Y 按 BT.601，U/V 落中灰附近。
        let px = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255];
        let (y, u, v) = rgba_to_yuv420(&px, 2, 2);
        assert_eq!(y.len(), 4);
        assert_eq!(u.len(), 1);
        assert_eq!(v.len(), 1);
        assert!((y[0] as i32 - 76).abs() <= 1, "{y:?}");
    }

    #[test]
    fn ivf_header_shape() {
        let ivf = mux_ivf(&[vec![1, 2, 3]], 16, 16, 30, 1);
        assert_eq!(&ivf[0..4], b"DKIF");
        assert_eq!(&ivf[8..12], b"AV01");
        assert_eq!(ivf.len(), 32 + 12 + 3);
    }

    #[test]
    fn sniff_audio_magics() {
        assert_eq!(sniff_audio(b"fLaC...."), Some("flac"));
        assert_eq!(sniff_audio(b"OggS...."), Some("ogg"));
        assert_eq!(sniff_audio(b"RIFF...."), Some("wav"));
        assert_eq!(sniff_audio(b"FORM...."), Some("aiff"));
        assert_eq!(sniff_audio(&[0, 0, 0, 0, b'f', b't', b'y', b'p', b'm', b'p', b'4', b'2']), Some("mp4"));
        assert_eq!(sniff_audio(&[0xff, 0xfb, 0, 0]), Some("mp3"));
        assert_eq!(sniff_audio(b"ID3....."), Some("mp3"));
        assert_eq!(sniff_audio(b"no"), None);
    }

    #[test]
    fn codec_names() {
        use symphonia::core::codecs::audio::well_known;
        assert_eq!(codec_name(well_known::CODEC_ID_MP3), "mp3");
        assert_eq!(codec_name(well_known::CODEC_ID_AAC), "aac");
        assert_eq!(codec_name(well_known::CODEC_ID_FLAC), "flac");
        assert_eq!(codec_name(well_known::CODEC_ID_VORBIS), "vorbis");
        assert_eq!(codec_name(well_known::CODEC_ID_PCM_S16LE), "pcm");
    }

    #[test]
    fn mp4_fixture_walks() {
        // 上游自带 beep-flac（Apache-2.0，进仓见 tests/fixtures/media/）。
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/media/beep-flac-audio.mp4");
        let bytes = std::fs::read(path).expect("fixture present");
        let w = walk_mp4(&bytes).expect("fixture demuxes");
        assert_eq!(w.tracks_json.len(), 1);
        assert_eq!(w.tracks_json[0]["codec"], serde_json::Value::String("flac".to_string()));
        assert!(!w.samples.is_empty());
        let (tid, _, _, size, off, _) = w.samples[0];
        assert_eq!(tid, 1);
        assert!(off as usize + size <= bytes.len());
    }

}
