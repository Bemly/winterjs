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
（超 Bun 🟡）、sys、dgram（recvbuf 簇已收官，余 connect/membership 等小簇见下）。

### 欠账（Bun 🟢 且我们有记档红簇，逐簇以后再说）

| 域 | 欠账簇 | 规模 | 性质 |
|---|---|---|---|
| http | TIMEOUT 簇（expect-continue/upgrade/trailer/管线背压/max-connections） | ~110→2026-09-23 G11 收尾轮（见下）；残：drain-writable-length/outgoing-properties（net.Socket 写侧流式化 + eager-parse outgoing 队列，两件同根基建另轮）+ socket.push ×2（同基建）+ execPath spawn ~18（CLI 全 flag 铁律冲突，需拍板）+ parser 内省 ~4（_http_common 面，记档偏离） | 流式深化，与 10b 整收口径的接缝工程 |
| http | ~~校验长尾 / chunk 限深~~ ✅ 2026-09-19 转绿（G3 六提交：chunk 扩展 413/trailer 431/校验门 15 件/Agent createSocket/IPC socketPath/write-after-end 语义/FIN 半开收口，点名 45 件 SAME0；余 OutgoingMessage outputData 缓冲模型 5 件**出局另轮专项**、假 socket socket.push 2 件需 net 流式化、TIMEOUT 110 归下行） | ~35→5 件 | 已收官，残件另案 |
| http2 | compat 层 `Http2ServerRequest/Response` 全流面 | ~105 件 | 最大单体簇，与 10b 同型工程 |
| http2 | server 流面 / settings/priority/ALPN 校验 | ~15 件 | 随 compat 轮 |
| fs | ~~validators 尾件 / unhandled-rej 尾件~~ ✅ 2026-09-19 转绿（G4 两提交：constants 16键+null原型/stat-bigint 全option链+Ns四键/throwIfNoEntry 只豁免 ENOENT/__Stats DEP0180 可调用形/fd_table 预注册标准流/statfs frsize+bigint/utimes·lutimes·futimes 秒口径+utimensat/lchown·lchmod·_toUnixTimestamp·lutimes 新面/rename oldPath·newPath/truncate len 校验/null-byte 全API含URL/__fdCb 值优先+孤儿promise根除/latin1字节直映/writeFile encoding+abort交付面/WriteStream真open；AGENTS §4.149）；残件：roundtrip 末段 async_hooks FSREQCALLBACK 资源面另案 | ~40→1 件（async_hooks 另案） | 已收官，async_hooks 资源面另轮 |
| fs | ~~watch hang（promises-watch/recursive/encoding）/ watch-ignore-glob / flush 选项 / pipe 读形~~ ✅ 2026-09-20 转绿（G8 八提交：ignore 全形态+递归相对路径/StatWatcher 单例+异步 stop+零 Stats/FSWatcher ref-unref+异步 close/encoding 转码/promises.watch 迭代+校验/_getActiveHandles/flush 选项/exit 首码赢/前沿防抖+Create 二判据+stat unref+首轮收口，watch 域 40/46；AGENTS §4.152-155）；残件：fs.glob ×2（Bun 快照无此行，记档另案）+ flush 三套件（待 node:test runner 深度）+ enoent-after-deletion 间歇超时另查 | ~23→3 件 | 已收官，残件另案 |
| net | ~~server close/listen 时序~~ ✅ 2026-09-19 转绿（G2 server 选项面 blockList/maxConnections/drop/pauseOnConnect/close 窗口 + G2 relisten `__closing` 残留旗修复）；余 cargo-harness 下 net_remote/unix_socket 的 SEGV 已修（with_str_args GC 悬垂） | ~10 件 | 已收官 |
| net | ~~TIMEOUT 9 / Happy Eyeballs 3 / worker 投递 3 / large-string 1~~ ✅ 2026-09-19 G6：13/16 转绿（large-string 分包解码/async-iter end(cb)挂finish/write-after-end-nt EPIPE面/abort-controller 信号面+侧表/ipv6 lookup family透传/HE-default 串行回落/HE校验×2；max-connections×2+bytes-stats 随 G2 已绿），残件 6 件 infra 级记档——throttle（native 读门控+写EAGAIN流控）/cluster×2（internalMessage协议，非net面）/worker×3（跨线程fd移交，可骑holdToken机制，独立轮） | ~16→6 件 | 残件另轮（bun-parity net 八轮，AGENTS §4.148） |
| zlib | ~~增量语义~~ ✅ 2026-09-19 转绿（G9-1 Rust 状态机 + G9-2 JS 流类接线：write 即时压出/flush 档位即时出边界/finishFlush 容忍/rejectGarbageAfterEnd/一次性面切引擎错误口径对真机；6 目标套件+连带 5 件全绿，zlib 域 73/81） | ~6 件 | 已收官 |
| zlib | ~~brotli 字典 / zstd pledged-src-size / Web DecompressionStream~~ ✅ 2026-09-19 转绿（G9-3：raw 字典构造期主动 set_dictionary；一次性压缩走引擎收口（dict/pledged/错误口径单点，字节对比零回归）；__zDictBytes 严格校验；pledgedSrcSize 全校验面 + 引擎终检 errno=72；constants.ZSTD_error_* 28 项；Web CS/DS 全局+stream/web——4 格式 roundtrip/尾垃圾 readable TypeError/proto 独立；6 目标套件全绿 + 82 件对拍零回归） | — | 已收官 |
| dgram | ~~recvbuf 系~~ ✅ 2026-09-19 转绿（G1：recvbuf/sendbuf 四方法+隐式绑定+EMSGSIZE 回调路由+数组 send+族匹配解析+ALREADY_BOUND/EBADF 形状；DIFF 53→32，余 connect 族/membership/bindSync/ipv6only 等独立小簇 ~32 件顺延） | 小簇 | ✅ 2026-09-21 独立小簇轮：send-bad-arguments（buffer 先行/端口先于地址/越界）+ child-index（fork env 宽容）+ cluster-reuse（SO_REUSEADDR 双落）+ getSendQueue*/getActiveResourcesInfo 双 API；16→11 跳过（internals）+ 5 G6 轮（_getServer/handle/3 TIMEOUT）+ 2 API 落盒（queue-info respawn transport 出局/unref 挂死归 cluster 退出 race，见 §4.177） |
| child_process | ~~async AbortSignal 尾件 / async 句柄面 / exec 多字节截断~~ ✅ 2026-09-21 转绿（G5b 三提交：参数归一逐字/kill+stdin+flush/stdio 转交+close 重放/fork env+internalMessage+net reusePort；作用域 76 件 DIFF 16→4，黑盒 19→23） | ~30→4 件 | 残 4 出局（handle 传递，见下行） |
| child_process | 残件：send-keep-open/server-close/recv-handle/send-returns-boolean（live 句柄跨会话 + 4 元 stdio + backlog 记账） | 4 件 | 出局（Bun 🟡 IPC 缺口同缺，拍板维持；线程底座 token 共享另轮） |
| worker_threads | terminate 深水 / 环境面尾件 | 部分 | Bun 🟡 → parity 确认档；Atomics.wait（引擎面）与 stdio 流面（Bun 同缺倾向）另核 |

