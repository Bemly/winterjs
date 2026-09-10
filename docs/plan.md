# winterjs 总动工计划

> 版本 `26.9.0`，引擎 `mozjs 0.26.0`。本计划是活文档：每 Phase 开工前更新对应节，
> 完工即打钩。依赖明细与平台矩阵见 `docs/dependencies.md`，工作规约见 `AGENTS.md`。
> 当前状态：Phase 2 已完工（2026-09-10，`cargo test` 36+5 全绿，0 警告，
> 基线 transpile≈5.4µs/resolve≈345ns）；下一步 Phase 3 Web API。
> 依赖于 2026-09-10 按用户拍板全量引入，
> 见 `docs/dependencies.md` 头部决策记录，Phase 0-8 的“引入依赖”清单已全部入库）。

## Phase 0 — 底座收尾（进行中）

- 目标：可分发、可观测、可测试的骨架。
- 引入依赖：`tracing`+`tracing-subscriber`、`miette`、`human-panic`、`vergen`（gitcl→vergen-gitcl）、
  `clap_complete`+`clap_mangen`、`config`+`schemars`、`smmalloc`（桌面）/`talc`（移动回退）、
  dev 全套（`assert_cmd`/`insta`/`criterion`）。
- 做：`--version`（vergen 信息）、报错渲染（miette）、panic 美化、配置文件加载、
  分配器切换＋超大分配测试、6-target CI、CLI 黑盒测试。
- 验收：`cargo build` 全绿；`winterjs --version` 含 commit；`cargo test` 全绿。
- 完成标准：新人 clone 后按 AGENTS.md §3 一次构建成功。

- [x] 2026-09-10：`--version` 带 commit/describe/build 时间（vergen-gitcl，git 缺失降级 unknown）
- [x] 2026-09-10：tracing 家族接线（`-v` 计数 / `WINTERJS_LOG` / `WINTERJS_LOG_FILE`，
      非 TTY 无 ANSI；log 桥接走 subscriber 内建）
- [x] 2026-09-10：`winterjs config [--schema]`（config TOML/JSON/INI + `WINTERJS_*` 环境变量
      覆盖；schemars 出 JSON Schema；strum 解析取值枚举）
- [x] 2026-09-10：错误模型 thiserror + miette（TTY 图形渲染带代码框；非 TTY 保持
      AGENTS §3 的一行格式 + `Caused by` 链，脚本/测试依赖不变）
- [x] 2026-09-10：human-panic（`setup_panic!`，release 生效）
- [x] 2026-09-10：`completions <shell>` + `man`（clap_complete/clap_mangen；EPIPE 静默）
- [x] 2026-09-10：分配器切换（桌面 smmalloc / 移动 talc+System 源）＋超大分配探针
      （`tests/alloc_probe.rs`，§16 风险②实测：33GiB→null 无回退，上限≈8GiB）
- [x] 2026-09-10：CLI 黑盒测试 14 例（eval/run/config/completions/man/环境变量覆盖）
- [ ] 6-target CI 矩阵（待立 CI 时做）
- 依赖全量引入（2026-09-10 用户拍板）见 `docs/dependencies.md` 头部；turso 因 icu 死锁暂缓。

## Phase 1 — console / timers / microtask（已完工 2026-09-10）

- 目标：同步 JS 跑通，异步地基打好。
- 引入依赖：无新增（`tokio` time + 自研 drain）。
- 做：`console.*`、`setTimeout/clearTimeout/setInterval/clearInterval`、
  `queueMicrotask`、Promise job queue（microtask drain，禁 `&mut` 别名，
  winterjs-old §7.9 教训）、`structuredClone`（`serde` 中转）。
- 验收：`winterjs eval 'await new Promise(r=>setTimeout(()=>r(1),10))'` → `1`；
  `probe` 链（then 三跳、嵌套 microtask）全绿。
- 完成标准：事件循环空转不饿死、不早退（regression 探针入库）。
- [x] 2026-09-10：`src/runtime.rs` + `src/state.rs` + `src/jobqueue.rs` +
  `src/jsapi_glue.rs` + `src/builtins/`（console/timers/clone+prelude）；
  `runner.rs` 由 `runtime.rs` 接替；`tests/cli.rs` 8 例 Phase 1 用例全绿
  （microtask 顺序/三跳链/顶层 await/interval/嵌套 microtask/clone/rejection/count+time）；
  踩坑回写 AGENTS §4.7–4.9。

## Phase 2 — ESM loader（已完工 2026-09-10）

- 目标：`import` 能跑，TS 能进。
- 引入依赖：`oxc_resolver`、`oxc`、`jsonc-parser`、`sourcemap`、`linkme`、`petgraph`、
  `string-interner`+`smol_str`、`slotmap`、`fs-err`、`normpath`、`pathdiff`、
  `glob`+`ignore`、`include_dir`、`target-lexicon`、`camino`、`dunce`。
  （已全量入库，按需接线；切片 a 只用 `oxc`+`url`+`fs-err`+`data-url`。）
- 做：resolve（node 语义＋tsconfig）→ fetch（file/http/data:）→ oxc 转译 →
  compile/link/instantiate → import.meta；循环依赖按 spec 行为；转译缓存
  （`blake3` key + `postcard` blob + `lru` 内存层）。
- 验收：`winterjs run app.ts`（含 npm 风格裸导入报错信息友好，miette 渲染）；
  循环 import 不死锁；`insta` 快照全绿。
