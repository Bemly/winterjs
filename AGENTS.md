# AGENTS.md — winterjs 工作规约

> Bun-like JS runtime，直连 Mozilla SpiderMonkey（经 `servo/mozjs` Rust 绑定）。
> 从 `winterjs-old`（WinterCG server + spiderfire）推倒重来，老项目只当参考，不合、不动。

## 0. 工作流铁律

1. **每次修改都要 `git commit`**：改完即提交，保持小步提交；提交前必看
   `git status --short` + `git diff`，只 stage 意图内的文件，绝不提交 secrets。
2. **先查证据再下结论**：读文件、跑构建、跑 `./target/debug/winterjs` 实测；
   发现与本文档矛盾的，以实测为准并更新本文档。
3. **踩坑必记**：新坑追加到 §4，写清症状 → 根因 → 修法 → 复现命令。
4. **依赖随缘更新**：除 `mozjs` 必须精确钉死外（§2 铁律），其余依赖不锁上限（caret），
   `cargo update` 随便跑；跑坏了就地修，并回写 `docs/dependencies.md`。
5. **新工具先找轮子**：每次想要新手写工具/模块/新功能时，不许直接手写；
   先去 crates.io 找依赖，符合标准就记入 `docs/dependencies.md`，
   然后**停下来问用户**等拍板，用户点头后才引入。
6. **能 safe 不 unsafe**：新增 `unsafe` 前必须先证伪 safe 路线
   （safe 写法、已有 crate、`unsafe` 构造器 + safe 访问器模式，见 §6）；
   存量 `unsafe` 只减不增，重构顺手收敛调用点。
7. **测试三件套随功能落地**：新功能必须同时带三层测试，完工标准含三绿——
   模块测试（`src/` 内 `#[cfg(test)]`，覆盖纯 Rust 可测逻辑：解析/转译/状态机/编解码）、
   黑盒测试（`tests/`，经 CLI 断言用户可见行为；**文件按 src 对齐**：
   `cli/builtins/crypto/fetch/ws/loader/node/pm/serve/acme/testrun/initpkg/repl/bun/permissions/lintfmt/sentry_report.rs`，
   共享 helper 进 `tests/common/mod.rs`（`use common::*;`），新 API 的黑盒进对应域文件，
   node 域二层按 `src/builtins/node/*.rs` 对齐（`tests/node/<mod>.rs` 经 `#[path]` 挂壳，
   域内共享脚手架进 `tests/node/helpers.rs`），
   每个新 API 必含正常 + 报错 + 边界三件；`UNSAFE-BOUNDARY` 新增必须配 panic 路径用例）、
   冒烟（§3 探针命令，构建后必跑，不过不提交）。
8. **CLI 全 flag 规范**：无裸子命令、无裸位置参数——所有动作一律 `-x/--xxx`
   显式 flag（如 `-r/--run <FILE>`、`-a/--add <PKG>`、`-i/--install <PKG>`）；
   一次恰好一个动作，多给即错；动作的必需值必须紧贴其 flag（`--run` 后直接跟
   别的 flag 会被判缺值）；修饰 flag（`--dry-run/--registry/--port` 等）只在对应
   动作下生效。help/补全/man 全由同一套 flag 生成（`localized_command`）。

## 1. 基线（2026-09-09）

- `mozjs = "=0.26.0"`（Gecko 153，crates.io 最新发布版），`Cargo.lock` 入库。
- Rust stable 最新（现 1.98），edition 2024（即 stable 最新；2027 尚不存在）。
- 版本号用 CalVer `YY.MM.PATCH`（如 `26.9.0`，cargo 可解析；`^26.9.0` 即年内自动升）。
  依赖清单与 10-target 矩阵见 `docs/dependencies.md`。
- 无 `rust-toolchain` pin、无 spiderfire/ion 依赖、无 server/request_handlers。
- CLI（全 flag，§0.8）：`winterjs --run <file|script>`（带脚本后缀→文件直跑；
  裸名→package.json `scripts` 优先、同名文件回落；JS bin 递归自身执行，零 node）/ `winterjs --eval <code>` /
  `winterjs --config [--schema]` / `winterjs --completions <shell>` / `winterjs --man` 等，
  见 `src/`（cli/runner/error/logging/settings/alloc 模块）。
- 依赖 2026-09-10 起全量入库（docs/dependencies.md 头部决策记录），代码按 Phase 接线。

## 2. 依赖铁律

- **mozjs 永远钉死精确版本**，只跟随 servo release 手动升级。
  **绝不 `cargo update -p mozjs`**（会浮到 servo main HEAD，当场炸）。
- 退路：若 153 线踩到上游 bug，退回 ESR140 线
 （`mozjs = "=0.15.18"` + `mozjs_sys =140.14.0-lts`，Servo 线上在用的线）。

## 3. 构建（macOS Apple Silicon，每次新 shell 必 export）

```bash
export SDKROOT="$(xcrun --show-sdk-path)"
export LIBCLANG_PATH="/opt/homebrew/opt/llvm/lib"   # bindgen 用
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
cargo build
```

- `mozjs_sys` 走预构建 `libjs_static.a`，debug 全量约 25 秒，不用怕。
- 验证：`./target/debug/winterjs --eval '40 + 2'` → `42`；
  `./target/debug/winterjs --eval 'throw new Error("boom")'` → 非 TTY 下
  `Error: eval.js:1:7: boom`，exit=1（TTY 下由 miette 图形渲染，带代码框，语义同）。

### 冒烟（每次构建后必跑，不过不提交）

```bash
./target/debug/winterjs --eval '40 + 2'                                                    # → 42
./target/debug/winterjs --eval 'await new Promise(r=>setTimeout(()=>r(1),10))'              # → 1
./target/debug/winterjs --eval 'new URL("https://ex.com/?a=1").search'                     # → ?a=1
./target/debug/winterjs --eval 'new TextEncoder().encode("hi").length'                     # → 2
./target/debug/winterjs --eval 'await (await fetch("data:text/plain,x")).text()'         # → x
```

## 4. 踩坑记录

### 4.1 取异常堆栈必须重进 Realm，否则 SEGV（2026-09-09）

- 症状：`throw new Error("boom")` 进程 exit=139（SIGSEGV），而非干净报错。
- 根因：`evaluate_script` 内部用 `AutoRealm` 进 realm，返回时已退出；
  此时调 `error_info_from_exception_stack`（内含 JSAPI）因无 current realm 直接野指针。
- 修法（`src/main.rs` Err 分支）：先 `AutoRealm::new_from_handle(rt.cx(), global.handle())`，
  再把 `&mut realm`（Deref 到 `&mut JSContext`）传给上报函数。
- **推广为铁律**：任何在 `evaluate_script` 返回之后调 JSAPI 的地方，
  先确认是否在 realm 内；不在就进 `AutoRealm`。

### 4.2 mozjs 153 删了旧 wrapper（2026-09-09）

- `Context::from_runtime`、`cx.await_native` 等老 API 在 Gecko153 已删
  （上游 "Drop deprecated code and old wrappers"）。
- 现用：`Runtime::new(engine.handle())` + `rt.cx() -> &mut JSContext` /
  `rt.cx_no_gc() -> &JSContext`；`rooted!(&in(rt.cx()) …)`；`CompileOptionsWrapper::new`。
- 对照以上游 `servo/mozjs` main 分支 `mozjs/examples/{minimal,eval}.rs` 为准，
  不要抄 `winterjs-old` 的 `sm_utils.rs`（那是 0.14 时代写法）。

### 4.3 shell 小坑：zsh 的 `=cmd` 展开（2026-09-09）

- `echo ===` 这类以 `=` 开头的词会被 zsh 当命令路径展开而报错，脚本里要加引号。

### 4.4 config 0.15 的 prefix 分隔符默认跟随 separator（2026-09-10）

- 症状：`WINTERJS_LOG__COLOR=always` 环境变量覆盖配置永远不生效。
- 根因：`Environment::with_prefix("WINTERJS").separator("__")` 时，prefix 分隔符
  **默认跟随 separator**，即前缀变成 `WINTERJS__`，`WINTERJS_` 开头的变量全部被跳过。
- 修法：显式 `.prefix_separator("_").separator("__")`（src/settings.rs）。

### 4.5 集成测试不继承 bin 的 `#[global_allocator]`（2026-09-10）

- 症状：超大分配探针在 `tests/` 里永远"通过"，以为 smmalloc 没问题。
- 根因：`tests/*.rs` 是独立 crate，链接的是 System 分配器，src 里的
  `#[global_allocator]` 对它不可见。
- 修法：探针测试文件里自己声明 `#[global_allocator] static ALLOC: smmalloc::Smalloc`。

### 4.6 tracing-subscriber 的 `init()` 已内建 log 桥接（2026-09-10）

- 症状：进程启动即 panic `failed to set global default subscriber: SetLoggerError(())`。
- 根因：`SubscriberInitExt::init()`（tracing-log 特性启用时）内部就会装 log 桥；
  再显式调 `tracing_log::LogTracer::init()` 抢占 `log::set_logger` 即冲突。
- 修法：二选一，用 `init()` 就不要再调 LogTracer（src/logging.rs 取前者）。

### 4.7 `UseInternalJobQueues` 在 153 下 SEGV，改 RustJobQueue glue（2026-09-10）

- 症状：最小 Runtime 下调 `js::UseInternalJobQueues` 即 SEGV，realm 内外皆崩。
- 根因：原因未深究（记坑）。
- 修法：用 `mozjs_sys` 自带 RustJobQueue glue（servo 同款）：`CreateJobQueue` +
  `SetJobQueue`，traps 的 `runJobs` 用 MicroTask 朋友 API 排空
  （`PeekNextMicroTask` / `DequeueNextRegularMicroTask` / `RunJSMicroTask`），
  首段脚本前在 realm 内 `install`；不装则 `RunJobs` 无队列可用同样 SEGV
  （`src/jobqueue.rs`，`src/runtime.rs`）。

### 4.8 引擎/运行时析构期 StoreBuffer 悬垂边 SEGV（2026-09-10）

- 症状：`Runtime` / `JSEngine` 正常 drop 时在 `JS_DestroyContext` / destroyRuntime
  的小 GC 里 SEGV（含带 timer 路径）；`RootedTraceableBox` 的 TLS 析构晚于引擎
  同样会 SEGV/abort。
- 根因：`Runtime` 的 StoreBuffer 记有指向 `RootedState` Heap 槽位的边，
  先 drop 槽位再销毁引擎即悬垂。
- 修法：结果就绪后 `process::exit` 跳过 teardown（`src/main.rs` `dispatch` 返回
  退出码）；`Runtime`/`JSEngine` 经 `forget_engine` 刻意泄漏（`src/runtime.rs`）；
  `RootedState` 经 `StateGuard` 在引擎存活期内从 TLS 摘除并 `mem::forget`
  （`src/state.rs`，进程退出由 OS 回收）。

### 4.9 native 内 `Rooted<ValueArray>` 注册会 SEGV（2026-09-10）

- 症状：native 回调内用 `Rooted<ValueArray>` 传参即 SEGV。
- 根因：其根注册路径在 native 内调用时有问题（未深究，记坑）。
- 修法：单实参用 `HandleValueArray::from(raw_handle(已 rooted 值))` 直构；
  `thisObj` 传 null 会 SEGV，必须传有效对象（用 global）
  （`src/builtins/clone.rs`）。

### 4.10 `WINTERJS_LOG` 被 config 误收导致启动失败（2026-09-10）

