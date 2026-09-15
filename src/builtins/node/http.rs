//! `node:http`：HTTP/1.1 Server/Client——帧层经 `node:internal/http_framing` 共享
//! （Phase 9d-6 重构，`node:https` 共用；解析/语义逐行保真，行为变更见下）。
//! 附带修：`createServer(options, cb)` 的 `cb` 原先被吞（`new Server(options, cb)`
//! 只认首参 listener），现双形态正常接线（http 既有黑盒全绿验证无回归）。
//! 10b：keep-alive（Agent 池 + `reusedSocket`）+ req/res 流式体（Readable/
//! Writable 全家，可 pipe/for-await）+ 分块编码；偏差见帧层头注。

/// 内嵌 ESM 源（`node:http`；net 底座 + 共享帧层）。
pub const SOURCE: &str = r#"
import * as net from "node:net";
import {
  STATUS_CODES, METHODS, maxHeaderSize, IncomingMessage, ServerResponse,
  OutgoingMessage, Agent as BaseAgent, withHttpServer, withClientRequest,
  normalizeRequestArgs, requestFrom, getFrom,
} from "node:internal/http_framing";

const FLAVOR = { protocol: "http:", defaultPort: 80, other: "node:https" };
const Server = withHttpServer(net.Server);
const ClientRequest = withClientRequest(
  (host, port) => net.connect(port, host),
  FLAVOR,
);
class Agent extends BaseAgent {}
Agent.prototype.__openSocket = (host, port) => net.connect(port, host);
Agent.prototype.__defaultPort = 80;
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
  const server = new Server();
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