- 完成标准：criterion 给 resolve/transpile 建性能基线。
- 引擎路线（2026-09-10 调研结论）：Gecko153 用新模块 API——`CompileModule1` +
  `LoadRequestedModules`（回调版，同步）→ `ModuleLink` → `ModuleEvaluate`；
  依赖边由 `SetModuleLoadHook`（HostLoadImportedModule）+ `FinishLoadingImportedModule`
  驱动；`usePromise` 按 payload 是否 Promise 区分动/静；referrer 定位走脚本文件名
  （§4.11，私有值通道在 153 下收不到 GC 字符串）；`import.meta.url` 经
  `SetModuleMetadataHook` 补。
- [x] 切片 a（2026-09-10）：file:/data: + 相对导入（含后缀探测）+ oxc TS 转译 +
  循环（spec 序）+ import.meta.url + 动态 import + 入口 TLA 重试 +
  裸导入/http 友好报错；`tests/cli.rs` 8 例全绿（总 32）；0 警告保持。
  引擎细节：referrer 定位走脚本文件名（§4.11）；动态分支内嵌 load 再 link。
  附带修 Phase 0 遗留：`WINTERJS_LOG` 被 config 误收（§4.10）。
- [x] 切片 b（2026-09-10）：`oxc_resolver`（node_modules/tsconfig 别名/后缀别名，
  `resolve_file` 生效见 §4.13；canonicalize 见 §4.12）+ 转译缓存
  （blake3 key + postcard blob + lru 内存 128 + 磁盘 `$WINTERJS_CACHE`/系统缓存；
  IO 失败当 miss）+ `insta` 快照 3 例（转译输出/import 表/直通）+ criterion 基线
  （transpile≈5.4µs、resolve≈345ns，本机值，退化即红）+ sourcemap 生成与 TS
  报错回映射（入口 rejection 带位置见 §4.15；`harness=false` 见 §4.16）。
  `cargo test` 36+5 全绿，0 警告保持。未接线轮子（jsonc-parser/linkme/petgraph 等）
  按需顺延，不为用而用。

## Phase 3 — Web API（WinterCG 兼容层，开工 2026-09-10，切片 a 进行中）

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
- 完成标准：§6 审计口径——无业务层 `unsafe` 新增、无裸指针新用法
  （边界入口块随 native 数增长如实计数，不算违规）。
- 路线（2026-09-10）：类实例状态只用保留槽存 JS 值（href 字符串等，可被 GC 追踪），
  不用裸指针 private + finalizer；`searchParams` 活视图靠双向槽链接。
- [x] 切片 a（2026-09-10）：URL/URLSearchParams（含双向活视图）+ TextEncoder/Decoder
  （`encoding_rs`，流式顺延）+ atob/btoa + `crypto.getRandomValues`/`randomUUID`。
  prelude 真类 + 纯字符串 native（复杂值走 JSON 桥）；新增 `unsafe` 仅 1 处
  （`te_encode` 的 `TypedArray::create`，边界调用）；`tests/cli.rs` 9 例；
  `cargo test` 45+5 全绿，0 警告。
- [x] 切片 b（2026-09-10）：fetch + Headers/Request/Response（`reqwest` §2 门控，
  ring provider 本模块安装）+ 最小 AbortController（无事件；in-flight 中断顺延）。
  并发：native 解析 + spawn，channel 回事件循环 `select` 结算（发送端活 TLS，
  回调存 RootedState）；data:/file: 同步直给（仍走 promise 语义）。
  附带修：`await` 参数位置解析坑（§4.17，重试触发放宽 + 原始报错保留）。
  `tests/cli.rs` 6 例（含本机回环 http）；`cargo test` 51+5 全绿，0 警告。
- [x] 切片 c-1（2026-09-10）：`subtle.digest`（SHA-1/256/384/512，
  同步 native + prelude async 包裹；标准向量钉住）。
  `tests/cli.rs` 2 例；`cargo test` 53+5 全绿，0 警告。
  切片 c-2（2026-09-10 完工）：streams 纯 prelude 实现（Readable/Writable/Transform，
  默认 reader + asyncIterator + pipe/tee；BYOB 顺延）+ fetch `Response.body` 对接
  （同一性缓存）。`tests/cli.rs` 2 例。
  切片 c-3（2026-09-10 完工）：SubtleCrypto 对称子集（AES-GCM 128/256 + HMAC，
  prelude 密钥 + JWK oct；192-bit/非对称顺延 c-4）+ WebSocket client
  （tokio-tungstenite，`on*` 回调，事件循环三路 select + `ws_open` 存活计数）。
  `tests/cli.rs` 4 例（含本机回环 echo/close 握手）；`cargo test` 59+5 全绿，0 警告。
- [x] 切片 c-4a（2026-09-10）：SubtleCrypto 非对称（RSASSA-PKCS1-v1_5 签验 +
  RSA-OAEP 加解密 + ECDSA P-256/384/521 + ECDH deriveBits/deriveKey；
  generateKey 返回 CryptoKeyPair；import/export pkcs8/spki/jwk/raw(EC公钥)；
  RSA-PSS/Ed25519/X25519 明确顺延 c-4x）。轮子：`sha2_010` 改名直引（digest 0.10
  互通）+ p 曲线 `ecdh,pkcs8` 显式特性（用户拍板，见 dependencies §7）；
  `rsa::rand_core/signature/pkcs8` 重导出零新增；密钥存 DER、私钥 PKCS#8/公钥 SPKI；
  ECDSA 裸 r‖s 口径。`tests/cli.rs` 6 例；`cargo test` 65+5 全绿，0 警告。
- [ ] 切片 c-4b：wss 测试 + `Response.body` 流式（fetch 边下边吐）+ in-flight abort。
  RSA-PSS/Ed25519/X25519（c-4x，按需排）。

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