- 症状：`WINTERJS_LOG=winterjs=debug …` 启动即
  `failed to load settings: invalid type: string …, expected struct LogSettings`。
- 根因：`WINTERJS_LOG` 按 `WINTERJS_` 前缀规则被收进 `log` 表（string 覆盖 struct）；
  它本是日志运行时的直读变量（`logging.rs` 直接读 EnvFilter），不经 config。
- 修法：`Settings::load` 期间暂存并移出 `WINTERJS_LOG`/`WINTERJS_LOG_FILE`，
  构建完原样恢复（启动期单线程）；回归测试 `winterjs_log_filter_does_not_break_config`。

### 4.11 模块 hook 的 referrer 定位：走脚本文件名，不走私有值（2026-09-10）

- 症状：`SetModulePrivate` + `SetScriptPrivate` 写入 URL 字符串后，
  load hook 的 hostDefined 仍为 undefined，相对导入 base 丢失。
- 根因：153 下 GC 字符串私有值送不到 hook（机制只适合 `PrivateValue` + ref hooks）。
- 修法：hook 内用 `JS_GetScriptFilename(referrer)` 取文件名
  （CompileOptions 写入的即模块 URL）作 base；`SetScriptPrivate` 整段删除。
- 附带：`ModuleLink` 要求先走完加载态，直接 link 报
  `module record has unexpected status: New`——动态 import 分支内嵌
  `load_dependencies` 再 link（`src/modules.rs` `ensure_subgraph`）。

### 4.12 file URL 必须规范化，否则同一模块判重失效（2026-09-10）

- 症状：循环 a↔b 跑出 `a b a`（模块被求值两次），而非 spec 序 `b a`。
- 根因：macOS `/var` 是到 `/private/var` 的 symlink，两边拼出的 URL 字符串不同，
  注册表按 URL 去重即失效。
- 修法：resolve 返回前一律 `canonicalize`（`src/loader/resolve.rs` `canonical_file_url`）；
  复现：`phase2_circular_import_no_deadlock`。

### 4.13 `TsconfigDiscovery::Auto` 只对 `resolve_file` 生效（2026-09-10）

- 症状：tsconfig `paths` 别名（如 `@lib/*`）报 `Cannot find module`。
- 根因：上游文档注明 Auto 发现只走 `resolve_file`，`resolve` 不读 tsconfig。
- 修法：有真实发起文件走 `resolve_file`，cwd 锚点才走 `resolve`
  （`src/loader/resolve.rs` `caller_file`/`resolve_with`）；
  复现：`phase2_tsconfig_paths_alias`。

### 4.14 `with_rooted`/`with_plain` 不可嵌套（2026-09-10）

- 症状：TS 报错路径 abort（`RefCell already borrowed`，non-unwinding panic，经 microtask 回调炸）。
- 根因：在 `with_plain` 闭包内调了同样走 `with_plain` 的函数（`entry_reason_string` 查 `module_debug`）。
- 修法：先算串再进 `with_plain`；推广为铁律：TLS 访问闭包内只做纯数据操作，
  不调同样走 TLS 的函数（`src/state.rs`）。

### 4.15 入口 promise 捕获必须在事件循环前挂载（2026-09-10）

- 症状：模块顶层抛错被报成无位置的 `unhandled rejection: ...`。
- 根因：`ModuleEvaluate` 的 rejection 在事件循环收尾被通用 unhandled 路径先收走，
  事后挂专用捕获已晚。
- 修法：`ModuleEvaluate` 成功后、进 `event_loop` 前即挂 `entry_*` 捕获，
  循环后只收割（`src/runtime.rs` `run_module`）。

### 4.16 criterion bench 须 `harness = false`（2026-09-10）

- 症状：`cargo bench` 只跑出 `running 0 tests`。
- 根因：bench target 默认 libtest harness，把 criterion main 当测试跑。
- 修法：`Cargo.toml` 加 `[[bench]] harness = false`。

### 4.17 `await` 在参数位置不报 await 错，TLA 重试须放宽触发（2026-09-10）

- 症状：`console.log(await ...)` 报 `missing ) after argument list`，模块重试不触发
  （旧逻辑只认 `await is only valid` 文案）。
- 根因：`await` 在参数位置按标识符解析，语句位置才报 await 错。
- 修法：经典 `SyntaxError` 一律用 oxc 试探解析，能解则跑模块，否则保留原始经典报错；
  eval 侧触发放宽到一切 `SyntaxError`，包装解不出回落原始报错（非 `exhausted`）。
  复现：`console.log(await Promise.resolve(5))`（`src/runtime.rs`）。

### 4.18 结算后退出的循环顶排空 race：progressed 轮不退（2026-09-10）

- 症状：流式 body 第三个 `read()` 永不决议（时序一变则进程 0 退出但丢输出）。
- 根因：`settle`（循环顶 `try_recv` 非阻塞排空）同步决议 promise，只排队
  microtask；随后退出检查全零即 `break`，掉队 microtask 等不到下一轮 `RunJobs`。
  旧缓冲实现罕发（单消息多在 `select` 臂内结算，次轮 `RunJobs` 兜住），流式多消息
  必发（chunk/Done 堆在顶排空）。
- 修法：本轮结算过（`progressed`）即使全 idle 也不退，回顶再 `RunJobs`；
  `select` 的 None 臂遇全 idle 直接 `continue`（只剩 microtask，不 park，否则永睡）。
  复现：`tests/fetch.rs::phase3_fetch_body_streams_chunks`（修前必挂；2026-09-12 拆分前在 tests/cli.rs）。
  推广为铁律：任何同步决议 JS promise 的结算点之后，必须保证至少一轮 `RunJobs`。
- 追补（2026-09-10，模块顶层 `process.exit` 必发 `[object Promise]` 案）：
  抛错的 job 会截断当轮 `RunJobs` 排空，入口捕获的反应 job 留到下一轮；
  若退出旗检查放在排空**前**，下一轮直接返回，反应永不触发（收割 None → 误打印
  promise + 仅靠旗退出）。修法：旗检查一律放 `RunJobs` **之后**。
  教训：`with_plain` 写本身无辜——二分时曾误判它，实为检查点顺序问题。

### 4.19 tower-http 默认追踪打错 target，会被默认 filter 静默（2026-09-10）

- 症状：`WINTERJS_LOG=winterjs=debug` 下 serve 有起停 INFO，但无逐请求日志。
- 根因：`TraceLayer::new_for_http()` 默认回调打 `tower_http::trace::*` target；
  默认 filter `winterjs=<level>`（`src/logging.rs`，依赖库保持安静）把它过滤。
  另：`CompressionLayer` 默认 predicate 跳过小 body（16B 无 content-encoding，
  5KB 才有），属轮子正常行为非 bug。
- 修法：`on_request/on_response/on_failure` 手写回调，用
  `tracing::debug!/warn!(target: "winterjs::serve", …)` 只记 method/uri/status/
  latency（不记 body/头）；压缩黑盒用大文件测（`src/serve.rs`）。
- 推广为铁律：凡引入打日志的轮子，先确认其事件 target 是否在默认 filter 内；
  不在就用回调/适配转进 `winterjs::*`，禁为此放宽默认 filter（依赖噪音）。

### 4.20 写文件命令的手工实测必须先 `cd` 进 probe 目录（2026-09-10）

- 症状：本机验证 `init` 时在仓库根直接跑，把 `package.json/index.js/hello.test.js`
  写进了 winterjs 仓库（差点污染提交）。
- 根因：`init`/`test` 这类以 cwd 为作用域的命令，实测 shell 的 cwd 即作用域；
  肌肉记忆 `./target/debug/winterjs …` 让人忘了先 `cd`。
- 修法：删掉误建文件并 `git status` 确认干净；此后凡实测写文件命令，
  一律 `mkdir -p /tmp/wjs-*-probe && cd` 进去再跑。
- 推广为铁律：黑盒测试不受影响（assert_cmd 设了 `current_dir`），只约束手工实测。

### 4.21 同一文件的 edit 与 append 禁并行（2026-09-10）

- 症状：给 `tests/cli.rs` 同时发 edit（改 man 计数）与 bash heredoc append
  （加 4 个 init 测试），append 的内容全部丢失，测试数 127 不增。
- 根因：两工具调用并行执行，edit 基于旧内容写回，覆盖了 append 的写入。
- 修法：补回 append；此后同一文件的多次变更一律串行（不同文件可并行）。
- 推广为铁律：工具并行只用于无依赖的不同文件；同文件操作串行排队。

### 4.22 `rt`/`engine` 声明顺序即 drop 逆序，搬代码别搬反（2026-09-10）

- 症状：抽 `init_session` 后 `Promise.reject(...)` 报
  `There are outstanding JS engine handles` panic（exit=101），而非 exit=1 可读错。
- 根因：重构把 `let mut rt` 写到了 `let engine` 前面，`?` 早退（跳过
  `forget_engine`）时 engine 先 drop，rt 仍持有 handle，`JSEngine::drop`
  的 outstanding 断言必炸；成功路径因双双 forget 被掩盖，只有报错路径暴露。
- 修法：`engine` 先声明、`rt` 后声明（`run_inner` 注释已钉住顺序）。
- 推广为铁律：凡涉及 `§4.8 forget_engine` 的重构，成功/报错双路径都要跑
  （报错路径用 rejection/timer-error 用例覆盖）。

### 4.23 `Object.create(prototype)` 实例无私有方法 brand 槽（2026-09-11）

- 症状：`bun:sqlite` 的 Statement 经 `Object.create(Statement.prototype)` 造
  （构造器要抛 Illegal constructor，故不能 new），调 `get #st()` 私有访问器即报
  `can't access private field or method: object is not the right class`。
- 根因：私有方法/访问器在实例上装 brand，`Object.create` 造的对象没有构造器的
  brand 槽，brand 检查必炸（私有 `#` 字段同理；原型上挂 WeakMap 查不到这个问题）。
- 修法：prelude 内部类若实例走 `Object.create` 造，状态一律 WeakMap + 自由函数，
  禁用 `#` 私有成员（`src/builtins/bun/sqlite.rs` SOURCE）。
- 复现：`await import("bun:sqlite")` 后 `db.run("CREATE TABLE t (x)")`（修前必炸）。
- 推广为铁律：prelude 新类先定实例制造方式——能 `new` 才可用 `#` 私有成员；
  `Object.create` 造的（内部类/状态后置挂的）一律 WeakMap 自由函数
 （URL/Headers/fetch 系既有类全走此路，与此一致）。

### 4.24 同进程多次 `runtime::run`：引擎单例 + 每文件独立线程（2026-09-11）

- 症状：`winterjs test` 多文件第二个文件起全报 `failed to init JS engine`
  （e1 起就坏，黑盒只放一文件没抓到；test --watch 把它显性化）。修引擎单例后
  又暴露第二层：`js::NewContext` 里 EXC_BAD_ACCESS（同线程建第二个 Runtime）；
  再改成 run 结束正常 drop Runtime，带 timer/microtask 残留的路径 SEGV（§4.8
  所记 teardown 问题如实复现，139 例黑盒掉 30+）。
- 根因（三层）：① `JSEngine::init()` 每进程只能成功一次（二次
  `AlreadyInitialized`）；② mozjs 的 CONTEXT TLS 断言一线程一 context，
  `Runtime::create` 前就炸（SEGV 在 `js::NewContext` C++ 侧，非 rust assert）；
  ③ `Runtime::drop` 的收尾 GC 在事件循环未完全排空（timer/rejection 残留）时
  必炸——§4.8 的 process::exit 就是为躲它。