### 待核对（1 项，已核对）

- **URLPattern** ✅ 2026-09-20 实测归属：Bun 1.4.2 实现（`new URLPattern` +
  `exec().pathname.groups` 可用），本仓 `typeof URLPattern === "undefined"`——
  按 §5 口径（Bun 实现了的才是欠账）列为真实欠账，另轮专项（WHATWG
  URLPattern 匹配语义 + groups 回填）。
  ✅ 2026-09-21 收官：`urlpattern` 0.6 接线（Rust 桥 + prelude 真类 +
  `node:url` 双导出），真套件三件双侧 rc=0，黑盒 `phase11_urlpattern_*`，
  组序记档偏离（AGENTS §4.176）。

> 验收口径：上表全部转绿/或逐簇书面偏离前，Phase 10 对"Bun 🟢 域"不算
> 逐字节到位；10f 的收官（§1 矩阵/报告/终局门）不受影响——欠账已全部
> 定位、定性、定量。

### 欠账轮落账与新会话入口（2026-09-19 session 收官，新会话据此开工）

**本 session 完成（全部已提交 master）：**

| 簇 | 状态 |
|---|---|
| G1 dgram recvbuf | ✅ 上 session（DIFF 53→32） |
| G2 net server 选项面 | ✅ 上 session |
| G9-1 zlib Rust 状态机 | ✅ 上 session |
| G9-2 zlib JS 流类接线 | ✅ 上轮（6 目标套件全绿 + 连带 5 件，zlib 域 73/81） |
| G3 http 校验长尾/chunk 限深 | ✅ 上轮（子 agent 六提交移植，点名 45 件 SAME0 + 连带 15 件） |
| **G9-3 zlib 尾件**（brotli/zstd 字典/pledged/Web CS·DS） | ✅ 本轮（6 目标套件全绿 + 82 件对拍零回归；一次性压缩走引擎收口 + raw 字典构造期设 + 严格字典校验 + pledged errno=72 + constants 28 项 + Web 全局；AGENTS §4.145-147） |
| **G6 net 尾件** | ✅ 本轮 13/16 转绿（large-string/async-iter/write-after-end-nt/abort/ipv6/HE×3 + G2 顺手 2 件；全量 159 件对拍零回归，black-box 223 全绿）；残件 6 件 infra 级记档（throttle 流控/cluster 协议×2/worker fd 移交×3）；AGENTS §4.148 |
| **G4 fs validators 尾件** | ✅ 本轮 20 套件转绿（stat族/constants/bigint/throwIfNoEntry/DEP0180/fd标准流/statfs frsize+bigint/utimes秒口径/lchown·lchmod·lutimes·_toUnixTimestamp新面/null-byte全API/rename·truncate·fchown·mkdir校验面/latin1/writeFile encoding+abort/WriteStream真open）；黑盒 fs 13/13 + 冒烟 5/5；AGENTS §4.149（e94db50 + 3a4704e） |
| **G5 child 尾件** | ✅ 本轮 25 套件转绿（ChildProcess.spawn 方法面/spawn 事件+多监听fan-out+dispose/stdio 数组+spawnargs/空字节横向校验/`-p` 自举/env 归一/ipc 门/paused 读/removeAllListeners/二次 disconnect 门/uid-gid EPERM/send 校验/stdin 继承/ERR_IPC_ONE_PIPE+INVALID_HANDLE_TYPE；黑盒 child 18/18 + 冒烟 5/5；AGENTS §4.150-151） |
| **G8 fs watch 尾件** | ✅ 本轮 30 套件转绿（ignore 全形态+递归相对路径/StatWatcher 单例EE+异步 stop+零 Stats 首轮/FSWatcher ref-unref+异步 close/encoding 转码/promises.watch 迭代+全校验+_getActiveHandles/flush 选项/exit 首码赢/前沿防抖+Create 二判据分发侧+stat 真 unref+首轮 return 收口）；残件：fs.glob ×2（Bun 快照无此行，记档另案）+ flush 三套件（待 node:test runner 深度）；黑盒 fs 18/18 + 冒烟 5/5；AGENTS §4.152-155 |
| 集成修复 ×2 | ✅ net relisten `__closing` 挂死（46 分钟）+ net SEGV（with_str_args GC 悬垂）+ stream 9b 回归（上轮） |
| 验收 | `cargo test` 全量 21 target 0 失败 0 警告 + 冒烟 5/5 |
| AGENTS.md | ✅ §4.140-144 四坑 + §4.145-147 三坑 + §4.148 net 五坑 |
| 本节欠账表 | ✅ G3/G2/G9-2/G9-3/G6 划线转绿 |

