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
| http2 | 🟢 | ✅（h2c+H3；无 push/trailer/Upgrade，10b-4 triage 全偏离） | trailer/push/Upgrade 三件评估：push 系 Web 已死特性、trailer 等 h2 流式切片、Upgrade 浏览器不用 | 10b |
| readline | 🟢 | ✅（10c-2：行/history/question/按键解码/Emacs 子集/迭代器全绿） | `question` 真实现、行编辑/history/异步迭代器、Emacs 快捷键子集 | 10c |
| tty | 🟢 | ✅（10c-1：net.Socket 基座 + ioctl winsize + 真 raw；构造器非 TTY 即抛与真机同） | net.Socket 基座、ioctl winsize、setRawMode 真标志（termios 按平台记档） | 10c |
| repl（模块面） | 🟡 | ✅ CLI + ✅ 模块（10c-3：REPLServer/start/Recoverable 全绿） | `node:repl` 注册：REPLServer/start/Recoverable（复用 10c 的 Interface） | 10c |
| url | 🟢 | ✅（WHATWG+file 系） | legacy `parse/format/resolve` + `Url` 类 + domainTo*（follow-redirects 已走原生分支，低风险） | 10a |
| timers | 🟢（promises/scheduler） | ✅（setImmediate 近似） | setImmediate check 语义定案（`scheduler.yield/wait` 已有；无 macrotask 分层，近似验收或偏离） | 10a |
| util | 🟢 | ✅（三件未移植） | `parseArgs`、`MIMEType/MIMEParams`、`getSystemErrorName/Message/Map`（uv errno 表随 fs 错误映射） | 10a |
| zlib | 🟢 | ✅（zstd 恒 Fastest） | `crc32` 落地（ISO-HDLC 自实现，零新依赖）；非 Fastest 档等上游 ruzstd（偏离，复议） | 10a |
| dns | 🟢（缺 resolveTlsa） | 🟡（std 底座） | CNAME/MX/TXT/SRV 深件经 hickory-resolver（已在树内；✅ 2026-09-15 拍板：全套+系统配置） | 10d |
| sqlite | 🟢 | ✅ bun:sqlite／— node:sqlite | `node:sqlite` 注册 + DatabaseSync/StatementSync 口径对齐（turso 底座，不跟系统 libsqlite） | 10d |
| dgram | 🟢 | ✅（10a-6 收官：connect/组播投递/ref 真计数；剩 recvbuf 系另切片） | 组播全家（addMembership/dropMembership/setBroadcast/组播 TTL/loopback）、connect/disconnect、ref 真计数 | 10a |
| cluster | 🟡（http 多绑限 Linux） | ✅（线程底座） | Bun 🟡 对等面已齐：primary/worker、fork/disconnect、scheduling 策略 | 10e |
| domain | 🟡 | 🟡（薄面） | 遗留薄面已齐：create/run/bind/intercept + error 路由（同步；异步不路由记档） | 10e |
| crypto | 🟡（缺 ed448/secp256k1/CCM 等） | ✅（差集已闭） | 差集已闭：✅ 2026-09-15 用户全批——`ccm` 0.6 + `ghash` 0.6 直引 + `ed448-goldilocks` 特批钉 `=0.14.0-pre.15` + GCM 任意 iv（J0 手工）+ bf-cbc 删项（真机无 bf 系）+ Ed448/X509；ocb 非 Node 面出局。采购单见 `docs/dependencies3.md` §1 | 10e |
| test | 🟡 | 🟡（起步） | parity 确认（10f 对拍定深浅，不预设切片） | 10f |
| v8 | 🟡（堆统计 JSC 口径） | 🟡（最小桥） | 堆统计/serialize 双方数字皆引擎口径、不可比——书面偏离，不做（见 §4） | — |
| vm | 🟢（全+ESM classes） | ✅（Module/SourceText/Synthetic 全链） | parity 确认（10f 对拍；`compileFunction`/`measureMemory` 行为差即修） | 10f |
| events/fs/stream等 | 🟢 | ✅ | parity 确认（10f 对拍，不预设改动） | 10f |
| sys | 🟢（即 util） | ✅（10a-1：同 util 单例，import/require 双形态） | `node:sys` 别名注册（一行，9x 顺手级） | 10a |
| wasi | 🟡 | — | 不做（plan2 §4 维持否决） | — |
| sea | 🔴 | — | 不做（无对等需求） | — |

