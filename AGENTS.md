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
- CLI（全 flag，§0.8）：`winterjs --run <file>` / `winterjs --eval <code>` /
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