**2026-09-21 test Slice A 收官**：API 核心面 11 套件转绿（suite/ctx/tags/
plan/waitFor/subtest/getTestContext/register；对拍 83 件 SAME0 6→17、
DIFF 64→53，零回归；黑盒 `phase10f_test_*` 四件；AGENTS §4.178）。
残 53：run API ~21/mock 全家并入下轮、spawn CLI ~18 与 reporter ~13 维持另案。

**2026-09-21 test Slice B1 收官**：MockTracker 核心落地
（`node:internal/test/mock` 新建 + 钩子归属重构为真机 Test.run 口径；
mocking.js 55/56，唯一红为私有字段 V8 文案引擎偏离；82 件零回归；
黑盒 `phase10f_test_mock_*` 两件；AGENTS §4.180）。
残：mock-timers 2 件（B2：fake 计时器基建）+ run API ~21 另轮。

**2026-09-21 test Slice B2 收官**：mock.timers 落地（对拍 SAME0 17→19、
DIFF 53→51，零回归；黑盒 `phase10f_test_mock_timers_*` 两件；
AGENTS §4.181）。残：run API ~21 另轮（`run()` 事件流）。

**2026-09-21 test Slice C 收官**：run(none) 事件流落地（同进程加载 +
事件六件 + 发现 + only/tag/plan 门 + 钩子时序全对；对拍 SAME0 19→24、
DIFF 51→46，零回归；testmod 按域拆 core/run；黑盒
`phase10f_test_run_none_and_plan_gates`；AGENTS §4.182）。
残：run process 隔离 ~15（子进程/线程传输）+ spawn CLI ~18 + reporter ~13。

