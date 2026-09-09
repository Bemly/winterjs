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
5. **新工具先找轮子**：每次想要新手写工具/模块时，不许直接手写；
   先去 crates.io 找依赖，符合标准就记入 `docs/dependencies.md`，
   然后**停下来问用户**等拍板，用户点头后才引入。

## 1. 基线（2026-09-09）

- `mozjs = "=0.26.0"`（Gecko 153，crates.io 最新发布版），`Cargo.lock` 入库。
- Rust stable 最新（现 1.98），edition 2024（即 stable 最新；2027 尚不存在）。
- 版本号用 CalVer `YY.MM.PATCH`（如 `26.9.0`，cargo 可解析；`^26.9.0` 即年内自动升）。
  依赖清单与 10-target 矩阵见 `docs/dependencies.md`。
- 无 `rust-toolchain` pin、无 spiderfire/ion 依赖、无 server/request_handlers。
- CLI：`winterjs run <file>` / `winterjs eval <code>` / `winterjs config [--schema]` /
  `winterjs completions <shell>` / `winterjs man`，见 `src/`（cli/runner/error/logging/settings/alloc 模块）。
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
- 验证：`./target/debug/winterjs eval '40 + 2'` → `42`；
  `./target/debug/winterjs eval 'throw new Error("boom")'` → 非 TTY 下
  `Error: eval.js:1:7: boom`，exit=1（TTY 下由 miette 图形渲染，带代码框，语义同）。

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
- 线程模型：`JSContext` 是 `!Send`，JS 永远跑在独占线程（tokio `LocalSet`），
  Rust 侧多线程只通过消息队列与 JS 线程通信，绝不跨线程共享 `&mut JSContext`
 （winterjs-old §7.9 的 aliasing-UB 教训）。