- 修法（`src/runtime.rs`）：① `engine_handle()` 进程级单例——JSEngine::init
  一次后本体 `mem::forget`（永不 shutdown），handle（Clone）给每次 run；
  ② `run_isolated(source, filename, args)`：每文件独立 OS 线程（16MB 栈——
  引擎 STACK_QUOTA 按主线程量级假设，线程默认栈不够）内起 current-thread
  tokio + LocalSet 跑 `run`，Runtime 照 §4.8 泄漏，线程退出即清 CONTEXT/state
  TLS（隔离边界 = 线程生灭）；③ `end_session` 维持 forget 泄漏语义（曾试图改
  正常 drop，实证炸 timer 路径后回退）。
- 复现：两个 `console.log` 的 `*.test.js` 跑 `winterjs test`（修前 exit=1
  第二个 `failed to init JS engine`）；修后全过 exit=0。
- 推广为铁律：凡"单 run 进程"假设的代码（runtime::run 内部多处）、要在同进程
  再跑一次 JS 的（test/watch/未来并行），一律走 `run_isolated` 新线程，
  禁在同一线程叠建 Runtime、禁改 `end_session` 为 drop。多 run 功能的黑盒
  必须含 ≥2 文件/≥2 轮的用例（e1 的教训：单文件用例漏掉整层回归）。

### 4.25 clap builder 的 by-value 改造 + `value_name` 只要 `&'static str`（2026-09-11）

- 症状：`cmd.about(x)` 报 `cannot move out of *cmd`（E0507）；`a.value_name(String)`
  报 `Str: From<String> 未实现`（E0277）；`get_value_names()` 回的是 `Option` 不是切片。
- 根因：`Command::about/mut_arg` 是 `mut self -> Self`（by-value，非 `&mut`），
  `&mut` 引用上调即 E0507；`value_name` 只收 `&'static str`。
- 修法：顶层用 `cmd = cmd.about(..)` 串联；子命令经 `get_subcommands_mut` +
  `mem::replace` 占位换回；译文 `value_name` 经 `Box::leak` 给 `'static`
  （中文 locale 下约 60 短串，进程生命期，注释写明；英文 locale 译文==原文直接跳过，
  零泄漏且英文输出逐字节不变）（`src/cli.rs` `localized_command`）。
- 附带：`rust-i18n` 的 `t!` 接受变量 key（运行时查表正常；扫不到的只是
  `cargo i18n` 提取工具——key 全在 yml 里，无需提取）。

### 4.26 全 flag CLI 的两条铁律（2026-09-11）

- 动作的必需值必须紧贴其 flag：`--run`（`num_args(1)`）后直接跟别的 flag
  即判缺值（`--run -l zh --help` 炸）。写法是值前置（`-l zh --run f.js --help`），
  测试里 9 处 `wjs(&["--run", <flags>, file])` 全因此调序。
- 机械改名脚本会误伤非 winterjs 调用：`&["init", "-q", ...]`（git helper）
  撞上 `["init", ` 模式。修法：脚本断言计数 + 事后按命令名复核 bare 残留；
  跨工具同名（git init）逐个手改（`src/cli.rs` 全 flag 重写，`tests/cli.rs`）。

### 4.28 空工具调用可能回滚工作区（2026-09-11）

- 症状：一次无参数的 edit 调用被 abort 后，工作区 4 个文件被回滚到旧快照
  （`mod.rs -190`/`fs.rs -47`/`tests -89`/`child.rs` 重现已删 PIPEDBG；
  未提交的 `publish.rs` 改动全丢；已提交的批2内容 HEAD 完好）。
- 根因：未深究（记坑；疑似 harness 对空调用/中断恢复了 stale 快照）。
- 修法：`git checkout HEAD -- <files>` 恢复已提交部分，未提交部分按历史重做；
  大 edit 后立即 `grep`/`tail` 确认落盘再跑测试。
- 推广为铁律：绝不发送无参数/空参数的工具调用；同文件 edit 串行且每次验落盘；
  大改动每完成一文件即 `git add`（不 commit 也先进 index，丢了能从 index 找回）。

### 4.29 serde 缺 rename 即静默丢字段（2026-09-11）

- 症状：`optionalDependencies` 装不上（求解树里根本没出现），且 `pkg@latest`
  之类 tag 安装也一直是坏的——两者都静默通过，无任何报错。
- 根因：serde 缺省按 Rust 字段名精确匹配；npm 线名是 kebab/驼峰
  （`dist-tags`/`optionalDependencies`），对不上即当未知字段忽略 +
  `#[serde(default)]` 补空，全程无声。
- 修法：`#[serde(rename = "...")]` 逐个显式改名（`registry.rs` Packument/
  VersionMeta）；回归测试反序列化真实线名（`npm_field_names_deserialize`）。
- 推广为铁律：凡对接外部 JSON（registry/npmrc/各类 API），单测必须用真实线名
  断言 roundtrip；`#[serde(default)]` + 外部源组合出现时先查 rename。

### 4.27 增量解码两阶段 + BYOB 三坑（2026-09-11）

- `decode_to_string` 的 `InputEmpty + read=0` 是"截断已缓存、等下次 feed"，
  不是"继续"——当继续写 loop 即 busy-loop（现象是超时无输出，`cargo test`
  也会被卡死；实测 `[E2]` 回 `(InputEmpty, read=1)`，`[] last=true` 才吐 `�`）。
  修法：排空（`last=false`）+ 收尾空调（`last=true`）两阶段；收尾零推进则置空
  输入再调一次落定（`encoding.rs stream_decode_chunk`，模块单测秒级验终止）。
- pump 里无条件 ByteToQueue 会提前搬空 byteQ，BYOB 永见 `byteLen 0`。
  修法：只在 default 读等待（`wantValue` 标记，closed 等待不算）时搬。
- 无关闭、无释放的 reader + 无条件 pull = prefetch 空转，进程退不出。
  修法：字节流纯按需 pull（BYOB 排队或 default 读等待才拉）；
  default 老路径不动（§4.18 时序敏感）。
- 追补（防抖 key 刷出顺序）：防抖表用 HashMap 即非确定序——新文件
  Create+Modify 双事件谁先刷不一定（`ev: change` vs `ev: rename` flaky）。
  修法：插入序 Vec，同键只留首事件并刷新 deadline，刷出按到达序
  （首事件赢，与无防抖时的先到先得一致）（`fs.rs debounce_loop`）。

### 4.30 `mime_guess` 把 `.ts` 当 MPEG 视频流，`serve` 须自改 MIME（2026-09-12）

- 症状：Vue 工程经 `winterjs serve` 起静态，浏览器拒载 `/src/main.ts`：
  `使用了不允许的 MIME 类型（"video/vnd.dlna.mpeg-tts"）`。
- 根因：`.ts` 与 MPEG-TS 同扩展名，`mime_guess`（冻结表，`ts/mts` 双中招；
  `jsx` 在 2.0.4 表里还是非法的 `text/jscript`）判错；`tower-http 0.7` 的
  `ServeDir` 写死 `mime_guess::from_path`，无覆盖接口（`append_mime_override`
  不存在，翻轮子源码确认）。
- 修法：`ServeDir` 外包最内层 `from_fn` 中间件，`ts/mts/cts/tsx/jsx`
  （大小写不敏感）成功响应改 `text/javascript`（Vite 对等），404 不动
  （`src/serve.rs` `rewrite_ts_mime` + `ts_family_js_mime`）。
- 复现：`tests/serve.rs::phase6_serve_ts_mime_as_javascript`（修前 content-type 含 video；2026-09-12 拆分前在 tests/cli.rs）。
- 取舍：扩展名本身有歧义（TypeScript 源码 vs MPEG-TS 视频，共用 `.ts`），静态服务器
  从后缀无法知道作者意图——与 Vite 一样按 Web 开发上下文判 JS 源码。
  真要服 MPEG-TS 视频请用 `.m2ts`/`.m2t` 后缀（同表，无歧义），不要用 `.ts`。

### 4.31 Node CJS → ESM 移植三坑（2026-09-12，Phase 9a）

- 症状一：`import { codes: { X } } from 'm'` 报 `Expected ',' or '}'`——import 绑定
  不支持嵌套解构（CJS `const { codes: { X } } = require(...)` 的习惯带进 ESM）。
  修法：`import errors from 'm'` 后再解构（node/internal 四处）。
- 症状二：`return { [Symbol.dispose]() { ...this.end / currentContext.set(this, ...) } }`
  里的 `this` 指向**返回的对象字面量本身**，不是外层通道/ALS——静默错绑不报错
  （BoundedChannel.withScope 报 undefined、ALS.__wjsWithScope 静默污染，两次踩中）。
  修法：`const self = this` 闭包捕获。
- 症状三：`dc.subscribe` 不返回退订函数（Node 同款返回 undefined），黑盒想当然存
  返回值致退订恒 false。教训：黑盒设计前先核对 Node 套件原文断言，勿凭记忆。
- 复现：`tests/node.rs::phase9a_diagnostics_channel_surface`（修前 `outside [object Object]`；拆分前在 tests/cli.rs）。

### 4.32 逐字移植的解环/形态坑（2026-09-12，Phase 9b）

- 症状一：`compose` 中路报 `Duplex is not a constructor`（p2 探针只测了 async
  generator 分支，Duplex 构造分支未覆盖）。根因：CJS→ESM 懒解环包装
  `__ensureDuplex()` 只包了 `.from()` 路径，`new Duplex({...})` 用了裸 `let`
  绑定，静默 undefined 运行时才炸。修法：解环包装完成后 grep 该符号在本模块
  全部裸引用逐处收口（本次 5 处漏 1）（`internal/streams/compose.rs:147`）。
- 症状二：`node:buffer` SlowBuffer 垫片写了 `new Buffer.alloc ? ...`——class
  静态方法无 `[[Construct]]`，`new Buffer.alloc` 直接 TypeError（想当然造的
  三元，非 Node 原文）。修法：逐字移植禁"顺手改写"，垫片按 Node 原文
  `return new Buffer(size)`（`node/buffer.rs`）。
- 症状三：unhandled rejection `aggregateTwoErrors is not a function`——4 个
  内部模块解构 errors 的符号而 errors 模块没导出，解构得 undefined 无声，
  运行时才炸。修法：内部模块新增解构 errors 符号前先 grep 导出面是否齐
  （`internal/errors.rs` 补 aggregateTwoErrors，errors.js:172 同款）。
- 教训延续（§4.31 症状三同源，9b 黑盒 4 处断言错全在"凭记忆"）：
  ① `pipeline`/`finished` 的 node:stream 命名导出是 callback 形态（末参必须
  函数，popCallback validateFunction），promise 形态只在 `node:stream/promises`；
  ② `Readable.from("ab")` 吐单块不逐码元（真 Node 实测同款）；
  ③ string chunk 经 push/write 转 Buffer（writable.js:475/readable.js:488），
  `chunk.constructor.name` 是 "Buffer" 非 "Uint8Array"；
  ④ close 事件时点 `isDestroyed` 已为 true。
  实现侧零 bug——全部先实测真 Node 再改断言，勿在黑盒里编码记忆里的语义。
- 复现：`tests/node.rs::phase9b_stream_duplex_transform_pipeline`
  （compose Duplex 分支修前报 `Duplex is not a constructor`）。

### 4.33 fs 补齐三坑（2026-09-12，Phase 9c）

