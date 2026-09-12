# winterjs 总动工计划

> 版本 `26.9.11`，引擎 `mozjs 0.26.0`。本计划是活文档：每 Phase 开工前更新对应节，
> 完工即打钩。依赖明细与平台矩阵见 `docs/dependencies.md`，工作规约见 `AGENTS.md`。
> 当前状态：Phase 0–8 代码切片全部完工 + 顺延全收官（2026-09-11，
> 批1 Node小件/spawn pipe/流防空转、批2 Buffer/fs流、批3 publish PUT/OAuth、
> 批4 ACME、批5 loader http；`cargo test` 全绿，0 警告，冒烟 5/5）。
> 剩验收项（发 `27.x` 前置）：6-target CI 矩阵立起 +
> android/ohos 列转正 + §1 ⚠️ 清零或书面理由（需 CI 环境，见 Phase 0/8 完成标准）。
> 不再做的（书面理由见各 Phase 尾）：object_store 远端（§2 禁 aws-lc 后只剩 fs，
> 与既有缓存重复）、russh（远端 git 含 ssh 全走 git CLI，覆盖）、
> Win 服务（mac 无法验证，留 CI）、
> netstat2（TOCTOU，不用）、oxc linter（上游未发布，穿透保留）、
> watch 导入图（全量重跑，偏差接受）。
> console-subscriber 已于 2026-09-11 接为 cargo feature `tokio-console`（默认关闭，dev 按需）。
> **2026-09-12 黑盒测试拆分**：`tests/cli.rs`（4674 行/188 例）按 src 布局对齐拆为`tests/{cli,builtins,crypto,fetch,ws,loader,node,pm,serve,acme,testrun,initpkg,repl,bun,permissions,lintfmt,sentry_report}.rs` + 共享 helper `tests/common/mod.rs`；测试名一律未改，仅换文件（本文历史章节的 `tests/cli.rs` N 例为拆分前存档）。拆分分支 `tests/src-aligned`。
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

## Phase 3 — Web API（WinterCG 兼容层，已完工 2026-09-11）

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
- [x] 切片 c-4b（2026-09-10）：wss 测试（`WINTERJS_TEST_CA_PEMFILE` 接缝 + rcgen
  自签回显，生产默认链不变）+ `Response.body` 流式（http(s) 边下边吐：head 即给
  Response + chunk 通道 + pull 泵；快照路径 data:/file:/构造体不变）+
  in-flight abort（signal 最小监听 + `fetch_abort` 取消任务/拒绝排队 pull/流中
  后继 read 拒 AbortError）。附带修事件循环退出 race（§4.18 progressed 轮不退）。
   `tests/cli.rs` 6 例；`cargo test` 71+5 全绿，0 警告。
- [x] 顺延收官（2026-09-11）：c-4x（RSA-PSS/Ed25519/X25519/AES-192 全接线，
  openssl 独立向量，模块 2 + 黑盒 4）+ TextDecoder 流式（有状态 Decoder）+
  Abort 事件对象/onabort/dispatchEvent/timeout/any + BYOB（byte 流/read(view)/
  byobRequest；字节流纯按需 pull）+ loader http(s)（批5：绝对直通/远端相对
  join/独立线程 reqwest，黑盒 2 例）。c-4x 前的“顺延”字样作废。

## Phase 4 — Node 兼容垫片（已完工 2026-09-11）

- 目标：`node:fs/path/os/process/child_process` 跑起来。
- 引入依赖：`sysinfo`+`if-addrs`+`mac_address`+`uzers`+`sys-locale`、`which`、
  `shlex`、`shellexpand`、`jiff`、`walkdir`、`normpath`（沙箱 join）、`fs-err`、
  `filetime`、`humantime`+`bytesize`（flag 解析）。
  （已全量入库，按需接线；`node:` 表走 `src/builtins/node/`，源内嵌 JS ESM。）
- 做：fs 全量（stat/read/write/watch 复用 `notify`）、path、process（argv/env/exit）、
  os（cpus/mem/netif/user/locale）、child_process（stdio 管道＋进程组杀树，见 §13）、
  `node:test` 起步（reporter 用 `similar`+`unicode-width`）。
- 验收：跑通一个真实小项目的脚本子集（如 lint 脚本）。
- 完成标准：`process.exitCode` 语义与 Node 一致（退出码测试入库）。
- 路线（2026-09-10）：`node:X` 在 `resolve()` 截获→规范 `node:` URL→`prepare()`
  取内嵌源（`load_js` 统一转译/提 imports）；`process` 另放全局（`NODE_PRELUDE`，
  与模块源同 natives）；`process.exit` 经 `__wjs_exit:<code>` 哨兵错逐层转
  `Error::Exit`（静默退出码，无渲染）；`exitCode` 存 Rust 侧、收尾映射。
- [x] 切片 a（2026-09-10）：`node:` 接线（resolve 截获→规范 URL→内嵌源；
  `fs` 与 `node:fs` 同一模块）+ `process` 全局（argv/env Proxy/cwd/exit/
  exitCode/platform/stdout.write/nextTick 等）+ `node:path`（纯 JS posix+win32）
  + `node:os`（sysinfo/if-addrs/uzers/sys-locale）+ `run` 透传 args +
  `Error::Exit` 静默退出。附带修：模块顶层 exit 的检查点顺序（§4.18 追补）。
  `tests/cli.rs` 6 例 + 模块单测 3 例；`cargo test` 77+8 全绿，0 警告。
- [x] 切片 b（2026-09-10）：`node:fs` 同步核心（read/write/append/stat/lstat/
  exists/mkdir/rm/rmdir/unlink/readdir/rename/copyFile/realpath/mkdtemp +
  `fs/promises` async 包裹（同步底层，文档记录）+ Node 形错误
  `.code/.syscall/.path`）。`tests/cli.rs` 3 例 + `io_code` 单测；
  `cargo test` 80+9 全绿，0 警告。
- [x] 切片 c（2026-09-10）：`child_process` 同步（execSync/spawnSync：cwd/env/
  input/timeout 直杀/maxBuffer/shlex 拼串 + pid/status/signal/err 形状）+
  `node:assert` 起步（ok/equal/strict/deep/throws/rejects/match + AssertionError）
  + `node:test` 起步（test/describe/it/skip/todo/only，串行泵 + 小结 + exitCode）。
  `tests/cli.rs` 4 例 + 状态映射单测；`cargo test` 84+10 全绿，0 警告。
- [x] 切片 d（2026-09-10）：`require()` CJS（node: 经 namespace/`default` 回落、
  文件经柯里化包装执行 + 预注册循环半成品 + JSON + `.cjs` 入口 + require.main/
  resolve；ESM 报 ERR_REQUIRE_ESM）+ `fs.watch`（notify 进事件循环，persistent
  续命）+ 异步 `spawn`（exit/close 双调 + kill + detached 组杀 unix）。
   `tests/cli.rs` 5 例；`cargo test` 89+10 全绿，0 警告。
- [x] 顺延收官（2026-09-11，批1/批2）：`require` 读 package.json type
  （module 一律 ERR_REQUIRE_ESM，其余强制 CJS）+ 入口同口径（`type: module`
  包的 extensionless bin 走模块，无扩展名按 `.js` 解析；无 type 即经典，
  Node 口径）+ spawn pipe 流（stdin/out/err
  live 流，残留先达再 Exited）+ `before/after/beforeEach/afterEach` 钩子 +
  `--test-name-pattern` 名过滤 + `fs.watch` 共享防抖线程（kind 保留，300ms）+
  Buffer 全局（Uint8Array 子类，hex/b64/utf8 等）+ fs 流（createRead/WriteStream，
  Web 流外形）。“已知缺口”段作废；孙进程 win 组杀留 CI（mac 无法验证）。

## Phase 5 — 包管理（install/publish/upgrade，已完工 2026-09-11）

- 目标：`winterjs add -a <pkg>`（工程本地）/ `winterjs install -a <pkg>`（全局）端到端。
- 引入依赖：`semver`+`deno_semver`、`tar`+`zip`（§2 门控）+`flate2`、`ssri`、
  `fs4`、`dirs`、`gix`（git 依赖）、`indicatif`+`dialoguer`+`console`、
  `spdx`+`rust-ini`（npmrc）、`self_update`、`remove_dir_all`、`reflink-copy`、
  `junction`（Win）、`memmap2`、`rayon`+`futures`（并发下载）。
  （已全量入库，按需接线；包管理代码进 `src/pm/`。）
- 做：packument 拉取→版本求解→完整性校验→缓存（`blake3` 布局）→解包→bin 链接→
  lifecycle 脚本（`shlex`+tokio process+进程组杀树）；lockfile（JSON）读写；
  `publish`（`oauth2`+`webbrowser`，后期）、`upgrade`（`self_update`）。
- 验收：空目录装下 `left-pad` 级别包并可 `run`；二次安装全命中缓存。
- 完成标准：中断后续传不 corrupt（kill -9 测试）。
- 路线（2026-09-10）：registry 默认 `https://registry.npmjs.org`（npmrc/镜像覆盖
  顺延）；先 `--dry-run` 只求解不落地；网络测试走本地 stub registry（回环，
  与 fetch/ws 测试同构），不碰外网。
- [x] 切片 a（2026-09-10）：包 spec 解析（`name@range` + scope）+ packument 拉取
  （reqwest ring 门控复用）+ 版本求解（deno_semver tag 先分流/range 匹配 +
  semver 取最大 + 垃圾版跳过 + BFS 传递闭包）+ `install --dry-run` 打印解树
  （本地 stub registry 验收，不碰外网）。附带：`man` 页计数 6→7。
  `tests/cli.rs` 2 例 + 模块单测 4 例；`cargo test` 91+14 全绿，0 警告。
- [x] 切片 b（2026-09-10）：tarball 下载 + ssri/shasum 校验 + 暂存解包
  （`package/` 剥离 + 越界拒绝 + 父目录自建）→ `node_modules` 落地 → bin 链接
  （unix symlink + 可执行位，win 退拷贝）→ lockfile 读写 + 真装闭环
  （stub 下发包，装完 `require` 可跑）。`tests/cli.rs` 1 例；
  `cargo test` 92+14 全绿，0 警告。
