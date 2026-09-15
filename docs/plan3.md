# Phase 10 计划 — node: 落地（再到 Bun 高度）

> 立项 2026-09-15（用户拍板）。目标：`node:` 兼容从 deno 高度（plan2 收官，
> 半 16 全达/超、全 23 剩 5 欠账）再到 **Bun 高度**——Bun 表 🟢 项逐项对齐，
> Bun 🟡 项确认 parity，差集书面偏离（见 §4）。
> Bun 基线：`docs/bun-compat.md` 立项快照（`oven-sh/bun` HEAD `6a92015`，
> targeting Node v26；第三方全量跑分 Bun 1.3.14 约 40.6%，见 bun-compat 头注）。
> 立项后不追 Bun HEAD（它走得快），以快照为准；终局重测时再对新版复核。
>
> 方法（三源对照，沿用 plan2）：**node 定语义**（`lib/` 原文 +
> `test/parallel` 断言原文入库），**bun 当词典**（`internal/` 文件名 1:1），
> **deno 对分层**（`op_*` Rust 底 ↔ natives，TS 壳 ↔ prelude）。
>
> 纪律：AGENTS 三件套（模块单测 + 黑盒正常/报错/边界 + 冒烟 5/5，
> `UNSAFE-BOUNDARY` 新增配 panic 用例）；踩坑记 AGENTS §4；
> vendoring 三家 JS 文件保留 MIT 头；新 crate 一律先走 §0.5（找轮子 →
> 记 `docs/dependencies.md` → **停下问用户** → 点头才引入）。
>
> 验收线（终局）：§3 矩阵 Bun 🟢 项全 ✅、Bun 🟡 项 parity 确认（✅/🟡+偏离注），
> 10f 对拍报告入库，`cargo test` 全绿 0 警告，冒烟 5/5。

## §1 缺口清单（Bun 快照 vs winterjs 现状，2026-09-15）

| 模块 | Bun | 现状 | 缺口 | 切片 |
|---|---|---|---|---|
| http | 🟢 | ✅（整收口径） | keep-alive、流式 req/res 体、IncomingMessage/ServerResponse 流全家 | 10b |
| https | 🟡（无 SNI 等） | ✅（同 http 记档） | 随 http 流式化走；SNI 回调等与 Bun 同缺，不追 | 10b |
| http2 | 🟢 | ✅（h2c+H3；无 push/trailer） | trailer/push/Upgrade 三件评估（能做做、不能偏离） | 10b |
| readline | 🟢 | 🟡（最小桥） | `question` 真实现、行编辑/history/异步迭代器、Emacs 快捷键子集 | 10c |
| tty | 🟢 | 🟡（薄面） | net.Socket 基座、ioctl winsize、setRawMode 真标志（termios 按平台记档） | 10c |
| repl（模块面） | 🟡 | ✅ CLI／— 模块 | `node:repl` 注册：REPLServer/start/Recoverable（复用 10c 的 Interface） | 10c |
| url | 🟢 | ✅（WHATWG+file 系） | legacy `parse/format/resolve` + `Url` 类 + domainTo*（follow-redirects 已走原生分支，低风险） | 10a |
| timers | 🟢（promises/scheduler） | ✅（setImmediate 近似） | setImmediate check 语义定案（`scheduler.yield/wait` 已有；无 macrotask 分层，近似验收或偏离） | 10a |
| util | 🟢 | ✅（三件未移植） | `parseArgs`、`MIMEType/MIMEParams`、`getSystemErrorName/Message/Map`（uv errno 表随 fs 错误映射） | 10a |
| zlib | 🟢 | ✅（zstd 恒 Fastest） | `crc32`（flate2 自带 `Crc`，零新依赖）；非 Fastest 档等上游 ruzstd（偏离，复议） | 10a |
| dns | 🟢（缺 resolveTlsa） | 🟡（std 底座） | CNAME/MX/TXT/SRV 深件经 hickory-resolver（已在树内，**接线前按 §0.5 问用户拍板**） | 10d |
| sqlite | 🟢 | ✅ bun:sqlite／— node:sqlite | `node:sqlite` 注册 + DatabaseSync/StatementSync 口径对齐（turso 底座，不跟系统 libsqlite） | 10d |
| dgram | 🟢 | ✅（base 面） | 组播全家（addMembership/dropMembership/setBroadcast/组播 TTL/loopback）、connect/disconnect、ref 真计数 | 10a |
| cluster | 🟡（http 多绑限 Linux） | — | Bun 🟡 对等面：primary/worker、fork/disconnect、scheduling 策略（骑 fork/worker 底座，不做真多进程超集） | 10e |
| domain | 🟡 | — | 遗留薄面：create/run/bind/intercept + error 路由（小） | 10e |
| crypto | 🟡（缺 ed448/secp256k1/CCM 等） | ✅（缺口不重合：GCM iv 限 12B、无 ccm/ocb） | 差集评估：ed448（ed448-goldilocks）、CCM（ccm crate）、GCM 任意 iv；**新 crate 走 §0.5，先评估后问用户** | 10e |
| test | 🟡 | 🟡（起步） | parity 确认（10f 对拍定深浅，不预设切片） | 10f |
| v8 | 🟡（堆统计 JSC 口径） | 🟡（最小桥） | 堆统计/serialize 双方数字皆引擎口径、不可比——书面偏离，不做（见 §4） | — |
| vm | 🟢（全+ESM classes） | ✅（Module/SourceText/Synthetic 全链） | parity 确认（10f 对拍；`compileFunction`/`measureMemory` 行为差即修） | 10f |
| events/fs/stream等 | 🟢 | ✅ | parity 确认（10f 对拍，不预设改动） | 10f |
| sys | 🟢（即 util） | —（未单注册） | `node:sys` 别名注册（一行，9x 顺手级） | 10a |
| wasi | 🟡 | — | 不做（plan2 §4 维持否决） | — |
| sea | 🔴 | — | 不做（无对等需求） | — |

