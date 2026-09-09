# winterjs 总动工计划

> 版本 `26.9.0`，引擎 `mozjs 0.26.0`。本计划是活文档：每 Phase 开工前更新对应节，
> 完工即打钩。依赖明细与平台矩阵见 `docs/dependencies.md`，工作规约见 `AGENTS.md`。
> 当前状态：Phase 0 进行中（`run`/`eval` 已可用）。

## Phase 0 — 底座收尾（进行中）

- 目标：可分发、可观测、可测试的骨架。
- 引入依赖：`tracing`+`tracing-subscriber`、`miette`、`human-panic`、`vergen`（gitcl）、
  `clap_complete`+`clap_mangen`、`config`+`schemars`、`smmalloc`（桌面）/`talc`（移动回退）、
  dev 全套（`assert_cmd`/`insta`/`criterion`）。
- 做：`--version`（vergen 信息）、报错渲染（miette）、panic 美化、配置文件加载、
  分配器切换＋超大分配测试、6-target CI、CLI 黑盒测试。
- 验收：`cargo build` 全绿；`winterjs --version` 含 commit；`cargo test` 全绿。
- 完成标准：新人 clone 后按 AGENTS.md §3 一次构建成功。

## Phase 1 — console / timers / microtask

- 目标：同步 JS 跑通，异步地基打好。
- 引入依赖：无新增（`tokio` time + 自研 drain）。
- 做：`console.*`、`setTimeout/clearTimeout/setInterval/clearInterval`、
  `queueMicrotask`、Promise job queue（microtask drain，禁 `&mut` 别名，
  winterjs-old §7.9 教训）、`structuredClone`（`serde` 中转）。
- 验收：`winterjs eval 'await new Promise(r=>setTimeout(()=>r(1),10))'` → `1`；
  `probe` 链（then 三跳、嵌套 microtask）全绿。
- 完成标准：事件循环空转不饿死、不早退（regression 探针入库）。

## Phase 2 — ESM loader

- 目标：`import` 能跑，TS 能进。
- 引入依赖：`oxc_resolver`、`oxc`、`jsonc-parser`、`sourcemap`、`linkme`、`petgraph`、
  `string-interner`+`smol_str`、`slotmap`、`fs-err`、`normpath`、`pathdiff`、
  `glob`+`ignore`、`include_dir`、`target-lexicon`、`camino`、`dunce`。
- 做：resolve（node 语义＋tsconfig）→ fetch（file/http/data:）→ oxc 转译 →
  compile/link/instantiate → import.meta；循环依赖按 spec 行为；转译缓存
  （`blake3` key + `postcard` blob + `lru` 内存层）。
- 验收：`winterjs run app.ts`（含 npm 风格裸导入报错信息友好，miette 渲染）；
  循环 import 不死锁；`insta` 快照全绿。
- 完成标准：criterion 给 resolve/transpile 建性能基线。

## Phase 3 — Web API（WinterCG 兼容层）

- 目标：fetch/编码/流/加密可用。
- 引入依赖：`reqwest`（§2 门控）、`url`+`data-url`、`base64`+`percent-encoding`+
  `form_urlencoded`+`serde_urlencoded`、`encoding_rs`、`cookie_store`（+PSL）、
  `tokio-tungstenite`、`async-compression`+`flate2`+`brotli`+`ruzstd`、
  `backon`、`hickory-resolver`（可选）、加密全家（§7）、`ipnet`、
  `rustls-pemfile`+`rustls-native-certs`+`rcgen`（本地调试证书）。
- 做：fetch（含 data:/blob:/file: scheme、redirect、timeout、AbortSignal）、
  Headers/Request/Response、URL/URLSearchParams、TextEncoder/Decoder、
  streams（引擎上实现）、SubtleCrypto 全算法、`crypto.getRandomValues`。
- 验收：对标 WinterCG 兼容测试 excitement：26 路由全绿（抄 winterjs-old test-suite 思路）。
- 完成标准：无 `unsafe` 新增（§6 审计）。

## Phase 4 — Node 兼容垫片

- 目标：`node:fs/path/os/process/child_process` 跑起来。
- 引入依赖：`sysinfo`+`if-addrs`+`mac_address`+`uzers`+`sys-locale`、`which`、
  `shlex`、`shellexpand`、`jiff`、`walkdir`、`normpath`（沙箱 join）、`fs-err`、
  `filetime`、`humantime`+`bytesize`（flag 解析）。