- [x] 切片 c（2026-09-10）：缓存（`src/pm/cache.rs`，有 integrity 按内容
  `blake3(integrity)` 跨 registry 命中、无则按 URL；`$WINTERJS_CACHE/pkgs` >
  系统缓存；命中复验防投毒 + `tmp+rename` 原子）+ lifecycle 脚本
  （`src/pm/lifecycle.rs`，`preinstall/install/postinstall` 按序，cwd 包目录，
  unix `/bin/sh -c`/win `cmd /C`，stdio 继承 + `kill_on_drop` + unix setsid 组长，
  `npm_package_*` + PATH 前置 `.bin`；非零即失败）+ 中断续传
  （暂存同盘 rename 提交 + 开头清 `.staging-*` 孤儿 + lockfile/cache 原子写 +
  `fs4` 独占锁串行化；kill -9 只留孤儿暂存，下次自愈）。
  `tests/cli.rs` 4 例（二次命中零回源/order 序/失败中断/残留自愈）+
  模块单测 7 例（key/往返/原子/order/env/失败/缺脚本）；
  `cargo test` 21+96 全绿，0 警告。
- [x] 切片 d（2026-09-10 完工，d1-d4 见下）：git 依赖 + publish/login + upgrade + 镜像/npmrc。
  - [x] d1 镜像/npmrc：`src/pm/npmrc.rs`（手写行解析，`rust-ini` 实测不适合见
    `dependencies.md` 附记）+ 优先级 flag > env `NPM_CONFIG_REGISTRY` >
    `<cwd>/.npmrc` > `$HOME/.npmrc` > 默认 + token 按 host 透传（值永不进日志）。
    模块单测 4 例 + 黑盒 4 例（镜像/坏源报错/flag 覆盖/env 覆盖）；
    `cargo test` 25+100 全绿，0 警告。
  - [x] d2 git 依赖：`spec` 扩展（`[<name>@]git+<url>[#<rev>]` +
    `github:<user>/<repo>[#<rev>]` 缩写，2026-09-11 收官）+
    `src/pm/git.rs`（本地 `file://`/路径走 `gix` open+rev-parse+worktree 拷贝，
    远端走 `git` CLI 浅克隆——`gix` 默认特性无网络客户端，补特性拖 transport，
    见 `dependencies.md` 附记；远端含 ssh 全走 git CLI，`russh` 不引入，见本节尾；
    落地后 bin 链接 + lifecycle 与 tarball 同待遇，
    lockfile 记 `git+<url>#<commit>`）。模块单测 7+3 例 + 黑盒 4 例
    （dry-run/未知 rev/裸名读包/真装 require）；`cargo test` 28+104 全绿，0 警告。
  - [x] d3 publish/login：`src/pm/publish.rs`（本地校验：名/版本（`semver`）/
    license（`spdx`，缺失 WARN/非法错）/`files` 表 + 打 tarball（`package/` 前缀）+
    真 `PUT {registry}/{name}`（versions/dist-tags/_attachments，Bearer 鉴权，
    409/401 可读错，2026-09-11 收官，批3；模块往返 + 黑盒 PUT/无 token/dry-run）；
    `login --token` upsert `//<host>/:_authToken` 进 `$HOME/.npmrc`（原子，
    值永不进日志），TTY 缺 token 走 `dialoguer` 密码提示；`login --oauth` 经
    `oauth2` 拼 `{registry}/oauth/authorize` URL + `webbrowser` 试开 +
    `--token code:<code>` 交换落盘（2026-09-11 收官）。
    CLI 新增 `publish`/`login`（man 7→9）。模块单测 4+1 例 + 黑盒 4+2 例
    （dry-run/缺名坏 license/token 落盘/oauth URL/真 PUT/无 token）；
    `cargo test` 32+108 全绿，0 警告。
  - [x] d1 补（2026-09-11，批1）：`@scope:registry` 作用域镜像（精确匹配，
    project > home；cli/env 仍最高）+ token 按生效 registry 逐包透传 +
    lifecycle 加 `prepare`（发包事件不跑，无远端发布流程）。
    模块单测 +5（scope 精确/优先级/github 表）。
  - [x] d5 可选依赖（2026-09-11）：`optionalDependencies` + `os`/`cpu` 平台过滤
    （npm 口径：缺省全平台、`!` 排除、双字段都过；仅可选边可达记 optional，
    必需边命中升级；求解/平台跳过静默，安装期失败 warn 跳过不记 lockfile）。
    附带修真 bug（§4.29）：`dist-tags`/`optionalDependencies` 缺 rename 被
    serde 静默丢弃——`pkg@latest` tag 安装一直是坏的，无报错；逐个显式改名 +
    真实线名回归测试。模块 +9（platform 表/求解三态/线名）+ 黑盒 1
    （仿 oxlint 形：命中装上/异平台跳过/packument 404 容忍/tarball 404 跳过/
    .bin 可跑）。真 oxlint 1.82.0 实装验证：`--install oxlint` 只装
    darwin-arm64 binding（余 19 平台跳过），见 Phase 8 真验证。
- Phase 5 完工（2026-09-10；顺延 2026-09-11 收官）：install/dry-run/真装/缓存/
  lifecycle/续传/npmrc/git/publish 真 PUT/login（含 OAuth 交换）/upgrade 干跑，
  `cargo test` 全绿，0 警告，冒烟 5/5。
- 不再做（书面理由）：`object_store` 远端缓存（§2 禁 aws-lc 后 http/aws/azure/gcp
  后端全禁，只剩 fs——与既有 `cache.rs` 重复，不引入）；`russh` 私有仓 SSH
  （远端 git 含 ssh 全走 git CLI，覆盖；`gix` 只做本地）；`keyring` 令牌保管
  （npm 口径即 `~/.npmrc` 明文 token，`login` 已对齐，不引入）。

## Phase 6 — serve（HTTP 服务）

- 目标：`winterjs serve` 可上线。
- 引入依赖：`axum`+`axum-extra`+`tower`+`tower-http`（§2 门控）、`headers`+
  `mime_guess`+`httpdate`+`cookie`、`tokio-rustls`、`governor`、`metrics`+
  `metrics-exporter-prometheus`、`tracing-appender`、`nix`+`systemd`+
  `windows-service`、`local-ip-address`+`qrcode`、`netstat2`、
  `instant-acme`（后期）、`console-subscriber`（2026-09-11 接为 `tokio-console` feature）。
- 做：静态文件（ServeDir＋range＋etag）/路由/中间件（cors/压缩/限流/追踪）、
  TLS 终止（PEM）、优雅停机、`/metrics`、systemd/Win 服务集成、启动 banner。
- 验收：wrk 压测不丢请求；SIGTERM 优雅退出不断连接。
- 完成标准：metrics 有 named 指标文档。
- [x] 切片 d1（2026-09-10）：`winterjs serve [dir] [--host] [--port]`
  （`src/serve.rs`，`ServeDir` 直服：mime/etag/range 全由轮子；
  `--port 0` 回显实际端口；SIGINT/SIGTERM 即停 exit=0，实测验证；
  复用 current-thread runtime，无 JS 线程冲突）。CLI 新增 `serve`（man 10→11）。
  模块单测 3 例 + 黑盒 4 例（静态+etag/range/坏目录/traversal 隔离）；
  `cargo test` 36+115 全绿，0 警告，冒烟 5/5。
- [x] 切片 d2（2026-09-10）：中间件（CORS permissive + gzip/br 压缩 +
  逐请求追踪，层序 CORS→压缩→追踪→文件）+ LAN 二维码 banner（`qrcode` 矩阵 +
  内建 unicode 渲染）。附带修 §4.19 追踪 target 静默坑（手写
  `winterjs::serve` 回调；压缩小 body 跳过属轮子行为，黑盒用大文件）。
  模块单测 +1（QR 形状），黑盒 +3（gzip roundtrip/CORS 头/追踪日志行）；
  `cargo test` 37+118 全绿，0 警告，冒烟 5/5。
- [x] 切片 d3（2026-09-10）：`/metrics`（`metrics` 全局 recorder +
  自服路由，`docs/metrics.md` 有 named 指标文档：requests counter /
  duration histogram / in-flight gauge）+ 全局限流（`governor`，
  `--limit-rps N`，burst=1，429 + `Retry-After`，429 本身不计数）。
  CLI 加 `--limit-rps`。模块单测 +1（配额/retry 表），黑盒 +2
  （metrics 精确计数/限流 200→429）；`cargo test` 38+120 全绿，0 警告，冒烟 5/5。
- [x] 切片 d4（2026-09-10）：TLS 终止（`--cert/--key` PEM，`tokio-rustls` +
  手写 axum `Listener`，单给即报错不降级；握手失败 warn 后继续 accept）+
  systemd READY 通知（仅 linux，失败忽略）。模块单测 +1（坏 PEM 三件），
  黑盒 +3（rcgen 自签真握手 e2e/半参报错/坏 PEM 报错）；200 并发零失败
  （wrk 缺席，python 10×20 替代，计数精确 200）。顺延（书面理由）：
  `instant-acme`（需真实域名）、Win 服务（mac 无法验证，留 CI 编译）、
  `console-subscriber`（当时顺延，2026-09-11 已接为 `tokio-console` feature）、`netstat2`
  （bind 错误已可读，预检有 TOCTOU，不用）。
  `cargo test` 39+123 全绿，0 警告，冒烟 5/5。
- [x] 切片 d5 ACME（2026-09-11，批4）：`--acme-domain`（缺省
  `winterjs.bemly.moe`）`--acme-email`/`--acme-cache`/`--acme-production`
  （缺省 staging 防限流）+ 缓存复用（notAfter>now+30d）+ HTTP-01（临时 `:80`
  应答）+ CSR（rcgen）+ 账户落盘；`instant-acme` 的 hyper-rustls 拖 aws-lc
  （§2 禁）故 ReqwestHttp 手写桥接（ring 同源）；与 `--cert/--key` 互斥；
  `--dry-run` 只校验打印。模块 5 + 黑盒 2（dry-run 计划/互斥）。
  约束书面记录：真签发需公网 `:80` + DNS 指到本机（当前域指 benchmark 段，
  签不出，先 staging 手工验）。
- Phase 6 完工（2026-09-10；ACME 2026-09-11 收官）：serve/中间件/指标限流/
  TLS/ACME，`cargo test` 全绿，0 警告，冒烟 5/5，200 并发零失败。
- 不再做（书面理由）：Win 服务（mac 无法验证，留 CI 编译）、
  `netstat2`（bind 错误已可读，预检有 TOCTOU，不用）。
  `console-subscriber` 已于 2026-09-11 接为 cargo feature `tokio-console`
 （`RUSTFLAGS="--cfg tokio_unstable" cargo build --features tokio-console`）。

## Phase 7 — runtime 补齐（sqlite/REPL/test/watch/FFI）

