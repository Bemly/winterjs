# napi 立项计划（已收官合流，存档）

> **存档（2026-09-25）**：napi M0–M6 已收官合流。 当前进度见 `docs/plan3.md` §0，索引 `docs/README.md`。


> 立项 2026-09-13（用户拍板）。目标：在 mozjs 上手写 host 侧 Node-API 实现，
> 终验收线 = **用户 vue-project 上 `winterjs -r dev` / `-r build` / `-r test`
> 三命令全绿**（vite 8.3.0 rolldown 系）。
> 收官（2026-09-15，M6）：`napi` 分支已合回 master，`winterjs-napi` worktree
> 已删；本文存档，不再更新。工作规约见 AGENTS.md，依赖口径见 docs/dependencies*.md。

## 0. 已拍板决策（2026-09-13，用户）

| 项 | 决定 |
|---|---|
| 首个目标包 | rolldown → vite dev（napi-rs 现代面；函数面优先级按 rolldown 实际调用序） |
| 终验收线 | dev + build + vitest 全绿（最重档） |
| 权限边界 | `.node` 加载复用 `--allow-ffi`（`permissions::check_ffi`；不传旗标行为不变） |
| 合流节奏 | 长分支到底：vite 全链跑通后一次性合 master；每 M 收尾 rebase master |
| 依赖拍板 | `bindgen` build-dep 引入（vendored 官方头生成 sys 类型，防 133 签名转写错） |
| 依赖勘误 | `cc` dev-dep **不引**：fixture 是 dylib（`-dynamiclib`），cc crate 只产静态库；
  沿用 `tests/common::build_ffi_dylib` 的 shell-out 先例扩 include 参数 |
| M2 再议 | `napi` dev-dep（Rust 真 addon fixture，验 napi-rs 生成路径）——M2 时定 |

## 1. 勘探证据（2026-09-13 实测，@rolldown/binding-darwin-arm64 8.3.0）

- **加载机制**：未定义符号 152 个，`napi_*` 零个（napi-rs 3 运行期 dlsym 查表）；
  绑定自定义 `napi_register_module_v1`（T 符号）。结论：**宿主只需导出 `napi_*`
  符号**（macOS `-Wl,-export_dynamic` / Linux `-rdynamic`），查表/链接期两种解析机制全兼容。
- **函数面**：133 个 `napi_*`（Node-API 8 标准面 + `napi_module_register`），
  名单存档本文件 §6；实现清单与验收大纲以此为准。
- **libuv 垫片**：`uv_*` 仅 `uv_run` 一个符号（`uv_event_loop` 是
  `napi_get_uv_event_loop` 的子串）——一行级垫片接事件循环单步。
- 无 nan/V8-API 依赖（纯 napi）；FSEvents/CoreFoundation 是 rolldown 自带系统库。
- 真实失败点复现（9j 记档延续）：绑定装好后 `require()` 抛
  `require() of native module '...' (.node) is not supported`（`require.rs:224`），
  rolldown 的 try/catch 把它吞成「Cannot find native binding…」误导文案。
- §0.5 轮子调研：host 侧 N-API 实现各家全自研（Node C++ / Bun Zig / Electron 用
  Node 本体），Rust 生态仅 addon 侧 napi-rs/napi-sys——手写为唯一路径。

## 2. 架构