**2026-09-21 test Slice E 收官**：run 语义深化（plan 子计数/stopTest 超时/
TestPlan wait/legacy done/tag 过滤子集/entryFile/调用点文件/种子洗牌/run
coverage 校验；对拍 plan/tags-events/entry-file/randomize 四转绿，
SAME0 30→34、DIFF 39→35，零回归；黑盒 `phase10f_test_run_semantics_*` +
`phase10f_test_run_tag_filter_and_randomize`；AGENTS §4.184）。
残：run 并发/上报深度 ~6 + spawn CLI ~18 + reporter ~13 + mocking 单行。

**2026-09-21 test Slice D 收官**：run(process) 经 worker 传输落地
（expect-error ×2/todo-skip/filetest-location 四转绿 + coverage ×2 附带；
对拍 SAME0 24→30、DIFF 46→39，零回归；黑盒
`phase10f_test_run_process_and_expect_failure`；AGENTS §4.183）。
残：run 并发/超时/randomize/tag 过滤 ~8 + spawn CLI ~18 + reporter ~13。

**2026-09-22 G11 http TIMEOUT 首轮收官**：18 提交（请求超时全家/101 摘池/
Trailer 校验/管线/FIN 递延/流出/abort 级联/1xx/头形态/maxHeadersCount/
keep-alive 修正/回池门/池键/ready 解禁/setTimeout 门控订正 + 黑盒
`tests/node/http/timeout.rs` 4 用例 + 构建 0 警告 + 冒烟 5/5 + http 域 17/17；
约 40 件转 SAME，http-only TIMEOUT 82→63、DIFF 105→100；
sweep2 混二进制（05:47–06:32 跨两次构建）仅当趋势，终局需干净重扫；
未闭环 3 件见上表 http 行；AGENTS §4.185）。

**2026-09-22 G11 半开双杀收官**：5 提交（写端 Close 即发 + holding/halfhold/
native 注册/JS 递延/黑盒；单测 `net_halfhold_balance` + 黑盒
`phase11_net_halfopen_releases_loop` + 冒烟 5/5 + http/net/stream/dgram
域 + bin 185 全绿；`server-keep-alive-timeout`/`server-close-idle-wait-
response` 转 SAME0；AGENTS §4.186）。

**2026-09-22 G11 upgrade 轮收官**：4 提交（升级块判定门 + 三形态 + 体路由/
直调/spill + socket 暂存/destroy 异步 + 黑盒 `phase11_http_upgrade_faces`；
node 域 271 全绿（t4）+ 冒烟 5/5；upgrade 6 件全转 SAME0；AGENTS §4.187）。

