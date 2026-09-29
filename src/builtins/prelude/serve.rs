//! serve 桥与 WebSocket（serve_socket/on_head/send_resp/ws 收发）（prelude 分域；拼接顺序见 mod.rs）。
pub const SERVE_JS: &str = r#"
// 服务端 socket 工厂（T4）：WS 表项与发送端由 native 分配挂靠（`__wjs2_serve_ws_create`），
// 对象进 `__wjs2_wsObjs` 复用 client 事件派发（`__wjs2_ws_emit` 按 id 路由，不分端）。
// 未配对 101 即由 decline/fail 回收；非 serve 请求传参即 TypeError。
globalThis.__wjs2_serve_socket = (req) => {
  const rst = __wjs2_reqState.get(req);
  const sid = rst ? rst.serveId : undefined;
  if (typeof sid !== "number") throw new TypeError("__wjs2_serve_socket needs a serve request");
  const wsId = __wjs2_serve_ws_create(sid);
  const o = {};
  __wjs2_wskState.set(o, {
    url: String(req.url).replace(/^http/, "ws"), protocol: "", readyState: 0,
    binaryType: "arraybuffer", id: wsId, server: true,
    onopen: undefined, onmessage: undefined, onclose: undefined, onerror: undefined,
  });
  __wjs2_wsObjs.set(wsId, o);
  o.send = (data) => {
    const st = __wjs2_wskState.get(o);
    if (st.readyState === 0) throw new Error("InvalidStateError: WebSocket is not open");
    if (st.readyState !== 1) return;
    if (typeof data === "string") __wjs2_ws_send(st.id, 0, data);
    else if (data instanceof Uint8Array) __wjs2_ws_send(st.id, 1, data);
    else if (data instanceof ArrayBuffer) __wjs2_ws_send(st.id, 1, new Uint8Array(data));
    else if (ArrayBuffer.isView(data)) __wjs2_ws_send(st.id, 1, new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
    else throw new TypeError("WebSocket send needs string/BufferSource, got " + typeof data);
  };
  o.close = (code, reason) => {
    const st = __wjs2_wskState.get(o);
    if (code === undefined) code = 1005;
    if (reason === undefined) reason = "";
    if (st.readyState === 3) return;
    st.readyState = 2;
    __wjs2_ws_close(st.id, code, String(reason));
  };
  return o;
};
globalThis.__wjs2_serve_on_head = (id, metaJson, streamId) => {
  let meta;
  try { meta = JSON.parse(metaJson); } catch (e) { __wjs2_serve_fail(id, "bad serve head"); return; }
  const headers = new Headers();
  for (const [k, v] of meta.headers) headers.append(k, v);
  const req = __wjs2_serve_make_request(meta.url, meta.method, headers, streamId, id);
  const fn = globalThis.__wjs2_serve_fetch;
  if (typeof fn !== "function") { __wjs2_serve_fail(id, "serve handler missing fetch"); return; }
  const fail = (e) => __wjs2_serve_fail(id, String((e && e.message) || e));
  let out;
  try { out = fn(req); } catch (e) { fail(e); return; }
  Promise.resolve(out).then(
    (resp) => { __wjs2_serve_send_resp(id, resp, !!meta.upgrade).catch(fail); },
    fail,
  );
};
async function __wjs2_serve_send_resp(id, resp, isUpgrade) {
  // T4：upgrade 请求返回服务端 socket（`__wjs2_serve_socket` 产物，`server` 品牌位）
  // 即接受升级（Rust 发 101 + 接管）；其余一律 Decline 走普通管线。
  // 不用 `new Response(101)` 表达——prelude/undici 口径 status 限 200-599，共享语义不动。
  const st = (resp !== null && (typeof resp === "object" || typeof resp === "function"))
    ? __wjs2_wskState.get(resp)
    : undefined;
  if (isUpgrade && st && st.server) { __wjs2_serve_ws_accept(id); return; }
  if (!(resp instanceof Response)) throw new TypeError("serve handler must return a Response");
  if (isUpgrade) __wjs2_serve_ws_decline(id);
  __wjs2_serve_head(id, JSON.stringify({ status: resp.status, headers: [...resp.headers] }));
  const body = resp.body;
  if (body !== null && body !== undefined) {
    for await (const c of body) {
      const u8 = c instanceof Uint8Array ? c : new Uint8Array(c);
      // 分片推送：单次 native 拷贝封顶 64KB，大体走多 Chunk 通道（plan4 §0-2 流式；
      // 快照在构造期已存在，此处只解决传输分片，不碰共享 Response 语义）。
      for (let off = 0; off < u8.length; off += 65536) {
        __wjs2_serve_push(id, u8.subarray(off, Math.min(off + 65536, u8.length)));
      }
    }
  }
  __wjs2_serve_push(id, null);
}
globalThis.__wjs2_make_ws_event = (kind, json, binU8, target) => {
  const meta = JSON.parse(json);
  if (kind === "open") return { type: "open", target, protocol: meta.protocol ?? "" };
  if (kind === "message-text") return { type: "message", target, data: meta.text };
  if (kind === "message-bin") return { type: "message", target, data: binU8.buffer };
  if (kind === "close") {
    return { type: "close", target, code: meta.code, reason: meta.reason, wasClean: !!meta.clean };
  }
  return { type: "error", target, message: meta.message };
};
const __wjs2_wsObjs = new Map();
globalThis.__wjs2_ws_emit = (id, prop, kind, json, binU8) => {
  const t = __wjs2_wsObjs.get(id);
  if (!t) return;
  const st = __wjs2_wskState.get(t);
  const event = globalThis.__wjs2_make_ws_event(kind, json, binU8, t);
  if (prop === "onopen") {
    st.readyState = 1;
    if (event.protocol) st.protocol = event.protocol;
  }
  if (prop === "onclose") {
    st.readyState = 3;
    __wjs2_wsObjs.delete(id);
  }
  const h = t[prop];
  if (typeof h === "function") h.call(t, event);
};
const __wjs2_wskState = new WeakMap();
globalThis.WebSocket = class WebSocket {
  static CONNECTING = 0; static OPEN = 1; static CLOSING = 2; static CLOSED = 3;
  constructor(url, protocols) {
    let protos = [];
    if (protocols !== undefined) {
      protos = Array.isArray(protocols) ? protocols.map(String) : [String(protocols)];
    }
    const href = String(url instanceof URL ? url.href : url);
    __wjs2_wskState.set(this, {
      url: href, protocol: "", readyState: 0, binaryType: "arraybuffer",
      bufferedAmount: 0, onopen: null, onmessage: null, onclose: null, onerror: null,
    });
    const id = __wjs2_ws_connect(href, JSON.stringify(protos), this);
    __wjs2_wskState.get(this).id = id;
    __wjs2_wsObjs.set(id, this);
  }
  get url() { return __wjs2_wskState.get(this).url; }
  get protocol() { return __wjs2_wskState.get(this).protocol; }
  get readyState() { return __wjs2_wskState.get(this).readyState; }
  get bufferedAmount() { return 0; }
  get binaryType() { return __wjs2_wskState.get(this).binaryType; }
  set binaryType(v) {
    if (v !== "blob" && v !== "arraybuffer") throw new TypeError("binaryType must be 'blob' or 'arraybuffer'");
    __wjs2_wskState.get(this).binaryType = v;
  }
  get onopen() { return __wjs2_wskState.get(this).onopen; }
  set onopen(v) { __wjs2_wskState.get(this).onopen = v; }
  get onmessage() { return __wjs2_wskState.get(this).onmessage; }
  set onmessage(v) { __wjs2_wskState.get(this).onmessage = v; }
  get onclose() { return __wjs2_wskState.get(this).onclose; }
  set onclose(v) { __wjs2_wskState.get(this).onclose = v; }
  get onerror() { return __wjs2_wskState.get(this).onerror; }
  set onerror(v) { __wjs2_wskState.get(this).onerror = v; }
  send(data) {
    const st = __wjs2_wskState.get(this);
    if (st.readyState === 0) throw new Error("InvalidStateError: WebSocket is not open");
    if (st.readyState !== 1) return;
    if (typeof data === "string") __wjs2_ws_send(st.id, 0, data);
    else if (data instanceof Uint8Array) __wjs2_ws_send(st.id, 1, data);
    else if (data instanceof ArrayBuffer) __wjs2_ws_send(st.id, 1, new Uint8Array(data));
    else if (ArrayBuffer.isView(data)) __wjs2_ws_send(st.id, 1, new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
    else throw new TypeError("WebSocket send: unsupported data type");
  }
  close(code = 1005, reason = "") {
    const st = __wjs2_wskState.get(this);
    if (code !== 1005 && (!Number.isInteger(code) || code < 1000 || code > 4999 || [1004, 1005, 1006, 1015].includes(code))) {
      throw new Error("InvalidAccessError: bad WebSocket close code");
    }
    if (st.readyState === 3) return;
    st.readyState = 2;
    __wjs2_ws_close(st.id, code, String(reason));
  }
};
"#;
