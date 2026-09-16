//! `node:https`：HTTPS Server/Client——帧层复用 `node:internal/http_framing`
//! （与 `node:http` 同语义，含 10b keep-alive/流式体/chunked），传输经
//! `node:tls`（Phase 9d-6）。
//! 偏差记档（帧层头注沿用）：
//! - 客户端 TLS 选项透传：`servername`/`ca`（PEM 串）/`rejectUnauthorized`
//!   （`tls.connect` 同口径）；缺省系统 roots 校验。
//! - 服务端 `createServer({ key, cert }, listener)`（PEM 串必填）。

/// 内嵌 ESM 源（`node:https`；tls 底座 + 共享帧层）。
pub const SOURCE: &str = r#"
import * as tls from "node:tls";
import {
  STATUS_CODES, METHODS, maxHeaderSize, IncomingMessage, ServerResponse,
  OutgoingMessage, Agent as BaseAgent, withHttpServer, withClientRequest,
  normalizeRequestArgs, requestFrom, getFrom,
} from "node:internal/http_framing";

const FLAVOR = { protocol: "https:", defaultPort: 443, other: "node:http" };
const __HttpServerBase = withHttpServer(tls.Server);
// Server 首参 listener 形态（Node 口径，与 node:http 同）。
function Server(...args) {
  const s = new __HttpServerBase(args[0] ?? {});
  const first = args[0];
  if (typeof first === "function") s.on("request", first);
  else if (args.length > 1 && typeof args[1] === "function") s.on("request", args[1]);
  return s;
}
Object.setPrototypeOf(Server, __HttpServerBase);
Server.prototype = __HttpServerBase.prototype;
const ClientRequest = withClientRequest(
  (host, port, extra) => tls.connect({
    port, host,
    servername: extra.servername,
    ca: extra.ca,
    rejectUnauthorized: extra.rejectUnauthorized,
  }),
  FLAVOR,
);
class Agent extends BaseAgent {}
Agent.prototype.__openSocket = (host, port, extra) => tls.connect({
  port, host,
  servername: extra.servername,
  ca: extra.ca,
  rejectUnauthorized: extra.rejectUnauthorized,
});
Agent.prototype.__defaultPort = 443;
const globalAgent = new Agent();
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
  const server = new __HttpServerBase(options ?? {});
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