## 切片

### 10a 注册与小面（零新依赖，全 JS/既有轮子）

> ✅ 2026-09-15 收官：sys 别名/url legacy/setImmediate+Timeout 真类（附带修
> fire_due 实参展开，见 AGENTS §4.85）/util 三件/crc32/dgram 全家全绿，
> `cargo test` 全绿 0 警告，冒烟 5/5。

- 做：`node:sys` 别名注册；url legacy 面（parse/format/resolve/Url/domainTo*，
  真机逐项对码与文案）；setImmediate 口径定案（`scheduler.yield/wait` 已有，
  只定 check 语义验收标准，不造分层）；
  `util` 三件（parseArgs/MIMEType/getSystemError*）；zlib `crc32`
  （ISO-HDLC 自实现——`flate2::Crc` 不收 seed；同步纯函数）；dgram 组播全家 + connect/disconnect +
  ref 真计数（tokio UdpSocket 底座能力先实测）。
- 验收：`test-url-*.js` legacy 子集、`test-timers-*.js` scheduler 项、
  `test-util-*.js` 三件项、`test-zlib-*.js` crc32 项、`test-dgram-*.js`
  组播项（回环组播 hermetic：`239.0.0.x` 本机环回）点名绿；黑盒三件套照旧。

### 10b HTTP 流式化（深水，http 栈重构）

> ✅ 2026-09-15 收官：帧层重写（IM/Res/Req 进 stream 全家）+ keep-alive
> （Agent 池/`reusedSocket`）+ chunked 双向 + 1MB 大体 + https 随行；
> http2 三件 triage 全偏离。`cargo test` 全绿 0 警告，冒烟 5/5。
> 踩坑见 AGENTS §4.86–4.89。

- 做：http 整收改流式——keep-alive 连接复用、req/res 体流式（IncomingMessage/
  ServerResponse 进 `node:stream` 全家，可 pipe/for-await）、分块编码；
  https 随行（同帧层）；http2 trailer/push/Upgrade 三件先评估后定做/偏离。
- 前置：§4.18/§4.35/§4.46 的结算/排空时序是流式体的生死线，改前重读；
  新事件/数据交叉点走 §4.35"先 emit 再喂体"口径。
- 验收：`test-http-*.js` 流式子集（含 keep-alive 复用计数、chunked 对拍、
  中途 destroy 语义）；既有回环黑盒全绿无回归；大体（≥1MB）压测无 §4.40 类崩
  （GC 压力探针同跑）。

### 10c 终端与交互（tty → readline → node:repl，顺流）

> ✅ 2026-09-15 收官：tty（Socket 基座/ioctl winsize/真 raw/构造抛，pty 实测）
> + readline 全面 + node:repl 全家 + vm sync-in 回落修（AGENTS §4.90）。
> `cargo test` 全绿 0 警告，冒烟 5/5。

- 做：tty 换 net.Socket 基座 + ioctl winsize（libc 经树内轮子？先查，无则
  §0.5；仅 unix，win 记档）+ setRawMode 真标志；readline 全面
  （`question` 真实现、行编辑/history 文件/异步迭代器/Emacs 子集，
  纯 JS over 输入输出流，CLI 的 rustyline 不动）；`node:repl` 注册
  （REPLServer/start/Recoverable/writer，骑新 Interface）。
- 验收：`test-tty-*.js`、`test-readline-*.js` 子集点名绿；`node:repl`
  黑盒（start/prompt/eval 入口/Recoverable）；TTY 相关 hermetic（伪终端无则
  用 pipe + `isTTY` 桩分支断言，不碰真终端）。

### 10d 数据与目录（拍板门×2）