- `src/napi/` 顶层模块（与 loader/builtins 平级）：
  - `include/`：vendored Node 头四件（v26.8.2，MIT 头原样；bindgen 输入 + fixture include）。
  - `sys.rs`：bindgen 生成（类型 + 常量；**函数声明 blocklist**——函数由本仓
    `#[no_mangle] pub unsafe extern "C" fn` 定义并导出，声明会造未定义引用）。
  - `env.rs`：`NapiEnv`（会话单例，JS 线程专用）——raw cx + global、值槽位 arena、
    scope 栈、last-error、fn 注册表、module 注册暂存。入 `state` 的 traced 结构
    （`Heap` 槽位可被 GC 追踪，§4.40 纪律）。
  - `value.rs`：napi_value = `*mut Heap<JS::Value>`（对 addon 完全不透明，N-API 契约
    即如此）；建/取值即 Box<Heap> 槽位进出 arena（Box 定址铁律）。
  - `scope.rs`：handle/escapable scope = arena 标记；回调返回值先拷贝到父 scope
    槽位再 truncate（Node Local 语义等价）；escape 即跨层拷贝。
  - `function.rs`：`napi_create_function` → `JS_NewFunction` + `JS_GetFunctionObject`
    + `js::SetFunctionNativeReserved`（slot0=cb 指针、slot1=data 指针，PrivateValue，
    GC 不可见）+ 单一 trampoline `__wjs_napi_trampoline`（callee=vp[0] 取回 cb/data，
    组 cbinfo 调 addon，返回值落 rval）。
  - `error.rs`：last-error per-env、throw 系、pending exception 桥接。
  - `loader.rs`：require/import `.node` 截获 → `check_ffi` → `libloading` dlopen
    （`napi_module_register` 经 constructor 暂存 + `napi_register_module_v1` 直查
    双入口）→ register 函数（env, exports）调用 → exports 返回 require 管线。
  - `uv.rs`：`uv_run` 垫片（UV_RUN_NOWAIT 单步接事件循环；M0 先 stub 返 0）
    + `napi_get_uv_event_loop` 返非空哑句柄。
- 符号导出：build.rs 按平台加链接参数；`nm -gU` 自检 + C fixture `dlsym(RTLD_DEFAULT)`
  实测（即 rolldown 的查表方式）双验证。
- TSFN（M3）：事件循环第 8 通道 `napi_rx`（net/worker 同构模式）。

## 3. 切片（每片三件套：模块单测 + 黑盒正常/报错/边界 + 冒烟 5/5）

- [x] **M0 地基**（2026-09-13 完工）：vendored 头（v26.8.2）+ bindgen sys
  （类型+常量，函数 blocklist）+ 符号导出（macOS `-Wl,-exported_symbols_list`
  glob `_napi_*`/`_uv_*`，nm 实证 22 符号 T）+ `src/napi/{mod,sys,env,api,loader}`
  + require `.node` 截获（`--allow-ffi` 门控）。
  落地面（超出 hello 子集）：create_string_utf8/int32/uint32/int64/double、
  get_undefined/null/boolean/global、get_value_double/int32/uint32/int64/bool/
  string_utf8、get_cb_info、typeof、throw_error、get_last_error_info、
  open/close_handle_scope、module_register、get_uv_event_loop、uv_run、
  fatal_error/exception + trampoline（reserved slots 携带 cb/data）+
  `.node` exports 缓存（require 幂等，`same: true` 实测）。
  验收全过：C fixture `hello 42 / add 42 / ver m0-ok`、dlsym 自检
  （rolldown 查表机制同款 11 符号全解析）、沙箱拒载 + 授后放行、
  报错可 catch（throw_error → pending exception 传播）。
  `cargo test` 437 全绿 0 失败（master 433 + napi 4），build 0 警告，冒烟 5/5。
  M0 实测三坑（§4 追补）：
  ① **缺符号 = lazy-bind 空桩 SIGSEGV**——fixture 调未实现的
  `napi_get_value_double`，dyld 惰性绑定解析不到宿主符号直接 139 无信息；
  修法=先补齐面再跑 + fixture 编译加 `-undefined dynamic_lookup`（已有），
  后续新面按"实现→fixture 同步"推进；
  ② **导出清单字面量 × dead-strip**——`-exported_symbols_list` 写字面量
  `_uv_run` 在测试二进制（dead-strip 掉无引用符号）被判未定义链接失败；
  修法=清单用 glob（`_napi_*`/`_uv_*`），存在性自检移交 dlsym fixture；
  ③ **NAPI_MODULE 宏已是符号注册**——v26.8.2 头不再走 constructor +
  `napi_module_register`（头注 deprecated），宏直接导出
  `napi_register_module_v1`；loader 的 dlsym 回落是主路径而非兜底
  （rolldown 绑定同为该符号），constructor 暂存保留为老式 addon 兼容。