- 目标：对标 Bun 的单体体验。
- 引入依赖：`turso`（§2 门控）、`rustyline`、`notify`+`notify-debouncer-mini`、
  `libloading`（bun:ffi）、`similar`（prod reporter）、`askama`（init 模板）。
  `keyring` 已移除（npm 明文口径，见 §5 尾）。
- 做：`bun:sqlite` 兼容层（turso 之上）、REPL（手写高亮，oxc Lexer 私有，§13）、
  `winterjs test`（watch 模式、`--filter` glob）、`winterjs init`、
  FFI（dlopen 直通，unsafe 审计从严）。
- 验收：REPL 多行粘贴可用；test 输出格式对标（diff 着色）。
- 完成标准：FFI 每个导出的函数都有 safety 注释＋测试。
- [x] 切片 e1（2026-09-10）：`winterjs test [paths...] [--filter <glob>]`
  （`src/testrun.rs`：`ignore` walk 尊重 gitignore 跳过
  node_modules/target/.git，`*.test.*`/`test-*` 两模式，glob 按相对路径或
  文件名过滤；每文件独立 `runtime::run` 天然隔离；TAP 对齐
  `ok/not ok` + `# pass, fail` 汇总；空列表 exit 0）。CLI 新增 `test`
  （man 11→12）。模块单测 3 例 + 黑盒 4 例（混合 exit1/全过/filter/坏路径）；
  `cargo test` 42+127 全绿，0 警告，冒烟 5/5。
- [x] 切片 e2（2026-09-10）：`winterjs init [name] [--yes]`（`src/initpkg.rs`，
  `askama` 内联三模板：package.json + index.js + hello.test.js，init 后
  `test` 即绿闭环；已存在不覆盖整体报错（2026-09-11 加 `--force` 逐个覆盖）；
  缺名取目录名；非 TTY 缺 `--yes` 即错）。CLI 新增 `init`（man 12→13）。
  模块单测 2 例 + 黑盒 4 例（闭环/坏名/冲突/非TTY）；
  `cargo test` 44+131 全绿，0 警告，冒烟 5/5。
- [x] 切片 e3（2026-09-10）：`winterjs repl`（`src/repl.rs` + `runtime.rs::repl`：
  `init_session/pump_once` 抽共用（`run` 零回归 44+131），持久会话 +
  rustyline 行编辑/历史/括号续行 + 手写高亮（oxc Lexer 私有，见依赖附记）+
  `.exit/.help` + 5ms 短轮询事件泵（channel 无 peek，select 直收会吞消息）+
  stdout 刷序修复）。顶层 await 暂报 SyntaxError 指引（已知局限）。
  TTY 实测：续行/高亮 ANSI/42；非 TTY：无 ANSI。CLI 新增 `repl`
  （man 13→14）。模块单测 3 例 + 黑盒 4 例（持久/报错恢复/无 ANSI/语法续行）；
  `cargo test` 47+135 全绿，0 警告，冒烟 5/5。
- [x] 切片 e4（2026-09-11）：`bun:sqlite` 兼容层（turso 0.6.1 之上，`src/builtins/bun/`，
  resolve `bun:` scheme 截获 + prepare 内嵌源）。同步语义的落法：Bun API 全同步、
  turso 全 async —— 每 Database 一条专用 worker 线程（自带 current-thread tokio
  runtime），native 经 crossbeam 阻塞往返（open 握手在构造期报错；`init_session`
  清表回收上一会话 worker）。Database（exec/run 返 this/query 按 SQL 缓存/
  prepare/transaction（抛错回滚，嵌套走 SAVEPOINT）/close 幂等/inTransaction）
  + Statement（get/all/values/iterate/run 返 changes 信息/as object|array|raw
  （raw=array 别名）/finalize）+ SqliteError；参数 positional + named（键须带
  $/:/@ 前缀）+ blob 经 `$blob` b64 JSON 桥（Uint8Array 进出）。偏差记模块头注
  （2^53 精度损失、readonly/create 忽略、non-finite 拒绑）。附带：§4.23 踩坑
  （`Object.create` 实例无私有方法 brand 槽，prelude 内部类改 WeakMap + 自由函数）；
  turso 双 `Params` 附记见 dependencies §9。模块单测 4 例 + 黑盒 3 例
  （内存 roundtrip/文件持久化+报错三件/未知 spec 报可用列表）；
  `cargo test` 52+138 全绿，0 警告，冒烟 5/5。
- [x] 切片 e5（2026-09-11）：`winterjs test --watch`（notify-debouncer-mini 300ms 防抖，
  变更即重跑；SIGINT/SIGTERM 优雅退出 exit=0（复用 serve `shutdown_signal`）；
  watchable 过滤 node_modules/.git/target/点文件，只认代码/JSON 后缀——防测试自写
  db/产物触发无限循环；重跑前重发现文件，新增/删除即生效）。偏差：变更后全量重跑
  （Bun 按导入图受影响文件，按需顺延）；重跑轮的 leaked Runtime 随次数累积
  （dev 工具可接受，文档记录）。附带修 e1 遗留 bug（§4.24）：`JSEngine::init`
  二次调用 `AlreadyInitialized` + 同线程建第二 Runtime 炸 —— test runner 多文件
  修前第二个文件起全挂（e1 黑盒只放了一文件没抓到，已补两文件回归）。
  修法：引擎进程级单例（`engine_handle`，本体泄漏永不 shutdown）+ `run_isolated`
  每文件独立线程（CONTEXT/state TLS 随线程生灭，16MB 栈）。模块单测 +1
  （watchable 过滤表）+ 黑盒 +2（watch 重跑+SIGINT e2e / 两文件全过回归）；
  `cargo test` 53+139 全绿，0 警告，冒烟 5/5。
- [x] 补（2026-09-11，批1）：`--test-name-pattern` 名过滤（子串或 `/re/flags`，
  经 env 进 `node:test` harness；未命中 skip，before/after 照跑）。
- [x] 切片 e6（2026-09-11）：`bun:ffi`（libloading 之上，`src/builtins/bun/ffi.rs`
  + build.rs 生成调用 shim）。动态调用引擎无 libffi 落法（调研：libffi/dyncall
  皆 C，纯 Rust 无轮子，§13 记手写件）：C ABI 按参数独立分类（INTEGER/SSE
  寄存器），`extern "C" fn(target, a0..an) -> R` 中转对目标函数完全 ABI 透明
  （编译器完成收/发两侧搬移，含栈参），build.rs 按 (元数≤6, f64 位置掩码,
  返回 I/D/F) 生成 ~380 shim + `invoke` 查表派发，零新依赖。
  支持：标量参数（整数系/bool/ptr/f64）+ 返回（整数系/f64/f32/void）、
  `ptr()`（字符串零结尾拷贝 Box::leak / TypedArray 数据裸地址
  `GetUint8ArrayLengthAndData` / 数值直通；同步调用期间调用帧保活）、
  `CString`、`toBuffer/toArrayBuffer`（拷贝，偏差记录）、`suffix`。
  不支持（模块头注记录）：结构体按值传参/返回、变参、f32 参数、元数>6、
  `JSCallback`、`close()`；Library 泄漏永不 dlclose（地址稳定）。
  UNSAFE-BOUNDARY 6 处（dlopen/ptr_str/ptr_view/call/cstring/bytes）各带
  前置条件与覆盖测试名（完成标准：每个导出都有 safety 注释＋测试 ✓）。
  黑盒 2 例（unix 门控，cc 现编测试 dylib：全类型矩阵 + 指针写回可见性 +
  报错九件）+ 模块单测 2 例（shim 全类别 roundtrip / 分类表）；
  `cargo test` 55+141 全绿，0 警告，冒烟 5/5。
- Phase 7 完工（2026-09-11）：sqlite/REPL/test/init/watch/FFI，
  `cargo test` 55+141 全绿，0 警告，冒烟 5/5。

## Phase 8 — polish（lint/权限/远程缓存/上报，已完工 2026-09-11）

- 目标：发布前收尾。
- 引入依赖：`winterjs lint/fmt` 走外部 CLI 穿透（oxc 门面无 linter/formatter 特性，
  见 dependencies §14）、`cap-std`（权限模型，`optional` feature 按需）、
  `sentry`（默认关闭 opt-in）。`object_store`/`russh` 已移除（见 §5 尾）。
- 做：`winterjs lint/fmt`、权限开关（`--allow-*`，边界校验打底，`cap-std` 后续可用）、崩溃上报 opt-in。
- 验收：沙箱模式下越界 fs 访问默认拒绝并给出可读错误（权限为 opt-in：
  不传 `--allow-*` 行为不变，Bun 同款；传任一 `--allow-*` 即进沙箱，
  未授权类默认拒绝）。
- 完成标准：发 `27.x` 前全矩阵 CI（含 android/ohos）转正，§1 的 ⚠️ 清零或有书面理由。
- [x] lint/fmt 命令穿透（2026-09-11 用户拍板，`src/lintfmt.rs`）：原"oxc 开特性"
  不成立（`oxc_linter` 未发布 crates.io、`oxc_formatter` 是 0.0.0 占位，顺延
  记录见 dependencies §14）——改走外部 CLI 转发：`winterjs lint ...` → `oxlint ...`、
  `winterjs fmt ...` → `oxfmt ...`，参数原样转发（trailing passthrough，首参
  flag 也在内）、stdout/stderr 继承、退出码透传（`Error::Exit` 静默），语义
  完全归上游（oxfmt 默认写回、`--check` CI 检查）。查找：node_modules/.bin
  从 cwd 逐级向上（monorepo 命中根）→ PATH（`which` 轮子，Windows 尊重
  PATHEXT）→ 可读指引（`winterjs install oxlint` / `npm install -D oxlint`，
  不静默按需安装——CI/离线/lockfile 可预测）。零新依赖；外部二进制先例：
  远端 git 克隆走 git CLI、vergen-gitcl。平台注记：oxc 上游 android/ohos 只出
  N-API binding 无 standalone CLI（release workflow `!android && !ohos`），
  移动 target 上查找落空报可读错（桌面开发期工具）。CLI 新增 lint/fmt
  （man 14→16）。模块单测 4 例 + 黑盒 2 例（假脚本：转发/stderr 直出/
  向上查找/退出码透传/未找到指引）；`cargo test` 64+146 全绿，0 警告，冒烟 5/5。
- [x] npx 回退的引入与删除（2026-09-11）：用户指出 `npx oxlint@latest` 本来
  可用后曾加入回退（本地+PATH 落空→`npx --yes <pkg>@latest`）；同日用户拍板
  删除——npx 必须跟 node 一起装，与零 node 目标冲突。现查找链：本地 →
  PATH → 可读错（指引 `release:` 形装 standalone，不再提 npm/npx）。
