//! prelude part 04 (byte-exact slice; order matters, see prelude/mod.rs).
pub const PART_04: &str = r#"
                  resolve();
                  return;
                }
                try { c.enqueue(chunk); } catch (e) { reject(e); return; }
                resolve();
              },
              (e) => reject(e));
          });
        },
        cancel() {},
      });
    } else {
      const bytes = st.bodyU8 ? st.bodyU8.slice() : new Uint8Array(0);
      st.reqStream = new ReadableStream({
        start(c) { if (bytes.length) c.enqueue(bytes); c.close(); },
      });
    }
  }
  return st.reqStream;
}
// 取全量字节（流式即读完流；快照即原路径；与 `__wjs_respStreamBytes` 同构）。
async function __wjs_reqStreamBytes(req, what) {
  const st = __wjs_reqState.get(req);
  if (st.streamId !== null && st.streamId !== undefined) {
    if (st.bodyUsed) throw new TypeError(`${what}: body already used`);
    st.bodyUsed = true;
    const chunks = [];
    let total = 0;
    for await (const c of __wjs_reqStream(req)) {
      const u8 = c instanceof Uint8Array ? c : new Uint8Array(c);
      chunks.push(u8);
      total += u8.length;
    }
    const out = new Uint8Array(total);
    let off = 0;
    for (const u8 of chunks) { out.set(u8, off); off += u8.length; }
    return out;
  }
  const b = __wjs_takeReqBody(req);
  return b ? b.slice() : new Uint8Array(0);
}
globalThis.Request = class Request {
  constructor(input, init = {}) {
    let url, method = "GET", headers = new Headers(), bodyU8 = null, signal = null;
    if (input instanceof Request) {
      const s = __wjs_reqState.get(input);
      url = s.url; method = s.method;
      for (const [k, v] of s.headers) headers.append(k, v);
      bodyU8 = s.bodyU8 ? s.bodyU8.slice() : null; signal = s.signal;
    } else if (typeof input === "string" || input instanceof URL) {
      url = String(input);
    } else throw new TypeError("Request: unsupported input");
    if (init.method !== undefined) method = String(init.method).toUpperCase();
    if (["CONNECT", "TRACE", "TRACK"].includes(method)) throw new TypeError(`Request: forbidden method ${method}`);
    if (init.headers !== undefined) { headers = new Headers(); __wjs_fillHeaders(headers, init.headers); }
    if (init.body !== undefined && init.body !== null) bodyU8 = __wjs_normBody(init.body, "Request");
    if ((method === "GET" || method === "HEAD") && bodyU8) {
      throw new TypeError("Request with GET/HEAD method cannot have body");
    }
    if (init.signal !== undefined && init.signal !== null) signal = init.signal;
    try { url = String(new URL(url)); } catch { throw new TypeError(`Request: Invalid URL: ${url}`); }
    __wjs_reqState.set(this, { url, method, headers, bodyU8, signal, bodyUsed: false });
  }
  get url() { return __wjs_reqState.get(this).url; }
  get method() { return __wjs_reqState.get(this).method; }
  get headers() { return __wjs_reqState.get(this).headers; }
  get signal() { return __wjs_reqState.get(this).signal; }
  get bodyUsed() { return __wjs_reqState.get(this).bodyUsed; }
  get body() {
    const st = __wjs_reqState.get(this);
    if (st.bodyUsed) return null;
    return __wjs_reqStream(this);
  }
  async text() { return new TextDecoder().decode(await __wjs_reqStreamBytes(this, "Request.text")); }
  async json() { return JSON.parse(await this.text()); }
  async arrayBuffer() { const b = await __wjs_reqStreamBytes(this, "Request.arrayBuffer"); return b.slice().buffer; }
};
globalThis.__wjs_make_response = (metaJson, bodyU8) => {
  const meta = JSON.parse(metaJson);
  const headers = new Headers();
  for (const [k, v] of meta.headers) headers.append(k, v);
  const resp = new Response(null);
  __wjs_respInit(resp, {
    status: meta.status, statusText: meta.statusText, headers,
    url: meta.url, bodyU8: bodyU8 ?? null,
    streamId: meta.streamId === undefined ? null : meta.streamId,
  });
  return resp;
};
globalThis.__wjs_make_fetch_error = (msg) => new Error(String(msg));
// 独立 serve 驱动（plan4 T1）：Rust dispatch 经 `__wjs_serve_on_head` 投递请求
//（id, metaJson, streamId；streamId 与请求 id 同号）。全异步：fetch 决议后排空
// 响应（头同步读 + 体流式推），失败一律 `__wjs_serve_fail` 落 500（未知 id
// 静默成功，见 serve_bridge）。
function __wjs_serve_make_request(url, method, headers, streamId) {
  const req = new Request(url, { method, headers });
  __wjs_reqState.get(req).streamId = streamId;
  return req;
}
globalThis.__wjs_serve_on_head = (id, metaJson, streamId) => {
  let meta;
  try { meta = JSON.parse(metaJson); } catch (e) { __wjs_serve_fail(id, "bad serve head"); return; }
  const headers = new Headers();
  for (const [k, v] of meta.headers) headers.append(k, v);
  const req = __wjs_serve_make_request(meta.url, meta.method, headers, streamId);
  const fn = globalThis.__wjs_serve_fetch;
  if (typeof fn !== "function") { __wjs_serve_fail(id, "serve handler missing fetch"); return; }
  const fail = (e) => __wjs_serve_fail(id, String((e && e.message) || e));
  let out;
  try { out = fn(req); } catch (e) { fail(e); return; }
  Promise.resolve(out).then(
    (resp) => { __wjs_serve_send_resp(id, resp).catch(fail); },
    fail,
  );
};
async function __wjs_serve_send_resp(id, resp) {
  if (!(resp instanceof Response)) throw new TypeError("serve handler must return a Response");
  __wjs_serve_head(id, JSON.stringify({ status: resp.status, headers: [...resp.headers] }));
  const body = resp.body;
  if (body !== null && body !== undefined) {
    for await (const c of body) {
      const u8 = c instanceof Uint8Array ? c : new Uint8Array(c);
      // 分片推送：单次 native 拷贝封顶 64KB，大体走多 Chunk 通道（plan4 §0-2 流式；
      // 快照在构造期已存在，此处只解决传输分片，不碰共享 Response 语义）。
      for (let off = 0; off < u8.length; off += 65536) {
        __wjs_serve_push(id, u8.subarray(off, Math.min(off + 65536, u8.length)));
      }
    }
  }
  __wjs_serve_push(id, null);
}
globalThis.__wjs_make_ws_event = (kind, json, binU8, target) => {
  const meta = JSON.parse(json);
  if (kind === "open") return { type: "open", target, protocol: meta.protocol ?? "" };
  if (kind === "message-text") return { type: "message", target, data: meta.text };
  if (kind === "message-bin") return { type: "message", target, data: binU8.buffer };
  if (kind === "close") {
    return { type: "close", target, code: meta.code, reason: meta.reason, wasClean: !!meta.clean };
  }
  return { type: "error", target, message: meta.message };
};
const __wjs_wsObjs = new Map();
globalThis.__wjs_ws_emit = (id, prop, kind, json, binU8) => {
  const t = __wjs_wsObjs.get(id);
  if (!t) return;
  const st = __wjs_wskState.get(t);
  const event = globalThis.__wjs_make_ws_event(kind, json, binU8, t);
  if (prop === "onopen") {
    st.readyState = 1;
    if (event.protocol) st.protocol = event.protocol;
  }
  if (prop === "onclose") {
    st.readyState = 3;
    __wjs_wsObjs.delete(id);
  }
  const h = t[prop];
  if (typeof h === "function") h.call(t, event);
};
const __wjs_wskState = new WeakMap();
globalThis.WebSocket = class WebSocket {
  static CONNECTING = 0; static OPEN = 1; static CLOSING = 2; static CLOSED = 3;
  constructor(url, protocols) {
    let protos = [];
    if (protocols !== undefined) {
      protos = Array.isArray(protocols) ? protocols.map(String) : [String(protocols)];
    }
    const href = String(url instanceof URL ? url.href : url);
    __wjs_wskState.set(this, {
      url: href, protocol: "", readyState: 0, binaryType: "arraybuffer",
      bufferedAmount: 0, onopen: null, onmessage: null, onclose: null, onerror: null,
    });
    const id = __wjs_ws_connect(href, JSON.stringify(protos), this);
    __wjs_wskState.get(this).id = id;
    __wjs_wsObjs.set(id, this);
  }
  get url() { return __wjs_wskState.get(this).url; }
  get protocol() { return __wjs_wskState.get(this).protocol; }
  get readyState() { return __wjs_wskState.get(this).readyState; }
  get bufferedAmount() { return 0; }
  get binaryType() { return __wjs_wskState.get(this).binaryType; }
  set binaryType(v) {
    if (v !== "blob" && v !== "arraybuffer") throw new TypeError("binaryType must be 'blob' or 'arraybuffer'");
    __wjs_wskState.get(this).binaryType = v;
  }
  get onopen() { return __wjs_wskState.get(this).onopen; }
  set onopen(v) { __wjs_wskState.get(this).onopen = v; }
  get onmessage() { return __wjs_wskState.get(this).onmessage; }
  set onmessage(v) { __wjs_wskState.get(this).onmessage = v; }
  get onclose() { return __wjs_wskState.get(this).onclose; }
  set onclose(v) { __wjs_wskState.get(this).onclose = v; }
  get onerror() { return __wjs_wskState.get(this).onerror; }
  set onerror(v) { __wjs_wskState.get(this).onerror = v; }
  send(data) {
    const st = __wjs_wskState.get(this);
    if (st.readyState === 0) throw new Error("InvalidStateError: WebSocket is not open");
    if (st.readyState !== 1) return;
    if (typeof data === "string") __wjs_ws_send(st.id, 0, data);
    else if (data instanceof Uint8Array) __wjs_ws_send(st.id, 1, data);
    else if (data instanceof ArrayBuffer) __wjs_ws_send(st.id, 1, new Uint8Array(data));
    else if (ArrayBuffer.isView(data)) __wjs_ws_send(st.id, 1, new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
    else throw new TypeError("WebSocket send: unsupported data type");
  }
  close(code = 1005, reason = "") {
    const st = __wjs_wskState.get(this);
    if (code !== 1005 && (!Number.isInteger(code) || code < 1000 || code > 4999 || [1004, 1005, 1006, 1015].includes(code))) {
      throw new Error("InvalidAccessError: bad WebSocket close code");
    }
    if (st.readyState === 3) return;
    st.readyState = 2;
    __wjs_ws_close(st.id, code, String(reason));
  }
};
// ---- Phase 3c-2: streams（纯 prelude 内存实现；默认 reader + BYOB）----
// BYOB 口径：`new ReadableStream({ type: "bytes", ... })` + `getReader({ mode: "byob" })`；
// `read(view)` 按 view 类型回同类前缀视图；`byobRequest.respond/respondWithNewView` 完整；
// 简化（文档记录）：done 时 value 为 undefined（非空视图）；respond 非元素对齐截断丢余量；
// 无 autoAllocateChunkSize；default reader 照常读字节流（Uint8Array 块）。
const __wjs_rsState = new WeakMap();
function __wjs_rsViewPrefix(r, n) {
  // 取 view 前 n 字节（元素对齐由调用方保证；DataView 按字节）。
  if (r.viewCtor === DataView) return new DataView(r.view.buffer, r.view.byteOffset, n);
  return new r.viewCtor(r.view.buffer, r.view.byteOffset, n / r.viewElem);
}
function __wjs_rsByobFill(st) {
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
    r.resolve({ value: __wjs_rsViewPrefix(r, take), done: false });
  }
}
function __wjs_rsByobReq(st) {
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
      r.resolve({ value: __wjs_rsViewPrefix(r, take), done: false });
      __wjs_rsPump(st);
    },
    respondWithNewView(v) {
      if (!ArrayBuffer.isView(v)) throw new TypeError("respondWithNewView needs a view");
      if (st.byobReads[0] !== r || st.byobReq === null) throw new TypeError("respondWithNewView: request is not active");
      r.view = v; r.viewCtor = v.constructor; r.viewElem = v.BYTES_PER_ELEMENT ?? 1;
    },
  };
}
function __wjs_rsByteToQueue(st) {
  // default reader 读字节流：整块搬运（有 _off 余量的半块留给 BYOB，不拆）
  while (st.byteQ.length && st.byteQ[0]._off === 0) {
    const q = st.byteQ.shift();
    st.byteLen -= q.length;
    st.queue.push(q);
  }
}
function __wjs_rsPull(st) {
  if (!st.reader || st.closed || st.error !== undefined || st.pulling) return;
  // pull 触发面（防微任务空转饿死事件循环，见 §4.27 追补）：
  // 只在新需求到达（read 推入等待）或有进展且需求还在（pump 尾）时调；
  // 无 pull 方法的源 + 挂起的读，eager 重拉即无限微任务链。
  st.pulling = true;
  st.pullProgress = false;
  // BYOB 读排队时带 byobRequest 进 pull（source 可直接写 view + respond）
  if (st.isBytes && st.byobReads.length && !st.byobReq) st.byobReq = __wjs_rsByobReq(st);
  try {
    const r = st.source.pull ? st.source.pull(st.controller) : undefined;
    Promise.resolve(r).then(() => { st.pulling = false; st.byobReq = null; __wjs_rsPump(st); }, (e) => {
      st.pulling = false; st.byobReq = null; __wjs_rsError(st, e);
    });
  } catch (e) { st.pulling = false; st.byobReq = null; __wjs_rsError(st, e); }
}
function __wjs_rsPump(st) {
  __wjs_rsByobFill(st);
  // default reader 读字节流：仅当有读等待（wantValue）才整块搬运；
  // closed 等待不搬，否则会饿死后来的 BYOB 读
  if (st.isBytes && st.pending.some((p) => p.wantValue)) __wjs_rsByteToQueue(st);
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
    if (demand && st.pullProgress) { st.pullProgress = false; __wjs_rsPull(st); }
  }
}
function __wjs_rsError(st, e) {
  if (st.closed || st.error !== undefined) return;
  st.error = e;
  st.queue.length = 0;
  st.byteQ.length = 0; st.byteLen = 0;
  __wjs_rsByobFill(st);
  __wjs_rsPump(st);
}
function __wjs_rsController(stream, st) {
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
        __wjs_rsPump(st);
      },
      close() {
        if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
        st.closed = true;
        st.pullProgress = true;
        __wjs_rsPump(st);
      },
      error(e) { __wjs_rsError(st, e); },
    };
  }
  return {
    get desiredSize() { return st.hwm - st.queue.length; },
    enqueue(chunk) {
      if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
      if (chunk === undefined) throw new TypeError("chunk must not be undefined");
      st.queue.push(chunk);
      st.pullProgress = true;
      __wjs_rsPump(st);
    },
    close() {
      if (st.closed || st.error !== undefined) throw new TypeError("stream is not readable");
      st.closed = true;
      st.pullProgress = true;
      __wjs_rsPump(st);
    },
    error(e) { __wjs_rsError(st, e); },
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
    st.controller = __wjs_rsController(this, st);
    __wjs_rsState.set(this, st);
    try {
      const r = underlyingSource.start ? underlyingSource.start(st.controller) : undefined;
      Promise.resolve(r).catch((e) => __wjs_rsError(st, e));
    } catch (e) { __wjs_rsError(st, e); }
  }
  get locked() { return !!__wjs_rsState.get(this).reader; }
  cancel(reason) {
    const st = __wjs_rsState.get(this);
    if (st.reader) throw new TypeError("stream is locked");
    st.queue.length = 0; st.closed = true;
    st.byteQ.length = 0; st.byteLen = 0;
    const c = st.source.cancel ? st.source.cancel(reason) : undefined;
    __wjs_rsPump(st);
    return Promise.resolve(c).then(() => undefined);
  }
  getReader(options) {
    const st = __wjs_rsState.get(this);
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
            __wjs_rsByobFill(st);
            __wjs_rsPull(st);
          });
        },
        releaseLock() { if (st.reader === reader) st.reader = null; },
        cancel(reason) {
          st.byteQ.length = 0; st.byteLen = 0; st.closed = true;
          const c = st.source.cancel ? st.source.cancel(reason) : undefined;
          if (st.reader === reader) st.reader = null;
          __wjs_rsPump(st);
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
          if (st.isBytes) __wjs_rsByteToQueue(st);
          if (st.queue.length) {
            const v = st.queue.shift();
            resolve({ value: v, done: false });
            __wjs_rsPull(st);
            return;
          }
          if (st.closed) { resolve({ value: undefined, done: true }); return; }
          st.pending.push({ resolve, reject, wantValue: true });
          __wjs_rsPull(st);
        });
      },
      releaseLock() { if (st.reader === reader) st.reader = null; },
      cancel(reason) {
        st.queue.length = 0; st.closed = true;
        st.byteQ.length = 0; st.byteLen = 0;
        const c = st.source.cancel ? st.source.cancel(reason) : undefined;
        if (st.reader === reader) st.reader = null;
        __wjs_rsPump(st);
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
    const st = __wjs_rsState.get(this);
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
const __wjs_wsState = new WeakMap();
globalThis.WritableStream = class WritableStream {
  constructor(underlyingSink = {}, strategy) {
    const hwm = strategy && strategy.highWaterMark !== undefined ? Number(strategy.highWaterMark) : 1;
    const st = {
      queue: [], writing: false, closed: false, errored: false, error: undefined,
      writer: null, hwm: Number.isNaN(hwm) ? 1 : hwm, sink: underlyingSink,
      closeReq: null,
    };
    __wjs_wsState.set(this, st);
    const stream = this;
    st.controller = { error(e) { __wjs_wsError(stream, e); } };
    try {
      const r = underlyingSink.start ? underlyingSink.start(st.controller) : undefined;
      Promise.resolve(r).catch((e) => __wjs_wsError(this, e));
    } catch (e) { __wjs_wsError(this, e); }
  }
  get locked() { return !!__wjs_wsState.get(this).writer; }
  abort(reason) {
    const st = __wjs_wsState.get(this);
    if (st.writer) throw new TypeError("stream is locked");
    const a = st.sink.abort ? st.sink.abort(reason) : undefined;
    __wjs_wsError(this, reason);
    return Promise.resolve(a).then(() => undefined);
  }
  close() {
    const st = __wjs_wsState.get(this);
    if (st.writer) throw new TypeError("stream is locked");
    return __wjs_wsCloseReq(this);
  }
  getWriter() {
    const st = __wjs_wsState.get(this);
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
          __wjs_wsPump(stream);
        });
      },
      close() { return __wjs_wsCloseReq(stream); },
      abort(reason) {
        const a = st.sink.abort ? st.sink.abort(reason) : undefined;
        __wjs_wsError(stream, reason);
        return Promise.resolve(a).then(() => undefined);
      },
      releaseLock() { if (st.writer === writer) st.writer = null; },
    };
    st.closeWaiters = st.closeWaiters || [];
    st.writer = writer;
    return writer;
  }
};
function __wjs_wsError(stream, e) {
  const st = __wjs_wsState.get(stream);
  if (st.errored) return;
  st.errored = true;
  st.error = e;
  for (const q of st.queue.splice(0)) q.reject(e);
  if (st.closeReq) { const c = st.closeReq; st.closeReq = null; c.reject(e); }
  for (const w of (st.closeWaiters || []).splice(0)) w.reject(e);
}
function __wjs_wsCloseReq(stream) {
  const st = __wjs_wsState.get(stream);
  return new Promise((resolve, reject) => { st.closeReq = { resolve, reject }; __wjs_wsPump(stream); });
}
function __wjs_wsPump(stream) {
  const st = __wjs_wsState.get(stream);
  if (st.writing || st.errored) return;
  const item = st.queue.shift();
  if (!item) {
    if (st.closeReq && !st.writing) {
      const c = st.closeReq; st.closeReq = null;
      const done = () => { st.closed = true; c.resolve(undefined); for (const w of (st.closeWaiters || []).splice(0)) w.resolve(undefined); };
      try {
        Promise.resolve(st.sink.close ? st.sink.close() : undefined).then(done, (e) => { __wjs_wsError(stream, e); });
      } catch (e) { __wjs_wsError(stream, e); }
    }
    return;
  }
  st.writing = true;
  try {
    Promise.resolve(st.sink.write ? st.sink.write(item.chunk, st.controller) : undefined).then(
      () => { st.writing = false; item.resolve(undefined); __wjs_wsPump(stream); },
      (e) => { st.writing = false; item.reject(e); __wjs_wsError(stream, e); __wjs_wsPump(stream); },
    );
  } catch (e) { st.writing = false; item.reject(e); __wjs_wsError(stream, e); }
}
// ---- QueuingStrategy 双类（Web 全局；WHATWG streams。真机 26 口径：highWaterMark
// 是原型 getter 非自有键、size 是可枚举 accessor 且全实例共享同一函数、构造器
// ARG_TYPE 文案 + highWaterMark 缺失 ERR_MISSING_OPTION；size 对 undefined/null
// 抛 TypeError、其余回 chunk.byteLength（原始值/普通对象 → undefined）；10f）----
const __wjs_qsState = new WeakMap();
function __wjs_qsArg(init) {
  if (init === null) return "null";
  if (typeof init === "string") return `type string ('${init}')`;
  if (typeof init === "number") return `type number (${init})`;
  if (typeof init === "function") return "type function";
  return `type ${typeof init}`;
}
const __wjs_qsSizeBL = (chunk) => {
  if (chunk === undefined || chunk === null) throw new TypeError("chunk must not be undefined or null");
  return chunk.byteLength;
};
const __wjs_qsSizeCount = () => 1;
globalThis.ByteLengthQueuingStrategy = class ByteLengthQueuingStrategy {
  constructor(init) {
    if (init === null || (typeof init !== "object" && typeof init !== "function")) {
      const err = new TypeError(`The "init" argument must be of type object. Received ${__wjs_qsArg(init)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (init.highWaterMark === undefined) {
      const err = new TypeError("init.highWaterMark is required");
      err.code = "ERR_MISSING_OPTION";
      throw err;
    }
    __wjs_qsState.set(this, init.highWaterMark);
  }
  get highWaterMark() { return __wjs_qsState.get(this); }
  get size() { return __wjs_qsSizeBL; }
};
globalThis.CountQueuingStrategy = class CountQueuingStrategy {
  constructor(init) {
    if (init === null || (typeof init !== "object" && typeof init !== "function")) {
      const err = new TypeError(`The "init" argument must be of type object. Received ${__wjs_qsArg(init)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    if (init.highWaterMark === undefined) {
      const err = new TypeError("init.highWaterMark is required");
      err.code = "ERR_MISSING_OPTION";
      throw err;
    }
    __wjs_qsState.set(this, init.highWaterMark);
  }
  get highWaterMark() { return __wjs_qsState.get(this); }
  get size() { return __wjs_qsSizeCount; }
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
const __wjs_csState = new WeakMap();
const __CS_KINDS = { gzip: 2, deflate: 0, "deflate-raw": 1, brotli: 8 };
const __DS_KINDS = { gzip: 6, deflate: 3, "deflate-raw": 4, brotli: 9 };
function __wjs_csFinishFlag(kind) { return kind <= 7 ? 4 : 2; }
function __wjs_makeCSClass(name, kinds, reject) {
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
      const id = __wjs_zlib_stream_new(kind, lv, null, -1, reject ? 1 : 0);
      let ctrl = null;
      let closed = false; // readable 已 close/error
      let freed = false;
      let done = false;   // 引擎已 StreamEnd（后续写入即尾垃圾）
      const free = () => { if (!freed) { freed = true; __wjs_zlib_stream_free(id); } };
      const fail = (err) => { if (!closed) { closed = true; ctrl.error(err); } };
      const feed = (u8, flag) => {
        const r = JSON.parse(__wjs_zlib_stream_feed(id, u8 ?? null, flag));
        if (r.code !== undefined) {
          const err = r.code === "ERR_TRAILING_JUNK_AFTER_STREAM_END"
            ? new TypeError(r.msg) : new Error(r.msg);
          err.code = r.code;
          fail(err);
          return false;
        }
        done = r.d === true;
        const out = __wjs_zlib_stream_out(id);
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
          const ok = feed(null, __wjs_csFinishFlag(kind));
          if (ok) { closed = true; ctrl.close(); }
          free();
        },
        abort(reason) { if (!closed) { closed = true; ctrl.error(reason); } free(); },
      });
      __wjs_csState.set(this, { readable, writable });
    }
    get readable() { return __wjs_csState.get(this).readable; }
    get writable() { return __wjs_csState.get(this).writable; }
  };
  Object.defineProperty(cls.prototype, Symbol.toStringTag, { value: name, configurable: true });
  return cls;
}
globalThis.CompressionStream = __wjs_makeCSClass("CompressionStream", __CS_KINDS, false);
globalThis.DecompressionStream = __wjs_makeCSClass("DecompressionStream", __DS_KINDS, true);
// ---- Blob（Web 全局；fetch/consumers/node:internal/blob 共用；9b）----
const __wjs_blobBytes = new WeakMap();
globalThis.Blob = class Blob {
  constructor(parts = [], options = {}) {
    const chunks = [];
    let size = 0;
    if (typeof parts === "string" || ArrayBuffer.isView(parts) || parts instanceof ArrayBuffer) {
      throw new TypeError("Blob parts must be an iterable");
    }
    for (const part of parts) {
      if (part instanceof Blob) {
        const u8 = __wjs_blobBytes.get(part);
        chunks.push(u8); size += u8.byteLength;
      } else if (typeof part === "string") {
        const u8 = new TextEncoder().encode(part);
        chunks.push(u8); size += u8.byteLength;
      } else if (ArrayBuffer.isView(part)) {
        chunks.push(new Uint8Array(part.buffer, part.byteOffset, part.byteLength));
        size += part.byteLength;
      } else if (part instanceof ArrayBuffer) {
        chunks.push(new Uint8Array(part)); size += part.byteLength;
      } else if (part == null) {
        // spec: null/undefined part 跳过
      } else {
        const u8 = new TextEncoder().encode(String(part));
        chunks.push(u8); size += u8.byteLength;
      }
    }
    const bytes = new Uint8Array(size);
    let off = 0;
    for (const c of chunks) { bytes.set(c, off); off += c.byteLength; }
    __wjs_blobBytes.set(this, bytes);
    const type = typeof options.type === "string" ? options.type : "";
    this.type = type.replace(/[^\x20-\x7E]/g, "").toLowerCase();
  }
  get size() { return __wjs_blobBytes.get(this).byteLength; }
  slice(start, end, contentType) {
    const b = __wjs_blobBytes.get(this);
    const s = start === undefined ? 0 : (start < 0 ? Math.max(b.byteLength + start, 0) : Math.min(start, b.byteLength));
    const e = end === undefined ? b.byteLength : (end < 0 ? Math.max(b.byteLength + end, 0) : Math.min(end, b.byteLength));
    const out = new Blob([], { type: contentType === undefined ? this.type : String(contentType) });
    __wjs_blobBytes.set(out, s < e ? b.slice(s, e) : new Uint8Array(0));
    return out;
  }
  arrayBuffer() {
    return Promise.resolve(__wjs_blobBytes.get(this).slice().buffer);
  }
  bytes() {
    return Promise.resolve(__wjs_blobBytes.get(this).slice());
  }
  text() {
    return Promise.resolve(new TextDecoder().decode(__wjs_blobBytes.get(this)));
  }
  stream() {
    const b = __wjs_blobBytes.get(this);
    return new ReadableStream({
      start(c) { c.enqueue(b.slice()); c.close(); },
    });
  }
  get [Symbol.toStringTag]() { return "Blob"; }
};
// File（Web/Node 20+ 全局；jsdom/vitest 生态取此面）：Blob 子类 + name/lastModified。
// 状态复用 __wjs_blobBytes（WeakMap 随原型链命中，§4.23 纪律）。
globalThis.File = class File extends Blob {
  constructor(parts = [], name, options = {}) {
    if (arguments.length < 2 || name === undefined) {
      throw new TypeError("File constructor: name is required");
    }
    super(parts, options);
    this.name = String(name);
    this.lastModified =
      typeof options.lastModified === "number" ? options.lastModified : Date.now();
  }
  get [Symbol.toStringTag]() { return "File"; }
};
globalThis.fetch = (input, init = {}) => {
  const req = new Request(input, init);
  const st = __wjs_reqState.get(req);
  if (st.signal && st.signal.aborted) {
    const reason = st.signal.reason !== undefined
      ? st.signal.reason
      : __wjs_make_fetch_error("AbortError: fetch aborted");
    return Promise.reject(reason);
  }
  const headersJson = JSON.stringify([...st.headers]);
  return new Promise((resolve, reject) => {
    // 监听留到流结束：head 结算只 resolve（流式 body 的 abort 还靠它）；
    // head 失败或 abort 触发或流终结时经 `__wjs_fetchCleanup` 摘除。
    let onAbort = null;
    const cleanup = () => {
      if (onAbort && st.signal) st.signal.removeEventListener("abort", onAbort);
      onAbort = null;
    };
    const id = __wjs_fetch_start(
      st.url, st.method, headersJson, st.bodyU8 ?? undefined,
      (v) => resolve(v),
      (e) => { if (id) __wjs_fetchCleanup(id); else cleanup(); reject(e); },
    );
    if (st.signal && id) {
      onAbort = () => {
        // Rust 侧取消任务 + 拒绝排队 pull（AbortError）；外层按原始 reason 拒绝。
        __wjs_abortedFetch.add(id);
        __wjs_fetch_abort(id);
        __wjs_fetchCleanup(id);
        reject(st.signal.reason);
      };
      st.signal.addEventListener("abort", onAbort);
      __wjs_fetchCleanups.set(id, cleanup);
    }
  });
};
// 已中止的流 id 集（pull 侧直接拒绝，不再进 Rust 状态）。
const __wjs_abortedFetch = new Set();
// 待摘的 abort 监听（流终结/取消时清理，长 signal 不堆积）。
const __wjs_fetchCleanups = new Map();
function __wjs_fetchCleanup(sid) {
  const fn = __wjs_fetchCleanups.get(sid);
  if (fn) {
    __wjs_fetchCleanups.delete(sid);
    try { fn(); } catch {}
  }
}
"#;