- 症状一：`openSync(p, "wx")` 对不存在文件报 ENOENT（应创建）。根因：JS 侧
  flags JSON 发 `createNew`，Rust `OpenFlags` 结构体字段 `create_new` 按
  serde 默认名匹配不上 → 静默 false → 无 O_CREAT（§4.29 二进宫：跨 JS/Rust
  JSON 边界一律 camelCase 线名 + `#[serde(rename)]`，新结构先写 roundtrip 测）。
- 症状二：`openSync` 返回值传 `readSync` 报 `fd must be a number`。根因：native
  经 `set_rval_str` 返回的 fd 是**字符串**，JS 侧忘了 `Number()` 包装。教训：
  本仓 native 数值返回统一走字符串，JS 层负责转数。
- 症状三：fd 写入报 `UNKNOWN`。根因：`io_code` errno 表缺 9 → EBADF。教训：
  新增 syscall 面（fd 系）前先对照 `io_code` 码表补 errno 映射。
- 探针侧：回调 API 黑盒里相互独立的异步链会交错执行，内容断言全脆——
  修法：严格嵌套链 + 内容断言只放链内确定点；另核实三个"断言错、实现对"：
  `"hello!"` 是 6 字节、`statSync` 对 symlink `isSymbolicLink()` 为 false（跟随
  语义，Node 同款）、readFile ENOENT 的 `err.syscall` 是 `"open"`。
- 复现：`tests/node.rs::phase9c_fs_sync_extras`（wx 修前 ENOENT）。

### 4.34 net 事件循环收尾四坑（2026-09-12，Phase 9d-1）

- 症状一：Server `__ev` 里 `this.emit is not a function`。根因：`call_two`
  派发以 **global 为 this** 调 target 方法（jsapi_glue 调用约定）——类方法做
  事件钩子必须 `this.__ev = this.__ev.bind(this)` 预绑定成自有属性（§4.31
  症状二的引擎版：非对象字面量，而是 native 调用约定）。
- 症状二：回环 echo 后进程 hang 不退。根因：**Node 默认 `allowHalfOpen=false`
  ——socket 收到远端 FIN（'end'）后自动 end 本端**；漏掉该语义则连接半开，
  `net_open` 永不归零，事件循环 idle 判定失败。教训：IO 面的生命周期必须逐条
  对齐 Node 默认关闭语义，`*_open()` 计数 + idle 检查会把缺口暴露成 hang。
- 症状三：`destroy()` 后对端 'close' 不发/双发。根因二连：① Close 事件在
  task 内 **先 purge 后派发**，dispatch 读不到 target（顺序坑：清态必须在
  派发之后）——改为 task 只置 `close_sent` 单发旗，purge 统一在 dispatch 后；
  ② writer task 死后（destroy 断写端），reader EOF 时 JS auto-end 的 End
  命令无人消费——entry 加 `writer_alive` 旗，reader EOF 见 writer 已死则
  代行 `close_once + Close`。
- 症状四：hermetic 陷阱——bogus 域名（`nope.invalid`）在 macOS 会被系统
  解析器经 search domain 意外"解析成功"。教训：DNS/网络黑盒**只依赖
  localhost + 空主机名**，失败路径断言 Error 形状（code 为 string）不断
  具体码；server 端口一律 `port 0`（并行测试安全）。
- 附：serde `SocketAddr` 序列化为 `"ip:port"` 串（IPv6 `[ip]:port`）；
  io_code 的 EADDRINUSE 是双 errno（macOS 48 / Linux 98）——平台差异 errno
  映射一律双码同列 + 单测双断言。
- 复现：`tests/node.rs::phase9d_net_echo_loopback`（allowHalfOpen 修前 hang）。

### 4.35 node:http 回环两坑（2026-09-12，Phase 9d-3）

- 症状一：POST 体丢失、res 永不结束、请求看似收到两次。根因：解析器在
  `emit("request", req, res)` **之前**就把体喂完（data/end 先发）——用户
  request 监听器里再挂 `req.on("data")` 永远收不到，res.end 不执行。修法：
  体先备齐缓存，**先 emit("request") 再喂体**（Node parser 同口径：request
  事件先于 body）。推广：凡"事件 + 数据流"API，数据派发必须在用户监听器
  可注册之后。
- 症状二：客户端 'end' 双发。根因：`res.__feed`（整收口径发 data+end）与
  `__finish`（又补 end）重复——整收口径下 end 只能由一处派发，其余路径只
  收尾连接（sock.end + req 'close'）。
- 黑盒教训：请求**无监听 error 事件即抛错**是 Node 正确行为——错误路径黑盒
  要补 `req.on("error", () => {})` 空监听，而不是改实现吞错。
- 复现：`tests/node.rs::phase9d_http_loopback`（喂体时序修前 POST 挂死）。

### 4.36 新事件域 checklist（2026-09-12，Phase 9d-4 dgram 二进宫沉淀）

- dgram 落地时把 §4.34 的坑 1（`__ev` 忘预绑定 → `this.emit is not a function`）
  和坑 3（task 侧 `net_purge` 抢在 Close 派发前 → 'close' 事件丢失）**各重踩一遍**。
- 沉淀为 checklist——今后凡基于 `dispatch → target.__ev` 模式新增事件域（tls/
  worker 等），三查：
  ① JS 构造器内 `this.__ev = this.__ev.bind(this)`（dispatch 以 global 为 this）；
  ② task 侧只置 `close_once` 旗发事件，**purge 一律放 dispatch 派发 Close 之后**；
  ③ native 数值返回（id/fd）JS 侧记得 `Number()` 包装。
- 另：构造器参数校验的 TypeError 要带 `e.code`（Node 口径），黑盒断言 code 而非
  message 前缀。
- 复现：`tests/node.rs::phase9d_dgram_loopback`（修前 'close' 丢/`bad-type undefined`）。

### 4.37 zlib 两坑（2026-09-12，Phase 9d-5）

- 症状一：`gzipSync(s, { level: 99 })` 报 `Z_DATA_ERROR` 而非 `ERR_OUT_OF_RANGE`。
  根因：Sync 把 `__zLevel` 校验写进了 `__zCall(fn)` 的 try 内——校验抛的
  RangeError 被错误包装函数按无前缀默认成 `Z_DATA_ERROR` 重包。
  修法：参数校验一律提到包装调用之外先执行；包装函数再加保险——已有
  `ERR_*` 码的错误直通不重包（`src/builtins/node/zlib.rs` `__zErr`）。
- 症状二（轮子文档与源码不符）：`dependencies.md` §6 记 ruzstd"编码五档"，
  实测 `encoding/mod.rs` 只有 `Fastest` 可用（`Default`/`Better`/`Best` 标
  `UNIMPLEMENTED`）。修法：zstd 编码恒 `Fastest`，`level` 接受忽略并记档；
  教训：轮子能力断言以源码/实测为准，不抄 README 一句话（§4.32 教训延续）。
- 复现：`tests/node.rs::phase9d_zlib_errors_boundary`（`lv-hi` 行修前为
  `Z_DATA_ERROR`）。

### 4.38 https/tls 三坑（2026-09-12，Phase 9d-6）

- 症状一：`https.get(url, opts, cb)` 三参回环 hang（测试 300s 超时）。
  根因：实现只收两参，`cb` 位置拿到 options 对象 → response 监听器未注册 →
  `server.close()` 永不执行 → 事件循环不退。修法：帧层加
  `normalizeRequestArgs`（url/options 合并，options 优先，跨协议即
  `ERR_INVALID_PROTOCOL`），http/https 双包层同走。
  教训：请求侧"无监听即挂起"是正常语义（§4.35 同源）——先查形态覆盖，再查实现。
- 症状二：`ca: "garbage"` 静默通过（空 roots，握手必挂）。
  根因：`rustls_pemfile::certs` 对无 armor 文本产空迭代无错。
  修法：`ca` 给出但零证书即同步 TypeError（fail fast；服务端 PEM 同口径）。
- 症状三（测试件）：openssl 默认自签带 `CA:TRUE`，rustls 报
  `CaUsedAsEndEntity`。教训：hermetic TLS 测试证书须 end-entity
  （`basicConstraints=CA:FALSE` + serverAuth EKU），或直接用 rcgen
 （`generate_simple_self_signed`，自带正确扩展；serve 黑盒同款）。
- 复现：`tests/node.rs::phase9d_https_loopback`（三参修前 hang）。

### 4.39 http2 请求事件双发：构造器与包层别双注册（2026-09-12，Phase 9d-7）

- 症状：3 路 h2 请求，服务端 `srv-req` 打印 6 次；多路复用黑盒丢一整流。
- 根因：`Http2Server` 构造器内 `if (typeof options === "function") this.on(...)`
  与 `createServer/createSecureServer` 包层接线重复——函数首参同时命中两处。
- 修法：构造器不再碰 request 监听器，只由包层接线（`src/builtins/node/http2.rs`）。
- 复现：3 流探针（修前每流双 `srv-req`）；`tests/node.rs::phase9d_http2_cleartext`。

### 4.40 `Heap::set` 后禁移动，违者 nursery GC 必崩（2026-09-12，Phase 9d-7 总根因）

- 症状：回调内制造 GC 压力（2 万小对象 / btoa 大串 / 100KB+ http2 体）后进程
  SIGSEGV/SIGBUS（exit=138/139），崩点不定（dispatch 内外皆可）；timer-only
  同样崩，与 net/http2/zlib 无关——此前"大字符串崩""http2 大包崩"全是该根因的表象。
- 根因：mozjs `Heap::set` 的 post-barrier 记录的是槽地址，set 后移动即悬垂
  （上游 `jsgc.rs` 明写 + 专设 `Heap::boxed` 防此）；本仓 `Vec<NetTarget/
  TimerEntry/ModuleEntry/CjsEntry/WatchCallback/ChildTarget/FetchCallback/
  StreamWaiter>` + `unhandled` 的 `push/realloc/retain/remove` 件件在搬运已 set
  的 Heap，下次 minor GC 读悬垂 store-buffer 边即炸。旧测试全绿只因从未在回调内
  制造 nursery 压力（btoa 大串走大对象空间，未必触发 minor GC，故时崩时不崩）。
- 修法：全部持 JS 值的 Vec 元素字段改 `Box<Heap<T>>`（Box 移动只搬指针，
  槽地址恒稳；`drop` 自带 clearing barrier，摘除安全）；构造点一律
  `Heap::boxed(v)`；读侧 `.get()` 经 Deref 零改；trace impl 零改（`Box` blanket）；
  `RootedState` 直属单值字段不动（已在 `RootedTraceableBox` 内稳定）。
  （`src/state.rs` + `timers.rs`/`modules.rs`/`runtime.rs` 共 11 构造点。）
- 复现：`tests/builtins.rs::phase1_gc_pressure_keeps_rooted_targets`
  （修前 exit=139；探针 `setTimeout` 内 2 万对象分配）。
- 推广为铁律：新增跨 GC 存活的 JS 值存储，一律 `Box<Heap>` 定址；
  禁裸 `Heap` 进一切可搬运容器（`Vec` 元素/`retain`/`remove`/结构体按值移动，
  含 interval 重排这类"自家搬自家"）。

### 4.41 `#[serial]` 只保互斥不保顺序，读全局态的用例须先复位（2026-09-12）

- 症状：全量 `cargo test` 里 `permissions::tests::grant_semantics` 挂
  （`check_read("/etc/passwd")` 期望未安装默认全开放），单跑、单线程全过。
- 根因：该用例首断言依赖"全局槽从未被安装过"；`#[serial]` 只保证串行，
  不保证顺序——9e 新增十余个单测改变线程调度后，
  `sandbox_denies_undropped_classes` 先抢锁装了沙箱，`grant_semantics` 后跑即挂。
  属旧测试的时序假设 bug，非功能回归（9e 未碰 permissions）。
