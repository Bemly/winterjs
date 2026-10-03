//! `node:internal/streams/iter/zlib_binding`——transform 用的缓冲式 native 句柄 shim（R3a）。
//!
//! 背景：`internal/streams/iter/transform` 经裸 `internalBinding('zlib')` 取
//! C++ 流式句柄（`Zlib/BrotliEncoder/ZstdCompress` + `init/write/writeSync/
//! close` 协议）；本仓无此绑定层。shim 实现同协议、压缩经 `node:zlib` 同步
//! 引擎（轮子算法不动，见 AGENTS §0.5）：`write()` 攒输入、`FINISH` 到时整包
//! 同步压、经 `writeState` 分次吐出、`processCallback` 异步回（threadpool 语义
//! 近似）。偏差记档：流粒度/背压/内存与真机不同；`dictionary` 透传引擎，
//! 不支持即引擎报错；`ZSTD_e_flush` 中间 flush 按 PROCESS 攒（无输出）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
import zlib from 'node:zlib';

const {
  gzipSync, gunzipSync, deflateSync, inflateSync,
  brotliCompressSync, brotliDecompressSync,
  zstdCompressSync, zstdDecompressSync,
  constants: zconsts,
} = zlib;

// 绑定层模式/冲刷常量（node 真机值；本仓 zlib.js constants 补齐前此处兜底）。
const constants = {
  ...zconsts,
  DEFLATE: 1, INFLATE: 2, GZIP: 3, GUNZIP: 4,
  BROTLI_DECODE: 8, BROTLI_ENCODE: 9,
  ZSTD_COMPRESS: 10, ZSTD_DECOMPRESS: 11,
  Z_MIN_CHUNK: 64, Z_MIN_LEVEL: -1, Z_MAX_LEVEL: 9,
};

// FINISH 判定：Z_FINISH(4) / BROTLI_OPERATION_FINISH(2) / ZSTD_e_end(2)。
// 其余（0/1 系 PROCESS/FLUSH）只攒不吐。
function isFinishFlush(flush) {
  return flush === 4 || flush === 2;
}

function concatBytes(parts, total) {
  const out = new Uint8Array(total);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

class BufHandle {
  constructor(run, opts) {
    this._run = run;
    this._opts = opts || {};
    this._parts = [];
    this._inLen = 0;
    this._out = null;
    this._outOff = 0;
    this._done = false;
    this._ws = null;
    this._cb = null;
    this.onerror = null;
  }
  init(writeState, processCallback) {
    this._ws = writeState;
    this._cb = processCallback;
  }
  _ingest(input, inOff, inLen) {
    if (inLen > 0) {
      const s = input.subarray ? input.subarray(inOff, inOff + inLen) : input.slice(inOff, inOff + inLen);
      this._parts.push(new Uint8Array(s));
      this._inLen += inLen;
    }
  }
  _finalize() {
    if (this._done) return;
    this._done = true;
    const input = concatBytes(this._parts, this._inLen);
    this._parts = [];
    try {
      const r = this._run(input, this._opts);
      this._out = r instanceof Uint8Array ? r : new Uint8Array(r || []);
    } catch (e) {
      const cb = this.onerror;
      this._out = new Uint8Array(0);
      if (typeof cb === 'function') {
        cb(String((e && e.message) || e), (e && e.errno) ?? -1, (e && e.code) || 'ERR_GENERIC');
      }
    }
    this._outOff = 0;
  }
  _step(flush, input, inOff, inLen, out, outOff, outLen) {
    this._ingest(input, inOff, inLen);
    if (isFinishFlush(flush)) this._finalize();
    let copied = 0;
    if (this._out && this._outOff < this._out.length) {
      copied = Math.min(outLen, this._out.length - this._outOff);
      out.set(this._out.subarray(this._outOff, this._outOff + copied), outOff);
      this._outOff += copied;
    }
    // writeState：[0]=剩余输出空间，[1]=未消费输入（恒全消费，即 0）。
    if (this._ws) {
      this._ws[0] = outLen - copied;
      this._ws[1] = 0;
    }
  }
  write(flush, input, inOff, inLen, out, outOff, outLen) {
    this._step(flush, input, inOff, inLen, out, outOff, outLen);
    // threadpool 异步语义近似：回调恒下一轮（调用方循环经 writeState 推进）。
    const cb = this._cb;
    queueMicrotask(() => { if (typeof cb === 'function') { try { cb(); } catch {} } });
  }
  writeSync(flush, input, inOff, inLen, out, outOff, outLen) {
    this._step(flush, input, inOff, inLen, out, outOff, outLen);
  }
  close() {
    this._parts = [];
    this._out = null;
  }
}

class Zlib extends BufHandle {
  constructor(mode) {
    super((input, opts) => {
      if (mode === constants.GZIP) return gzipSync(input, opts);
      if (mode === constants.GUNZIP) return gunzipSync(input, opts);
      if (mode === constants.DEFLATE) return deflateSync(input, opts);
      return inflateSync(input, opts);
    }, {});
    this._mode = mode;
  }
  init(windowBits, level, memLevel, strategy, writeState, processCallback, dictionary) {
    if (typeof level === 'number') this._opts.level = level;
    if (dictionary !== undefined) this._opts.dictionary = dictionary;
    super.init(writeState, processCallback);
  }
}

class BrotliEncoder extends BufHandle {
  constructor(mode) {
    super((input, opts) => brotliCompressSync(input, opts), {});
    void mode;
  }
  init(paramsArray, writeState, processCallback, dictionary) {
    const q = paramsArray ? paramsArray[constants.BROTLI_PARAM_QUALITY] : undefined;
    if (typeof q === 'number' && q >= 0) this._opts.quality = q;
    if (dictionary !== undefined) this._opts.dictionary = dictionary;
    super.init(writeState, processCallback);
  }
}

class BrotliDecoder extends BufHandle {
  constructor(mode) {
    super((input, opts) => brotliDecompressSync(input, opts), {});
    void mode;
  }
  init(paramsArray, writeState, processCallback, dictionary) {
    if (dictionary !== undefined) this._opts.dictionary = dictionary;
    super.init(writeState, processCallback);
  }
}

class ZstdCompress extends BufHandle {
  constructor() {
    super((input, opts) => zstdCompressSync(input, opts), {});
  }
  init(initArray, pledgedSrcSize, writeState, processCallback, dictionary) {
    if (dictionary !== undefined) this._opts.dictionary = dictionary;
    super.init(writeState, processCallback);
  }
}

class ZstdDecompress extends BufHandle {
  constructor() {
    super((input, opts) => zstdDecompressSync(input, opts), {});
  }
  init(initArray, pledgedSrcSize, writeState, processCallback, dictionary) {
    if (dictionary !== undefined) this._opts.dictionary = dictionary;
    super.init(writeState, processCallback);
  }
}

export { Zlib, BrotliEncoder, BrotliDecoder, ZstdCompress, ZstdDecompress, constants };
export default { Zlib, BrotliEncoder, BrotliDecoder, ZstdCompress, ZstdDecompress, constants };
"#;