- [x] release 二进制安装（2026-09-11，无 node 机器开箱用）：
  `[<name>@]release:github/<owner>/<repo>@<tag>/<prefix>`（tag 必显式）→
  GitHub API 取 asset 表 → 按平台挑包（`{prefix}-{arch}-{os}` 惯例，linux
  优先 gnu）→ 下载解包落 `node_modules/.bin/<name>`（unix `+x`）→ lockfile
  记 `github-release:…` + sha512。`GITHUB_API` 可覆盖（stub 回环 hermetic）。
  真验证：`--add 'oxlint@release:github/oxc-project/oxc@apps_v1.82.0/oxlint'`
  落 12MB 真二进制，随后 `--lint` 即出真警告（闭环）。
  模块 9 + 黑盒 2（dry-run 选包/真装落盘+lockfile/落盘可执行）。
- [x] 真验证（2026-09-11，GitHub apps_v1.82.0 standalone 实测）：`winterjs
  --lint` → 真 oxlint（no-debugger/no-unused-vars 警告逐行透传）；
  `--deny-warnings` exit=1 透传；`--fmt` 真写回（`const   x=1` → `const x = 1;`）。
  边界书面记录：npm 版 oxlint 的 bin 是 JS + `.node`（napi），无 node 的机器跑
  不了——standalone 二进制（`release:` 形安装）是唯一零 node 路径；`.node`
  加载是 napi 级工程，不在本次范围（`require('module')` 之类缺口同理，
  报可读错）。
- [x] 切片 b（2026-09-11）：权限开关 `--allow-*`（`src/permissions.rs`，opt-in 沙箱：
  不传旗标行为不变（141 例存量黑盒零回归），传任一 `--allow-*`/`--allow-all` 即进沙箱，
  未授权类默认拒绝，错误 `PermissionError: ...`（可读、可 catch、fs/sqlite 包装层直通不转形）。
  旗标：`--allow-read[=path,...]`/`--allow-write[=...]`/`--allow-env[=VAR,...]`/
  `--allow-run[=cmd,...]`/`--allow-ffi`/`--allow-all`（Run/Eval/Test 三子命令 flatten；
  裸旗标=该类全开，`=a,b` 清单，require_equals 防吞位置参数）。授权模型：Grant
  {None=未授, Some(空)=全开, Some(清单)}；路径清单 canonicalize 后前缀包含
  （不存在目标归一化到最近存在祖先，防 symlink 逃逸）；env 按键名、run 按首词
  basename、ffi 布尔。强制点：fs 全 native（arg_path_checked 类别显式）+ fs.exists +
  env 四 native + 枚举专用检查 + child_process 三入口 + sqlite open（:memory: 豁免，
  文件库读+写双查）+ ffi dlopen。
  偏差（书面记录）：cap-std ambient-authority Dir 化未采用——需重构 15+ 个 fs
  native，与收益不成比；落法为边界校验（canonicalize + 前缀），cap-std 依赖保留
  批准单内后续可用。模块单测 5 例（语义/拒绝/路径包含/env-run 匹配/CLI 解析，
  serial）+ 黑盒 3 例（fs 四态/env+run/sqlite+ffi）；`cargo test` 60+144 全绿，
  0 警告，冒烟 5/5。
- [x] 切片 c（2026-09-11）：sentry 崩溃上报（`src/sentry_report.rs`，opt-in）。
  开关 `WINTERJS_SENTRY_DSN`（未设/空 = 不初始化零成本；坏 DSN stderr 告警后继续）。
  实现勘误后大幅简化：无需自实现 transport（dependencies §2 勘误）——
  `sentry::init` 内部 `apply_defaults` 自动装 PanicIntegration/Context/
  stacktrace 集成 + DefaultTransportFactory（reqwest 特性下 ReqwestHttpTransport，
  自带后台 tokio 线程与 JS 线程零交互）；ring provider 在 init 提前 install_default
  （fetch 懒装可能更晚）。退出顺序：panic 路径自身 flush(None)（发完才 unwind →
  human-panic/标准 hook 链），main 的 process::exit 前防御性 flush(2s)（§4.8 兼容）。
  上报是旁路：不可达端点/任何 sentry 失败绝不影响 CLI（黑盒钉住）。
   模块单测 3 例（stub server 信封 e2e：POST /api/<proj>/envelope/ +
   X-Sentry-Auth + 消息内容 / panic hook 链真路 / DSN 门控）+ 黑盒 1 例
   （坏 DSN/不可达/未设 三态）；`cargo test` 67+147 全绿，0 警告，冒烟 5/5。
- [x] 顺延收官（2026-09-11）：全 flag CLI（`src/cli.rs` 重写：无裸子命令，
  动作一律 `-x/--xxx`，`§0.8` 规范；一次恰好一个动作；man 回单页）+
  中英双语 help（`rust-i18n`，`-l/--lang` > `WINTERJS_LANG` > 系统 > en；
  英文输出逐字节不变）+ add/install 拆分（本地 `node_modules` / 全局数据目录）。
  踩坑 §4.25（by-value 改造）§4.26（值紧贴/机械改名误伤）§4.28（空调用回滚）。
  收尾计数（2026-09-11 实测）：`cargo test` 100（单测）+176（黑盒）全绿
  （另 alloc 探针 1 过 1 忽略）；`cargo build` 0 警告；冒烟 5/5。

## Phase 9 — node: 落地（plan2.md，2026-09-12 立项）

- 目标：node: 兼容先到 deno 高度（全 23 + 半 16），终局全实现（除 plan2 §4 不做项）。
  切片/对标矩阵/依赖映射见 `docs/plan2.md` + `docs/dependencies2.md`
  （9a–9e 全部轮子已在闭包内，零新 crate；hyper/quinn 已拍板未引入）。
- 架构变化：node: 内嵌源经**绝对 `node:` URL 互引**（resolve 不依赖 base）；
  `node:internal/*` 共享小件入表但**不进 `available()`**（只供互引）。
- [x] 9a 纯 JS 先行（2026-09-12 完工）：
  `node:events`（全语义：errorMonitor/captureRejections/kEmitting 可变数组快照/
  shapeMode/once+AbortSignal/on 异步迭代器水位+close/静态面（Node module.exports 同款）/
  EventEmitterAsyncResource）；
  `node:async_hooks`（ALS/AsyncResource 实做：构造期上下文快照 + runInAsyncScope 传播 +
  bind/snapshot；createHook/executionAsyncId stub 口径；**跨 await 传播不支持**——
  引擎无 async_hooks 原语，记档）；
  `node:util`（format 占位符逐字/promisify(custom+DEP0174)/callbackify(falsy 包裹)/
  inherits/deprecate/isDeepStrictEqual(严格面务实重写：循环 memo/无序 Map/Set/
  TypedArray 内容/原型同一性)/toUSVString/convertProcessSignalToExitCode/debuglog/
  styleText(最小表)/legacy is*(DEP0044-57)；parseArgs/parseEnv/MIME/getSystemError*
  顺延记档）+ `node:util/types`；
  `node:querystring`（escape/unescape/stringify/parse 逐字）+ internal/querystring；
  `node:punycode`（punycode.js 2.1.0 逐字，RFC 3492 向量过）；
  `node:string_decoder`（纯 JS：utf8 经 TextDecoder 流式 + lastChar 预扫描/
  utf16 奇尾 FFFD flush/base64 组缓存/hex/latin1/ascii；legacy lastNeed/lastTotal 面保持）；
  `node:diagnostics_channel`（惰性激活原型切换/publish/bindStore/runStores(ALS 集成)/
  TracingChannel traceSync/tracePromise/traceCallback 五窗口；using/DisposableStack →
  手动 dispose、WeakRefMap → Map，记档）；
  `node:trace_events`（JS 侧类别集；native CategorySet 面记档）；
  `node:tty`（薄面：isatty 走既有 stdio native、setRawMode 标志位、基座 EventEmitter，
  net 未落地记档）。
  `node:internal/*` 九件（errors E 机制+消息逐字/validators/fixed_queue 逐字/util/
  util/inspect(format 逐字+inspect 务实重写)/util/types/events:abort_listener/
  events:symbols/event_target）；primordials 还原直调，引擎差异实测
  （`Error.captureStackTrace`+`stackTraceLimit` 可用，hideStackFrames 保真），
  偏差逐条记模块头注。process prelude 补 `emitWarning`+`on('warning')`。
  黑盒 +18（Node 套件命名子集断言、消息逐字）；`cargo test` 102+194 全绿，
  0 警告，冒烟 5/5。踩坑记 AGENTS §4.31。
- [x] 9b 流与缓冲（2026-09-12 完工）：
  `node:stream` 整批——`internal/streams` 24 文件逐字内嵌（readable/writable/
  duplex/duplexify/duplexpair/transform/passthrough/from/operators/iter_classic/
  iter_types/compose/pipeline/end_of_stream/destroy/add_abort_signal/state/utils/
  legacy/lazy_transform 等），require → 静态图垫片映射，真类继承环
  （duplexify↔duplex）经 registry 双方自注册 + node:stream body 末尾拉 duplexify
  进图，循环依赖一律懒解环（`__ensureXxx()`）；
  四类流 + PassThrough 全语义（flow/pause-resume/cork-uncork/destroy/finish/
  error 传播/write-after-end），chunk 口径与 Node 一致（string → Buffer，
  writable.js:475/readable.js:488）；`compose`（含 Duplex 构造分支）、
  `pipeline`（命名导出 callback 形态）、`finished`（promise + callback 双形态）、
  `addAbortSignal`、hwm 存取、`duplexPair`；
  `Readable.from`（字符串单块/iterable/async gen，Node 同款）+ 异步迭代器
  （for await 早退销毁）+ `toArray` + objectMode；
  `toWeb`/`fromWeb`（Readable/Writable 四向）；`node:stream/web`（web 全局
  re-export）、`node:stream/promises`、`node:stream/consumers`（六件套）；
  FastBuffer ↔ Buffer 原型桥（lib/buffer.js:157 口径：instanceof/isBuffer/
  constructor.name 显 Buffer；共享原型 → 本仓 setPrototypeOf 桥接，微偏差记模块头注）；
  `node:buffer` 模块面（named/default/constants/INSPECT_MAX_BYTES/SlowBuffer，
  全局 Buffer prelude 口径不变）+ 全局 **Blob**（Web spec 语义：iterable parts/
  嵌套 Blob/size/type 小写化/text/arrayBuffer/bytes/slice 负索引/stream）；
  `node:timers/promises`（setTimeout/setImmediate/setInterval AsyncIterator/
  scheduler.yield/wait，AbortError 口径；setImmediate≈setTimeout(0) 记档）。
  修复：internal/errors 补 `aggregateTwoErrors`（4 模块在用而未导出，静默雷）；
  compose Duplex 懒解环漏收口；SlowBuffer `new Buffer.alloc` 静态非构造。
  记档缺口（非 9b）：全局 setTimeout 返回裸 number（无 Timeout 对象
  refresh/unref/ref）、全局 setImmediate 缺失（process/timers 全局面）。
  黑盒 +7（buffer 六编码+报错三件/Blob/RW 核心/duplex-transform-pipeline/
  from-iterator/web+consumers/timers-promises）；`cargo test` 102+202 全绿，
  冒烟 5/5。踩坑记 AGENTS §4.32。