- [x] **M1 值系统矩阵**（2026-09-14 完工）：`src/napi/value.rs` + `property.rs`
  + api.rs 调用面——落地面 ~35 个：create_object/array(+with_length)/
  string_utf16/latin1/symbol、get_value_string_utf16/latin1、coerce 四族
  （bool 返 napi_value，对照头文件 188 行）、instanceof/strict_equals/is_array/
  is_error（instanceof Error 近似，记档）、错误对象族（create/throw ×3，
  code 属性挂接）、get_and_clear_last_exception、get/set/has/delete 属性
  （named char\* + generic string key；symbol key M2 记档）+ 元素四件 +
  get_property_names（Object.keys 直调路线，记档）+ get_prototype +
  get_array_length + define_properties（value/method/data/attrs；getter/
  setter 随 M2 class）+ napi_call_function（N 参经 prelude
  `__wjs_napi_call` apply 展开 + glue call_three）。
  实测修正四件（对照 vendored 头逐签名审计）：create_error 族 code/msg 是
  napi_value 非 char\*；has_own/delete 的 key 是 napi_value；coerce_to_bool
  result 是 napi_value\*；create_int32 必须 Int32Value（get_value_int32 的
  to_int32 断言依赖 tag）。int64/uint32 经 f64 截断为 N-API 语义（记档）。
  bigint/date 未实现（rolldown 132 名单未引用，M4 实测需要时补）。
  fixture 矩阵：m1_values.c（15 check）/ m1_props.c（4 check），
  黑盒 2 例（values/props 全矩阵）+ M0 存量 4 例零回归。
  `cargo test` 439 全绿 0 失败，build 0 警告，冒烟过。
  坑追补：① cc 隐式函数声明即错（fixture 忘 stdio.h 时静默跑旧 dylib，
  排查走 `strings` 验产物）；② js::ToObjectSlow 断言 !isObject（对象必须
  直返，MOZ_ASSERT 实测炸）；③ jsval to_int32 断言 int32 tag（建值需
  Int32Value，读值用 to_number 兜双 tag）。
- [x] **M2 函数与类深水**（2026-09-14 完工）：cbinfo 全语义/make_callback/
  async_init+async_destroy+callback_scope、define_class/new_instance/wrap/
  unwrap/remove_wrap/external+finalize、node_api_create/throw_syntax_error、
  escapable scope（统一栈 + LIFO 校验 + escape 产物独立池由 trampoline 回收）。
  验收：finalize 释放链用例（malloc/free 成对计数，dhat 口径等价——两轮 40 万
  次灌入后 free 追上 alloc 一半、free≤alloc 恒成立；`m2_finalize.c`）。
  落地面：
  - `class.rs`：静态 JSClass 两枚全 napi 类共享（NAPI_INSTANCE/EXTERNAL，判定走
    `JS_InstanceOf` 指针比对），私有数据 reserved slots 0..3（data/finalize/hint/
    env，全 PrivateValue——double-tag，GC 不追；未写槽 = undefined，`is_double`
    即哨兵）。`JSCLASS_FOREGROUND_FINALIZE` 钉死主线程（默认 background 会在
    helper 线程跑 finalize op，addon 回调碰 NapiEnv 即数据竞争）。
  - trampoline 回调按进入水位截断主 arena（Node 契约：napi_value 仅回调存活期
    有效；同时是 finalize 链前提——槽位是 GC 根，不截断则回调产物永不可达死态）。
    escaped 池单独截断。loader register 直调不截断（exports 由 modules 表锚定）。
  - napi_set_named/set_property/set_element 走 sloppy prelude helper
    `__wjs_napi_set`（JSAPI JS_SetProperty 是 strict 语义，对只读+不可配置属性
    抛 TypeError；Node 的 napi_set_property 走 v8 非严格 set 静默返回 ok，
    m1_props 探针实测修正）。define 面保持 JSAPI（Node define 同为 strict）。
  - napi_new_instance / define_class 访问器经 prelude `__wjs_napi_new`（new 全
    语义）/`__wjs_napi_accessor`（Object.defineProperty，setter undefined =
    getter-only）——免变长 HandleValueArray 与 JSAPI 访问器旗帜位雷区。
  - `define_one`（property.rs）：define_properties/define_class 共用 descriptor
    落地（method → getter/setter 访问器 → value；static 位拆放置目标）。
  - napi_create_function/define_class 构造器函数都设 `JSFUN_CONSTRUCTOR`
    （SM native 函数默认**不可**构造，`new` 报 not a constructor，2026-09-14 实测）。
  偏差（M4 实测再议）：napi_wrap 仅限 define_class 实例（external 的 data 槽与
  wrap 单槽不混、任意对象缺 GC 驱动 finalize 通道，均 fail-fast 不静默泄漏）；
  napi_wrap 的 napi_ref 出参 M3 前报错；finalizer 在 GC sweep 内直调（Node 同期
  语义，finalizer 期间禁大多数 napi_*）；napi_typeof 的 external 判定走类比对。
  fixture：m2_class.c（类矩阵 8 面）/ m2_finalize.c（释放链），
  黑盒 +2（441 全绿 0 失败，M1 439 零回归），build 0 警告，冒烟过；
  导出符号 69 → 89（+ node_api_* 错误族 glob `_node_api_*`）。