## 切片

### 10a 注册与小面（零新依赖，全 JS/既有轮子）

- 做：`node:sys` 别名注册；url legacy 面（parse/format/resolve/Url/domainTo*，
  真机逐项对码与文案）；setImmediate 口径定案（`scheduler.yield/wait` 已有，
  只定 check 语义验收标准，不造分层）；
  `util` 三件（parseArgs/MIMEType/getSystemError*）；zlib `crc32`
  （flate2::Crc 直通，sync+async 双形态）；dgram 组播全家 + connect/disconnect +
  ref 真计数（tokio UdpSocket 底座能力先实测）。
- 验收：`test-url-*.js` legacy 子集、`test-timers-*.js` scheduler 项、
  `test-util-*.js` 三件项、`test-zlib-*.js` crc32 项、`test-dgram-*.js`
  组播项（回环组播 hermetic：`239.0.0.x` 本机环回）点名绿；黑盒三件套照旧。

### 10b HTTP 流式化（深水，http 栈重构）

- 做：http 整收改流式——keep-alive 连接复用、req/res 体流式（IncomingMessage/
  ServerResponse 进 `node:stream` 全家，可 pipe/for-await）、分块编码；
  https 随行（同帧层）；http2 trailer/push/Upgrade 三件先评估后定做/偏离。
- 前置：§4.18/§4.35/§4.46 的结算/排空时序是流式体的生死线，改前重读；
  新事件/数据交叉点走 §4.35"先 emit 再喂体"口径。
- 验收：`test-http-*.js` 流式子集（含 keep-alive 复用计数、chunked 对拍、
  中途 destroy 语义）；既有回环黑盒全绿无回归；大体（≥1MB）压测无 §4.40 类崩
  （GC 压力探针同跑）。

### 10c 终端与交互（tty → readline → node:repl，顺流）

- 做：tty 换 net.Socket 基座 + ioctl winsize（libc 经树内轮子？先查，无则
  §0.5；仅 unix，win 记档）+ setRawMode 真标志；readline 全面
  （`question` 真实现、行编辑/history 文件/异步迭代器/Emacs 子集，
  纯 JS over 输入输出流，CLI 的 rustyline 不动）；`node:repl` 注册
  （REPLServer/start/Recoverable/writer，骑新 Interface）。
- 验收：`test-tty-*.js`、`test-readline-*.js` 子集点名绿；`node:repl`
  黑盒（start/prompt/eval 入口/Recoverable）；TTY 相关 hermetic（伪终端无则
  用 pipe + `isTTY` 桩分支断言，不碰真终端）。

### 10d 数据与目录（拍板门×2）

- 做：dns 深件经 hickory-resolver（CNAME/MX/TXT/SRV + promises 面；
  **开工前按 §0.5 问用户拍板接线**，crate 已在树内）；`node:sqlite`
  注册 + DatabaseSync/StatementSync 口径对齐（turso 底座；Node 22+ 实验面
  为准，StatementSync 迭代器/命名参数逐项对）。
- 验收：`test-dns-*.js` 深件子集（hermetic：本地 stub DNS？无则只断 localhost
  + 错误形状，沿 §4.34 符）；`node:sqlite` 黑盒（建表/读写/预处理/事务，
  与 `bun:sqlite` 行为一致性断言）。

### 10e 新域与差集（设计先行）

- 做：cluster Bun 🟡 对等面（primary/worker、fork/env、disconnect/suicide、
  scheduling 策略 RR；骑 fork/worker 线程底座——真多进程语义不做，书面记档）；
  domain 遗留薄面；crypto 差集评估报告（ed448/CCM/GCM-任意-iv 三项的
  crate 候选 + 纯度 + 量级，**评完问用户再开工**，不开工也算交付）。
- 验收：`test-cluster-*.js` 基础项（fork+message+exit 码）；
  `test-domain-*.js` 薄面项；crypto 评估报告进 `docs/dependencies.md` 候选节。

### 10f 对拍验收（Bun 高度的证明）

- 做：Node `test/parallel` 子集逐模块点名（events/fs/stream/crypto/http/net/
  timers/util/dns/zlib/vm/worker/buffer/path/url/querystring/punycode/
  string_decoder/diagnostics_channel/trace_events/os/assert/child_process），
  出 parity 报告：绿/红/偏离（红即修或记 §4，无第三种状态）。
- 交付：`docs/bun-parity.md`（模块 × 用例 × 结果 × 偏离理由），§3 矩阵据此
  转正（✅/🟡/偏离注）；红项修完进三件套回归。

## §4 不做与书面偏离（v1）

- `wasi`（plan2 §4 维持否决）、`sea`（无对等需求）。
- `v8` 堆统计/serialize：双方数字皆引擎口径、跨引擎不可比——跳过是正确语义，
  不是欠账（`startupSnapshot` 桥保留，vite 解挡）。
- zstd 非 Fastest 档：等上游 ruzstd 补齐实现后复议（AGENTS §4.37）。
- `setImmediate` check 阶段语义：本仓无 macrotask 分层，`setTimeout(0)` 近似
  为既定口径（10a 只定案验收标准，不造分层）。
- 既有记档维持（不因换高度而重做）：fork 线程底座（stdio 恒 null 等）、
  GCM iv 限 12B（除非 10e 批了重做）、promises 底层同步实现、CCW 跨域判定、
  H3 串行、`node:test` reporter 深度（Bun 同 🟡）。