- 修法：tests 模内加 `reset()`（槽写回 `None`），`grant_semantics` 首行调用；
  其余用例先 `install` 再断言，本就顺序无关不动（`src/permissions.rs`）。
- 推广为铁律：凡读进程级全局（权限槽/env/分配器开关）的单测，
  先复位再断言；`#[serial]` 只防并发不防跑序。

### 4.42 黑盒标签断言禁子串，`&&` 合并打印禁弱断言（2026-09-12，Phase 9e）

- 症状一（空转）：`assert!(out.contains("md5 true"))` 恒过——输出里另有一行
  `hmac-md5 true`，子串命中，md5 哈希路径坏了也测不出。
  修法：纯哈希标签改名 `md5vec`，使任一标签都不构成另一标签的子串
  （`tests/node.rs::phase9e_crypto_hash_hmac`，`hmac`/`hmac-md5`/`hmac-s3`/
  `md5vec` 四标签互不包含）。
- 症状二（弱断言）：`console.log("empty", a && b && c)` 只打一个布尔——
  挂了不知挂在哪项，且复制粘贴时易漏项。
  修法：多布尔分参打印 `console.log("empty", a, b, c)`，断言逐项精确匹配
  （`5fb5692`，perf 黑盒 empty/timerify/mel 三处）。
- 推广为铁律：黑盒输出标签设计时即做子串检查；一行多断言一律分参，
  禁 `&&` 打包成单个布尔。

### 4.43 手写密码学面的版本墙：digest 0.10 双轨 + HMAC 自架（2026-09-12，Phase 9e）

- 背景：`Cargo.toml` 实测——`digest` 双版共存（0.10.7 供 `rsa 0.9`，0.11.3 供
  `sha1/sha2 0.11`）、`hmac 0.13`（绑 digest 0.10 系 traits）、`sha3 0.12`
  （digest 0.11 系），三者 traits 互不兼容。
- 症状：`rsa 0.9` 的 OAEP/v1.5 接口要 `digest 0.10` 的哈希类型，
  手头 `sha2 0.11` 传不进去；`hmac 0.13` 与 `sha3` 组合不出 HMAC-SHA3。
- 修法（零新依赖）：`sha2_010` 改名直引（c-4 旧例）+ OAEP-SHA1/v1.5-SHA1-MD5
  手写（MGF1 + `rsa::BigUint` 模幂，`src/builtins/node/crypto.rs`
  `rsa_encrypt_v15/rsa_decrypt_v15/crypto_mgf`）+ HMAC 通用构造自架；
  双向真机交叉验证钉住（本仓⇄真 Node 互解）。
  `sha1_010` 这类新 crate 按 §0.5 须先问用户——本次没问，直接手写。
- 推广为铁律：RustCrypto 系先查 `Cargo.lock` 里谁绑谁（digest 大版本），
  不兼容先走重导出/改名直引/手写三档，最后一档才 §0.5 问用户加依赖。

### 4.44 `format!` 拼 JS 一律绕行：花括号冲突改文件落盘（2026-09-12，Phase 9e）

- 症状：测试想把大段 PEM/JS 经 `format!` 拼进探针脚本，JS 的 `{`/`}` 全被当
  占位符——转义 `{{}}` 满屏且一漏就编译错/运行时串错。
- 修法：大块载荷（X509 内嵌证书）改文件落盘——`dir.child("c.pem").write_str(pem)`，
  JS 侧 `fs.readFileSync("c.pem")` 读回（`tests/node.rs::phase9e_crypto_x509`）；
  `format!` 只拼小标量（路径/数字）。
- 推广为铁律：`format!` 与 JS 模板字符串/对象字面量同现时，默认选文件落盘，
  不选 `{{}}` 转义。

### 4.45 9e 杂项小坑三则（2026-09-12）

- `cmd | tail` 掩盖退出码：管道退出码是 `tail` 的，前面的测试/构建挂了也看不见。
  修法：断言前先取 `${PIPESTATUS[0]}` 或改 `cmd >file 2>&1; rc=$?; tail file`。
- `gen` 是 edition 2024 保留字（生成器）：Rust 变量/字段/函数名避开 `gen`
  （如 DH/素数生成相关命名用 `genkey`/`generate` 全称），编译期即报错，改名即好。
- 真机口径先行：`createHash('nope')` 无码原文错（不自编 `code`）、`getMacs`
  真机不存在（不做）、Hmac 二次 digest 回空、`digest(badEnc)` 回 Buffer、
  `checkHost` 返匹配串、`hkdfSync` 回 ArrayBuffer、空 histogram 哨兵
  （`min=INT64_MAX`）——黑盒先对真机实测再写断言（§4.31 症状三/§4.32 教训延续）。

### 4.46 `progressed` 后直接 park 会饿死 microtask-only 结算（2026-09-12，§4.18 推广）

- 症状：worker 端口 `postMessage` 后进程 hang（无 timer 时必发；有 timer 时
  到 sleep 醒才送达——延迟送达是同一根因的轻症）。
- 根因：端口 `__ev` 只排 `queueMicrotask`（flush 在下轮 `RunJobs`），而循环在
  `progressed` 后直接进 `select!` park——再无 `RunJobs` 机会。旧代码只对
  "结算致 idle"（fetch 模型）有效：`idle && progressed` 经 None 分支 `continue`
  回顶；而端口/net 类结算不改变 open 计数，`idle=false` 即 park。
  同理潜伏：timer 回调内决议 promise + 他域 open 计数未清，同样 hang。
- 修法（`src/runtime.rs`）：`idle && !progressed` 照旧 break，其后
  `if progressed { continue; }`——回顶下一轮 pump 先 `RunJobs`，无新进展才 park，
  不忙转（progressed 源皆有限：通道缓冲/timer 触发）；None 分支内
  `if idle { continue; }` 已不可达，删除。
- 复现：`w11.mjs`（监听 + 投递，无 timer，修前 hang；`tests/node.rs` 落盒时已修）。
- 推广为铁律：§4.18 铁律的完整形态——结算点之后**到 park 之前**必须保证至少一轮
  `RunJobs`；新增事件域若结算只排 microtask，必走此路径验证（无 timer 用例）。

### 4.47 `newListener` 在监听入表*之前*触发（2026-09-12，Phase 9f-2）

- 症状：迟挂监听（先 post、后 `on('message')`）永远收不到，端口关不掉 hang。
- 根因：`newListener` 事件在监听**入表前**触发（events.js:371 同款语义）——
  处理器里查 `listenerCount('message')` 仍为 0，flush 门控永不开。
- 修法：newListener 处理器内 `queueMicrotask(() => maybeFlush())`，延迟到入表后
  再判（`src/builtins/node/worker.rs` MessagePort 构造器）。
- 推广为铁律：凡用 `newListener` 做"监听到达即…"门控，一律延迟一轮再读表；
  同步读表恒为旧值。

### 4.48 native 重名静默覆盖 + `static` 判重跨会话误报（2026-09-12，Phase 9f-2）

- 症状：`phase4_node_process_argv_env` 挂——`process.env` 的 `Object.keys` 看不见
  新变量、`delete` 删不掉；与 worker 八竿子打不着。
- 根因：worker 环境数据 native 取名 `__wjs_env_get/set`，撞上 `process.env`
  同名 native——`define_all` 同表后注册静默覆盖，get/set 走新表、keys/del 走旧表，
  两张皮。教训：`__wjs_*` 无命名空间校验，全靠自觉。
- 修法：改名 `__wjs_worker_env_*` + `define_all` 的 `web` 表循环加
  `debug_assert` 判重（`src/builtins/mod.rs`，负验证：临时插重复名即 panic）。
- 连环坑：判重集初版用 `static`——`define_all` 每会话跑一次（test 多文件/
  worker 线程），第二会话起全误报。改函数局部 `HashSet`（`#[cfg(debug_assertions)]`）。
- 推广为铁律：新增 `__wjs_*` 前先 grep 有无重名；判重状态一律函数局部，
  禁 `static` 累积（多会话进程必误报）。

### 4.49 worker 早失败必须也发 rendezvous，否则主侧超时 + 事件丢失（2026-09-12，Phase 9f-3）

- 症状：`new Worker("./nope-missing.js")` 卡 30 秒报 `failed to start`；
  若修超时，`WError`/`WExit` 又因先于 JS `attach` 到达被丢弃（exit 事件永不到）。
- 根因：文件读错发生在会话起之前，rendezvous 从未发出（主侧 `recv_timeout`
  干等）；早发的错误事件在目标登记前到达即丢（dispatch 查不到 target）。
- 修法（`src/runtime.rs::run_worker_thread`）：早失败路径先排 `WError`/`WExit`，
  再发 rendezvous（parked 收件箱——接收端已 drop，后续投递即失败不堆积）；
  主侧 spawn 返回后 JS 同步 attach，早于任何分发，事件不丢。
- 推广为铁律：跨线程 rendezvous 的**所有**出口（含失败出口）都必须发一次；
  目标登记晚于事件到达是常态，设计时即保证"先排队、后登记、再分发"时序。

### 4.50 任务尾的资源释放：重构拿掉等待点后即变杀手（2026-09-13，Phase 9g-2）

- 症状：`createBidirectionalStream` 永不 resolve，随后会话无故 `close`。
- 根因：connect 任务尾有 `endpoint.close()`——9g-1 时任务卡在 `closed().await`，
  该行只在会话自然结束后执行，无害；9g-2 改驱动接管守望后任务直达尾部，
  上线即关（实证：quinn `Endpoint::close` 杀其名下活会话）。
- 修法：发起侧 endpoint 移交会话表项保活（`client_ep`），收尾时才关
  （`quic_sess_remove`）；任务尾只 detach（`src/builtins/node/quic.rs`）。
- 推广为铁律：凡"等待点之后"的清理代码，拿掉等待点时必须重审其前提；
  跨任务共享的资源（socket/句柄）归属写进表项，不靠任务尾 drop 顺带。

### 4.51 校验必须待在 `__callNative` 闭包之外（2026-09-13，Phase 9g-2）

- 症状：`cc: "nope"`/`idleTimeout: -1`/`resetStream(-1)` 全报 `ERR_QUIC_ERROR`，
  与黑盒断言的 `ERR_INVALID_ARG_VALUE`/`ERR_OUT_OF_RANGE` 对不上。
- 根因：校验调用写成了 `__callNative(() => native(..., __normCc(v)))` 的实参——
  抛错发生在闭包内，被包装函数一律重包成 `ERR_QUIC_ERROR`。
  同源：`resetStream/stopStream` 传 `String(code)` 给只收 number/bigint 的
  native，直接 `ERR_OUT_OF_RANGE`（`quic_sess_close` 传串是对的——它家 native
  收串，各家约定不一致，调用前先看 native 侧类型）。
- 修法：校验提到 `__callNative` 之外先执行，结果变量再传入
  （`src/builtins/node/quic.rs` `connect`/`listen`/`stopSending`/`resetStream`）。
- 推广为铁律：`__callNative` 闭包内只放纯 native 调用 + 已校验的值；
  踩过一次的"码被重包"（§4.37 症状一同源），新增包装函数时先查。

### 4.52 会话收尾先收半端任务，否则 teardown 噪声变 fatal（2026-09-13，Phase 9g-2）

- 症状：`c.close()` 后进程报 `read failed (connection lost)` exit=1（流无 error
  监听时）；有监听则多一行本不该有的 error。
