# Phase 11 计划 — 独立 serve（WinterCG handler 全交 Rust 依赖）

> **存档（2026-09-25）**：Phase 11 独立 serve 已收官并合入 master（2026-09-21）。 当前进度见 `docs/plan3.md` §0，索引 `docs/README.md`。


> 立项 2026-09-21（用户拍板）。目标：`winterjs --serve` 从纯静态升级为
> **独立动态服务端**——JS 写 `Request → Response`（WinterCG 形状），
> H1/H2/H3 全由树内 Rust 轮子承载，自研只剩 JS 边界一圈。
> 用户口径：“比 node 对齐少 10 倍代码量，越精简越好，但必须满足抽象工程范式
> 与网络编程框架最佳实践”。
>
> 方法：协议传输层零自研（axum/hyper/quinn/h3 全委托），自研收敛在
> `axum fallback → 通道 → JS 线程` 单一桥接点。
>
> 纪律：AGENTS 三件套（模块单测 + 黑盒正常/报错/边界 + 冒烟 5/5，
> `UNSAFE-BOUNDARY` 新增配 panic 用例）；踩坑记 AGENTS §4；
> 新 crate 一律先走 §0.5（本轮 H3 桥接已按此引入 `h3-axum`，见下）；
> CLI 全 flag 规范（§0.8）：不新增动作，只加 `--serve` 下的修饰 flag。
>
> 验收线（终局）：H1/H2/H3 同 `Router` 对外；双形态 handler 全绿；
> 请求/响应体全程流式；WS upgrade 通；`--handler` 缺省 = 现状纯静态零回归；
> `cargo test` 全绿 0 警告，冒烟 5/5；新增代码（含测试）≤ 7k 行
> （node 对齐约 66k 的十分之一，`builtins/node` 实测为准）。

## §0 用户拍板（2026-09-21，四项）

| # | 问题 | 决定 | 影响 |
|---|---|---|---|
| 1 | Handler 形态 | **两种都认**：`export default { fetch }` 优先，回落具名 `export function fetch` | 加载期双探，缺其一即启动期可读错 |
| 2 | 体形态 | **S1 即流式**，无整收档 | 首轮即做 chunk 通道 + 背压 |
| 3 | H3 排期 | **同轮**，不另起轮 | S1 即含 H3（需 TLS，无证书则 H3 跳过 + warn） |
| 4 | WS upgrade | **含** | axum upgrade 直通，S4 一并收官 |

## §1 现状与缺口（证据）

- `src/main.rs:244-277`：`--serve` 直接进 `serve::serve()`，全程不碰 JS；
  `src/serve.rs:8` 原话“serve 路径无 JS”——现状是纯静态（`ServeDir` +
  `/metrics` + CORS/压缩/限流/TLS），无 handler 入口。
- 动态逻辑现走 `node:http`（`IncomingMessage`/`ServerResponse`），与 WinterCG
  `Request`/`Response` 非同一形状（见 `tests/node/http.rs:14`）。
- 依赖就绪（含本轮采购）：H1/H2 = `axum::serve`（`hyper` server+http2；
  `Cargo.toml` §10）；H3 = `h3-axum 0.2.0`（`axum^0.8/h3^0.0.8/
  h3-quinn^0.0.10` 精确命中，零新增传递依赖；落选 `axum-h3`，见
  `docs/dependencies.md` §10）；QUIC/TLS = `quinn/h3-quinn/tokio-rustls
  (ring)`；静态/中间件 = `tower-http`；WS 线协议 = `tokio-tungstenite`；
  体类型 = `http/http-body-util/bytes`。缺口为零，无需再采购（不够再按 §0.5 加）。
- JS 侧 `Request`/`Response` 真类已在 prelude（`src/builtins/fetch.rs:6`
  “本模块只做传输 + 交付”），serve 复用同一对类，不另造。

## §2 架构（抽象范式：三层 + 单桥）