- [x] **M3 异步与 buffer**（2026-09-14 完工，两个切片）：
  promise/deferred（`JS::NewPromiseObject(executor=null)` 即 deferred 语义，
  句柄 = Heap 槽位地址，一次性；落定走引擎标准 reaction 路径，§4.18 由循环
  拓扑保证）、refs（句柄 = Box 指针；**weak 同追偏差**——SM 无 embedder 弱值
  通道，不追即 GC 后悬垂）、arraybuffer/typedarray（11 种全走 global 构造器
  `__wjs_napi_new`；**SM 与 napi 的 Scalar::Type 序号不同**——clamped 8↔2，
  显式双向映射表）/dataview（`JS_NewDataView` + FixedLengthClassPtr 判定）/
  node Buffer（Uint8Array+Buffer.prototype；external buffer 与 external AB 同
  `JS::BufferContentsFreeFunc` 桥——Box 携带 env/cb/hint）、async_work + TSFN
  （第 8 通道 `napi_rx`，fetch/net 同构模式）。验收：真 OS 线程回调进 JS ✓
  （`m3_async.c`：async_work 线程 fib(20)=6765 经 complete→deferred→await；
  TSFN 线程 3 条消息经 call_js_cb 回 JS → release → thread_finalize 落定）。
  M3-3 踩坑四则（对照 AGENTS §4 风格）：
  ① **通道 Sender 必须随记录走**：OS 线程侧禁经 `with_plain` 取通道（TLS 是
  JS 线程的，§4.24）——Sender 在 JS 线程 create 时捕获存进 rec/shared；
  ② **TSFN 句柄必须自包含**：`Arc::into_raw` 指针即句柄（from_raw/into_raw
  严格往返），call/acquire/release/ref/unref 全零 TLS；按 id 查 env 表的
  设计在 OS 线程上必炸（state::init not called）；
  ③ **cancelled 的 complete 必达**：complete/data 嵌进事件本身——addon 在
  complete 前合法 delete_async_work 会让按 id 查表的 dispatch 扑空丢回调
  （fixture cancel+delete 实测断链）；async_pending 计数只 queue++/dispatch--，
  cancel 不动（防双减）；
  ④ **closing 的 TSFN 不再 keep-alive**：pending_count 过滤 closing（否则
  release 后循环永不退）。
  偏差（记 §4）：weak ref 钉值至 delete；SM 小 AB inline 存储 GC 可搬移，
  data 指针跨回调须重取 `napi_get_arraybuffer_info`（M4 rolldown 实测）；
  async_hooks/async_context 未接；external free 恒主线程（单 GC 线程）。
  黑盒 +2（m3_value 矩阵 / m3_async OS 线程）→ 443 全绿 0 失败，
  build 0 警告，冒烟过。
