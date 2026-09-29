//! Web Streams（Readable/Writable/Transform/排队/压缩流）（prelude 分域；拼接顺序见 mod.rs）。
pub const STREAMS_JS: &str = r#"
// ---- Phase 3c-2: streams（纯 prelude 内存实现；默认 reader + BYOB）----
// BYOB 口径：`new ReadableStream({ type: "bytes", ... })` + `getReader({ mode: "byob" })`；
// `read(view)` 按 view 类型回同类前缀视图；`byobRequest.respond/respondWithNewView` 完整；
// 简化（文档记录）：done 时 value 为 undefined（非空视图）；respond 非元素对齐截断丢余量；
// 无 autoAllocateChunkSize；default reader 照常读字节流（Uint8Array 块）。
const __wjs2_rsState = new WeakMap();
function __wjs2_rsViewPrefix(r, n) {
  // 取 view 前 n 字节（元素对齐由调用方保证；DataView 按字节）。
  if (r.viewCtor === DataView) return new DataView(r.view.buffer, r.view.byteOffset, n);
  return new r.viewCtor(r.view.buffer, r.view.byteOffset, n / r.viewElem);
}
function __wjs2_rsByobFill(st) {
  // 用 byteQ 填充排队的 BYOB 读；closed/出错同样结算
  while (st.byobReads.length) {
    const r = st.byobReads[0];
    try { new Uint8Array(r.view.buffer, 0, 0); }
    catch { st.byobReads.shift(); r.reject(new TypeError("BYOB view is detached")); continue; }
    if (st.error !== undefined) { st.byobReads.shift(); r.reject(st.error); continue; }
    if (st.byteLen === 0) {
      if (st.closed) { st.byobReads.shift(); r.resolve({ value: undefined, done: true }); continue; }
      break;
    }
    const n = Math.min(r.view.byteLength, st.byteLen);
    const take = n - (n % r.viewElem);
    if (take === 0) break;
    let off = take;
    for (const q of st.byteQ) {
      if (off === 0) break;
      const c = Math.min(q.length - q._off, off);
      new Uint8Array(r.view.buffer, r.view.byteOffset + (take - off), c).set(q.subarray(q._off, q._off + c));
      q._off += c; off -= c;
    }
    while (st.byteQ.length && st.byteQ[0]._off >= st.byteQ[0].length) st.byteQ.shift();
    st.byteLen -= take;
    st.byobReads.shift();
    r.resolve({ value: __wjs2_rsViewPrefix(r, take), done: false });
  }
}
function __wjs2_rsByobReq(st) {
  const r = st.byobReads[0];
  if (!r) return null;
  return {
    get view() { return r.view; },
    respond(n) {
      n = Number(n);
      if (!Number.isInteger(n) || n < 0 || n > r.view.byteLength) throw new RangeError("respond: bad byte count");
      if (st.byobReads[0] !== r || st.byobReq === null) throw new TypeError("respond: request is not active");
      st.byobReads.shift();
      st.byobReq = null;
      // 非元素对齐截断（余量丢弃，见头注）
      const take = n - (n % r.viewElem);
      r.resolve({ value: __wjs2_rsViewPrefix(r, take), done: false });
      __wjs2_rsPump(st);
    },
    respondWithNewView(v) {
      if (!ArrayBuffer.isView(v)) throw new TypeError("respondWithNewView needs a view");
      if (st.byobReads[0] !== r || st.byobReq === null) throw new TypeError("respondWithNewView: request is not active");
      r.view = v; r.viewCtor = v.constructor; r.viewElem = v.BYTES_PER_ELEMENT ?? 1;
    },
  };
}
function __wjs2_rsByteToQueue(st) {
  // default reader 读字节流：整块搬运（有 _off 余量的半块留给 BYOB，不拆）
  while (st.byteQ.length && st.byteQ[0]._off === 0) {
    const q = st.byteQ.shift();
    st.byteLen -= q.length;
    st.queue.push(q);
  }
}
function __wjs2_rsPull(st) {
  if (!st.reader || st.closed || st.error !== undefined || st.pulling) return;
  // pull 触发面（防微任务空转饿死事件循环，见 §4.27 追补）：
  // 只在新需求到达（read 推入等待）或有进展且需求还在（pump 尾）时调；
  // 无 pull 方法的源 + 挂起的读，eager 重拉即无限微任务链。
  st.pulling = true;
  st.pullProgress = false;
  // BYOB 读排队时带 byobRequest 进 pull（source 可直接写 view + respond）
  if (st.isBytes && st.byobReads.length && !st.byobReq) st.byobReq = __wjs2_rsByobReq(st);
  try {
    const r = st.source.pull ? st.source.pull(st.controller) : undefined;
    Promise.resolve(r).then(() => { st.pulling = false; st.byobReq = null; __wjs2_rsPump(st); }, (e) => {
      st.pulling = false; st.byobReq = null; __wjs2_rsError(st, e);
    });
  } catch (e) { st.pulling = false; st.byobReq = null; __wjs2_rsError(st, e); }
}
function __wjs2_rsPump(st) {
  __wjs2_rsByobFill(st);
  // default reader 读字节流：仅当有读等待（wantValue）才整块搬运；
  // closed 等待不搬，否则会饿死后来的 BYOB 读
  if (st.isBytes && st.pending.some((p) => p.wantValue)) __wjs2_rsByteToQueue(st);
  while (st.pending.length && (st.queue.length || st.closed || st.error !== undefined)) {
    const { resolve, reject } = st.pending.shift();
    if (st.error !== undefined) { reject(st.error); continue; }
    if (st.queue.length) {
      const v = st.queue.shift();
      resolve({ value: v, done: false });
    } else { resolve({ value: undefined, done: true }); }
  }
  // pump 尾再拉：仅当需求还在且本轮有进展（enqueue/close/error 置 pullProgress）；
  // 干 pull（无进展）不再重拉——新需求到达时 read() 会拉。
  if (!st.closed && st.error === undefined && !st.pulling) {
    const demand = st.byobReads.length > 0 || st.pending.some((p) => p.wantValue);
    if (demand && st.pullProgress) { st.pullProgress = false; __wjs2_rsPull(st); }
  }
}
function __wjs2_rsError(st, e) {
  if (st.closed || st.error !== undefined) return;
  st.error = e;
  st.queue.length = 0;
  st.byteQ.length = 0; st.byteLen = 0;
  __wjs2_rsByobFill(st);
  __wjs2_rsPump(st);
}
function __wjs2_rsController(stream, st) {
  if (st.isBytes) {
    return {
      get desiredSize() { return st.hwm - st.byteLen; },
      get byobRequest() { return st.byobReq; },
      enqueue(chunk) {
        if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
        if (!ArrayBuffer.isView(chunk)) throw new TypeError("byte stream chunk must be a view");
        const v = new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
        v._off = 0;
        st.byteQ.push(v);
        st.byteLen += v.length;
        st.pullProgress = true;
        __wjs2_rsPump(st);
      },
      close() {
        if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
        st.closed = true;
        st.pullProgress = true;
        __wjs2_rsPump(st);
      },
      error(e) { __wjs2_rsError(st, e); },
    };
  }
  return {
    get desiredSize() { return st.hwm - st.queue.length; },
    enqueue(chunk) {
      if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
      if (chunk === undefined) throw new TypeError("chunk must not be undefined");
      st.queue.push(chunk);
      st.pullProgress = true;
      __wjs2_rsPump(st);
    },
    close() {
      if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
      st.closed = true;
      st.pullProgress = true;
      __wjs2_rsPump(st);
    },
    error(e) { __wjs2_rsError(st, e); },
  };
}
globalThis.ReadableStream = class ReadableStream {
  constructor(underlyingSource = {}, strategy) {
    const hwm = strategy && strategy.highWaterMark !== undefined ? Number(strategy.highWaterMark) : 1;
    const utype = underlyingSource ? underlyingSource.type : undefined;
    if (utype !== undefined && utype !== "bytes") throw new TypeError("ReadableStream type must be 'bytes'");
    const st = {
      queue: [], pending: [], closed: false, error: undefined,
      reader: null, pulling: false, pullProgress: false, hwm: Number.isNaN(hwm) ? 1 : hwm,
      source: underlyingSource, controller: null,
      isBytes: utype === "bytes", byteQ: [], byteLen: 0, byobReads: [], byobReq: null,
    };
    st.controller = __wjs2_rsController(this, st);
    __wjs2_rsState.set(this, st);
    try {
      const r = underlyingSource.start ? underlyingSource.start(st.controller) : undefined;
      Promise.resolve(r).catch((e) => __wjs2_rsError(st, e));
    } catch (e) { __wjs2_rsError(st, e); }
  }
  get locked() { return !!__wjs2_rsState.get(this).reader; }
  cancel(reason) {
    const st = __wjs2_rsState.get(this);
    if (st.reader) throw new TypeError("stream is locked");
    st.queue.length = 0; st.closed = true;
    st.byteQ.length = 0; st.byteLen = 0;
    const c = st.source.cancel ? st.source.cancel(reason) : undefined;
    __wjs2_rsPump(st);
    return Promise.resolve(c).then(() => undefined);
  }
  getReader(options) {
    const st = __wjs2_rsState.get(this);
    if (st.reader) throw new TypeError("stream is locked");
    const mode = options ? options.mode : undefined;
    if (mode !== undefined && mode !== "byob") throw new TypeError(`Unknown reader mode '${mode}'`);
    const stream = this;
    if (mode === "byob") {
      if (!st.isBytes) throw new TypeError("getReader({ mode: 'byob' }) needs a byte stream");
      const reader = {
        get closed() {
          return new Promise((resolve, reject) => {
            if (st.error !== undefined) reject(st.error);
            else if (st.closed && !st.byteLen) resolve(undefined);
            else st.pending.push({ resolve: () => resolve(undefined), reject, wantValue: false });
          });
        },
        read(view) {
          return new Promise((resolve, reject) => {
            if (!ArrayBuffer.isView(view)) { reject(new TypeError("BYOB read needs a view")); return; }
            try { new Uint8Array(view.buffer, 0, 0); }
            catch { reject(new TypeError("BYOB view is detached")); return; }
            if (view.byteLength === 0) { reject(new TypeError("BYOB view must not be empty")); return; }
            if (st.error !== undefined) { reject(st.error); return; }
            st.byobReads.push({
              view, viewCtor: view.constructor, viewElem: view.BYTES_PER_ELEMENT ?? 1,
              resolve, reject,
            });
            __wjs2_rsByobFill(st);
            __wjs2_rsPull(st);
          });
        },
        releaseLock() { if (st.reader === reader) st.reader = null; },
        cancel(reason) {
          st.byteQ.length = 0; st.byteLen = 0; st.closed = true;
          const c = st.source.cancel ? st.source.cancel(reason) : undefined;
          if (st.reader === reader) st.reader = null;
          __wjs2_rsPump(st);
          return Promise.resolve(c).then(() => undefined);
        },
      };
      st.reader = reader;
      return reader;
    }
    const reader = {
      get closed() {
        return new Promise((resolve, reject) => {
          if (st.error !== undefined) reject(st.error);
          else if (st.closed && !st.queue.length) resolve(undefined);
          else st.pending.push({ resolve: () => resolve(undefined), reject, wantValue: false });
        });
      },
      read() {
        return new Promise((resolve, reject) => {
          if (st.error !== undefined) { reject(st.error); return; }
          if (st.isBytes) __wjs2_rsByteToQueue(st);
          if (st.queue.length) {
            const v = st.queue.shift();
            resolve({ value: v, done: false });
            __wjs2_rsPull(st);
            return;
          }
          if (st.closed) { resolve({ value: undefined, done: true }); return; }
          st.pending.push({ resolve, reject, wantValue: true });
          __wjs2_rsPull(st);
        });
      },
      releaseLock() { if (st.reader === reader) st.reader = null; },
      cancel(reason) {
        st.queue.length = 0; st.closed = true;
        st.byteQ.length = 0; st.byteLen = 0;
        const c = st.source.cancel ? st.source.cancel(reason) : undefined;
        if (st.reader === reader) st.reader = null;
        __wjs2_rsPump(st);
        return Promise.resolve(c).then(() => undefined);
      },
    };
    st.reader = reader;
    return reader;
  }
  pipeThrough(t, options) {
    this.pipeTo(t.writable, options);
    return t.readable;
  }
  async pipeTo(dest, options = {}) {
    const preventClose = !!(options && options.preventClose);
    const reader = this.getReader();
    const writer = dest.getWriter();
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        await writer.write(value);
      }
      if (!preventClose) await writer.close();
    } finally {
      reader.releaseLock();
      writer.releaseLock();
    }
  }
  tee() {
    const st = __wjs2_rsState.get(this);
    if (st.reader) throw new TypeError("stream is locked");
    // 简化 tee：顺序读源，两分支各收一份（引用共享；无背压，见文档）。
    const q1 = [], q2 = [];
    const mkBranch = (q) => new ReadableStream({
      pull(c) {
        if (q.length) { c.enqueue(q.shift()); return; }
        if (done) { c.close(); return; }
        if (failed !== undefined) { c.error(failed); return; }
        waiters.push(() => {
          if (q.length) { try { c.enqueue(q.shift()); } catch {} return; }
          if (done) { try { c.close(); } catch {} return; }
          if (failed !== undefined) { try { c.error(failed); } catch {} }
        });
      },
      cancel() {},
    });
    let done = false, failed;
    const waiters = [];
    const wake = () => { for (const w of waiters.splice(0)) w(); };
    const r1 = mkBranch(q1), r2 = mkBranch(q2);
    const src = this.getReader();
    st.reader = null;
    const loop = () => src.read().then(({ value, done: d }) => {
      if (d) { done = true; wake(); return; }
      q1.push(value); q2.push(value);
      wake();
      loop();
    }, (e) => { failed = e; wake(); });
    loop();
    return [r1, r2];
  }
  async *[Symbol.asyncIterator]() {
    const reader = this.getReader();
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return;
        yield value;
      }
    } finally { reader.releaseLock(); }
  }
};
const __wjs2_wsState = new WeakMap();
globalThis.WritableStream = class WritableStream {
  constructor(underlyingSink = {}, strategy) {
    const hwm = strategy && strategy.highWaterMark !== undefined ? Number(strategy.highWaterMark) : 1;
    const st = {
      queue: [], writing: false, closed: false, errored: false, error: undefined,
      writer: null, hwm: Number.isNaN(hwm) ? 1 : hwm, sink: underlyingSink,
      closeReq: null,
    };
    __wjs2_wsState.set(this, st);
    const stream = this;
    st.controller = { error(e) { __wjs2_wsError(stream, e); } };
    try {
      const r = underlyingSink.start ? underlyingSink.start(st.controller) : undefined;
      Promise.resolve(r).catch((e) => __wjs2_wsError(this, e));
    } catch (e) { __wjs2_wsError(this, e); }
  }
  get locked() { return !!__wjs2_wsState.get(this).writer; }
  abort(reason) {
    const st = __wjs2_wsState.get(this);
    if (st.writer) throw new TypeError("stream is locked");
    const a = st.sink.abort ? st.sink.abort(reason) : undefined;
    __wjs2_wsError(this, reason);
    return Promise.resolve(a).then(() => undefined);
  }
  close() {
    const st = __wjs2_wsState.get(this);
    if (st.writer) throw new TypeError("stream is locked");
    return __wjs2_wsCloseReq(this);
  }
  getWriter() {
    const st = __wjs2_wsState.get(this);
    if (st.writer) throw new TypeError("stream is locked");
    const stream = this;
    const writer = {
      get closed() {
        return new Promise((resolve, reject) => {
          if (st.errored) reject(st.error);
          else if (st.closed) resolve(undefined);
          else st.closeWaiters.push({ resolve, reject });
        });
      },
      get desiredSize() { return st.hwm - st.queue.length; },
      get ready() { return Promise.resolve(); },
      write(chunk) {
        if (chunk === undefined) return Promise.reject(new TypeError("chunk must not be undefined"));
        if (st.errored) return Promise.reject(st.error);
        if (st.closed) return Promise.reject(new TypeError("stream is closed"));
        return new Promise((resolve, reject) => {
          st.queue.push({ chunk, resolve, reject });
          __wjs2_wsPump(stream);
        });
      },
      close() { return __wjs2_wsCloseReq(stream); },
      abort(reason) {
        const a = st.sink.abort ? st.sink.abort(reason) : undefined;
        __wjs2_wsError(stream, reason);
        return Promise.resolve(a).then(() => undefined);
      },
      releaseLock() { if (st.writer === writer) st.writer = null; },
    };
    st.closeWaiters = st.closeWaiters || [];
    st.writer = writer;
    return writer;
  }
};
function __wjs2_wsError(stream, e) {
  const st = __wjs2_wsState.get(stream);
  if (st.errored) return;
  st.errored = true;
  st.error = e;
  for (const q of st.queue.splice(0)) q.reject(e);
  if (st.closeReq) { const c = st.closeReq; st.closeReq = null; c.reject(e); }
  for (const w of (st.closeWaiters || []).splice(0)) w.reject(e);
}
function __wjs2_wsCloseReq(stream) {
  const st = __wjs2_wsState.get(stream);
  return new Promise((resolve, reject) => { st.closeReq = { resolve, reject }; __wjs2_wsPump(stream); });
}
function __wjs2_wsPump(stream) {
  const st = __wjs2_wsState.get(stream);
  if (st.writing || st.errored) return;
  const item = st.queue.shift();
  if (!item) {
    if (st.closeReq && !st.writing) {
      const c = st.closeReq; st.closeReq = null;
      const done = () => { st.closed = true; c.resolve(undefined); for (const w of (st.closeWaiters || []).splice(0)) w.resolve(undefined); };
      try {
        Promise.resolve(st.sink.close ? st.sink.close() : undefined).then(done, (e) => { __wjs2_wsError(stream, e); });
      } catch (e) { __wjs2_wsError(stream, e); }
    }
    return;
  }
  st.writing = true;
  try {
    Promise.resolve(st.sink.write ? st.sink.write(item.chunk, st.controller) : undefined).then(
      () => { st.writing = false; item.resolve(undefined); __wjs2_wsPump(stream); },
      (e) => { st.writing = false; item.reject(e); __wjs2_wsError(stream, e); __wjs2_wsPump(stream); },
    );
  } catch (e) { st.writing = false; item.reject(e); __wjs2_wsError(stream, e); }
}
// ---- QueuingStrategy 双类（Web 全局；WHATWG streams。真机 26 口径：highWaterMark
// 是原型 getter 非自有键、size 是可枚举 accessor 且全实例共享同一函数、构造器
// ARG_TYPE 文案 + highWaterMark 缺失 ERR_MISSING_OPTION；size 对 undefined/null
// 抛 TypeError、其余回 chunk.byteLength（原始值/普通对象 → undefined）；10f）----
const __wjs2_qsState = new WeakMap();
function __wjs2_qsArg(init) {
  if (init === null) return "null";
  if (typeof init === "string") return `type string ('${init}')`;
  if (typeof init === "number") return `type number (${init})`;
  if (typeof init === "function") return "type function";
  return `type ${typeof init}`;
}
const __wjs2_qsSizeBL = (chunk) => {
  if (chunk === undefined || chunk === null) throw new TypeError("chunk must not be undefined or null");
  return chunk.byteLength;
};
const __wjs2_qsSizeCount = () => 1;
globalThis.ByteLengthQueuingStrategy = class ByteLengthQueuingStrategy {
  constructor(init) {
    if (init === null || (typeof init !== "object" && typeof init !== "function")) {
      const err = new TypeError(`The "init" argument must be of type object. Received ${__wjs2_qsArg(init)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (init.highWaterMark === undefined) {
      const err = new TypeError("init.highWaterMark is required");
      err.code = "ERR_MISSING_OPTION";
      throw err;
    }
    __wjs2_qsState.set(this, init.highWaterMark);
  }
  get highWaterMark() { return __wjs2_qsState.get(this); }
  get size() { return __wjs2_qsSizeBL; }
};
globalThis.CountQueuingStrategy = class CountQueuingStrategy {
  constructor(init) {
    if (init === null || (typeof init !== "object" && typeof init !== "function")) {
      const err = new TypeError(`The "init" argument must be of type object. Received ${__wjs2_qsArg(init)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (init.highWaterMark === undefined) {
      const err = new TypeError("init.highWaterMark is required");
      err.code = "ERR_MISSING_OPTION";
      throw err;
    }
    __wjs2_qsState.set(this, init.highWaterMark);
  }
  get highWaterMark() { return __wjs2_qsState.get(this); }
  get size() { return __wjs2_qsSizeCount; }
};
globalThis.TransformStream = class TransformStream {
  constructor(transformer = {}, writableStrategy, readableStrategy) {
    let rsCtrl;
    const readable = new ReadableStream({
      start(c) { rsCtrl = c; },
    }, readableStrategy);
    const writable = new WritableStream({
      write: (chunk, c) => transformer.transform
        ? transformer.transform(chunk, {
            enqueue: (out) => rsCtrl.enqueue(out),
            get desiredSize() { return rsCtrl.desiredSize; },
            terminate() { rsCtrl.close(); },
          })
        : rsCtrl.enqueue(chunk),
      close: () => {
        if (transformer.flush) {
          return Promise.resolve(transformer.flush({
            enqueue: (out) => rsCtrl.enqueue(out),
            get desiredSize() { return rsCtrl.desiredSize; },
            terminate() { rsCtrl.close(); },
          })).then(() => rsCtrl.close());
        }
        rsCtrl.close();
      },
      abort: (r) => rsCtrl.error(r),
    }, writableStrategy);
    try {
      const r = transformer.start ? transformer.start({
        enqueue: (out) => rsCtrl.enqueue(out),
        get desiredSize() { return rsCtrl.desiredSize; },
        terminate() { rsCtrl.close(); },
      }) : undefined;
      Promise.resolve(r).catch((e) => rsCtrl.error(e));
    } catch (e) { rsCtrl.error(e); }
    this.readable = readable;
    this.writable = writable;
  }
};
// ---- CompressionStream / DecompressionStream（10f 欠账 G9-3：Web 全局 +
// node:stream/web；真机 26.8.2 对拍——不继承 TransformStream（proto 链独立，
// instanceof TransformStream false）、format 枚举校验 TypeError（文案逐字）、
// 解压侧尾垃圾/截断错误落 readable（pipeThrough 场景 Array.fromAsync 可见
// reject），junk = TypeError ERR_TRAILING_JUNK_AFTER_STREAM_END（node:zlib
// 引擎 junk 码同文复用））----
const __wjs2_csState = new WeakMap();
// Web 扩展 `zstd`（ruzstd 底座；编码恒 Fastest，见 node/zlib.rs 头注）：
// 引擎侧 ZstdEnc=10/ZstdDec=11 已就绪，此处只加格式表（finish 档走 flag 2）。
const __CS_KINDS = { gzip: 2, deflate: 0, "deflate-raw": 1, brotli: 8, zstd: 10 };
const __DS_KINDS = { gzip: 6, deflate: 3, "deflate-raw": 4, brotli: 9, zstd: 11 };
function __wjs2_csFinishFlag(kind) { return kind <= 7 ? 4 : 2; }
function __wjs2_makeCSClass(name, kinds, reject) {
  const cls = class {
    constructor(format) {
      if (new.target === undefined) {
        throw new TypeError(`Failed to construct '${name}': Please use the 'new' operator, this DOM object constructor cannot be called as a function.`);
      }
      const fmt = String(format);
      const kind = kinds[fmt];
      if (kind === undefined) {
        throw new TypeError(`Failed to construct '${name}': 1st argument '${fmt}' is not a valid enum value of type CompressionFormat.`);
      }
      // brotli 压缩档位对齐真机默认 11；zlib 族 -1（引擎 clamp 默认）。
      const lv = kind === 8 ? 11 : -1;
      const id = __wjs2_zlib_stream_new(kind, lv, null, -1, reject ? 1 : 0);
      let ctrl = null;
      let closed = false; // readable 已 close/error
      let freed = false;
      let done = false;   // 引擎已 StreamEnd（后续写入即尾垃圾）
      const free = () => { if (!freed) { freed = true; __wjs2_zlib_stream_free(id); } };
      const fail = (err) => { if (!closed) { closed = true; ctrl.error(err); } };
      const feed = (u8, flag) => {
        const r = JSON.parse(__wjs2_zlib_stream_feed(id, u8 ?? null, flag));
        if (r.code !== undefined) {
          const err = r.code === "ERR_TRAILING_JUNK_AFTER_STREAM_END"
            ? new TypeError(r.msg) : new Error(r.msg);
          err.code = r.code;
          fail(err);
          return false;
        }
        done = r.d === true;
        const out = __wjs2_zlib_stream_out(id);
        if (!closed && out.length) ctrl.enqueue(out);
        return true;
      };
      const readable = new ReadableStream({
        start(c) { ctrl = c; },
        cancel() { closed = true; free(); },
      });
      const toU8 = (chunk) => {
        if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
        if (ArrayBuffer.isView(chunk)) return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
        throw new TypeError(`Failed to construct '${name}': The provided value is not of type '(ArrayBuffer or ArrayBufferView)'`);
      };
      const writable = new WritableStream({
        write(chunk) {
          if (closed || freed) return;
          const u8 = toU8(chunk);
          if (done) {
            // done 后写入 = 尾垃圾（type-error 套件 [valid, empty] case；
            // 错误落 readable 而非 write 拒绝——pipeTo 语义下后者走 cancel）
            const err = new TypeError("Trailing junk found after the end of the compressed stream");
            err.code = "ERR_TRAILING_JUNK_AFTER_STREAM_END";
            fail(err);
            return;
          }
          feed(u8, 0);
        },
        close() {
          if (closed || freed) return;
          const ok = feed(null, __wjs2_csFinishFlag(kind));
          if (ok) { closed = true; ctrl.close(); }
          free();
        },
        abort(reason) { if (!closed) { closed = true; ctrl.error(reason); } free(); },
      });
      __wjs2_csState.set(this, { readable, writable });
    }
    get readable() { return __wjs2_csState.get(this).readable; }
    get writable() { return __wjs2_csState.get(this).writable; }
  };
  Object.defineProperty(cls.prototype, Symbol.toStringTag, { value: name, configurable: true });
  return cls;
}
globalThis.CompressionStream = __wjs2_makeCSClass("CompressionStream", __CS_KINDS, false);
globalThis.DecompressionStream = __wjs2_makeCSClass("DecompressionStream", __DS_KINDS, true);
"#;
