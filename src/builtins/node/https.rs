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
// Server 首参 listener 形态（Node 口径，与 node:http 同）；options 对象才透传。
function Server(...args) {
  const opts = args[0] !== null && typeof args[0] === "object" && !Array.isArray(args[0]) ? args[0] : undefined;
  const s = new __HttpServerBase(opts);
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
// Agent 函数式构造器（node 口径：`https.Agent({...})` 无 new 亦合法）。
function Agent(options = {}) {
  if (!(this instanceof Agent)) return new Agent(options);
  BaseAgent.prototype.__init.call(this, options);
}
Object.setPrototypeOf(Agent.prototype, BaseAgent.prototype);
Object.setPrototypeOf(Agent, BaseAgent);
Agent.prototype.__openSocket = (host, port, extra) => tls.connect({
  port, host,
  servername: extra.servername,
  ca: extra.ca,
  rejectUnauthorized: extra.rejectUnauthorized,
});
Agent.prototype.__defaultPort = 443;
// https 键位在 http 基础上加 TLS 会话面字段（node lib/https.js 口径；pfx/cert
// 等进键——不同证书不同池位，agent-getname 套件逐字段对拍）。
Agent.prototype.getName = function (options = {}) {
  let name = BaseAgent.prototype.getName.call(this, options);
  name += ":";
  if (options.ca) name += options.ca;
  name += ":";
  if (options.cert) name += options.cert;
  name += ":";
  if (options.clientCertEngine) name += options.clientCertEngine;
  name += ":";
  if (options.ciphers) name += options.ciphers;
  name += ":";
  if (options.key) name += options.key;
  name += ":";
  if (options.pfx) name += __pfxAgentKey(options.pfx, options.passphrase);
  name += ":";
  if (options.rejectUnauthorized !== undefined) name += options.rejectUnauthorized;
  name += ":";
  if (options.servername && options.servername !== options.host) name += options.servername;
  name += ":";
  if (options.minVersion) name += options.minVersion;
  name += ":";
  if (options.maxVersion) name += options.maxVersion;
  name += ":";
  if (options.secureProtocol) name += options.secureProtocol;
  name += ":";
  if (options.crl) name += options.crl;
  name += ":";
  if (options.honorCipherOrder !== undefined) name += options.honorCipherOrder;
  name += ":";
  if (options.ecdhCurve) name += options.ecdhCurve;
  name += ":";
  if (options.dhparam) name += options.dhparam;
  name += ":";
  if (options.secureOptions !== undefined) name += options.secureOptions;
  name += ":";
  if (options.sessionIdContext) name += options.sessionIdContext;
  name += ":";
  if (options.sigalgs) name += JSON.stringify(options.sigalgs);
  name += ":";
  if (options.privateKeyIdentifier) name += options.privateKeyIdentifier;
  name += ":";
  if (options.privateKeyEngine) name += options.privateKeyEngine;
  return name;
};
function __pfxAgentKey(pfx, passphrase) {
  if (!Array.isArray(pfx)) return pfx;
  let key = "";
  for (const v of pfx) {
    const raw = v?.buf || v;
    const pass = v?.passphrase || passphrase;
    key += `:${raw}:${pass}`;
  }
  return key;
}
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
  const opts = options !== null && typeof options === "object" && !Array.isArray(options) ? options : undefined;
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