- 根因：会话关 → 读写任务的 pending 读/写立刻失败 → `StreamError` 先于
  `SessionClose` 派发——用户只关了会话，流 error 属 teardown 噪声。
- 修法：驱动发 `SessionClose` 前先对名下全流 `quic_stream_finish`
  （abort 半端任务，无声），再发事件；分发侧收尾照旧
  （`src/builtins/node/quic.rs` 驱动 `finish` 闭包）。
- 推广为铁律：父域收尾（会话/进程）必须先静默摘除子域任务，再发自己的
  终结事件；顺序反了，子域的临终报错必先到（§4.36 purge 顺序的跨任务版）。

### 4.53 quinn 默认 idle 30s：悬空会话 hang 测试，收尾必须显式关（2026-09-13，Phase 9g-2）

- 症状：黑盒跑 30.9s 才退（平时 4s）；connect 到已关 endpoint 同样 30s 才报。
- 根因：quinn 默认 `max_idle_timeout` 30s——服务端接受了但从不关的会话、
  发往黑洞的握手，全按 30s 结算。`quic_open` 计数如实续命，属正确语义，
  非 bug。
- 修法：测试里服务端 `secure` 即关（用完即走）；需要等失败的用例给短
  `idleTimeout`（如 1500ms）或错配 CA（TLS alert 即时失败）。
  黑盒禁依赖"对端会关"的用例形状。
- 推广为铁律：凡引入带默认超时的轮子（idle/握手），先查清默认值并写入
  测试纪律；hang 30s 整首先怀疑默认超时，其次才是死锁。

### 4.54 试解循环按长度分发，撞长即误判：看 OID 不看坐标（2026-09-13，Phase 9h-1）

- 症状：本仓签的 secp256k1 自验全过，真 Node 的签名恒验不过（`false` 不抛错）。
- 根因：SPKI/PKCS#8 导入按"逐曲线试解"定 curve——P-256 与 secp256k1 坐标同为
  32 字节，P-256 先试先"成功"（SPKI 解析根本不看曲线 OID），验签用了错曲线。
  自签自验全绿是因为加解密同错，属对称性盲区。
- 修法：`__wjs_ec_guess_curve` 读算法参数 OID 直判（SPKI/PKCS#8 通吃；
  P-256=`1.2.840.10045.3.1.7`…k256=`1.3.132.0.10`）+ SEC1 `[0]` 显式 OID；
  openssl 实测向量单测钉住（`ec_curve_name_reads_oid_not_coords`）。
  教训：跨实现交叉验证必须双向（9e-1c 只做了"真机验我"，漏了"我验真机"）。
- 推广为铁律：凡"试解定类型"的导入面，键长/形状重合即视为已撞——必须找
  自描述字段（OID/魔数/版本号）直判；交叉验证永远双向。

### 4.55 k256 验签拒 high-S：验前 normalize_s（2026-09-13，Phase 9h-1）

- 症状：上条修完后，Node 的签名仍验不过——仅当 `s > n/2` 时（对比两边 DER：
  过的是 70B（s 无填充），挂的是 71B（s 首字节 `00` 填充）。
- 根因：OpenSSL 接受可锻造签名（high-S 照验），k256 的验签拒绝 high-S。
- 修法：验签前 `sig.normalize_s()`（inherent 方法，无需 trait；P-* 系无影响，
  低 S 是恒等变换）（`src/builtins/crypto.rs` `ecdsa_verify`）。
- 推广为铁律：密码学"验不过"先按字节比对两边产物的形态差异（填充/长短/
  大小端），再怀疑算法；可锻造性是验签侧必须兼容的语义，不是 bug。

### 4.56 Node PKCS#8 省公钥 y：`y=g^x mod p` 补算（2026-09-13，Phase 9h-1）
- 症状：真 Node 的 DSA 私钥导不进（`Invalid PKCS#8 key`），自家往返全过。
- 根因：Node 的 DSA PKCS#8 只含 `(p,q,g,x)`（公钥 y 可选省略），信封解码硬要 y。
- 修法：`dsa_envelope` 内 y 缺失且 x 在，即补算 `y=g^x mod p`
  （`rsa::BigUint::modpow` 重导出直用，零新依赖）。
- 推广为铁律：外部输入的"可选字段省略"是常态（尤其 OpenSSL 系编码），
  解码器必须按"缺啥补啥"写，而不是按"我家导出形状"收；双向交叉时，
  导入真机产物的用例与导出给真机的用例缺一不可。

### 4.57 跨域 `instanceof Promise` 恒 false + 跨域求值恒异步（2026-09-13，Phase 9i-1）

- 症状：vm 模块顶层 `throw` 后 `evaluate()` 反而 resolve，随后进程报
  `unhandled rejection: Error: boom` exit=1；成功模块的完成值也全是 promise。
- 根因（二连）：① JS 壳用 `r instanceof Promise` 判完成值形态——r 来自 vm
  compartment（CCW），其原型是彼域 `Promise.prototype`，主域 `instanceof`
  恒 false，落定被丢弃、成功靠巧合（回 undefined）、失败变 unhandled；
  ② 跨域（native 内 `AutoRealm` 切 compartment 且 JS 在栈上）的
  `ModuleEvaluate` 恒走异步求值（主域同序列对照亦然，非 compartment 之过；
  `require` 同调用内无切换故同步）——rval 为 promise 是正确语义，不是 bug。
- 修法：JS 侧按 thenable 结构认领（`typeof r.then === "function"`），成功
  认领后置 evaluated 位 + 读 namespace，失败置 errored + 记 `module.error`；
  Rust 侧 promise 路径不预置 evaluated 位（`__wjs_vm_mod_settled` 由壳在落定后
  补记；`vm_mod_ns` 照旧以位为门）。
  二分过程的临时 `vm_dbg_*` natives 用完即删，不进提交（本次删干净，
  `grep vm_dbg` 为空）。
- 推广为铁律：跨 compartment 的值一律按结构判形态（thenable/数组用
  `Array.isArray` 跨域安全），禁 `instanceof`；凡 `evaluate` 族 API 的壳必须
  同时兼容同步完成值与 promise 两种 rval。
- 复现：`tests/node.rs::phase9i_vm_module_boundary`（`m9iB-evthrow` 行修前为
  `OK` + 进程 exit=1）。

### 4.58 迁移排空三件套：offer 留 target + forwarded 排空 + 分发回退（2026-09-13，Phase 9i-2）

- 症状：跨线程端口迁移后，主→worker 方向首条消息必丢（worker→main 方向正常）。
- 根因（三连）：① offer 即摘 target 则竞态消息无处排队；② `forwarded`
  排空时通道里在途的迟到消息晚于排空到达，此时 target 已摘即丢；
  ③ 本引擎 `structuredClone` 不支持 BigInt（`DataCloneError`），"先 clone
  探路"的信封设计与 BigInt 支持互斥；另 `SharedArrayBuffer` 全局不存在，
  `instanceof SharedArrayBuffer` 直接 ReferenceError（非 false）。
- 修法：offer 置 `moved` 停计数但保留 target（竞态消息照常排队）→
  `PortForward` 派发后调目标 `__ev("forwarded")` 经现转发路由排空 →
  分发侧 `PortMsg` 见 target 空而有转发路由即改道（`port_forward_route`
  回退）；可克隆性改 walk 全权判定（`__denyClone` 显式拒绝表）；
  不存在的全局一律 `typeof` 守卫先行。
- 推广为铁律：凡"先摘后建"的跨会话移交，必须回答"在途消息去哪"——排空点
  + 分发回退缺一不可；引擎能力断言（structuredClone/BigInt/SAB）以上手实测
  为准，不抄文档记忆。
- 复现：`tests/node.rs::phase9i_worker_transfer_cross_thread_and_broadcast`
 （`w9i-xfer` 第二项修前为 false）。

### 4.59 CJS 互操作垫片吞掉纯 TLA 的 `.js` 依赖（2026-09-13，Phase 9j）

- 症状：CJS 互操作上线后，`phase2_top_level_await_entry` 挂——入口 `tla.js`
  （顶层 `await` 专属、无 import/export）报 `await is only valid in async
  functions...`，而非走模块重试。
- 根因：`cjs_interop` 用 `is_module`（`has_module_syntax`）判 ESM——纯 TLA
  文件无模块语法即判 CJS，打上 `export default` 垫片；垫片求值期同步
  require，CJS 包装走经典脚本求值，顶层 `await` 即炸。入口经典路径的
  TLA 重试（§4.17）够不着依赖。
- 修法：歧义集（无 type 的 `.js`/`.jsx`）加经典目标试解析
  （`parses_as_script`，`with_module(false)`）：但注意 oxc 在 script goal
  下仍会对无歧义顶层 `await` 置模块升级信号（Babel 式 `sawUnambiguousESM`，
  `set_module_syntax` + 延迟错丢弃）——所以试解析必须同时看
  `!has_module_syntax`，只看"无错"不够（`src/modules.rs`）。
- 复现：`tests/node.rs::phase9j_tla_dep_stays_esm`（修前 TLA 依赖进垫片炸）。
- 推广为铁律：oxc `with_module(false)` ≠"无模块信号"——`module_record.
  has_module_syntax` 才是升级真相；任何"经典/CJS 兜底"判定都要先过 TLA
  专属文件这一关（入口 `tla.js` 即现成探针）。

### 4.60 `util.parseEnv` 结果键按 ASCII 排序，非插入序（2026-09-13，Phase 9j）

- 症状：四组真机差分探针前三组全同，第四组（多行引号）仅键序不同——
  `B=1\nA=2` 真机出 `{"A":"1","B":"1"}`，不是插入序 `{"B","A"}`。
- 根因：Node 的 parseEnv 实现按 ASCII 排序输出键（`B=1\nA=2` → A,B，
  大小写敏感大写在前），与 JS 对象插入序直觉相反。
- 修法：解析期 `Map` 收集，组装期 `[...keys()].sort()` 再写入
  （`src/builtins/node/util.rs` `parseEnv`）。
- 复现：`tests/node.rs::phase9j_util_parse_env` 首断言
  （`{"A":"1","B":"1"}` 顺；修前为插入序）。
- 推广为铁律：§4.32 教训延续——"顺序"也是语义，真机差分必须连键序一起
  `diff`，逐行 `JSON.stringify` 对拍（本仓即靠它抓到）。

### 4.61 自递归子进程的动作 flag 碰撞：脚本参数禁复用 winterjs 动作名（2026-09-13，Phase 9j）

- 症状：vue 形 `-r dev` 管线黑盒用 `tool --serve` 作 fixture，子进程报
  `specify exactly one action, got: --run, --serve`。
- 根因：9i-10 JS bin 自递归把脚本参数原样透传给自身 `--run <bin>`；
  `--serve` 是 winterjs 已知动作 flag，子进程 clap 即判双动作（§4.26
  全 flag 铁律）。未知 flag（如 `--watch-mode`）因 `trailing_var_arg`
  透传无事——只有**已知动作名**才炸。
- 修法：fixture 改中性参数（`--watch-mode`）；真脚本如需透传动作名，
  走 `--` 分隔（9i-10 语义）。
- 复现：`tests/cli.rs::run_script_vue_dev_shape_through_node_module`
  （`--serve` 形修前必炸）。
- 推广为铁律：黑盒 fixture 的脚本参数不得与 winterjs 动作 flag 同名；
  新增动作 flag 时 grep 测试 fixtures 有无撞名。

### 4.62 stash 期间构建会污染 target，pop 后必须重编再探（2026-09-13）

- 症状：`git stash → cargo build → git stash pop` 后，`./target/debug/winterjs`
  探针报旧行为（`'node:module' is not a builtin`），而 `cargo test` 全绿——
  两边结论打架。