**2026-09-22 G11 头面 batch5 收官**：校验门三件（数字头名 HTTP_TOKEN/
奇数组 ARG_VALUE/重发头 HEADERS_SENT）+ 拼写覆写 + 220 unknown +
数组双行 + 对形 writeHead + Host 恒拼/IPv6 框 + 拒写旗（新码
BODY_NOT_ALLOWED，检查禁入 `_write`）+ 黑盒
`tests/node/http/surface.rs::phase11_http_header_face_batch5`；
28 件头面对拍 SAME0（`header-overflow` 的 `socket.push` 系既定另轮）+
http/net/https 域 + 冒烟 5/5 + 构建 0 警告；AGENTS §4.188）。

**2026-09-22 G11 TIMEOUT 深水第一铲**：hostname 优先 + auth 补 Basic +
CONNECT（authority-form/Host 取 path/隧道 detach 双端 end:1）+
server timeout 进门 + socket HWM 65536 + 基类 setTimeout + req.protocol +
黑盒 `tests/node/http/surface.rs::phase11_http_timeout_deep_host_auth_connect`；
11 件转 SAME0；http 27/27 + net 18/18 + 冒烟；AGENTS §4.189。
未闭环：`outgoing-properties`（wl 记账专项）+ handler 抛吞 hang（另单元）。

**2026-09-22 G11 TIMEOUT 深水第二铲**：server 选项类（IM/SR 请求期构造）+
建连选项透传（HWM 进 Socket 构造器）+ socket 双 65536/res 跟随 +
黑盒 `tests/node/http/surface.rs::phase11_http_server_options_surface`；
3 件转 SAME0；http 28/28 + net 18/18 + 冒烟；AGENTS §4.190。
附带 splitting 一件（ERR_INVALID_CHAR `["key"]` 后缀）：response-splitting
转 SAME0 + 黑盒 `phase11_http_invalid_char_key`；AGENTS §4.191。
附带 response 双件（write-after-end 拦截 + 状态码门注册）：res-write-after-end/
response-statuscode 转 SAME0 + 黑盒 `phase11_http_response_gates`；
AGENTS §4.192；未竟 response-cork（另单元）。

**2026-09-23 G11 收尾轮收官**：7 提交（cork 面 / uncaught 双向 / 小面四件 /
multi-CL；§4.193）。cork 三件套（response-cork/drain-cork/outgoing-end-cork）+
uncaught-from-request-callback + test-http-1.0 + null-prototype-options +
max-headers-count + response-multi-content-length 转 SAME0；request-timeout-
keepalive 实为绿（15s sweep alarm 误判"真机自挂"，25s 实证双边绿——§4.193 坑五）。
终局 serial 重扫（sweep4，25s alarm，干净二进制，TEST_THREAD_ID 3599）：
409 件 SAME0=294/SAME1=0/DIFF=102/TIMEOUT=13（8 件转绿逐项复核在册）。
基建轮（2026-09-23，AGENTS §4.194）终局重扫（sweep6，同口径）：
409 件 SAME0=307/SAME1=0/DIFF=91/TIMEOUT=11（+13：push 面 2 + HPE 面 5 +
记账/队列 2 + eager 连带 4；零新增红项）。残件：reuse-drained（process.report
缺失，另域）+ execPath spawn ~18（待拍板）+ parser 内省 ~4（记档偏离）。
黑盒 `phase11_http_cork_faces` + `phase11_http_uncaught_throws` + http 域 25/25 +
node 域 278/278 + 冒烟 5/5。**残件全部定性**：drain-writable-length +
outgoing-properties（outputData 记账 + writableLength 合成 getter）与
header-overflow/read-in-error（socket.push）同根——需 **net.Socket 写侧
流式化**（socket 层写队列/HWM/drain）+ **eager-parse outgoing 队列**
（管线请求立即建 res、无 socket 排队），基建轮另案；execPath spawn ~18 件
（套件 spawn process.execPath 裸脚本 vs CLI 全 flag 铁律 §0.8，需拍板）；
parser 内省 ~4（_http_common parser.initialize/onIncoming 面，记档偏离）；
余散件（async_hooks 资源面/domain 集成/Atomics.wait/process.report/
optimize-empty-requests 等）逐套件记 bun-parity。