```text
Transport（Rust，多线程）        Bridge（通道，纯数据）        Handler（JS，独占线程）
axum Router ─┬─ ServeDir（静态命中即返，不进 JS）
             ├─ /metrics（不变）
             └─ fallback ── mpsc 帧 ──→ JS 组装 Request → fetch(Request)
H1/H2: axum::serve              ←─ stream 帧 ──  Response.body 边吐边写
H3: h3-axum 同一 Router          oneshot 收尾（status/headers/trailer）
TLS: 既有 TlsListener           WS: axum upgrade → tungstenite 会话
```

- 分层铁律：Transport 只见 `http` crate 类型与字节，永不见 JS 值；
  Handler 只见 WinterCG `Request`/`Response`，永不见 socket；
  Bridge 只传纯数据帧（method/url/headers/chunk/trailer），是唯一手写层。
- 线程模型（§6）：JS 永远独占线程（`LocalSet`，`JSContext !Send`）；
  axum 多线程经 `tokio::mpsc`（有界，背压）+ `oneshot`（收尾）与 JS 线程通信，
  绝不跨线程共享 `&mut JSContext`（`fetch.rs` client 模型同构：native 只解析 +
  spawn，结算回事件循环）。
- 最佳实践清单（网络框架口径，逐项验收）：
  1. 背压：有界通道 + 慢消费反压写端，不整收兜底（S1 即流式，§0-2）。
  2. 限流/超时：复用既有 `governor` + 按请求 deadline（413/408 字节对拍，
     取 `node:http` 口径）。
  3. 优雅停机：复用 `shutdown_signal`，在飞请求排空后再退（serve 现有语义）。
  4. 可观测：`winterjs::serve` target（§4.19），只记 method/uri/status/latency，
     禁记 body/头。
  5. 错误映射：handler 缺失（启动期错）/抛错（500）/超限（413）/WS 握手失败
     （400），无挂起态（§4.35“无监听即挂起”反面：每个入口必有终态）。

## §3 切片（同轮四步，步步可提交）

### T1 — H1 + 双形态 handler + 流式体（最小闭环）

- 做：`--handler <file>` 修饰 flag（仅 `--serve` 下生效，无则纯静态）；
  handler 文件随 serve 启动加载一次（双探：`default.fetch` → 具名 `fetch`）；
  `fallback_service` → mpsc/oneshot 桥 → JS 组装 `Request`（新 native 1-2 个，
  `jsapi_glue` 收敛 + `UNSAFE-BOUNDARY`）→ 调 `fetch` → `Response` 拆解流式写回。
- 验收：GET/POST 回声、404/500、大体 ≥1MB 流式无 §4.40 类崩、并发 20×10、
  缺 handler 启动期可读错；黑盒三件套（正常/报错/边界）。

### T2 — TLS + H2 + keep-alive

- 做：复用 `TlsListener`/`PlainListener` + `axum::serve`，H2 经既有 `hyper`
  特性；复用计数断言（`reusedSocket` 思想，WinterCG 面不断也行，有即测）。
- 验收：自签 TLS 回环 + H2 回声 + keep-alive 复用；`--cert/--key` 单给仍报错
  不降级（既有语义）。

### T3 — H3（同 Router）

- 做：`quinn` endpoint + `h3/h3-quinn` 建连，`h3-axum::serve_h3_with_axum`
  复用同一 `Router`（`examples/server.rs` 形态）；无 TLS 证书则 H3 跳过 + warn，
  H1/H2 不受影响。
- 验收：`curl --http3-only` 回声；H3 缺证书跳过分支（warn + H1 照服）；
  `curl` 不可用环境记档（harness 探针替代）。

### T4 — WS upgrade + 收尾

- 做：axum `upgrade` 直通 `tokio-tungstenite`，JS 侧 `on*` 回调形态；
  静态 fallback + 动态 handler + WS 同端口路由序钉死（WS 路径优先，余下按
  静态 → handler）；`process.exit` 哨兵与在飞请求收尾顺序（§4.70 同族）。