- [x] 9c fs 补齐（2026-09-12 完工）：
  native +16（read_link/link/symlink/truncate/utimes/chmod/access/open/close/
  read_fd/write_fd/ftruncate/fstat/fchmod/futimes/fsync），**全 std 零新 crate**
  （dependencies2.md §9c 口径：fs-err + std + tokio fs；utimes 走 File::set_times、
  chmod 走 PermissionsExt、stat unix 元字段走 MetadataExt/FileTypeExt）；
  fd 表为进程级合成号（自 3 起单调不复用，记档；EBADF 入 io_code）；
  `read(fd,…)` 经 native 返回新 Uint8Array + JS 层 `.set()` 写回用户 buffer——
  零新 UNSAFE-BOUNDARY（§0.6 证伪路线）；access 的 X_OK 近似 mode 搜索位（记档）。
  JS 面：同步增补（accessSync/truncateSync/utimesSync/chmodSync/linkSync/
  symlinkSync/readlinkSync/cpSync 递归/opendirSync）、fd 系（openSync 返回数字 fd/
  readSync/writeSync/closeSync/fstatSync/ftruncateSync/fchmodSync/futimesSync/
  fsyncSync/fdatasyncSync）、flags 字符串全表（r/r+/w/wx/w+/a/ax/a+/ax+；数字
  位值取 Linux 口径，记档）、__Stats 补 dev/ino/nlink/uid/gid/rdev/blksize/blocks +
  FIFO/socket/block/char 判定、`Stats`/`Dirent`/`Dir`（惰性游标 + 同步/异步迭代器，
  记档非真流式）、`FileHandle`（read/write/stat/truncate/chmod/utimes/sync/datasync/
  readFile/writeFile/appendFile/close）、**回调全家 28 API**（err-first，queueMicrotask
  派发，util.promisify 互操作）、`fs.promises` 挂 node:fs 本体、fs/promises 反向
  re-export 免环；writeFileSync/readFileSync 支持 `flag`；writeFile `mode` 仅新建
  应用（记档近似）。记档不做：chown/lchmod/lutimes/statfs/watchFile（std 无
  syscall 底座，验收子集不含）。
  黑盒 +3（同步 extras/FileHandle+promises/回调全家）；`cargo test` 102+205 全绿，
  冒烟 5/5。踩坑记 AGENTS §4.33。
- [x] 9d-1/2 net + dns 切片（2026-09-12 完工；http/https/http2/tls/dgram/zlib 顺延）：
  `node:net`——TCP Socket/Server（tokio net 底座），事件模型与 child 同构
  （task → channel → 事件循环 pump → dispatch 调 target 预绑定的 `__ev` 钩子，
  EventEmitter 翻译）；半关双旗（half_read/half_write）+ `close_sent` 单发 +
  dispatch 后统一 purge 的收尾单出口；**allowHalfOpen=false 默认自动回 FIN**
  （Node 口径，漏掉即连接半开、`net_open` 不归零、事件循环 hang）；writer task
  死亡标记（destroy 路径 reader EOF 代行收尾）；Socket connect/write/end/destroy/
  address/setEncoding/data(end/close/connect/error)、Server listen/close/address/
  connection/listening/error/close、createServer/createConnection/connect、
  Stream 别名；fd/事件 id 合成号（记档）；unix socket/ref-unref/write flush 语义
  记档不做。事件循环第 5 条通道 net_rx 全链接线（SessionInit/run/pump/event_loop/
  repl）+ idle 条件加 `net_open()==0`。
  `node:dns`——lookup（含 all/family 选项）/resolve4/resolve6 + promises
  （std `ToSocketAddrs` 底座，hermetic 只测 localhost；resolve* 深件 CNAME/MX/TXT
  需 hickory-resolver **行级增补，动工前按 §0.5 问用户拍板**，记档）。
  native +7（net_connect/net_listen/net_attach/net_write/net_end/net_destroy/
  dns_lookup）；io_code 补 EBADF(9)/EADDRINUSE(macOS 48/Linux 98)。
  黑盒 +3（回环 echo+destroy/EADDRINUSE/dns localhost，port 0 hermetic 并行安全）；
  `cargo test` 102+208 全绿，冒烟 5/5。踩坑记 AGENTS §4.34。
- [x] 9d-3 http（2026-09-12 完工；https/tls、dgram/zlib、http2 顺延）：
  `node:http`——**纯 JS 架在 node:net 之上，零新 native、零新依赖**（架构决策：
  node:http 是纯 HTTP/1.1，Node 本尊即 net + 独立解析器结构；hyper 直引/
  httparse 行级增补就此**不需要**，§0.5 拍板项绕开；解析器 JS 子集：请求/响应
  头 latin1 解码 + Content-Length + chunked 双向解码，trailer 忽略）。
  Server（net.Server 子类；连接级解析喂入，体备齐后先 emit("request") 再喂
  data/end——监听器时序）+ ServerResponse（writeHead/setHeader/getHeader/
  write/end/finish，Content-Length 自动补、connection: close）；ClientRequest
  （request/get，options 对象 + URL 字符串形态，头小写、Host 默认、体缓冲
  connect+end 齐后一次成帧、无 CL 响应读到 EOF、https URL 即 ERR_INVALID_PROTOCOL）
  + IncomingMessage（data/end/complete/destroy）+ STATUS_CODES/METHODS/
  maxHeaderSize/Agent stub（globalAgent）。
  偏差记档：无 keep-alive（单请求一连接，双侧 connection: close）；体整收单块
  'data'；IncomingMessage/OutgoingMessage 非 node:stream 全家（EventEmitter
  形状）；chunked trailer 忽略。io_code 补 ECONNREFUSED（macOS 61/Linux 111）。
  黑盒 +2（回环 GET/POST/404/500/finish/close 全链 + 客户端错误路径：
  ECONNREFUSED/https 拒绝/write-after-end）；`cargo test` 102+210 全绿，
  冒烟 5/5。踩坑记 AGENTS §4.35。
- [x] 9d-4 dgram（2026-09-12 完工；https/tls、zlib、http2 顺延）：
  `node:dgram`——UDP Socket（tokio net 底座），**复用 net 事件通道与状态机**
  （NetCmd::SendTo + NetKind::DgramListening/DgramMessage 变体、net_open 计数、
  close_once 单发旗、dispatch Close 后统一 purge——net.rs 零重复实现）；
  单 task `select!`（recv_from ↔ 命令通道）；bind/send(address+port 双形态)/
  close/message(msg, rinfo)/listening/error/close/address/ref-unref；
  createSocket 类型校验（udp4/udp6，ERR_SOCKET_BAD_TYPE 带 code）。
  偏差记档：无 connect/disconnect、组播/广播、setTTL/setBroadcast 系（子集）。
  踩坑：§4.34 坑一/坑三二进宫（__ev 忘预绑、task 侧 purge 抢跑）——已沉淀为
  「新事件域 checklist」，见 AGENTS §4.36。
  黑盒 +1（UDP 回环双 socket + 类型校验）；`cargo test` 102+211 全绿，冒烟 5/5。
- [x] 9d-5 zlib（2026-09-12 完工；https/tls、http2 顺延）：
  `node:zlib`——convenience 面（`src/builtins/node/zlib.rs`，11 natives +
  JS 薄壳，零新 crate）：deflate/inflate/deflateRaw/inflateRaw/gzip/gunzip/
  unzip（flate2）+ brotliCompress/brotliDecompress（brotli 9，quality 缺省 11，
  `params[1]` 即 BROTLI_PARAM_QUALITY 等效）+ zstdCompress/zstdDecompress
  （ruzstd；编码恒 Fastest——Default/Better/Best 标 UNIMPLEMENTED，见 §4.37）；
  Sync + 回调双形态（回调经 `queueMicrotask` 派发，同步底层记档）；
  `constants`（Z_* 取 zlib.h 通用值，BROTLI_* 经轮子源码实测）+ `codes` 双向表 +
  顶层非 BROTLI 别名（Node 口径）；输出 `Buffer.prototype` 桥接
  （`Buffer.isBuffer` 为 true）。
  偏差记档：流式类/`createXxx`、`crc32`、Zip 实验面、dictionary/flush 系不做；
  异步无线程池（大块阻塞记档）。
  踩坑：Sync 校验抛在错误包装内被误标 code（§4.37）。
  黑盒 +3（sync 五格式往返/回调嵌套链/报错边界）；`cargo test` 104+214 全绿，
  0 新增警告，冒烟 5/5。
- [x] 9d-6 https/tls（2026-09-12 完工；http2 顺延）：
  `node:tls`（`src/builtins/node/tls.rs`，2 natives：握手底座，读写复用 net 通道/
  状态机/`net_*` natives，`spawn_pumps` 泛化零重复实现）+ `node:https`
  （`src/builtins/node/https.rs`，帧层与 http 同语义）+ 帧层抽
  `node:internal/http_framing`（`withHttpServer`/`withClientRequest` 双注入，
  http 改薄包层逐行保真；附带修 `createServer(opts,cb)` 掉回调 + 补
  `request/get(url,opts,cb)` 三形态）。
  TLS：`ca` PEM 自建 roots / `rejectUnauthorized:false` 跳校验 / 缺省系统 roots；
  服务端 PEM 错同步 TypeError；握手失败 `ERR_TLS_HANDSHAKE`（细粒度 cert 码顺延）。
  偏差记档：`createSecureContext`/`getPeerCertificate`/客户端证书不做。
  踩坑：三参 `get` 缺失致回环 hang（§4.38）；自签测试证书须 end-entity（§4.38）。
  黑盒 +3（tls 回环双授权路径/https GET+POST/错误路径，rcgen 实时签发 hermetic）；
  `cargo test` 106+217 全绿（serve 3 例并行 flake 一次，单跑+重跑全绿），
  0 新增警告，冒烟 5/5。