**2026-09-23 对拍提速 mapper + createConnection 转绿**：§4.202-① 断言 mapper
落地（`tests/node/helpers.rs` run_suite_mapped：实际值截 200 字 + 套件侧
调用点折算物理行 ±2 节选三行定位；`WJS_MAP_SUITE=` + `phase_mapper_locate_suite
-- --ignored` 用；raw-headers 物理 110 / mutable-headers 物理 187 一击定位；
实测钉住：无壳位置=assert SOURCE 行号安套件名、栈帧行号=CJS 前奏 +1、
rejection 拦不到/exit-hook fatal 不触发；AGENTS §4.202）。同轮
test-http-createConnection 转绿（修前 TIMEOUT：请求级 createConnection 的
oncreate 吞 err——async cb 错永悬、sync throw 靠穿透构造器侥幸；修法真机
_http_client.js 591-607 行逐字 err 臂 nextTick emitErrorEvent + try/catch
收口 + settled 防双投；黑盒三形 `phase11_http_create_connection_error_routing`；
AGENTS §4.203）。http/net/https 三域 52 绿 + node 域 285 绿 + 冒烟 5/5。

**2026-09-23 sweep8 红件簇清扫（15 件转绿）**：四簇连修——① 重复头真机
表驱动口径（joinable+未知头恒 ', '、19 头单值表首个赢、查询面过滤自动头、
GET+用户 TE 帧化；multiheaders×5/mutable-headers/raw-headers 转绿）；
② agent 池（res.destroy 后复用 + req close 蕴含 destroyed；abort-keep-alive/
override-global-agent 转绿）；③ parser 面（TE 整词 token+teInvalid 400、
冒号空格拒收、parser 全局 freelist、writeInformation 门序三形、
optimizeEmptyRequests+IM._dumpAndCloseReadable；smuggling/te-repeated/
parser-free/write-information/optimize-empty/chunk-extensions-limit 转绿）；
④ res 侧 timeout 桥（responseOnTimeout 打 res + IM.setTimeout 自武装，
监听数契约守卫；client-response-timeout 转绿）。出局 4（internals/flags×3 +
process.report）+ 偏离 2（domain 异步/Atomics.wait）+ 预存挂 1（client-
timeout-on-connect）。残：DIFF 5（set-timeout-server/request-timeout-upgrade/
url.parse-https.request/headers-timeout-keepalive/server-capture-rejections）+
TIMEOUT 5（no-read-no-dump/capture-rejection/non-utf8-header/reject-chunked/
should-keep-alive，流控与二进制头深水）。提交 f4b8a52/029a244/834bc15/431fc0e；
AGENTS §4.204；node 域 285 绿 + 冒烟 5/5 ×4 轮。

**2026-09-25 sweep 残部二批（5 件转绿）**：server captureRejections 兜底
（nodejs.rejection 逐字）+ TLSSocket _secureEstablished + ServerResponse.
setTimeout + IM/server 超时桥带 socket 实参 + HPE 门序前置（TE+CL 先于
requireHost）+ 头串 latin1 上网 + OutgoingMessage hasInstance 品牌判定
（原型桥改道 super 全链实锤后弃用）。capture-rejections/url.parse-https.
request/reject-chunked/non-utf8-header/set-timeout-server(前四块) 转绿；
提交 2b1a1ab/566117d；AGENTS §4.204 追补；node 域全绿 + 冒烟 5/5 ×3 轮。
残 4：outgoing-message-capture-rejection / should-keep-alive / no-read-no-dump
（流控与判定矩阵深水）+ set-timeout-server 末段 exit-hold（paused client
EOF 急切检测，G6 infra 族）。

