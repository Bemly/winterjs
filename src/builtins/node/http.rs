//! `node:http`：HTTP/1.1 Server/Client——帧层经 `node:internal/http_framing` 共享
//! （Phase 9d-6 重构，`node:https` 共用；解析/语义逐行保真，行为变更见下）。
//! 附带修：`createServer(options, cb)` 的 `cb` 原先被吞（`new Server(options, cb)`
//! 只认首参 listener），现双形态正常接线（http 既有黑盒全绿验证无回归）。
//! 10b：keep-alive（Agent 池 + `reusedSocket`）+ req/res 流式体（Readable/
//! Writable 全家，可 pipe/for-await）+ 分块编码；偏差见帧层头注。

/// 内嵌 ESM 源（`node:http`；net 底座 + 共享帧层）。
pub const SOURCE: &str = r#"
import * as net from "node:net";
import { codes } from "node:internal/errors";
import {
  STATUS_CODES, METHODS, maxHeaderSize, IncomingMessage, ServerResponse,
  OutgoingMessage, Agent as BaseAgent, withHttpServer, withClientRequest,
  normalizeRequestArgs, requestFrom, getFrom,
} from "node:internal/http_framing";

const FLAVOR = { protocol: "http:", defaultPort: 80, other: "node:https" };
const __HttpServerBase = withHttpServer(net.Server);
// Server 首参 listener 形态（Node: `new Server(cb)`/`Server(cb)` 即 request 监听）。
// withHttpServer 的包装只透传基类构造实参（connection 监听），request 监听在此接。
// options 为对象时透传基类（timeout 三件套等 http 面选项在帧层解析）。
// options 类型门（node _http_server.js Server 构造口径，server.js 套件：
// 'foo'/42/true/[] → ERR_INVALID_ARG_TYPE；undefined/null 缺省、函数=监听器）。
function __serverOptions(args) {
  const first = args[0];
  if (first === undefined || first === null) return undefined;
  if (typeof first === "function") return undefined;
  if (first !== null && typeof first === "object" && !Array.isArray(first)) return first;
  throw new codes.ERR_INVALID_ARG_TYPE("options", "object", first);
}
// Server 首参 listener 形态（Node: `new Server(cb)`/`Server(cb)` 即 request 监听）。
// withHttpServer 的包装只透传基类构造实参（connection 监听），request 监听在此接。
// options 为对象时透传基类（timeout 三件套等 http 面选项在帧层解析）。
function Server(...args) {
  // `http.Server.call(this, cb)` 形（upgrade-server 套件 testServer 老式继承）：
  // this 的原型链已指向 Server.prototype，但 net.Server 的派发状态
  //（__ev 绑定钩子/句柄桩/连接表）只在其构造器里建。net 侧构造无法对既有
  // this 重跑（类构造器），故在临时实例上完整构造后把自有状态搬运到 this，
  // 派发钩子按 __ServerClass.prototype 重绑（Rust 侧按 listen 目标对象派发）。
  if (new.target === undefined && this instanceof __HttpServerBase) {
    const fresh = new __HttpServerBase(__serverOptions(args));
    for (const k of Object.getOwnPropertyNames(fresh)) {
      if (!Object.prototype.hasOwnProperty.call(this, k)) this[k] = fresh[k];
    }
    const __base = Object.getPrototypeOf(fresh);
    if (typeof __base.__ev === "function") this.__ev = __base.__ev.bind(this);
    try { (globalThis.__wjs_netXfer ??= new Map()).set(this, "net.Server"); } catch { /* guard */ }
    __HttpServerBase.__initOn(this, args);
    const first = args[0];
    if (typeof first === "function") this.on("request", first);
    else if (args.length > 1 && typeof args[1] === "function") this.on("request", args[1]);
    return undefined;
  }
  const opts = __serverOptions(args);
  const s = new __HttpServerBase(opts);
  const first = args[0];
  if (typeof first === "function") s.on("request", first);
  else if (args.length > 1 && typeof args[1] === "function") s.on("request", args[1]);
  return s;
}
Object.setPrototypeOf(Server, __HttpServerBase);
Server.prototype = __HttpServerBase.prototype;
const ClientRequest = withClientRequest(
  // IPC 形（node 口径）：options.socketPath 在 → UDS 连接，否则 TCP；
  // lookup 函数透传 net（noop lookup → socket 永不连通，agent-timeout-option
  // 套件形态）。
  (host, port, extra) => {
    const o = extra ?? {};
    if (o.socketPath !== undefined) {
      return o.lookup !== undefined ? net.connect({ path: o.socketPath, lookup: o.lookup }) : net.connect({ path: o.socketPath });
    }
    return o.lookup !== undefined ? net.connect({ port, host, lookup: o.lookup }) : net.connect(port, host);
  },
  FLAVOR,
);
// Agent 函数式构造器（node 口径：`http.Agent({...})` 无 new 亦合法）。
function Agent(options = {}) {
  if (!(this instanceof Agent)) return new Agent(options);
  BaseAgent.prototype.__init.call(this, options);
}
Object.setPrototypeOf(Agent.prototype, BaseAgent.prototype);
Object.setPrototypeOf(Agent, BaseAgent);
Agent.prototype.__openSocket = (host, port, extra) => {
  const o = extra ?? {};
  const base = o.socketPath !== undefined ? { path: o.socketPath, noDelay: true } : { port, host, noDelay: true };
  if (o.lookup !== undefined) base.lookup = o.lookup;
  return net.connect(base);
};
Agent.prototype.__defaultPort = 80;
const globalAgent = new Agent({ keepAlive: true, scheduling: "lifo" });
FLAVOR.defaultAgent = globalAgent;

export function request(a, b, c) {
  const [options, cb] = normalizeRequestArgs(a, b, c, FLAVOR);
  return requestFrom(ClientRequest, options, cb);
}
export function get(a, b, c) {
  const [options, cb] = normalizeRequestArgs(a, b, c, FLAVOR);
  return getFrom(ClientRequest, options, cb);
}
export function createServer(options, cb) {
  const opts = __serverOptions([options]);
  const server = new __HttpServerBase(opts);
  if (typeof options === "function") server.on("request", options);
  else if (typeof cb === "function") server.on("request", cb);
  return server;
}

export {
  STATUS_CODES, METHODS, maxHeaderSize, Server, ServerResponse,
  IncomingMessage, ClientRequest, OutgoingMessage, Agent, globalAgent,
};
const __api = {
  STATUS_CODES, METHODS, maxHeaderSize, request, get, Agent, globalAgent,
  Server, ServerResponse, IncomingMessage, ClientRequest, OutgoingMessage, createServer,
};
export default __api;
"#;