- [x] **M4 rolldown 闭环**（2026-09-14 完工）：真 rolldown 1.2.8（连带
  @rolldown/binding-darwin-arm64，自家 pm 真网络安装）跑通 transform/bundle。
  验收 ✓：rolldown 内核产出正确 bundle（双模块内联 + TS 输入类型擦除，产物
  可执行），黑盒 `phase_napi_m4_rolldown_bundle_real_network`（**真网络标注**
  `#[ignore]`，验收跑 `--ignored`，6.7s）。按 §6 名单补缺口三函数：
  `napi_add/remove_env_cleanup_hook`（`lifecycle.rs`；end_session 收敛点 LIFO
  触发，同 (fun,arg) 重复 add 拒绝）+ `napi_get_node_version`（与
  process.version 同源 CalVer，进程级快照）。
  实战揪出的四个宿主缺口（对照 M1–M3 的偏差记录全部见真章）：
  ① **napi_wrap 的 napi_ref 出参**：napi-rs 3 的 ctor 恒传非空 `&mut
  object_ref`（Reference 簿记），我们的 fail-fast 直接让全部 napi-rs 类构造
  炸出 "Failed to initialize class `constructor`"（`{constructor}` 是 napi-rs
  报错模板的占位串，非类名）——补上 Node 口径 ref（初始计数 0）；
  ② **TSFN 的 js_callback 可空**：napi-rs 的 JsDeferred 恒传 null func
  （call_js_cb 内直接 resolve deferred）——放宽校验；
  ③ **register 直调遗留 pending**：loader 对 register 期 pending 异常显式
  检测 + 上抛（此前静默吞掉会污染后续所有 JSAPI 面）；
  ④ M0 的 `--allow-env` 权限面被 rolldown prelude 实际触发
  （NAPI_RS_WASI_FLAVOR 查询），权限门控行为符合预期。
  调试纪律：临时 eprintln 探针（`[wdbg]` 前缀，对照 §4.57 vm_dbg 惯例）用完
  即删，`grep -rn wdbg src/` 为空后提交。
