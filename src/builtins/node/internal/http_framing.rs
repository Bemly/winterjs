//! `node:internal/http_framing`：HTTP/1.1 共享帧层（`node:http` 与 `node:https`
//! 共用；Phase 9d-6 新建，10b 流式化重构）。内容逐行取自 `node:http` 原 SOURCE
//! （行为保真，http 既有黑盒为回归网）；9d-6 起：解析器子集 + IncomingMessage +
//! ServerResponse + `withHttpServer(Base)` 服务端混入 +
//! `withClientRequest(open, flavor)` 客户端工厂。
//! 注入点仅两处：服务端基类（net.Server / tls.Server）、客户端开 socket 函数。
//!
//! 10b（Bun 高度）：整收改流式——
//! - `IncomingMessage`/`ServerResponse`/`ClientRequest` 进 `node:stream` 全家
//!  （Readable/Writable；pipe/for-await 免费；暂停/流动走标准语义）。
//! - 服务端 keep-alive：逐请求协商（HTTP/1.1 默认保活，`connection: close`
//!   则关），响应完回解析循环（microtask 续行，防管线洪水爆栈）；
//!   `server.close()` 销毁跟踪中的全部连接（含空闲保活）。
//! - 客户端 Agent 池：`keepAlive` 按 host:port 复用，`reusedSocket` 可观测；
//!   缺省全局 Agent 不池化（与 9d 行为一致）。
//! - 分块编码：10f G3 起按真机口径——CL 快路径仅当 end() 是首个头触发点
//!  （无 writeHead/write/flushHeaders 前置）且允许体（服务端随请求版本、
//!   客户端随方法族）；否则 chunked（HTTP/1.0 服务端裸体 close-delimited）。
//! - 204/304/HEAD 无体（CL 照算但不发字节，保活不断帧）。
//! 偏差记档（10b）：
//! - 请求头在首字节实际发出前不出网（write 先缓冲；包级时序差，语义同）。
//! - 管线请求顺序处理（后请求的解析等前响应结束；并发语义同，时序差）。
//! - 服务端无空闲保活超时（`server.close()` 即全毁；Node 有 keepAliveTimeout）。
//! - 1xx 中间响应按终态处理（无 `continue` 事件；100-continue 流程另切片）。
//! - trailer 不收不发（chunked 尾部直接终结；10f G3 起限深照算：trailer 名+值
//!   累计 ≥ maxHeaderSize 即 431、无冒号行 400；http2 triage 见 plan3 10b-4）。
//! - 10f G3：chunk 扩展限深（尺寸行扩展总量 > 16KiB 即 413、扩展字符集校验
//!   400）——llhttp 计数语义，真机 26.8.2 逐项实测定标。
/// §0.9 按域分块：`framing_head.js`（解析/头帧/IncomingMessage/ServerResponse）+
/// `framing_outgoing.js`（OutgoingMessage 本体）+ `framing_agent.js`
///（Agent/工厂/收尾），concat 字节恒等。
pub const SOURCE: &str = concat!(
    include_str!("framing_head.js"),
    include_str!("framing_outgoing.js"),
    include_str!("framing_agent.js"),
);
