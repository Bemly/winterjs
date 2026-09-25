# AGENTS.md — winterjs 工作规约

> Bun-like JS runtime，直连 Mozilla SpiderMonkey（经 `servo/mozjs` Rust 绑定）。
> 从 `winterjs-old`（WinterCG server + spiderfire）推倒重来，老项目只当参考，不合、不动。
>
> **新会话开工顺序**：本文件 → `docs/plan3.md` §0（唯一进度入口）→ 动某域前
> 在 `docs/pitfalls.md` 索引里 grep 该域条目。其余文档见 `docs/README.md`（多为存档，不必读）。

## 0. 工作流铁律

1. **每次修改都要 `git commit`**：小步提交；提交前必看 `git status --short` +
   `git diff`，只 stage 意图内的文件，绝不提交 secrets。
2. **先查证据再下结论**：读文件、跑构建、跑 `./target/debug/winterjs` 实测；
   与文档矛盾以实测为准并更新文档。
3. **踩坑必记**：新坑追加到 `docs/pitfalls.md` 末尾（编号续排 `4.N`：症状 → 根因 →
   修法 → 复现 → 推广铁律）；只有**新的通用铁律**才在本文件 §4 摘要补一行。
4. **依赖随缘更新**：除 `mozjs` 必须精确钉死外（§2），其余 caret 不锁上限，
   `cargo update` 随便跑；跑坏就地修并回写 `docs/dependencies.md`。
5. **新工具先找轮子**：想手写新工具/模块/功能时，先去 crates.io 找依赖，
   符合标准记入 `docs/dependencies.md`，然后**停下来问用户**，点头后才引入。
6. **能 safe 不 unsafe**：新增 `unsafe` 前先证伪 safe 路线（§6 三问）；存量只减不增。
7. **测试三件套随功能落地**（完工标准含三绿）：
   - 模块测试：`src/` 内 `#[cfg(test)]`，覆盖纯 Rust 可测逻辑。
   - 黑盒测试：`tests/`，经 CLI 断言用户可见行为；文件按 src 域对齐
     （`tests/<域>.rs`，共享 helper 进 `tests/common/mod.rs`；node 域二层
     `tests/node/<mod>.rs` 经 `#[path]` 挂壳，域内脚手架进 `tests/node/helpers.rs`）；
     每个新 API 含**正常 + 报错 + 边界**三件；`UNSAFE-BOUNDARY` 新增必配 panic 路径用例。
   - 冒烟：§3 五条，构建后必跑，不过不提交。
   - 全量回归：`cargo nextest run`（见 §4 测试与跑分）。
8. **CLI 全 flag 规范**：无裸子命令、无裸位置参数——所有动作一律 `-x/--xxx`；
   一次恰好一个动作，多给即错；动作的必需值紧贴其 flag；修饰 flag
   （`--dry-run/--registry/--port` 等）只在对应动作下生效。help/补全/man 由同一套
   flag 生成（`localized_command`）。
9. **单文件 ≤1000 行**：项目内全部 `.rs`（`src/`+`tests/`+`benches/`）与 `src/**/*.js`
   （内嵌 JS SOURCE，2026-09-25 D3 纳入）不超过 ~1000 行，无豁免（`sample/` 不管）；
   提交前跑 `scripts/check-lines.sh`。JS 片经 `concat!(include_str!…)` 拼接，按方法边界切、
   concat 字节恒等（HEAD 原件 `cmp` 新片拼接）。
   拆分纪律：① 纯搬移先行（`git diff -w` 只见路径），调用方经 `pub use` 原位重导出；
   ② 一文件一提交，每步 0 警告 + 对应域测试绿；③ 引擎协议代码（`runtime`/`state`/
   `jsapi_glue`/`jobqueue`/`modules`）只拆纯逻辑，会话管线与 trace 不动；
   ④ Node 移植的 JS SOURCE 沿"域/算法族"切，SOURCE 随实现走。
10. **大数据走外置盘**：系统盘余量紧张，node 套件检出、sweep 工件、探针一律放
    `/Volumes//wjs-data`（软链 `~/wjs-data`）；`cargo test` 不改 `TMPDIR`（4.207），见 plan3 §0.5。

## 1. 基线（2026-09-09）

- `mozjs = "=0.26.0"`（Gecko 153，crates.io 最新发布版），`Cargo.lock` 入库。
- Rust stable 最新（现 1.98），edition 2024（即 stable 最新；2027 尚不存在）。
- 版本号用 CalVer `YY.MM.PATCH`（如 `26.9.13`，cargo 可解析；`^26.9.0` 即年内自动升）。
  依赖清单与 10-target 矩阵见 `docs/dependencies.md`。