> ✅ 2026-09-15 收官：dns 深件（hickory 全套：Cname/Mx/Txt/Srv/Ns/Ptr +
> resolveAny + getServers/setServers/setDefaultResultOrder，系统配置直读，
> `lookup` 维持 std）+ `node:sqlite`（DatabaseSync/StatementSync：turso 底座，
> CRUD/命名参数/迭代器/列元数据/BigInt 口径全绿）。`cargo test` 全绿 0 警告，冒烟 5/5。

- 做：dns 深件经 hickory-resolver（CNAME/MX/TXT/SRV + promises 面；
  ✅ 2026-09-15 用户拍板：接线做全套——Cname/Mx/Txt/Srv/Ns/Ptr + resolveAny +
  getServers/setServers/setDefaultResultOrder，读系统 DNS 配置（/etc/resolv.conf），
  `lookup` 维持 std `ToSocketAddrs`（真机 getaddrinfo 口径）；crate 已在树内）；
  注册 + DatabaseSync/StatementSync 口径对齐（turso 底座；Node 22+ 实验面
  为准，StatementSync 迭代器/命名参数逐项对）。
- 验收：`test-dns-*.js` 深件子集（hermetic：本地 stub DNS？无则只断 localhost
  + 错误形状，沿 §4.34 符）；`node:sqlite` 黑盒（建表/读写/预处理/事务，
  与 `bun:sqlite` 行为一致性断言）。

### 10e 新域与差集（设计先行）

> ✅ 2026-09-15 收官：crypto 差集（CCM 三档 + GCM 任意 iv + Ed448，
> 真机逐字节交叉）+ cluster 🟡对等面（fork/message/exit/disconnect，
> 线程底座）+ domain 薄面（同步路由，异步不路由记档）。
> `cargo test` 全绿 0 警告，冒烟 5/5。bf-cbc 删项（真机 26 无 bf 系）。
> 踩坑见 AGENTS §4.92–4.93。

- 做：cluster Bun 🟡 对等面（primary/worker、fork/env、disconnect/suicide、
  scheduling 策略 RR；骑 fork/worker 线程底座——真多进程语义不做，书面记档）；
  domain 遗留薄面；crypto 差集开工（✅ 2026-09-15 全批落锁：ccm/ghash/
  ed448-pre.15，bf-cbc 真机实测后定，见 `docs/dependencies3.md` §1）。
- 验收：`test-cluster-*.js` 基础项（fork+message+exit 码）；
  `test-domain-*.js` 薄面项；crypto 评估报告进 `docs/dependencies.md` 候选节。

### 10f 对拍验收（Bun 高度的证明）

> ✅ 2026-09-19 收官：点名全覆盖——10f 清单 22 模块 + §1 划入的 `test`
> 行（83 件 `test-runner-*` 补点名，DIFF 66 分簇定性：API 面 ~35 下轮可修 /
> 自 spawn CLI ~18 另案 / reporter 深度 ~13 偏离；矩阵维持 🟡，见
> `docs/bun-parity.md ## test`）。其余 23 域红项逐簇定性——本轮修复进
> 三件套回归，长尾分簇"下轮可修 / 另案（理由）/ 双红对齐"全部书面记档
> （红即修或记 §4，无第三种状态）。§1 矩阵据此维持 ✅/🟡+偏离注。
> 各域转化：buffer 63✅+9⏭️、timers 45✅+14⏭️、vm 47✅+26🟡+27⏭️、
> stream 190✅+50⏭️、fs 同绿 39→123、net 30→86（七轮校验族 11/11）、
> crypto 24→38+六轮 PSS/PBES2/raw 门、http 95→125+流式头体分离、
> worker 35→55+七轮环境面、zlib Zip 面 15/17、child 同步族+exec/abort 面、
> url/timers/dc/dns/path/assert/os 收官红 0。连带根修：§4.137 入口失败+
> 开着句柄永不收割（eval/模块双路）。`cargo test` 全绿 0 警告（仅依赖
> proc-macro-error2 的 future-incompat 提示，非本仓代码），冒烟 5/5。

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