- 验收：WS 回声 + 握手失败 400 + 关闭握手；全量 `cargo test` + 冒烟 5/5；
  代码量审计（§验收线 7k 预算）。

## §4 CLI 形状（§0.8 合规）

- 不新增动作：`actions_present` 不加项（`src/cli.rs:227-278` 维持）。
- 只加修饰 flag：`--handler <FILE>`（`num_args(1)`，值紧贴 flag，§4.26），
  仅 `--serve` 下生效，其余动作下给即错；`--dir/--host/--port/--cert/--key/
  --limit-rps` 语义不变。
- help/补全/man 由同一套 flag 生成（`localized_command`），中英双语照旧。

## §5 测试矩阵（三件套随功能）

| 层 | 内容 | 对齐 |
|---|---|---|
| 模块 | 帧 ↔ `Request`/`Response` 往返、双探加载、背压（慢消费）、错误映射表 | `src/serve*` 内 `#[cfg(test)]` |
| 黑盒 | GET/POST/404/500/静态 fallback/大体流式/413/并发/TLS-H2/H3/WS | `tests/serve.rs`（既有文件续写，不另起） |
| 冒烟 | §3 五条 + `serve` 起停（既有黑盒覆盖） | 构建后必跑 |

## §6 风险（预登记，踩中即记 AGENTS §4）

1. GC 根：桥接两端持 JS 值（`Request`/`Response`/body chunk）→ `Box<Heap>`
   定址 + trace 同步（§4.40/§4.68），新增存储先 grep trace 覆盖。
2. 结算时序：通道结算只排 microtask → `progressed` 回顶再 `RunJobs`
   （§4.18/§4.46），H3/WS 新结算点逐个走无 timer 用例。
3. `__wjs_*` 重名：新增 native 前先 grep（§4.48），判重集函数局部。
4. H3 证书门：自签 `CA:TRUE` 被 rustls 拒（§4.38 症状三），测试证书走 rcgen
   end-entity 或直接复用 serve 黑盒同款。
5. H3 环境：`curl --http3-only` 不可用即 harness 探针替代，不硬等外网。

## §8 S1 — WinterCG 侧存储（2026-09-28 新增，用户拍板）

> plan4 已收官（§0 存档注），本节是用户指定的新活存放处（serve 后续活）。
> 目标：WinterCG 侧自有持久化——全局 `storage` async KV + `localStorage`
> 同步垫片，`turso` 单文件底座（零新依赖，不走 §0.5 引轮），项目级文件隔离。
> CLI：`--storage-path` 修饰（run/eval/test/repl/serve）+ 新动作 `-b/--db`
>（turso 透传查库）配 `--exec` 修饰。纪律：三件套 + 冒烟 + `check-lines.sh`。

| # | 项 | 状态 |
|---|---|---|
| S1-a | Rust 底座（`builtins/storage.rs` + 注册/收尾） | ✅ 2026-09-28（turso KV + 8 natives + 模块静态表 + 单测 2） |
| S1-b | JS 面（`prelude/storage.rs` 全局 `storage`/`localStorage`） | ✅ 2026-09-28（async KV + 同步垫片 + `$blob` 桥） |
| S1-c | CLI（`--db`/`--exec`/`--storage-path` + 双语 help/补全/man） | ✅ 2026-09-28（归属校验 + 沙箱门控 + dry-run） |
| S1-d | 样例/测试/文档（`sample/storage/` + `tests/storage.rs` + 中英 api/cli/README） | ✅ 2026-09-28（黑盒 4 + 全量 nextest strict） |

## §7 不做（书面）

- `node:http` 不动（并存，另案演进；adapter 薄层思想保留给用户态）。
- H3 之前 `Outgo
...[truncated 615 chars]