- 做：fs 全量（stat/read/write/watch 复用 `notify`）、path、process（argv/env/exit）、
  os（cpus/mem/netif/user/locale）、child_process（stdio 管道＋进程组杀树，见 §13）、
  `node:test` 起步（reporter 用 `similar`+`unicode-width`）。
- 验收：跑通一个真实小项目的脚本子集（如 lint 脚本）。
- 完成标准：`process.exitCode` 语义与 Node 一致（退出码测试入库）。

## Phase 5 — 包管理（install/publish/upgrade）

- 目标：`winterjs install <pkg>` 端到端。
- 引入依赖：`semver`+`deno_semver`、`tar`+`zip`（§2 门控）+`flate2`、`ssri`、
  `fs4`、`dirs`、`gix`（git 依赖）、`indicatif`+`dialoguer`+`console`、
  `spdx`+`rust-ini`（npmrc）、`self_update`、`remove_dir_all`、`reflink-copy`、
  `junction`（Win）、`memmap2`、`rayon`+`futures`（并发下载）。
- 做：packument 拉取→版本求解→完整性校验→缓存（`blake3` 布局）→解包→bin 链接→
  lifecycle 脚本（`shlex`+tokio process+进程组杀树）；lockfile（JSON）读写；
  `publish`（`oauth2`+`webbrowser`，后期）、`upgrade`（`self_update`）。
- 验收：空目录装下 `left-pad` 级别包并可 `run`；二次安装全命中缓存。
- 完成标准：中断后续传不 corrupt（kill -9 测试）。

## Phase 6 — serve（HTTP 服务）

- 目标：`winterjs serve` 可上线。
- 引入依赖：`axum`+`axum-extra`+`tower`+`tower-http`（§2 门控）、`headers`+
  `mime_guess`+`httpdate`+`cookie`、`tokio-rustls`、`governor`、`metrics`+
  `metrics-exporter-prometheus`、`tracing-appender`、`nix`+`systemd`+
  `windows-service`、`local-ip-address`+`qrcode`、`netstat2`、
  `instant-acme`（后期）、`console-subscriber`（开发期）。
- 做：静态文件（ServeDir＋range＋etag）/路由/中间件（cors/压缩/限流/追踪）、
  TLS 终止（PEM）、优雅停机、`/metrics`、systemd/Win 服务集成、启动 banner。
- 验收：wrk 压测不丢请求；SIGTERM 优雅退出不断连接。
- 完成标准：metrics 有 named 指标文档。

## Phase 7 — runtime 补齐（sqlite/REPL/test/watch/FFI）

- 目标：对标 Bun 的单体体验。
- 引入依赖：`turso`（§2 门控）、`rustyline`、`notify`+`notify-debouncer-mini`、
  `libloading`（bun:ffi）、`similar`（prod reporter）、`askama`（init 模板）、
  `keyring`（后期 login 令牌）。
- 做：`bun:sqlite` 兼容层（turso 之上）、REPL（含 oxc 高亮，§13）、
  `winterjs test`（watch 模式、`--filter` glob）、`winterjs init`、
  FFI（dlopen 直通，unsafe 审计从严）。
- 验收：REPL 多行粘贴可用；test 输出格式对标（diff 着色）。
- 完成标准：FFI 每个导出的函数都有 safety 注释＋测试。

## Phase 8 — polish（lint/权限/远程缓存/上报）

- 目标：发布前收尾。
- 引入依赖：oxc 自带 linter/formatter（开特性）、`cap-std`（权限模型）、
  `object_store`（远程缓存，后期）、`sentry`（默认关闭，后期）、`russh`（私有仓，后期）。
- 做：`winterjs lint/fmt`、权限开关（`--allow-*`，cap-std 打底）、崩溃上报 opt-in。
- 验收：默认权限拒绝越界 fs 访问并给出可读错误。
- 完成标准：发 `27.x` 前全矩阵 CI（含 android/ohos）转正，§1 的 ⚠️ 清零或有书面理由。

## 全局纪律

- 每 Phase 开工前更新本计划对应节；完工打钩并 commit。
- 每 Phase 收尾跑：`cargo build`＋`cargo test`＋`./target/debug/winterjs` 实测三件套。
- CI 矩阵扩展节奏：Phase 0 结尾立 6 列；android/ohos 在 Phase 3 后逐个点亮。
- 性能基线从 Phase 2 起用 criterion 锁定，退化即红。