- 无 `rust-toolchain` pin、无 spiderfire/ion 依赖、无 server/request_handlers。
- CLI（全 flag，§0.8）：`winterjs --run <file|script>`（带脚本后缀→文件直跑；
  裸名→package.json `scripts` 优先、同名文件回落；JS bin 递归自身执行，零 node）/ `winterjs --eval <code>` /
  `winterjs --config [--schema]` / `winterjs --completions <shell>` / `winterjs --man` 等，
   见 `src/`（cli/runtime/dispatch/error/logging/settings/alloc 模块；`runner.rs` 已由 `runtime.rs` 接替，见 plan.md）。
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
  `./target/debug/winterjs --eval 'throw new Error("boom")'` → 非 TTY 下 node 形
  （`eval.js:1` / 源行 / `^` / 空行 / `Error: boom` / `    at eval.js:1:7`），exit=1
  （TTY 下由 miette 图形渲染，带代码框，语义同；2026-09-25 D4）。

### 冒烟（每次构建后必跑，不过不提交）

```bash
./target/debug/winterjs --eval '40 + 2'                                                    # → 42
./target/debug/winterjs --eval 'await new Promise(r=>setTimeout(()=>r(1),10))'              # → 1
./target/debug/winterjs --eval 'new URL("https://ex.com/?a=1").search'                     # → ?a=1
./target/debug/winterjs --eval 'new TextEncoder().encode("hi").length'                     # → 2
./target/debug/winterjs --eval 'await (await fetch("data:text/plain,x")).text()'         # → x
```

## 4. 铁律摘要（全文见 `docs/pitfalls.md`，编号即 `§4.N`）

**GC / 引擎边界**
- `evaluate_script` 返回后、逐任务微任务执行前，调 JSAPI 先进 `AutoRealm`（4.1/4.116）。
- 跨 GC 存活的 JS 值：`Box<Heap>` 定址 + trace 同步，缺一不可；禁裸 `Heap` 进可搬运容器（4.40/4.68）。
- Rust→JS 调用：函数体第一行把全部 JS 值参数入 `rooted!` 槽，之后才许分配；调用链中间值、
  队列批量取出值一律当场 rooted——`call_one` 入口 rooting 不保调用间（4.80/4.118/4.141/4.161）。
- `with_rooted`/`with_plain` 不可嵌套，闭包内只做纯数据（4.14）。
- 不 drop `Runtime`/`JSEngine`（`process::exit` + forget）；同进程再跑 JS 走 `run_isolated` 新线程；
  `engine` 先于 `rt` 声明（4.8/4.22/4.24）。
- 非 JS 线程（notify/回调线程）不碰 TLS state，生数据送回 JS 线程再判定（4.153）。
- GC finalize 内只做纯 Rust 簿记，addon 回调排到安全点（4.78）；napi 值随 scope、跨 scope 走 ref（4.77/4.79）。

**事件循环**
- 同步决议 promise 的结算点到 park 之前必须至少一轮 `RunJobs`；退出旗检查放 `RunJobs` 之后（4.18/4.46）。
- park 唤醒集 ≠ 存活判定集：unref 源到点要醒、不续命、不算 progressed（4.94）。
- fatal 类失败（入口错/未处理 rejection）要有提前跳出的检查点，不能只在循环尾收割（4.70/4.137）。
- `nextTick` 走原生队列，禁用 `queueMicrotask` 模拟（4.118）；"等回包"路径禁阻塞 native，改投递 + 轮询（4.112）。
- 新事件域三查：`__ev` 构造期 bind、purge 放在派发 Close 之后、native 数值 `Number()` 包装（4.36）。
- 父域收尾先静默摘子域任务再发终结事件（4.52）；跨线程 rendezvous 所有出口（含失败）都发（4.49）。
- 流/句柄的"构造即完成"同步链一律递延派发终结事件；sync 底座配计数器续命（4.160）。

**移植与语义（node 定语义）**
- 真机先行：断言前对真机逐项实测**全部**可观察项；旧断言与真机冲突先实测再翻转（4.65/4.71/4.82）。
- 移植先读套件全文；校验顺序逐行对 node `lib/` 原文（4.115/4.120）。
- 校验放包装闭包外（`__callNative`/`__fsCall`/`__zCall`）；错误包装函数必须幂等、直通已带 code 的错误（4.37/4.51/4.119/4.157）。
- 跨 compartment 按结构判形态，禁 `instanceof`（4.57）；`Object.create` 造的实例禁 `#` 私有成员（4.23）。
- 宿主调用户回调走 `Reflect.apply`；内部调用经默认导出对象（用户可 mock）（4.95/4.166）。
- 宿主转发用户异常留 pending 原样透传，禁转串重抛；文案读 `message` 属性兜底（4.208）。
- node 文档写明异步的面（warning/写回调/destroy error）即使能同步完成也异步触发（4.74/4.102/4.187）。
- 对接外部 JSON 一律显式 `serde(rename)` + 真实线名 roundtrip 单测（4.29/4.33）。
- "JS 生成 JS"的模板块内禁内层模板字面量；`format!` 与 JS 同现改文件落盘（4.44/4.151）。
- 新增 `__wjs_*` native 前 grep 重名（4.48）；"缓存了/传了"≠"用上了"，新选项要验到引擎（4.85/4.136/4.146）。
- 密码学手工路径必须双向真机交叉，自交绿不算数（4.54/4.135）。