- [x] 9d-7 http2（2026-09-12 完工；9d 关口收官，剩 9e）：
  `node:http2`（`src/builtins/node/http2.rs`，4 natives；hyper `server`+`http2`
  行级直引 `Cargo.toml`，§h2 拍板口径）：h2c prior-knowledge + h2s（ALPN h2，
  tls 复用 `server_config_h2`/`client_config_h2`）；事件复用 net 通道
  （`H2Request`/`H2Stream`/`H2SessionClose` 变体 + `H2Respond`/`H2Open` 命令，
  零新 channel）；服务端整收应答经 oneshot；客户端 session 驱动与 conn 同 task。
  偏差记档：无 Upgrade/h1c 回落、体整收、无推送/trailer/流控 knob、反射面仅 compat 子集。
  踩坑两件：① `createServer` 构造器与包层双注册 request 致请求事件双发（§4.39）；
  ② 1MB 体 SIGBUS——实为全域 `Heap` 搬运 UB（§4.40），非 http2 之过。
   黑盒 +3（h2c 双流多路/h2s 回环/拒连+空体+1MB 边界）；`cargo test` 108+220 全绿
   （单测 108 含 http2 新增 2，黑盒 node 63 含 http2 3，builtins 17 含 GC 回归 1），
   0 新增警告，冒烟 5/5。
- [x] 9e-1a node:crypto 哈希/随机（2026-09-12 完工）：
  `src/builtins/node/crypto.rs`（natives + JS 薄壳，零新 crate）：
  Hash 流式（new/update/digest/copy，md5/sha1/sha256/sha384/sha512，
  真机向量逐字节对）+ Hmac 流式（二次 digest 回空，Node 口径）+
  随机（randomBytes/randomInt/fill + `randomUUID` 复用）+ 杂项（getHashes/
  getCiphers 常量表）。真机口径：`createHash('nope')` 无码原文错（不编 `code`）、
  `getMacs` 真机不存在（不做）、`digest(badEnc)` 回 Buffer。
  黑盒 +3（哈希向量/随机链/报错边界，`tests/node.rs::phase9e_crypto_hash_hmac` 等）；
  模块单测（`crypto_hash_known_vectors`/`crypto_hash_norm_table` 等）。
- [x] 9e-1b 对称密码（2026-09-12 完工）：
  CBC/CTR 流式（Cipher/Decipher：new/update/final，PKCS#7 填充，非恒定时间记档）+
  GCM（tagLength/tag 校验，iv 限 12B 记档）/ChaCha20-Poly1305，
  6 组真机向量全对。黑盒 +2（分组往返+认证/填充/状态报错）。
- [x] 9e-1c 非对称（2026-09-12 完工）：
  KeyObject（createSecretKey/createPublicKey/createPrivateKey，DER/JWK 进出）+
  Sign/Verify（含 RSA-PSS/Ed25519，裸 r‖s 口径沿用 c-4a）+ RSA 加解密
  （OAEP-SHA1 与 v1.5-SHA1-MD5 **手写**：MGF1 + BigUint 模幂——`rsa 0.9` 绑
  `digest 0.10`，`sha1_010` 需新 crate，按 §0.5 未引，见 AGENTS §4.43）+
  ECDH/DH（`dh_genkey/dh_secret/dh_range`）+ 素性（`is_prime` Miller-Rabin；
  `bigint` 形态 9h-1 落地，见下）；双向真机交叉验证
  （本仓加密→真机解密 + 真机加密→本仓解密）。
  零新依赖铁律：`digest`/`cipher`/`rsa::BigUint`/`rsa::rand_core` 全经重导出直用；
  `hmac 0.13` 与 `sha3` 不兼容 → HMAC 通用构造自架。
  黑盒 +3（密钥签名/加解密交换/素性边界）。
- [x] 9e-1d KDF + X509（2026-09-12 完工）：
  pbkdf2/scrypt/hkdf（`hkdfSync` 回 ArrayBuffer，真机口径）/argon2
  （`kdf_argon`；AD 在 9h-1 接线，见下）+ X509 解析（`X509Certificate`：
  subject/issuer/serialNumber/validity/fingerprint，openssl 交叉格式一致；
  verify 不做记档）。黑盒 +2（KDF 向量/X509 内嵌证书全断言）。
- [x] 9e-3 node:perf_hooks（2026-09-12 完工）：
  纯 JS（`src/builtins/node/perf_hooks.rs` 内嵌源）：mark/measure/observer
  （observer 语义与真机对齐修过一次）/Histogram（含 empty 哨兵
  `min=INT64_MAX`，真机口径）/ELU 采样器。黑盒 +1
  （`phase9e_perf_hooks_surface`：计时/观察者/直方图/采样器全链）。
- [x] 9e-4 node:inspector + child 角落（2026-09-12 完工）：
  inspector 会话薄层（`Runtime.evaluate` 真求值 + 域 ack，
  `phase9e_inspector_session`）+ child 角落（exec/execFile 异步 +
  execFileSync + 退出码属性，`phase9e_child_corners`；fork/真 IPC 不做记档）。
  黑盒 +2。
- 9e 收官（2026-09-12）：`cargo test` 单测 118 + 黑盒 234 全绿
  （node 76 含 9e 新增 13：crypto 10/perf 1/inspector+child 2；
  另 alloc 探针 1 过 1 忽略；总 352 passed + 1 ignored，0 failed），
  0 新增警告（5 预存），冒烟 5/5。`cluster` 按 plan2 §4 顺延 v1 之后（多进程语义重）。
  9e 记档缺口：ripemd160/XOF（sha3 0.12 已无 SHAKE，轮子待定，§0.5 问用户）、
  PQ（ml-kem）、fork/真 IPC 通道、X509 verify、GCM iv 限 12B、
  PKCS#7 非恒定时间。
- [x] 9f-1 node:vm（2026-09-12 完工；引擎深水）：
  同 Runtime 多 global 沙箱（`src/builtins/node/vm.rs`，9 natives + JS 壳，
  零新 crate）：`JS_NewGlobalObject(SIMPLE_GLOBAL_CLASS)` 建独立 global
  （新 compartment，标准类同套懒 resolve，无 prelude；job queue per-context
  共享不重装）→ `Box<Heap>` 入 `state::vm_contexts`（§4.40 定址），id 单调；
  求值走 `evaluate_script`（自进目标 realm）+ 同步一轮 `RunJobs`
  （afterEvaluate 等效）；完成值对象跨 compartment 以 CCW 传递；
  沙箱快照式同步（run 前 sync-in、后 sync-out，只回写非标准初始键）；
  错误保 name/message（包络重建同名 Error）；`Script` 构造期 `Compile1` 预检；
  `compileFunction`（主/parsingContext 双目标 + contextExtensions）；
  `measureMemory` 实验警告 + 恒 reject。真机逐项对齐（隔离/回写/`isContext`/
  RangeError 保真/ctor SyntaxError）。
  偏差记档：沙箱非活绑定；`timeout`/`breakOnSigint` 只校验；无字节码缓存；
  `microtaskMode` 恒 afterEvaluate 等效；模块/`import()` 不支持；错误无 stack 跨域。
  黑盒 +2（上下文隔离/Script/compileFunction + 报错边界）；`cargo test` 全绿。
- [x] 9f-2 worker 消息通道（2026-09-12 完工）：
  `src/builtins/node/worker.rs` + 事件循环第 6 通道 `worker_rx`
  （`init_session`/`pump_once`/`event_loop`/`repl` 全链，idle 条件加
  `worker_open()==0`）：`MessageChannel`/`MessagePort`（JSON 线经对端会话收件箱，
  同会话回环亦走循环故恒异步；paused 口径——无监听只排队，`newListener` 开闸；
  计数 `open && refed && listening`）+ `receiveMessageOnPort`/
  `moveMessagePortToContext`（恒返自身）/`markAsUncloneable`（`DataCloneError`
  具名）+ `isMainThread`/`threadId`（主 true/0）/`parentPort`/`workerData`
  （主 null）/`resourceLimits` `{}`/`SHARE_ENV`/环境数据（进程级共享）。
  附带修事件循环真 race（§4.46）：`progressed` 后直接 park 会饿死只排了 microtask
  的结算（无 timer 即 hang）——改为回顶再跑一轮；`define_all` 加重名 native
  `debug_assert`（§4.48）。
  黑盒 +2（通道往返/迟监听/receive + 线程信息/边界）；`cargo test` 全绿。
- [x] 9f-3 Worker（2026-09-12 完工）：
  每 worker 独立 OS 线程 + 完整会话（`runtime::run_worker_thread`，16MB 栈，
  §4.24 哲学；boot 经线程局部槽进 `init_session` 落地：身份/workerData/
  parentPort/权限继承 CLI 快照）：boot rendezvous（收件箱 + parentPort 就绪才
  返回；早失败亦发 parked 端 rendezvous，事件不丢、主侧不超时）→ `WOnline` →
  用户脚本 → `WMsg`/`WError` → `WExit`。文件 worker 走文件管线；eval 串嗅探
  ESM→落临时 `.mjs`（复用管线），否则经典求值。退出码：排空 0/未捕获错 1/
  终止 1（`WTerminate` 检查点生效）/`process.exit(n)`→n。
  黑盒 +3（eval+data/双向 terminate/错误边界 + 文件 worker；真机 w1–w4 逐行对齐）。
  偏差记档：transfer 忽略；eval completion 照脚本语义打印；同步死循环停不下来；
  workerData `undefined`→`null`；stdio 恒 null；execArgv 等接受忽略；
  `BroadcastChannel` 等不导出；无 error 监听即 fatal（Node 同款）。
- 9f 收官（2026-09-12）：`cargo test` 单测 119 + 黑盒 241 全绿
  （node 83 含 9f 新增 7：vm 2/channel 2/worker 3；总 360 passed + 1 ignored，
  0 failed），0 新增警告（5 预存），冒烟 5/5，零新 crate（`Cargo.toml` 未动）。
- quinn 接线（2026-09-13）：`quinn 0.11.11` 入 `Cargo.toml`（optional + 进
  `default`，自身 default 特性全开，`cargo tree` 无 aws-lc/C 新增）+
  `tests/quic.rs` 回环实证 2 例（握手/双向流 echo/自签负路径，hermetic）；
  `node:quic` JS 面未出（v1 不验收；dependencies2 §quic 记全）。