## §5 Bun 🟢 域欠账清单（2026-09-19 盘点，用户拍板："Bun 没实现的不做，
## Bun 实现了的才是欠账"，先记档以后再说）

> 口径修正（本轮起生效）：欠账判定以 **bun-compat.md 快照的 Bun 列**为准，
> 不再以 node 套件红数为准——child_process（Bun 🟡：IPC 若干缺口）与
> worker_threads（Bun 🟡）整体降级为"parity 确认"档，其 DIFF 中 Bun 同缺的
> 簇**出局不算欠账**；仅下表所列 Bun 🟢 域的簇是真实欠账。

### 已收官（Bun 🟢 且红 0 / 仅引擎边界偏离，无欠账）

buffer、events、stream、url、path、querystring、punycode、string_decoder、
os、assert（message 文本偏离）、timers、util（`%o` 布局引擎边界）、vm
（26 偏离记档）、trace_events、dns、readline、tty、sqlite、repl、crypto
（超 Bun 🟡）、sys、dgram（除 recvbuf 小簇，见下）。

### 欠账（Bun 🟢 且我们有记档红簇，逐簇以后再说）

| 域 | 欠账簇 | 规模 | 性质 |
|---|---|---|---|
| http | TIMEOUT 簇（expect-continue/upgrade/trailer/管线背压/max-connections） | ~110 件 | 流式深化，与 10b 整收口径的接缝工程 |
| http | 校验长尾 / chunk 限深 / 假 socket 深件 | ~35 件 | 逐 API 续补，下轮可修 |
| http2 | compat 层 `Http2ServerRequest/Response` 全流面 | ~105 件 | 最大单体簇，与 10b 同型工程 |
| http2 | server 流面 / settings/priority/ALPN 校验 | ~15 件 | 随 compat 轮 |
| fs | validators 尾件 / unhandled-rej 尾件 | ~40 件 | 逐 API 续补，下轮可修 |
| fs | watch hang（promises-watch/recursive/encoding）/ watch-ignore-glob / flush 选项 / pipe 读形 | ~23 件 | watch 事件流 + glob 语义，中等工程 |
| net | server close/listen 时序（drop-connections/pause-on-connect 等） | ~10 件 | 下轮可修 |
| net | TIMEOUT 9 / Happy Eyeballs 3 / worker 投递 3 / large-string 1 | ~16 件 | 背压语义 + 竞速回落，另案 |
| zlib | 增量语义（flush/premature-end/truncated/write-after-end/reject-garbage） | ~6 件 | 需真流式编解码状态机，与"整收"架构冲突 |
| zlib | brotli 字典 / zstd pledged-src-size / Web `DecompressionStream` | ~4 件 | 零散，DecompressionStream 可另切片 |
| dgram | ~~recvbuf 系~~ ✅ 2026-09-19 转绿（G1：recvbuf/sendbuf 四方法+隐式绑定+EMSGSIZE 回调路由+数组 send+族匹配解析+ALREADY_BOUND/EBADF 形状；DIFF 53→32，余 connect 族/membership/bindSync/ipv6only 等独立小簇 ~32 件顺延） | 小簇 | 已动工，余件逐 API 续补 |
| child_process | async AbortSignal 尾件 / async 句柄面 / exec 多字节截断 | ~30 件 | Bun 实现了这些，欠；**fork/IPC handle 传递 ~25 件出局**（Bun 🟡 IPC 缺口，同缺不追） |
| worker_threads | terminate 深水 / 环境面尾件 | 部分 | Bun 🟡 → parity 确认档；Atomics.wait（引擎面）与 stdio 流面（Bun 同缺倾向）另核 |

### 待核对（1 项）

- **URLPattern**（url 域 3 件 ⏭️）：bun-compat 快照无此行，需对 Bun 1.3 实测
  确认其是否实现；实现了即入上表，没实现维持出局。

> 验收口径：上表全部转绿/或逐簇书面偏离前，Phase 10 对"Bun 🟢 域"不算
> 逐字节到位；10f 的收官（§1 矩阵/报告/终局门）不受影响——欠账已全部
> 定位、定性、定量。