**测试与跑分**
- 黑盒标签禁子串、一行多断言分参打印（4.42）；读全局态的单测先复位（4.41）。
- 跑分包装 `exec @ARGV or die` + glob 解析路径；"全绿/全红得可疑"先查执行痕迹（4.145/4.168/4.205）。
- 并行跑 node 套件逐进程设 `TEST_THREAD_ID`；对拍前断言 fixtures 完备（4.122/4.158）。
- 判 hang 只认退出码；长驻探针输出落盘；管道取 `${PIPESTATUS[0]}`（4.45/4.67/4.93）。
- 全量测试用 `cargo nextest run`（约 2 分钟；挂死件自动杀、flaky 标注，配置 `.config/nextest.toml`），提交前 `--profile strict`；`cargo test` 仅作兜底（禁套 alarm）；禁并行压力循环与重复全量子集（4.126/4.143/4.175）。
- 新红先 `scripts/flake-classify.py` 分类再动手；"手工过/cargo 挂"先查状态机残留，不是环境问题（4.140/4.197/4.202）。
- stash/checkout 换过代码必重编再探（4.62/4.69）；改 `cfg(test)` 用到的结构体跑 `cargo test --bin`（4.173）。

**工具与环境**
- 同文件编辑串行；多行 edit 后 `git diff` 核对每条删除行；禁空参数工具调用（4.21/4.28/4.150）。
- 禁连续建 worktree，bisect 用主仓 checkout/stash；bisect 后提交前核 HEAD 归属（4.142/4.206）。
- 写文件命令的手工实测先进 probe 目录（4.20）；zsh 以 `=` 开头的词加引号（4.3）。

**CLI 与依赖**
- 动作值紧贴 flag；spawn 自家 CLI 透传参数用 `--` 收尾；fixture 参数不撞动作名（4.26/4.61/4.63）。
- 引入打日志的轮子先查事件 target（4.19）；轮子能力以源码/实测为准，不抄 README（4.37/4.170）；
  RustCrypto 先查 `Cargo.lock` 里的 digest 大版本（4.43）。

## 5. 目标、方法与入口（2026-09-25）

- **目标**：`node:` 兼容到 **Bun 高度**。范围 = Bun 自带 node 测试清单
  （`docs/bun-scope.txt`，快照 `oven-sh/bun@dc30df0`）；已做且超过 Bun 的保留，
  清单外且未做的不做。
- **方法**：**node 定语义**（`lib/` 原文 + `test/parallel` 断言原文），Bun 只定范围。
  deno 参照随 plan2（deno 高度）对齐完结，不再作为参照。
- **入口**：唯一进度真相是 `docs/plan3.md` §0；逐轮日志写 `docs/plan3-journal.md`，
  对拍明细写 `docs/bun-parity.md`。Phase 0–9 / napi / serve 各计划均已收官存档（`docs/README.md`）。

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
- 存量基线（2026-09-15 实数，`rg` 文本值；`console_sink!` 宏展开后更多）：
  `unsafe extern "C"` 450（C ABI 强制，不可去；其中 napi N-API 面 ~133，
  其余随 native 数线性增长，每个 JSNative 入口 +1）、
  `unsafe impl Traceable` 17（GC 协议，不可去）；
  `unsafe{}` 块 1067，其中每个 JSNative 入口固定 2 个边界块
  （`wrap_cx` + `Frame::from_raw`，随 native 数线性增长，结构性不可去）；
  `wrap_cx` 维持 unsafe（`from_ptr` 本质 unsafe）；
  其余 FFI 体（`JS_GetProperty`/`JS_CallFunctionValue`/`evaluate_script`/
  `RunJobs`/`TypedArray::create` 等）不可去——mozjs 本身就是选定的轮子，
  没有更上层的 safe 运行时可选。
  审计口径：禁业务层 `unsafe`、禁裸指针新用法（状态一律走保留槽/JSON 桥/TypedArray
  safe 读）；边界入口块如实计数，不算违规。
  napi 面追补（2026-09-15，M6 合流）：`src/napi/` 全部 `#[no_mangle] pub unsafe
  extern "C" fn` 为 N-API C ABI 面（~133 符号，rolldown 名单 §6 对齐）——结构性
  新增，不逐个计数；其业务体（Rust 侧）仍守"禁业务层 unsafe"口径，JSAPI 调用
  经 NapiEnv/Heap 槽位 arena（§4.40 `Box<Heap>` 定址 + trace 补根双铁律）。
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