- [x] **M5 vite 全链**（2026-09-15 完工，终线三命令全绿）：dev server（HMR ws
  服务端面）+ vite build + vitest（tinypool/worker_threads 9f 底座）。
  - 进展（2026-09-14）：build 黑盒已落（HEAD）；dev 前置缺口收敛中——upgrade
    派发 + Socket 流桩 + `fs.watchFile` 纯 JS 轮询 + TSFN pending 守卫 +
    Buffer 整数读写系（HMR error 推送根因）；
    polling 后端 dev 全链已验证（listen/transform/WS/full-reload/CLOSED，
    ignored 黑盒 `phase_napi_m5_vite_dev_polling_real_network` 落账）；
    fsevents 默认路径 139 已修复（`NapiEnv::trace` 漏标 `tsfns.js_cb`，
    见 AGENTS §4.68；`hmr-min9.mjs` 修后 full-reload + `EXIT:0`）；
    vitest 双池全绿（threads/forks 跑 basic.test.js 均 pass、EXIT:0；仅剩收尾
    close timed out / 2 Vite servers 妆饰警告）；
    node 黑盒 8 件落账（console/subpath-timers/statfs/stdio/worker-stdio/
    cjs-named/fork-ipc/fork-errors，`tests/node/` 下 src 对齐 40 文件，
    116 例全绿）；fork 缺失模块 hang 根因收敛（入口失败 + 开端口永不收割，
    见 AGENTS §4.70；control 分支补 `parentPort.close()`；assert 默认形态
    对真机五项，见 §4.71）。
  - 进展（2026-09-15，终线①收官 + ②遇阻）：
    - **终线① ignored 三件全绿**：`cargo test --test napi -- --ignored`
      （M4 rolldown / M5 build / M5 dev polling，39.5s，0 failed）。
    - **终线② vue-project**：`-I --yes` 依赖全装 ✓（vite 8.3.0/vitest 4.1.11/
      vue 3.5.42 全家 + .bin + lockfile，自家 pm 真网络）。三命令中
      `-r dev`/`-r build` 阻塞于**深水 bug**（下条）；`-r test` 无该名脚本
      （scripts 为 `test:unit`，实际验证走 vitest 腿）。
    - **深水 bug（未解，证据链齐）**：vite resolveConfig/createServer 崩
      138/139 随 GC 时序漂移；bisect 定位——**必要条件 = JS 插件钩子 × TSFN
      桥**（无插件全量 bundle 6/6 过；同款 rolldown 调用 + 外部化钩子 10/10
      过；钩子返回 null 走默认解析即崩）。lldb 实锤：活解释器帧引用
      **全零 cell**（GC 已回收）+ map 字被覆写（非对齐 + 怪 tag
      0x5800/0xd800/0xf800 高位 + 合法低位堆址）；非 JIT 专属（关 JIT 照崩）。
      本轮落四项加固（全量 458 绿零回归）：① napi 建面 AB 稳定存储（§4.75）；
      ② napi_wrap ref 出参两路同发（§4.76）；③ finalizer 延迟收敛（§4.78，
      GC sweep 内只入队）；④ call_impl func 形态先验（§4.79）。
      另 §4.77 pin-all 反例实证（槽位不截断 = finalizer 永不跑 = external
      内存无底洞），已回退截断。**下一会话入口**：rolldown-binding 的 TSFN
      载荷生命周期审计（`data` 指针谁释放、跨线程时序）+ lldb watchpoint
      抓覆写者；探针资产：`/Volumes//Projects/vue-project/bisect*.mjs`
      （E=过/F2=崩/H=过/C=崩）、`/tmp/wjs-vite-probe` 旧探针（已清）。
  - 进展（2026-09-15 终，终线②收官）：
    - **深水 138/139 根治（两连根因）**：① TSFN drain 循环跨 GC 持 js_cb
      栈位拷贝（05fcd6d，修后 F2 30/30）；② `require_cjs_file` 五连柯里化
      链裸 JSVal 栈拷贝跨 call_one（c2470fe，§4.80；lldb 实锤 worker 线程
      加载 css-tree 深图时 cur/arg 位型 0xFFF8/0x5800 垃圾写穿）——
      require 条件族让 CJS 图暴增才把该潜在 UB 养出。修后 vitest 全链通。
    - **require 条件族**（c2470fe）：resolver 按 import/require 分流双单例；
      真机对拍 imports-only 包 require 报错（§4.81）；magic-string 双条件包
      命中 CJS 入口。附带 require(esm)+detect-module 语义升级的旧断言翻转
      （§4.82）+ `require.resolve` 直挂原生（scripted caller 帧修正）。
    - **生态全局面**（055dd1b）：DOMException（legacy code getter）/
      File/MessageChannel·MessagePort 全局 + MessagePort.onmessage（赋值即
      开闸）/SAB+Atomics（主域+vm context）/vm DONT_CONTEXTIFY（真机 24+
      口径，新 native `__wjs_vm_global`）/path.toNamespacedPath/buffer
      isAscii·isUtf8（真机全集就两个，超集已删）。
    - **fs 无编码读返回 Buffer**（7657d65，§4.83）：裸 Uint8Array 的
      String() join 位型把 vite PostCSS 配置加载（JSON.parse(buf)）炸出
      column 4 假错——Node 语义收口 `Buffer.from`。
    - **终线②验证**（对等口径）：`-r build` exit 0（44 模块，产物 99.58 kB
      gzip 38.51）；`-r dev` exit 0（VITE ready/HTTP 200/TS transform/HMR
      full-reload）；`-r test:unit`——HelloWorld.spec.ts（jsdom 环境）过；
      hello.test.js（init 模板的 node:test）真机 node 26.8.2 同败
      （"No test suite found" exit 1）→ 汇总两侧逐行同（1 failed | 1 passed），
      挪开该 fixture 本仓 vitest **exit 0 全绿**。唯一残留差异：本仓多
      "close timed out" 妆饰警告（§4.67 记档，无碍）。
