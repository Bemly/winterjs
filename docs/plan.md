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

## Phase 4 — Node 兼容垫片（开工 2026-09-10，切片 a 进行中）

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

## Phase 5 — 包管理（install/publish/upgrade，开工 2026-09-10，切片 a 进行中）

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

## Phase 8 — polish（lint/权限/远程缓存/上报，开工 2026-09-11，权限先行）

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

## 全局纪律

- 每 Phase 开工前更新本计划对应节；完工打钩并 commit。
- 每 Phase 收尾跑：`cargo build`＋`cargo test`＋`./target/debug/winterjs` 实测三件套。
- CI 矩阵扩展节奏：Phase 0 结尾立 6 列；android/ohos 在 Phase 3 后逐个点亮。
- 性能基线从 Phase 2 起用 criterion 锁定，退化即红。