**2026-09-25 §4.202-②③ 对拍提速工具落地**：① mapper 已落地（§4.202-①），
本轮补齐 ② `scripts/flake-classify.py`（新红先分类：整文件×3 + 单块 repro×N，
GREEN/FLAKY/RED-DETERMINISTIC/NODE-FLAKY 四分流，flaky 走定级法勿深挖）+
③ `scripts/sweep-bg.py`（全量 serial sweep 双 fork 后台直跑、status/wait/
tail/stop 轮询、工件落 ~/.wjs-sweep/、失败行 stderr 首行 + debug/ 全量落盘）。
两坑实证（AGENTS §4.205）：统一 runner 给 node 带 `--run` 假红全表（node 22+
--run=跑 package.json scripts）；前缀过滤只认 .js 丢 6 件 mjs（基线 409 口径）。
dogfood 首件：**dump-req-when-res-ends 判 FLAKY（wjs 0,142,142 挂死型，
node 2/2 绿）**——挂死型 flaky，归 §4.148 流控 infra 族随 no-read-no-dump
同轮处理。sweep7 全量 409 件基线后台直跑中（终态见 ~/.wjs-sweep/sweep7/）。

**2026-09-25 sweep 残部三批（2 件转绿 + ②③工具轮）**：②③落地后逐件啃
http 尾巴——① **outgoing-message-capture-rejection** 转绿（`fcf416b`）：
ServerResponse.destroy(err) 把 err 丢在 super.destroy() 外、_destroy 永裸杀
（socket 'error' 不发）→ _destroy 从 __resErrored 找回；连带修 client 侧
体未齐断连 error 递送（destroy(__e) 被 IM._destroy 吞错口径递不出去，改同步
守门递送，真机 p4 差分序 aborted → error ECONNRESET → close 对齐）；②
**should-keep-alive** 转绿（`a683ca5`）：__release 回池门只看 Connection 头，
1.0 缺省响应 socket 被错误入池 → 复用死连接挂死 → 门改 req.shouldKeepAlive
（版本×Connection 折算）+ 池态 socket 收 EOF 即销毁摘池（node socketOnEnd
口径）。黑盒两件（capture_rejection_routing / should_keep_alive_matrix）+
家族对拍零回归。**dogfood ② 分类**：dump-req-when-res-ends 判 FLAKY
（0,142,142 挂死型）；child exec_shell_self 负载形 flaky（6/6 单跑绿）。
**sweep7 终局基线**（409 件，sweep-bg 首跑）：SAME0=375 / SAME1=6 / DIFF=19 /
TIMEOUT=9（sweep6 SAME0=307 → +68）。残件定性：http 尾巴剩 no-read-no-dump
（流控 infra）+ 时序敏感两件（request-timeout-upgrade / headers-timeout-
keepalive）+ set-timeout-server exit-hold（G6 infra 族）；sweep7 新现红
（outgoing-finished / matchKnownFields / 1.0-keep-alive [object Object] 文案 /
catch-uncaughtexception / client-parse-error / writable-true-after-close /
chunk-extensions-limit flake）另批分类。node 域 286 黑盒全绿 + 冒烟 5/5。