- [x] 9g-1 node:quic Endpoint + 会话（2026-09-13 完工）：
  `src/builtins/node/quic.rs`（9 natives + JS 壳，零新 crate；事件循环第 7 通道
  `quic_rx`）：`listen(cb, opts)`（key/cert PEM 必给，ALPN 必选，cc 三档真映射，
  idleTimeout 毫秒）/ `connect(addr, opts)`（ca 缺省系统 roots，
  `rejectUnauthorized:false` 跳校验，servername 缺省 host）→ `QuicEndpoint`
  （address/close/`'session'`）/ `QuicSession`（`'secure'`/`'close'`/`'error'`、
  close/destroy、local/remoteAddress、alpnProtocol/servername、真值 stats 子集
  rtt/udp 收发/cwnd/loss）+ `QuicError` + `CC_ALGO_*`。握手失败客户端
  `ERR_QUIC_HANDSHAKE` + Close（tls §9d-6 同口径），服务端静默丢（防扫描杀服务）。
  偏差记档：无 ping（quinn 0.11 无原语）、无客户端证书、endpoint 无 stats、
  updateKey/SNI 上下文/qlog/keylog/ticket/0-RTT/headers 全不支持、
  `DEFAULT_CIPHERS/GROUPS` 不导出。
  黑盒 +2（secure 回环 + 握手失败/校验边界，rcgen 自签 hermetic）。
- [x] 9g-2 QUIC 流/数据报（2026-09-13 完工）：
  会话驱动任务（命令/open/accept/数据报/`closed()` 守望三合一，`done` 旗防双发
  `SessionClose`）+ 读写半端任务（任一半终结即整流收尾，单出口）：
  `createBidirectionalStream()`/`createUnidirectionalStream()`（Node 同款 async，
  就绪 resolve）/ `QuicStream`（EventEmitter：data/end/finish/close/error，
  write/end/close/destroy/stopSending/resetStream，id/direction，码 number/bigint）
  / `sendDatagram`（超限静默丢，Node 同款）/`'datagram'`/`maxDatagramSize`。
  收尾纪律：驱动发 `SessionClose` 前先收半端任务（teardown 噪声零误报）；
  会话收尾带走名下全流。偏差记档：流非 `stream.Duplex`（事件形）；
  半开独立语义弱化（首个终结即整流关）；headers/trailers/priority/stats 不支持。
  黑盒 +2（双向 echo + 单向 + 数据报 + reset/半流/超限/码边界全链）。
- 9g 收官（2026-09-13）：`cargo test` 单测 119 + 黑盒 247 全绿
  （quic 6：rust 实证 2 + 黑盒 4；总 366 passed + 1 ignored，0 failed），
  0 新增警告（5 预存），冒烟 5/5。
- [x] 9h-1 crypto 补齐（2026-09-13 完工；树内轮子，零新 crate）：
  secp256k1（`k256` 加 `ecdh` 特性：ECDH/签名/`generateKeyPair`/`getCurves`，
  双向真机交叉验证）+ DSA（`dsa` 加 `hazmat` 特性：生成/签名 deterministic
  RFC6979/验签/`pkcs8`·`spki`·`jwk` 进出全链，双向交叉验证）+
  argon2 AD（`ParamsBuilder::data`，真机向量逐字节对）+
  `generatePrimeSync` bigint（16 进制桥，零 native 改动）。
  附带修两真 bug：① SPKI/PKCS#8 试解靠坐标长度把 secp256k1 误判 P-256
  （同 32 字节）→ 算法 OID 直判（`__wjs_ec_guess_curve` + SEC1 `[0]` OID，
  openssl 向量单测钉住）；② k256 验签拒 high-S → 验前 `normalize_s`
  （OpenSSL 接受可锻造签名）；③ Node PKCS#8 省公钥 y → `y=g^x mod p` 补算。
  黑盒 +2（k256 真机向量/ECDH 自洽/签名编解码全链 + DSA 全链/bigint/AD 向量）。
  剩缺口：ripemd160、XOF（9h-2 落地，见下）、PQ、X509 verify、
  GCM 非 12B iv（轮子不支持）。
- [x] 9h-2 ripemd160 + SHAKE（2026-09-13 完工；用户拍板两项全要）：
  `ripemd 0.2`（digest 0.11 直通，零版本墙）+ `tiny-keccak 2`（`shake` 特性；
  `sha3 0.12` 已无 SHAKE 类型）→ 流式 Hash 注册表（定长 ripemd + 变长 XOF，
  长度注册时带；`copy()` 同克隆）+ `getHashes` + HMAC（ripemd 进表；
  HMAC-SHAKE 真机抛无码错，本仓同款抛无码 `Error`，不做超集）+
  缺省输出长（shake128→16/shake256→32 + DEP0198 真机文案警告）+
  `outputLength` 非法即 `ERR_INVALID_ARG_VALUE`。真机向量逐字节对
  （ripemd/shake128/shake256/HMAC-ripemd；记忆向量全错过一次，先对真机，
  §4.32 教训延续）。
  黑盒 +1（向量/HMAC/拷贝/缺省警告/DEP0198 stderr）。
- 9h 收官（2026-09-13）：`cargo test` 单测 120 + 黑盒 250 全绿
  （node 86 含 9h 新增 3；总 370 passed + 1 ignored，0 failed），
  0 新增警告（5 预存），冒烟 5/5。
- 9h 收官（2026-09-13）：`cargo test` 单测 120 + 黑盒 250 全绿
  （node 86 含 9h 新增 3；总 370 passed + 1 ignored，0 failed），
  0 新增警告（5 预存），冒烟 5/5。
  剩终局缺口：PQ、X509 verify、GCM 非 12B iv（轮子不支持）、`vm` 模块系、
  `worker_threads` 传输细节、quic headers 面、
  plan2 §4 不做项（wasi/v8 口径/sea/napi/cluster/domain/repl 口径）。
- [x] 9i-1 vm 模块系（2026-09-13 完工；零新 crate）：
  `Module` 基类（link 校验 linker 函数形态，`ERR_INVALID_ARG_TYPE` 真机口径）+
  `SourceTextModule`（`__wjs_vm_compile_mod/link/evaluate/mod_ns/mod_release/
  mod_settled` 6 natives + JS 壳 status 机：compile 在目标 compartment 内
  `load_js` 转译（含 TS，ext 从 identifier 点后缀取）+ `CompileModule1`
  （filename=identifier）；link 走 `load_dependencies` + `ModuleLink`
  （零导入恒过；带导入 v1 报 `ERR_VM_MODULE_LINK_FAILURE`，linker 切片后续——
  进程级 load hook 按主注册表工作，vm 记录带入即 compartment 错配）；
  evaluate 走 `ModuleEvaluate` + 一轮 `RunJobs`，完成值 thenable 认领
  （跨域 promise 恒异步，`instanceof` 恒 false，见 AGENTS §4.57）+
  落定后补 evaluated 位/读 namespace（`vm_mod_ns` 以位为门），失败置 errored +
  `module.error`；`dependencySpecifiers` 真表（编译期存 `deps`）。
  `SyntheticModule` 纯 JS（出生即 linked，linker 可选，evaluateCallback 回填，
  namespace 冻结快照；`setExport` 终态后拒）。
  真机逐项对齐（`node --experimental-vm-modules` 实测：默认 `vm:module(n)`/
  status 四态/link 必函数/evaluate 恒 promise/二次求值成功/二次 link 拒/
  error 只读 errored 态/Synthetic 出生 linked）。
  偏差记档：`context` 未给走新建上下文（返 `undefined`，非活对象）；
  `importModuleDynamically`/`initializeImportMeta` 接受忽略；linker 回调 v1
  从不调用；`import()` 仍走主线未定义。
  黑盒 +2（零导入全链/隔离/TLA + 报错边界 17 断言）；`cargo test` 单测 120 +
  黑盒 252 全绿（node 88；总 372 passed + 1 ignored，0 failed），0 新增警告，
  冒烟 5/5。踩坑记 AGENTS §4.57。
- [x] 9i-2 worker 传输细节（2026-09-13 完工；零新 crate）：
  线信封 v2（单 JSON 串，Rust 通道零改动）：BigInt/undefined/Date/Map/Set/
  ArrayBuffer/视图（类型保留，视图全量字节 + 偏移复原）全保留；循环/函数/
  symbol/不可克隆内置（`__denyClone` 显式拒绝表——本引擎 structuredClone
  不支持 BigInt，探路逻辑与之互斥，见 §4.58）/非法 transfer/分离中 buffer
  报 DataCloneError；transfer 非数组忽略（真机实测）；共享引用变多份拷贝、
  SAB 只拷贝（变普通 AB）记档。
  端口迁移（`__wjs_port_offer/accept/withdraw/detach` 4 natives + `PortForward/
  PortDrop` 事件 + 源表项转发器）：offer 置 `moved` 停计数但保留 target →
  accept 建表（路由直连原对端）并回发 `PortForward` → 目标 `__ev("forwarded")`
  经现路由排空后摘除；分发侧 `PortMsg` 备 `port_forward_route` 回退（排空竞态
  不丢）；承接端 close 发 `PortDrop` 拆转发器；源端 neutered 后用静默
  （真机同款）；`workerData.transferList`/`parentPort`/`Worker.postMessage`
  全接线，workerData 缺省仍 `null`（存量不断）。
  `BroadcastChannel`（`__wjs_bc_sub/unsub/pub/flags/attach` 5 natives + `BcMsg`
  事件 + 进程级注册表 + 会话 `bc_targets`，计数抄端口口径）：同名跨会话扇出，
  发者自收排除（`(sess, sub)` 对），关者止收；基座 EventEmitter（真机
  EventTarget，记档），`message` 载荷裸值 + `onmessage` 收 `{data}`。
  黑盒 +3（buffer/detach/类型/报错 10 行 + 同会话迁移双向/neuter + 跨线程
  workerData 端口双向/BC 同会话与跨线程）；`cargo test` 单测 120 + 黑盒 255
全绿（node 91；总 375 passed + 1 ignored，0 failed），0 新增警告，冒烟 5/5。
  踩坑记 AGENTS §4.58。