- 根因：stash 期间的构建把旧代码编进了 `target/debug/winterjs`；
  `cargo test` 的测试二进制每次现编（新代码），手工探针用的却是 stale 主二进制。
- 修法：pop 后立即 `cargo build` 再探；结论打架时先对 `ls -la target/debug/winterjs`
  时间戳。
- 推广为铁律：凡中途 stash/checkout 换过代码再探，必须重编主二进制；
  `cargo test` 绿 + 手工探针红 ≠ 代码问题，先查二进制新鲜度。

### 4.63 子进程复用自家 CLI 时透传参数必须 `--` 收尾（2026-09-13，9k）

- 症状：scripts `{"v": "node-which --version"}` 经 .bin JS bin 自递归
  （`winterjs --run <bin> --version`）打出 winterjs 版本横幅，bin 本体没跑。
- 根因：本仓已知内置 flag（`--version`/`--help`）在 trailing positional
  收集前就被子进程 clap 拦截；§4.61 的"未知 flag trailing_var_arg 透传"
  只覆盖未知 flag，已知 flag 是它的补集缺口。
- 修法：子递归统一 `--run <bin> -- <args>`，`--` 由 clap 消费、rest 原样落
  `cli.args`（`src/scripts.rs`）。
- 推广为铁律：凡 spawn 自家 CLI 传用户参数，一律 `--` 收尾；每新增一个
  内置 flag，grep 全部这类调用点复核一遍。

### 4.64 成对分隔符夹在中间时 strip_suffix 必空（2026-09-13，9k）

- 症状：stub registry 按 `/<name>/-/<name>-1.0.0.tgz` 剥后缀解析 tarball
  路径，`strip_suffix("-1.0.0.tgz")` 后再 `strip_suffix("/-")` 恒失败——
  剩余串以包名结尾（如 `init-dep-a/-/init-dep-a`），`/-` 在中间不在尾部。
- 修法：`split_once("/-/")` 取左名 + 右文件名全等校验。
- 推广为铁律：路径/串解析先画全形再选 API——分隔符在串中段用 split 家族，
  strip_suffix 只配"真后缀"；stub 请求数对不上时先打印实际 path。

### 4.65 宽松 API 重构到严格语义：先真机实测，再改伪语义断言（2026-09-14，M5）

- 症状：EventTarget 全局化（cac `class CAC extends EventTarget` 前置）并把
  AbortSignal 重构到基类后，`tests/fetch.rs::streams_abort_events` 挂
  （`dispatchEvent requires an Event instance`）。
- 根因：旧 AbortSignal 的 dispatchEvent 收普通对象且手动 dispatch 会置
  aborted 位；新实现按 DOM/Node 标准只收 Event 实例。真 node 26 实测：
  普通对象 → TypeError（`The "event" argument must be an instance of Event`）；
  `dispatchEvent(new Event("abort"))` 返 true 但 aborted **不变**（置位只归
  abort 算法）。旧测试编码的是伪语义，非实现回归。
- 修法：测试按真机改（`new Event("abort")` + 断言 aborted 不置位 +
  普通对象断 TypeError）。
- 推广为铁律：把宽松 API 重构到标准严格语义后必跑全量抓旧测试；断言与
  真机冲突时先实测再改——测试也可能在编码实现的历史偏差（§4.32 测试侧版）。
- 复现：修前 `cargo test --test fetch streams_abort_events` 必挂。

### 4.66 napi-rs 3 的 Promise 转换暗面 + Either 兜底吞真因（2026-09-14，M5）

- 症状：vite build JS API 报 `The function returned \`object\`, but expected
  \`undefined\`.`（stack 空）；CLI 路径则死在 vite 配置加载
  `export declarations may only appear at top level of a module`。
- 根因（二连）：① 报错文案出自 rolldown binding 的
  `Either<Ret, InvalidReturnValue>` 兜底分支——它把一切转换失败吞成统一文案；
  真因是 napi-rs 3 的 `PromiseRaw.then/catch` 会**立即对自家
  `napi_create_function` 产物调 `napi_wrap` 挂 finalizer**，撞上 M3 的
  "仅 define_class 实例" fail-fast（真 Node 的 napi_wrap 本就收任意对象——
  m2 fixture 编码了偏差）。② vite 配置打包链
  （bundleConfigFile→loadConfigFromBundledFile）依赖 `require.extensions` +
  `module._compile`（内存 CJS 产物求值），require 无视钩子直接读盘即炸。
- 修法：napi_wrap 任意对象路进 env 登记表（无 GC 驱动 finalize，
  end_session 收敛 LIFO 触发；ref 出参仅类实例路）；createRequire 系
  require 消费 extensions（`.js` 兜底同 vite loaderExt；cache 先于
  extensions，Node 口径）+ native `__wjs_cjs_compile`（CJS 包装口径与
  require 全同，require 以文件自身为 base）。
- 定位手法（可复用）：文案不可信时给 `napi_call_function` 插 [wdbg]——
  被调函数名 + 匿名函数经 `Function.prototype.toString` 吐源码前段 +
  rval 类型，TSFN 加 resource_name；报错紧跟的最后一条即命中。
- 推广为铁律：napi-rs 3 起 Promise 转换是**有副作用**的（挂 then/catch
  回调 + napi_wrap）；凡 "expected X" 类报错先查 Either 兜底吞没的真因。
  fixture 断言偏离真机语义的，真机口径一锤定音（§4.65 同源）。
- 复现：`cd /tmp/wjs-vite-probe/min-proj && build-probe.mjs`（napi Wrap 前
  必报 expected undefined；改 vite.config.js 前必报 export declarations）。

### 4.67 vite dev 真变更 139：fsevents 回调专属，polling 通（2026-09-14，M5）

- 症状：vite dev（rolldown transform 拉起 + WS 握手完成）后真文件 append 即
  `EXIT:139`（`EXC_BAD_ACCESS 0x4b4b4b4bXXXXXXXX`，栈顶 JIT
  `tryAttachTypedArrayElement`；调用栈 `asyncwork::dispatch(TsfnDrain)` →
  `fse_dispatch_event` → `napi_call_function` → JS 内崩，`CHOK-CHANGE` 前）。
  `hmr:false` 照崩；`usePolling:true` 不崩。
- 隔离矩阵（`/tmp/wjs-vite-probe/min-proj/hmr-min*.mjs`，落盘法读输出——
  进程不退出时管道输出会被吞）：无 WS/手动 emit/仅 fetch/仅 append 全过；
  `fetch(transform)+握手+真 append` 必崩（WS 事先 close 照崩——握手期 state
  已埋雷）；`fetch(@vite/client 静态)+握手+append` 不崩（transform 必需）。
  [wdbg] 实证 napi 实参形态正确（path 字符串/flags 数字/id 数字）。
- 落袋三件（实锤缺口，与崩溃因果未完全钉死但均为 dev 必经面）：
  ① `stream.resume is not a function`（min13 关停路径 unhandled rejection
  实录）→ Socket/IncomingMessage/ServerResponse 补 pause/resume/read/
  setTimeout/cork/uncork 桩（整收口径 read 恒 null）；
  ② `fs.watchFile is not a function`（polling 37 路 rejection 实录）→ 纯 JS
  stat 轮询实现 + `unwatchFile`（零 native；persistent:false/bigint 记档）；
  ③ TSFN dispatch 无 pending 守卫（M4-③ loader 同类，net/worker 系均有
  `failed(cx)` 收敛）→ `TsfnDrain/AsyncDone` 回调后查 pending 即转可读错
  （本次未触发——崩溃在回调**内**，属防御性收敛）。
- 未闭环：fsevents 原生回调同步 JS 路径的崩溃根因未定（napi 层无异常遗留；
  0x4b 高位恒定、低位浮动； suspects 剩堆损坏延迟引信/形状损坏，待深入）。
  M5 dev 验证暂走 polling（`server.watch.usePolling`，HMR `CHOK-CHANGE` +
  干净退出已验证）；默认 fsevents 路径记档待修。
  **→ 已闭环（2026-09-14，见 §4.68：`NapiEnv::trace` 漏标 `tsfns.js_cb`），
  默认 fsevents 路径 `hmr-min9.mjs` 修后 full-reload + `EXIT:0`。**
- 复现：`hmr-min11.mjs` 形（transform fetch + WS 握手 + 真 append，三件齐崩；
  任缺一件即过）；崩溃报告见 `~/Library/Logs/DiagnosticReports/winterjs-*.ips`
 （`0x4b4b4b4b` 高位恒定）。
- 推广为铁律：长驻探针进程一律输出落盘再读（`>file 2>&1` + 定时 kill），
  管道直连超时即丢输出；新事件域 dispatch 先抄 `failed(cx)` 收敛再接线。
- 追补（2026-09-14）：`writeUInt16BE is not a function` 是另一独立缺口——
  HMR 重变换链（sourcemap 编码）直调 Buffer 整数系，本仓全局 Buffer 从未实现
  `read/write*整数/浮点` 全家（`phase9b_buffer_int_rw` 落盒，DataView 直通
  32 方法）。症状是 HMR 推 `{"type":"error"}` 而非 update（有 CHOK 无 update
  即查此面），与 fsevents 139 无关（polling 后端同样先 error 后随补齐转绿）。
  `/var` 下"能侦测不推送"即此缺口所致，非路径/时序问题（教训：先看推了什么
  消息类型再怀疑路径）。

### 4.68 TSFN 的 JS 回调漏标 GC 根：fsevents 真变更 139 根因（2026-09-14，M5）

- 症状：§4.67 的 139——真文件 append 后 `EXIT:139`
 （`EXC_BAD_ACCESS 0x4b4b4b4bXXXXXXXX`，高位恒定、低位浮动；调用栈
  `asyncwork::dispatch(TsfnDrain)` → `fse_dispatch_event` →
  `napi_call_function` → `call_impl` → `JS_CallFunctionValue` → JS 内崩）。
  崩点在回调**内**（`tryAttachTypedArrayElement`/`GetProperty` 都见过——
  崩点是受害现场，不是根因）；napi 实参形态正确、无 pending 遗留。
- 根因：`NapiEnv::trace` 只标 `slots/escape_slots/deferreds/refs/wrap_sym/
  modules`，漏了 `tsfns` 记录里的 `js_cb`。TSFN 的 JS 回调**只被该记录持有**
  （dispatch 截断 `slots` 后无其他根），一次 GC 后回调即悬垂，下次事件
  dispatch 取悬垂槽值进 `fn.apply` 就是 UAF。fsevents 是长驻回调 +
  transform/fetch 制造 GC 压力 → 稳定复现；m3 旧 fixture 全程无压力 → 全绿
  掩盖（§4.40 同源：旧测试从未在回调内存活期外造 nursery 压力）。
- 修法（`src/napi/env.rs`）：trace 加 `for (_, t) in &self.tsfns {
  t.js_cb.trace(trc) }`。回归：`tests/fixtures/napi/m3_tsfngc.c`
  （线程延迟 600ms 投递 3 条，JS 侧 8 轮 × 20 万小对象 ≈ 80MB nursery 压力；
  大对象直进 tenured 触发不了 minor GC，故不用大数组）+
  `tests/napi.rs::phase_napi_m3_tsfngc_roots_callback`。
  Revert-check：注释掉该行即挂（修前 139/修后过；`/tmp/wjs-tsfngc` 手动复现同，
  崩溃报告 `0x4b4b4b4b00000000` 与线上同特征）。