**2026-09-25 sweep 残部四批收官（http 尾巴 5/7 转绿）**：接三批续啃——
④ **should-keep-alive**（`a683ca5`）：__release 回池门只看 Connection 头，
1.0 缺省响应 socket 被错误入池 → 复用死连接挂死；门改 req.shouldKeepAlive
+ 池态 socket 收 EOF 即销毁摘池。⑤ **no-read-no-dump**（`7c916b6`）：服务端
体背压流控 infra 四件联动（泵 backpressured 旗 / __feed 停读+pause /
Socket pause/resume 事件 / req._read 消费即解暂停）+ res 完成清 framing 根修
（体在途字节被当新请求头 → HPE 断连）。⑥ **时序敏感两件**（`2ce37ef`）：
Host 校验搬位到升级检测后（node parserOnIncoming 头部对 upgrade return 0）+
劫持撤计时 + headersTimeout 计时模型统一（连接建立/新消息首字节开、请求完成
撤、空闲归 keepAliveTimeout）。黑盒新增三件，node 域 288 全绿，冒烟 5/5；
AGENTS §4.206。**残件**：dump-req-when-res-ends（挂死型 flaky——今日根因
定位到流端口 flowing 排空语义，dump 机制需先修 readable_flow push/flow，
独立轮；§4.206 坑二）+ set-timeout-server 末段 exit-hold（G6 infra 族）+
sweep7 新现红件分类（outgoing-finished/matchKnownFields/1.0-keep-alive 文案
等）。**sweep8 终局**（409 件）：SAME0=379 / SAME1=6 / DIFF=16 / TIMEOUT=8——五件
修复零红；但暴露两件**本轮回归**（outgoing-flush-drain TIMEOUT +
upgrade-large-body-unread DIFF，sweep7 均绿）。**回归根修**（`6a2d64e`）：
二分三段实锤 chunked 泵背压 early-return 为元凶（终结段扣 fr.buf 等再喂
而包不会再有 → 泵停摆）；终解=背压改**状态驱动事件**（缓冲 ≥HWM 发
'pause'、落回发 'resume'，泵不停读不中断，缓冲有界=体长）+ 泵恢复无早退形；
两回归转绿 + 五件守卫绿 + node 域 288 全绿（fifo 按 §4.175 剔除）。AGENTS
§4.206 坑四/坑五（分离 HEAD 提交：bisect 后直接 commit 落 detached，父=旧
提交缺后续修复——cherry-pick 回 master 解）。**残件**：dump-req-when-res-
ends（挂死型——dump 机制需先修流端口 flowing 排空语义，独立轮）+
set-timeout-server 末段 exit-hold（G6 infra 族）+ sweep8 散红分类
（matchKnownFields/outgoing-finished/1.0-keep-alive 文案/catch-uncaughtexception/
client-parse-error/writable-true-after-close + node 侧独红的 loader 形 8 件）。

**新会话入口（按优先级，2026-09-20 G8 轮后更新）：**

1. **G5 child ~30 件 → G8 fs watch ~23 件 ✅ 双收官**（G5 25 套件 + G8 30 套件；
   AGENTS §4.150-155；残件：fork/IPC handle 传递出局 + fs.glob×2/flush 三套件
   待 node:test + enoent-after-deletion 间歇超时另查）。
2. **fs 残簇**（2026-09-21 七轮收官：cp/write/read/handle 129 件 70/59 →
    **126 SAME/3 DIFF**，e2f0d28/6412095/b2a473e/4703a16/4f9cc70/七轮读流，
    残件与黑盒见 bun-parity fs 七轮注记）：残 3 全另案——read-worker ×1
    （worker fd 移交，归 G6 残件同族）、eagain/flush ×2（node:test mock，
    runner 深度）；expose-internals ×3 跳过类；pull/writer ×3（需
    stream/iter+zlib/iter 新模块，另轮）；stream 余 err（增量流重写轮）+
    write-patch-open（fork 父端 exit，child 域）；黑盒 fs 26/26 + 全量
    cargo test 21 target 0 失败。
3. **G6 残件 6 件**（infra 级，需独立轮）：throttle（native 读门控+写 EAGAIN
   流控）、cluster×2（internalMessage 协议）、worker×3（跨线程 fd 移交）。
4. **大簇另案**：G10 http2 compat ~105 件、G11 http TIMEOUT ~110 件、
   dgram 余 ~32 件、http OutgoingMessage 缓冲模型 5 件（G3 遗留专项）、
   async_hooks 资源面（FSREQCALLBACK 生命周期，fs roundtrip 末段牵引）。
5. **URLPattern 归属**：对 Bun 1.3 实测后拍板（见上"待核对"）。

（编号对表：G4=fs validators 尾件行、G5=child_process 行、G6=net 尾件行、
G8=fs watch 簇行、G10=http2 compat 行、G11=http TIMEOUT 行；
G1/G2/G3/G9 已收官。）

**新会话开场提示**：先读 AGENTS.md §4.140-149（三轮十一坑，尤其 §4.140
"手工过/cargo 挂≠环境问题"、§4.142 "禁连续建 worktree"、§4.145 跑分
"exec or die"+glob 路径）+ 本节欠账表；跑分 `TEST_THREAD_ID` 用 35xx+
（§4.122 互踩防线）；net 域黑盒先单跑验证（`cargo test --test node net`，
全绿后再并发——§4.140/§4.141 两坑都在 cargo harness 时序下才现形）。