- [x] 9i-3 X509 verify（2026-09-13 完工；零新 crate）：
  `X509Certificate.verify(publicKey)`/`publicKey`/`ca` 三件就位（verify/签发/checkIssued/
  checkPrivateKey 的 ERR_NOT_SUPPORTED 占位退役；9h 收官缺口清单中的 X509 verify 销账）。
  Rust 底座（`src/builtins/crypto.rs` `__wjs_x509_verify`，复用验签件）：手解外层 TLV 取
  **TBS 裸段**（含自身 TLV，无重编码风险）→ 签名算法 OID 分发：RSA 全档
  （md5/sha1/sha224/256/384/512WithRSA，DigestInfo 前缀表 + `BigUint` modpow 手工
  EMSA-PKCS1-v1_5，OpenSSL 口径 FF≥8 非定长；绕开 rsa 0.9 的 digest 0.10 版本墙，
  §4.43 同源取舍，单测与 rsa crate 自签交叉钉住）/ ECDSA（ecdsa-with-SHA1-512，
  复用 `ec_curve_name`+`with_curve!`+`verify_prehash`，DER `SEQ{r,s}` → 定长裸 r‖s
  还原，high-S normalize_s 沿 §4.55）/ Ed25519（纯签名，裸 32B 公钥直验）；
  未知 OID/验签不过一律 false。`x509_parse` 增 `ca`（BasicConstraints 2.5.29.19，
  缺省 false）与 `spkiB64`（der::Encode 忠实重编，publicKey 经 createPublicKey 重建）。
  JS 壳真机口径（node 26.8.2 实测逐项对齐）：无参/非 KeyObject →
  `ERR_INVALID_ARG_TYPE`；私钥 → `ERR_INVALID_ARG_VALUE`；错钥/异族（含 x25519）/
  RSA-PSS/不支持算法 → false 不抛；`rsa-pss` KeyObject 按 rsa 分发（SPKI 同构）。
  真机交叉：本仓 ⇄ node 26.8.2 对同一组 openssl 证书（P-256/RSA/Ed25519/leaf）
  verify/ca 结果逐项一致。黑盒烤入 openssl 固件（RSA/Ed25519 PEM 常量，hermetic）+
  rcgen 自签 + 篡改（末字节翻转）/错钥/跨族/报错 13 断言（`phase9i_x509_verify`）；
  9e 遗留断言（verify 抛 ERR_NOT_SUPPORTED）随语义升级改写。模块单测 5
  （TLV 形/TBS 裸段逐字节/DigestInfo 标准前缀/EMSA 手工⇄rsa crate 交叉/DER 签名还原）；
  `cargo test` 单测 125 + 黑盒 256 全绿（总 381 passed，0 failed），0 新增警告，
  冒烟 5/5。记档不做：RSA-PSS 证书、checkIssued/checkPrivateKey、签发、
  显式参数 EC 证书（named-curve 缺失时 SPKI 试解失败即 false）。
- [x] 9i-4 ml-kem（2026-09-13 完工；`ml-kem 0.3.2` 已在 c0ef72d 落账，本次接线）：
  FIPS 203 三档（`generateKeyPair('ml-kem-512'|'ml-kem-768'|'ml-kem-1024')`）+
  封装面 `encapsulate(key)`/`decapsulate(key, ct)`。Rust 五 natives
  （`src/builtins/node/crypto.rs`：gen/seed_from_pkcs8/kind_from_spki/encaps/decaps）：
  getrandom 自造 64B 种子 → `DecapsulationKey::from_seed` 展开；密钥 DER 走
  **LAMPS 种子形 PKCS#8**（`SEQ{INT 0, SEQ{OID}, OCTET{[0] 64B}}`，总长恒 86）与
  SPKI（头定长 22B，ek 裸 800/1184/1568）——手拼 TLV（`mlkem_tlv`），导入即展开校验；
  封装 `encapsulate_with_rng`（rand 0.10 直通 CryptoRng）/解封装 `decapsulate_slice`
  （长度内建校验 + FIPS 203 隐式拒绝：等长坏文不抛回伪随机）。KeyObject 走
  material=DER 直通（export der/pem 免改）；导入分支进 pkcs8/spki try 链
  （X509 `publicKey` 对 ml-kem 证书同链可用）；JWK `kty:"AKP"`（pub=ek、priv=种子，
  b64url）。真机口径（node 26.8.2 逐项实测对齐）：尺寸表
  （SPKI 822/1206/1590、PKCS#8 86、ct 768/1088/1568、ss 32）、`encapsulate` 公/私
  KeyObject 均收、`decapsulate` 公钥 `ERR_CRYPTO_INVALID_KEY_OBJECT_TYPE`/非 ml-kem
  私钥无码错/非 KeyObject `ERR_OSSL_UNSUPPORTED`/错长 ct `ERR_CRYPTO_OPERATION_FAILED`、
  第二参（异步回调形）`ERR_INVALID_ARG_TYPE`。**双向真机交叉**：本仓密文真机解 ✓、
  本仓 SPKI/PKCS#8 真机导入互解 ✓、真机密文本仓解 ✓。
  模块单测 3（参数/OID 表+TLV 长形/PKCS#8+SPKI wrap-parse 往返+真 crate 展开三档）；
  黑盒 +1（23 断言：三档全链/报错/隐式拒绝/PEM）；`cargo test` 单测 128 + 黑盒 257
  全绿（总 385 passed，0 failed），0 新增警告，冒烟 5/5。记档不做：
  `generateKey` 单面（真机不支持，本仓 generateKey 亦仅 secret 面）、
  `encapsulate` 异步回调形、`generateKeyPair` 未知类型码（pre-existing 偏差）。
- [x] 9i-6 ml-dsa（2026-09-13 完工；`ml-dsa 0.1.1` 用户拍板引入，FIPS 204）：
  三档 `generateKeyPair('ml-dsa-44'|'ml-dsa-65'|'ml-dsa-87')` + 顶层
  `sign(null, data, key)`/`verify(null, data, key, sig)` 纯签名（非 null 即真机码
  `ERR_OSSL_INVALID_DIGEST`）。Rust 六 natives（gen/seed_from_pkcs8/kind_from_spki/
  public/sign/verify）：getrandom 自造 32B 种子 → `SigningKey::from_seed`；
  密钥 DER 种子形 PKCS#8（`[0]` 32B，总长恒 54）+ SPKI（22B 头 + 裸 pk
  1312/1952/2592）——复用 ml-kem 的 TLV 拼装/解析（种子长参数化）；导入进
  pkcs8/spki try 链 + `__derivePublic` 补 ml-kem/ml-dsa 两支（`createPublicKey`
  对 PQ 私钥可用）；JWK AKP（priv=种子）。crate Signer 走确定性档（真机 hedged，
  互验不受影响）。X.509 验签收 ml-dsa 证书（签名 OID 与密钥 OID 同族
  2.16.840.1.101.3.4.3.17/18/19，`x509_verify_impl` 增 ml-dsa 臂）。
  真机口径（node 26.8.2 实测对齐）：尺寸表、hash=null 语义、JWK。**双向交叉**：
  本仓确定性签名真机验 ✓、真机 hedged 签名本仓验 ✓、真机 PKCS#8/SPKI 本仓导入 ✓；
  openssl 3.6 ML-DSA-65 实签证书真机/本仓同验 ✓（固件入库 `tests/fixtures/`）。
  模块单测 2（表/TLV 往返 + sign-verify 核往返与档位错配）；黑盒 +1（20 断言，
  含真机签名与 ML-DSA 证书两件真机固件）；`cargo test` 总 388 passed，0 failed。
  依赖（dependencies2 §9i-6 注记）：`ml-dsa 0.1.1`（2024-10 建库/2026-09 维护/
  传递闭包纯 Rust/rand_core 0.10 与本仓 rand 同线/无 nightly），signature 3 +
  crypto-common 0.2 traits；`shake 0.1` 随入（RustCrypto/hashes，H 置换用）。
- [x] 9i-7 X509 checkIssued/checkPrivateKey（2026-09-13 完工；零新 crate）：
  `checkIssued(otherCert)`——native `__wjs_x509_check_issued` 按 OpenSSL
  `X509_check_issued` 主体口径：① 名字裸 DER 比较（TBS 内容 TLV 行走取
  issuer/subject 裸段，canonical 等价于 `X509_NAME_cmp`）+ ② leaf AKID.keyid
  在即要求 issuer SKI 相等（真机探针钉死判别器：**同名不同钥 CA → false**）+
  ③ issuer keyUsage 有则必须含 keyCertSign；`checkPrivateKey(privateKey)`——JS
  侧派生公钥（`__derivePublic` 补 ml-kem/ml-dsa 两支）与证书 SPKI 逐字节比
  （ed25519/x25519 material 裸 32B 手工包 SPKI 头；DSA 经 envelope 转换）。
  真机口径（node 26.8.2 实测）：非 X509Certificate/KeyObject →
  `ERR_INVALID_ARG_TYPE`，公钥入参 → `ERR_INVALID_ARG_VALUE`（与 verify 同形）。
  模块单测 1（名字 TLV 行走，含 v1 无版本头形态）；黑盒 +1（openssl 链固件
  入 `tests/fixtures/`：issued 三态/privkey 两态/报错四件/OKP/PQ，8 断言）；
  `cargo test` 总 390 passed，0 failed。记档：issuer 无 SKI 视为不匹配
  （真机不可复现该形，openssl 3.x 自动补 SKI）；无 AKID leaf 回落名字判。
- [x] 9i-5 quic-h3 门控实证（2026-09-13 完工；h3/h3-quinn 已在 c0ef72d 入
  `quinn` 特性组，本次 Rust 回环实证 + 纯度复查）：
  `tests/quic.rs::phase9i_h3_over_quinn_loopback`（hermetic，port 0，rcgen 自签，
  ALPN `h3`）：quinn 握手 → `h3::server::Connection` accept → 请求头
  `GET /probe`（method/path 断言）→ 响应头 200 + 自定头回送 → 客户端
  `recv_response` 收齐（driver `wait_idle` 后台轮询驱动）。两坑即时修：
  ① 服务端发完响应即退出任务 → conn drop 连带 ApplicationClose，客户端收不到
  响应帧——accept 循环守到对端关连接（§4.50/4.53 同族：任务尾清理抢跑）；
  ② h3 驱动必须被轮询，`client::new` 的 driver 挂 `wait_idle` 后台任务。
  `cargo tree -i aws-lc-rs`/`-i native-tls` 空（§2 门控红线保持）；
  h3 0.0.8/h3-quinn 0.0.10 经 `quinn = ["dep:quinn","dep:h3","dep:h3-quinn"]`
  特性组随 default 启用。`cargo test` 总 386 passed 0 failed，冒烟 5/5。
  记档：`node:quic` JS 面的 headers 分支（H3 上行）未出，v1 不验收
  （`node:quic` 裸流面照旧）；本切片只验门控与轮子可用性。

## 全局纪律

- 每 Phase 开工前更新本计划对应节；完工打钩并 commit。
- 每 Phase 收尾跑：`cargo build`＋`cargo test`＋`./target/debug/winterjs` 实测三件套。
- CI 矩阵扩展节奏：Phase 0 结尾立 6 列；android/ohos 在 Phase 3 后逐个点亮。
- 性能基线从 Phase 2 起用 criterion 锁定，退化即红。