- [x] **M6 收尾合流**（2026-09-15 完工）：
  - Node 官方套件选点回归：`test/js-native-api` 的 2_function_arguments +
    3_callbacks **原文 verbatim** 入库（official/ 布局镜像，common.h/
    common-inl.h/entry_point.h 同源 vendored，MIT 头原样）——编译即验
    vendored 头忠实度；断言取官方 test.js 子集（strict 驱动 + node:assert；
    recv 原始值经 strict 函数直通，真机对拍）。揪出并修掉
    `napi_get_cb_info` 余槽填 undefined 硬契约（§4.84，node `Args()` 原文
    实锤）。
  - AGENTS §6 审计口径补 napi 面（~133 `unsafe extern "C"` 结构性新增，
    不逐个计数）。
  - 黑盒清单：`tests/napi.rs`（15 例：M0–M3 fixture 矩阵 + M4 rolldown
    真网络 + M5 build/dev polling 真网络 + M6 官方选点）。
  - 收官计数（2026-09-15）：`cargo test` 466 全绿 0 失败，build 0 警告，
    冒烟 5/5；vue-project 三命令对等全绿 → 长分支一次性合 master。

## 4. 风险与纪律

- GC 三连：`Box<Heap>` 定址（§4.40）/ 结算点后保 RunJobs（§4.18/4.46）/
  禁裸指针存 JS 值（保留槽/traced 结构）。napi_value 原始指针只在 scope 存活期内
  有效——addon 违约即 UB，与 Node 契约一致（记档）。
- M0 追补坑：缺符号 lazy-bind 空桩 SIGSEGV（先补面再跑）；
  导出清单禁字面量（dead-strip × 清单校验冲突，用 glob）；
  NAPI_MODULE 符号注册为主路径（dlsym 回落升级为主入口）。
- `NapiEnv` 跨 napi 调用存续（Node 同语义：addon 在 Init 存 env 复用）→ 会话单例 +
  init_session 清表（§4.24 哲学）；JS 线程专用，TSFN 跨线程必须在 JS 线程侧才调 napi_*。
- 符号导出失败 = 整个 napi 不可用：nm 自检 + fixture dlsym 双保险进黑盒。
- 平台：macOS 先行；Linux `-rdynamic` 同路径；Windows 导出（dllexport/.def）留 CI。
- 真网络测试标注：rolldown e2e 用自家 pm 安装（pm 真装测试同款隔离）。

## 5. 测试资产

- C fixture：`tests/fixtures/napi/*.c`（手写 napi addon，include vendored 头，
  `tests/common` 扩 dylib 编译 helper 带 `-I`）；node 官方头 MIT 头原样保留。
- Node 套件：`test/addons/` 按需单文件取（M6）。
- rolldown e2e：真包经 `winterjs -a` 安装（真网络，标注）。

## 6. rolldown 引用的 napi 符号名单（132，2026-09-13 strings 实测）

- 全集见 `docs/rolldown-napi-symbols.txt`（绑定二进制 strings 提取，去重后 132 项；
  含少量非函数串如 `napi_ok`/`napi_value`——以 node_api.h 标准面为准实现，
  本名单为 M4 缺口对照的大纲）。
- 标准面之外的观察：**无 bigint / date / type_tag / get_all_property_names** 系列
  （rolldown 查表未含）——M4 深水区比标准面预估更小。
- libuv 面仅 `uv_run` 一个符号；另有 `napi_get_uv_event_loop`、
  `napi_module_register`、`napi_register_module_v1`、`napi_get_node_version`。