- 复现：`hmr-min9.mjs`（transform fetch + WS 握手 + 真 append）修前 139、
  修后 `full-reload` + `CLOSED` + `EXIT:0`；mimic 探针的 BigInt 是红鲱鱼
  （`Number(id & 0xffffn)` 混用抛 TypeError 触发 fsevents.c 的 assert，
  改纯 Number 探针即过，与 139 无关）。
- 推广为铁律：§4.40 的完整形态——**`Box<Heap>` 定址只保地址稳，trace 才保
  可达，两者缺一不可**；凡新增跨 GC 存活的 JS 值存储（含 HashMap value、
  新 napi 记录），trace 必须同步加，上线前 grep trace 覆盖。FinalizationRegistry
  在本引擎不可靠（真机同为 false），回归探针用"确定性压力 + 投递存活"断言，
  不用 canary（试过，不稳定）。

### 4.69 目录首条目 tarball 炸解包：暂存预建（2026-09-14，M5 vitest 牵引）

- 症状：`winterjs -a vitest` 在 `@types/chai@5.2.3` 必败：
  `cannot unpack @types/chai@5.2.3: unpack failed: failed to create
  node_modules/.staging-<pid>-<rand>`；其前 100+ 包全过。
- 根因（二连）：① 该包 tarball 非 `package/` 布局——条目以 `chai/` **目录条目**
  打头（`tar -tzf` 首行 `chai/`，常规 npm 包首条目即文件、无根目录条目）；
  ② tar 0.4.46 的 `unpack_in` 在父链校验里 `canonicalize(dst)`，首条目为目录
  时本仓又跳过预建父链（只对非目录条目建），暂存尚不存在即
  `failed to create <staging>`。常规包因首个文件条目的预建顺带建了暂存，
  故从未暴露。另：`chai/` 根的回落（暂存内唯一顶层目录即包根）本就写好，
  只是一直没活到那一步。
- 修法（`src/pm/install.rs::unpack_tgz`）：循环前 `create_dir_all(staging)`
  预建暂存，一行。回归：模块单测 `unpack_dir_first_non_package_root`
  （现场打目录首条目 gz，目标为尚不存在的暂存；revert-check 注释预建即挂）+
  黑盒 `phase5_install_dir_first_tarball_stub`（stub 下发→真装→require 出 42）。
  实证：修后 `-a vitest` 一遍过（25 包，`vitest@5.0.0`）。
- 过程教训：中途一次"旧二进制却装成功"系误判——`cargo test --test pm`
  会先编出**带修复的主二进制**（集成测试要 `CARGO_BIN_EXE`），其后对
  `install.rs` 的 revert/恢复编辑只改了 mtime 没改净内容，二进制实际已含修复；
  `ls -la` 的新旧比较证明不了二进制由哪版源码编出。结论打架时先查构建指纹链，
  不只看 mtime（§4.62 姊妹篇）。
- 复现：`curl registry @types/chai/-/chai-5.2.3.tgz | tar -tzf -` 首行即目录；
  修前模块单测必挂。
- 推广为铁律：凡"前 N 个全过、特定包必败"的安装失败，先 `tar -tzf` 看该包
  条目布局（根目录条目/非 package 根/符号链接三件），再怀疑网络与版本。

### 4.70 入口失败 + 开着的句柄 = 事件循环永不收割（2026-09-14，M5 fork 牵引）

- 症状：`fork("/no/such/xxx.mjs")` 父端无 error 无 exit，进程 hang（`bad-arg`
  同步校验正常；裸 eval worker 同样缺失 import 却正常 error+exit 1）。
- 根因：`run_module` 的入口 rejection 收割在 `event_loop` **之后**
  （`src/runtime.rs`）——子会话开着 parentPort（message/close 双监听）时循环
  永不 idle，收割点永不到。裸 worker 无句柄，首轮即 idle，收割正常。
  二分探针：端口在场 + 未捕获顶层拒绝即挂（`p6.mjs`），catch 住即回 END 后
  空转（`p5.mjs`，端口续命本身是正确语义）。
- 修法：`PumpStats` 加 `entry_failed` 旗——`pump_once` 内 RunJobs **之后**
  查 `entry_rejection.is_some()`（与 `process_exited`/`worker_terminated`
  同族检查点、同顺序），`event_loop` 见旗即 `break` 走既有收割上报
  （入口带专用捕获，不进 `unhandled` 表，收尾 `report_unhandled_rejections`
  不受影响；`process.exit` 优先顺序不动）。
- 复现：`tests/node/child.rs::phase9m_child_fork_errors`（修前 hang；
  另带出父断子不断 control 分支漏 `parentPort.close()`，同批修，
  `phase9m_child_fork_ipc` 的 `EXIT-EV 0 0` 覆盖）。
- 推广为铁律：凡"失败只在循环尾收割"的设计，必须回答"循环不退时失败去哪"——
  fatal 类决议（入口错/uncaught）一律检查点提前跳出，不等自然排空。

### 4.71 `Object.assign` 不自建自引用 + 真机 assert 形态先行（2026-09-14，M5）

- 症状：`assert.ok is not a function`（`phase4_node_assert_subset`），连带
  `net_echo_loopback`（服务端 handler 首行即 `assert.ok`，抛错后响应永不结束→
  链式 stall→server 不关→全进程 hang；http 回环 hang 同源，修 assert 即好，
  无 http 侧改动）。
- 根因：M5 把默认导出改可调用时写成 `Object.assign(ok, {...})`——assign 不建
  自引用键，`default.ok` 为 undefined。想当然补 `__default.ok = __default`
  又错第二遍：真机（node 26.8.2 实测）`assert.ok !== assert`，
  `assert.ok === assert.strict.ok`，名分别为 `assert`/`ok`。
- 修法（`src/builtins/node/assert.rs`）：具名 `function assert(value, message)`
  转调 `ok`，再 assign 全方法（`ok/strict/AssertionError` 等）后默认导出——
  五项逐项对真机：`typeof function`×2、自反 false、`ok===strict.ok`、
  双名、`assert(true)` 直调。
- 教训：§4.65 姊妹篇——"真机口径"必须**逐项实测**，不能只验规划的那两项
  （本次若只验 `typeof` + 直调，自引用错就漏网；`console` 黑盒的
  `assert-callable true false` 即真机逐项，修完直接绿）。
- 推广为铁律："与真机同款"断言必须列出全部可观察项并逐项对拍，缺一项即欠账。

### 4.72 fork 单槽监听 + CJS 具名上线后的黑盒同步（2026-09-14，M5）

- 症状一：`phase9m_child_fork_ipc` 修 hang 后报
  `NotSupportedError: ChildProcess event 'w9m-never'`——测试用凭空事件名验
  once/off，而 `on()` 对未知名按设计抛错。
- 修法（测试侧）：占位改 fork 路径永不触发的 `spawn` 事件——不用 error/exit
  是因单槽位 `once` 会顶掉同名常驻监听（`off` 对 exit/close 双重 wrap 也摘不净），
  不用 message/disconnect 是因流程内真会触发。注释写明三选理由。
- 症状二：`phase9j_cjs_interop_default` 的"命名导入必须失败"边界在具名导出
  上线后反绿为红（`import { v }` 成功）。
- 修法（测试侧）：边界翻转为成功断言（`cjs-named 41`），与
  `phase9j_cjs_interop_named` 同口径；旧"缺导出"注释标退役。
  另：`cjs_named` 的 `defaults … function …` 系 fixture/plain-object 与期望
  打架的测试 bug（`typeof` 应为 object），同批改。
- 推广为铁律：功能上线即全 grep 旧边界断言（"必须失败/必须抛"类），上线不改
  旧断言等于埋红；单槽事件设计下，测试占位事件必须选"永不触发 + 无常驻监听"
  的那一个。


## 5. 路线图（按序）

1. `console` / timers（含 `queueMicrotask`）
2. Promise job queue（microtask drain，直连 `JSContext`，禁 `&mut` 别名，见 winterjs-old §7.9 教训）
3. ESM loader（resolve/fetch/compile/link）
4. `fs` / `path` / `process`（Node 兼容垫片起点）

## 6. 架构原则：runtime 纯 Rust，mozjs 是墙（2026-09-09 决策）

- 引擎本体（C++，经 `mozjs_sys` 编译）不算本项目代码，它只是依赖。
  本项目自己的代码（CLI、event loop、builtins、loader、Node 垫片）**全部纯 Rust**。
- `unsafe` 只允许出现在 mozjs 边界（rooting、`AutoRealm`、FFI 调用），
  业务逻辑层禁 `unsafe`；新增 `unsafe` 必须在注释写清前置条件。
- 收敛铁律：除 hooks（`modules.rs`）/jobqueue/`state.rs` 的引擎协议代码外，
  裸 JSAPI 调用一律收敛进 `src/jsapi_glue.rs`（唯一的集中边界模块）；
  新增收敛函数必须带 `UNSAFE-BOUNDARY` 标签（前置条件 + 覆盖测试名），
  黑盒测试重点回归这些标签（§0.7）。
- 新增 `unsafe` 三问（按序证伪，答完才写）：① 有 safe 写法或已有 crate 代替吗
  （先走 §0.5 找轮子）？② 能把 `unsafe` 收敛进构造器、对外只暴露 safe 访问器吗
  （`Frame` 模式：`from_raw` unsafe，`arg`/`set_rval` safe + 越界断言）？
  ③ 前置条件写进注释了吗？
- 存量基线（2026-09-10 实数，`rg` 文本值；`console_sink!` 宏展开后更多）：
  `unsafe extern "C"` 50（C ABI 强制，不可去；Phase 3a 起每新增 native +1）、
  `unsafe impl Traceable` 2（GC 协议，不可去）；
  `unsafe{}` 块 132，其中每个 JSNative 入口固定 2 个边界块
  （`wrap_cx` + `Frame::from_raw`，随 native 数线性增长，结构性不可去）；
  `wrap_cx` 维持 unsafe（`from_ptr` 本质 unsafe）；
  其余 FFI 体（`JS_GetProperty`/`JS_CallFunctionValue`/`evaluate_script`/
  `RunJobs`/`TypedArray::create` 等）不可去——mozjs 本身就是选定的轮子，
  没有更上层的 safe 运行时可选。
  审计口径：禁业务层 `unsafe`、禁裸指针新用法（状态一律走保留槽/JSON 桥/TypedArray
  safe 读）；边界入口块如实计数，不算违规。
- 可观测性（2026-09-10）：所有功能模块必须带分级 `tracing` 埋点（Phase 0 已接线），
  分级：INFO=阶段里程碑（run/eval 起止、事件循环退出）；DEBUG=状态变迁
  （timer 注册/触发/取消、fallback 路径选择、rejection 捕获）；TRACE=热路径逐条
  （microtask 出队计数）；WARN=降级/可疑（非常规但可恢复）。
  纪律：禁把用户脚本原文打进日志（只记长度等元信息）；禁在 `console.*` 内打日志
  （用户输出通道，避免刷屏/递归）；热路径昂贵构造先用 `tracing::enabled!` 守卫。
  调试：`winterjs -vv …` / `WINTERJS_LOG=winterjs=debug …` /
  `WINTERJS_LOG_FILE=…`（子 target `winterjs::xxx` 自动被 `winterjs=<level>` 覆盖）。
- 线程模型：`JSContext` 是 `!Send`，JS 永远跑在独占线程（tokio `LocalSet`），
  Rust 侧多线程只通过消息队列与 JS 线程通信，绝不跨线程共享 `&mut JSContext`
 （winterjs-old §7.9 的 aliasing-UB 教训）。
