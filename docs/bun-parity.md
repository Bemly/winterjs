# Bun 高度对拍报告（Phase 10f）

> 方法：Node `test/parallel` 子集逐模块点名（nodejs/node `v26.8.2`，
> `/tmp/wjs-node10f` sparse 检出，仓库内不留套件）。
> 跑法：`winterjs --run <file>` 与 `node <file>` 同条件
> （`cwd=test/parallel`，`NODE_SKIP_FLAG_CHECK=1`，20s 超时），**只比退出码**；
> `--run` 经典脚本 completion 回显（`[object Promise]` 等）为既有行为，不计入。
> 跳过类（不红不绿）：`--expose-internals` 件（引擎内部面）、自 spawn 位置参数件
> （全 flag CLI 设计）、需外网/特殊权限件。
> 偏离类：跨引擎不可比（栈格式/V8 内省），书面记录。
> 每修一项，断言原文进三件套回归（模块单测 + `tests/node/<mod>.rs` 黑盒）。

## 图例

- ✅ 点名文件全绿；🟡 部分绿（红项逐条列理由）；🔴 全红；⏭️ 整文件跳过（理由）。

## events

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-events-once.js | 0 | 0 | ✅（修：abort 侧表、`rejects` 收 promise，见 §4 候选） |
| test-events-list.js | 0 | 0 | ✅ |
| test-events-getmaxlisteners.js | 0 | 0 | ✅（修：EventTarget 默认/AbortSignal 0、具名导出补齐） |
| test-events-listener-count-with-listener.js | 0 | 0 | ✅ |
| test-events-customevent.js | ≠0 | ≠0 | ⏭️ `--expose-internals`（`internal/event_target`） |
| test-events-static-geteventlisteners.js | ≠0 | ≠0 | ⏭️ `--expose-internals` |
| test-events-on-async-iterator.js | ≠0 | ≠0 | ⏭️ `--expose-internals` |
| test-events-uncaught-exception-stack.js | 1 | 0 | 🟡 偏离：Error 栈格式引擎口径（SM `fn@file:line:col` vs V8 `at` 形） |

## querystring / punycode / string_decoder

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-querystring.js | 0 | 0 | ✅ |
| test-querystring-escape.js | 0 | 0 | ✅ |
| test-querystring-multichar-separator.js | 0 | 0 | ✅ |
| test-punycode.js | 0 | 0 | ✅（修：DEP0040、`url` 懒加载） |
| test-string-decoder.js | 0 | 0 | ✅（修：utf16 hold 模型/`lastChar`/`text()`/超限门） |
| test-string-decoder-end.js | 0 | 0 | ✅（修：base64url 输出字母表） |
| test-string-decoder-fuzz.js | 0 | 0 | ✅（修：ascii 掩码——实为 Buffer 侧） |
| test-string-decoder-utf8-large.js | 0 | 0 | ✅ |

## buffer

> 10f 收官：72 点名 = 63 ✅ + 9 ⏭️（6 件 expose-internals/allow-natives 类 + 2 件
> 自 spawn 裸文件参数 + 1 件 zero-fill flag），红 0。本轮改动主体：Buffer 全局
> 按 node lib/buffer.js v26.8.2 逐字移植（prelude）+ 模块面 exports 对齐
> （SlowBuffer 移除/kMaxLength=MAX_SAFE_INTEGER/INSPECT_MAX_BYTES 具名 +
> transcode/isUtf8/isAscii/btoa/atob/constants/File/Blob）；
> 伪 AB 品牌拒收（`void v.byteLength` 试探）、`structuredClone` transfer-detach
> （Rust `DetachArrayBuffer`）、小串池化（64KB 池/8 字节对齐/满即新池 +
> `transfer()` 抛 TypeError + worker 拒收 DataCloneError(25)）、
> `Uint8Array` Proxy 透传 `newTarget`（子类化不断链，stream fromWeb 回归）、
> `util.inspect` depth -1 空容器显体 + 函数直通（`{__proto__:null}` 空回
> `[Object: null prototype] {}`，`from` 套件门）。
> 偏离维持记档：`allocUnsafe` 恒零填（无未初始化内存暴露）、pool 仅 from(string)
> 小串（alloc 系未池化）、`alignment` 形参不做。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-buffer-alloc-alignment.js | 1 | 1 | ⏭️ `--expose-internals`（`internal/test/binding`） |
| test-buffer-alloc-unsafe-is-initialized-with-zero-fill-flag.js | 0 | 0 | ✅ |
| test-buffer-alloc-unsafe-is-uninitialized.js | 0 | 0 | ✅ |
| test-buffer-alloc.js | 0 | 0 | ✅ |
| test-buffer-arraybuffer.js | 0 | 0 | ✅（修：伪 AB 品牌拒收 `an instance of AB`） |
| test-buffer-ascii.js | 0 | 0 | ✅ |
| test-buffer-backing-arraybuffer.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-buffer-badhex.js | 0 | 0 | ✅ |
| test-buffer-bigint64.js | 0 | 0 | ✅ |
| test-buffer-bytelength.js | 0 | 0 | ✅ |
| test-buffer-compare-offset.js | 0 | 0 | ✅ |
| test-buffer-compare.js | 0 | 0 | ✅ |
| test-buffer-concat.js | 0 | 0 | ✅ |
| test-buffer-constants.js | 0 | 0 | ✅（修：kMaxLength=MAX_SAFE_INTEGER，真机 26 口径） |
| test-buffer-constructor-deprecation-error.js | 0 | 0 | ✅ |
| test-buffer-constructor-node-modules-paths.js | 1 | 0 | ⏭️ 自 spawn 裸文件参数（全 flag CLI 设计） |
| test-buffer-constructor-node-modules.js | 1 | 0 | ⏭️ 自 spawn 裸文件参数（全 flag CLI 设计） |
| test-buffer-constructor-outside-node-modules.js | 0 | 0 | ✅ |
| test-buffer-copy-immutable.js | 0 | 0 | ✅ |
| test-buffer-copy.js | 0 | 0 | ✅ |
| test-buffer-equals.js | 0 | 0 | ✅ |
| test-buffer-failed-alloc-typed-arrays.js | 0 | 0 | ✅ |
| test-buffer-fakes.js | 0 | 0 | ✅ |
| test-buffer-fill.js | 1 | 1 | ⏭️ `--expose-internals`（`internal/errors`） |
| test-buffer-from.js | 0 | 0 | ✅（修：空 null-proto `inspect` depth -1 + `copyBytesFrom` 门） |
| test-buffer-generic-methods.js | 0 | 0 | ✅ |
| test-buffer-includes.js | 0 | 0 | ✅ |
| test-buffer-indexof.js | 0 | 0 | ✅ |
| test-buffer-inheritance.js | 0 | 0 | ✅ |
| test-buffer-inspect.js | 0 | 0 | ✅ |
| test-buffer-isascii.js | 0 | 0 | ✅（修：`structuredClone` transfer-detach，视空为真） |
| test-buffer-isencoding.js | 0 | 0 | ✅ |
| test-buffer-isutf8-isascii-fast.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-buffer-isutf8.js | 0 | 0 | ✅（修：同 isascii） |
| test-buffer-iterator.js | 0 | 0 | ✅ |
| test-buffer-new.js | 0 | 0 | ✅ |
| test-buffer-no-negative-allocation.js | 0 | 0 | ✅ |
| test-buffer-nopendingdep-map.js | 0 | 0 | ✅ |
| test-buffer-of-no-deprecation.js | 0 | 0 | ✅ |
| test-buffer-over-max-length.js | 0 | 0 | ✅ |
| test-buffer-parent-property.js | 0 | 0 | ✅ |
| test-buffer-pending-deprecation.js | 0 | 0 | ✅ |
| test-buffer-pool-untransferable.js | 0 | 0 | ✅（修：小串池化共享 + 拒收 25/TypeError） |
| test-buffer-prototype-inspect.js | 0 | 0 | ✅ |
| test-buffer-read.js | 0 | 0 | ✅ |
| test-buffer-readdouble.js | 0 | 0 | ✅ |
| test-buffer-readfloat.js | 0 | 0 | ✅ |
| test-buffer-readint.js | 0 | 0 | ✅ |
| test-buffer-readuint.js | 0 | 0 | ✅ |
| test-buffer-resizable.js | 0 | 0 | ✅ |
| test-buffer-safe-unsafe.js | 0 | 0 | ✅ |
| test-buffer-set-inspect-max-bytes.js | 0 | 0 | ✅（修：INSPECT_MAX_BYTES 具名导出 live） |
| test-buffer-sharedarraybuffer.js | 0 | 0 | ✅ |
| test-buffer-slice.js | 0 | 0 | ✅ |
| test-buffer-slow.js | 0 | 0 | ✅（修：SlowBuffer 移除，真机 26 `undefined`） |
| test-buffer-swap-fast.js | 1 | 1 | ⏭️ `--expose-internals` + `--allow-natives-syntax` |
| test-buffer-swap.js | 0 | 0 | ✅ |
| test-buffer-tojson.js | 0 | 0 | ✅ |
| test-buffer-tostring-4gb.js | 0 | 0 | ✅ |
| test-buffer-tostring-range.js | 0 | 0 | ✅ |
| test-buffer-tostring-rangeerror.js | 0 | 0 | ✅ |
| test-buffer-tostring.js | 0 | 0 | ✅ |
| test-buffer-write-fast.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-buffer-write-utf8-two-byte.js | 0 | 0 | ✅ |
| test-buffer-write.js | 0 | 0 | ✅ |
| test-buffer-writedouble.js | 0 | 0 | ✅ |
| test-buffer-writefloat.js | 0 | 0 | ✅ |
| test-buffer-writeint.js | 0 | 0 | ✅ |
| test-buffer-writeuint.js | 0 | 0 | ✅ |
| test-buffer-zero-fill-cli.js | 0 | 1 | ⏭️ 需 `--zero-fill-buffers` flag（未实现；本仓 allocUnsafe 恒零填，无 flag 即过，真机无 flag 即挂） |
| test-buffer-zero-fill-reset.js | 0 | 0 | ✅ |
| test-buffer-zero-fill.js | 0 | 0 | ✅ |

## util

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-util-format.js | 1 | 0 | 🟡 差集：`%o` 多行布局引擎（breakLength/缩进/`<ref*>`/函数内部）未做；null-proto 构造名需 V8 map 内省（引擎边界）。已修：numericSeparator、depth 方向、`%s` 内建判定、数组额外属性 |
| test-util-isdeepstrictEqual.js | 0 | 0 | ✅（修：装箱槽判定、skipPrototype、typed 附加键） |
| test-util-inherits.js | 0 | 0 | ✅ |
| test-util-inspect.js | ≠0 | ≠0 | ⏭️ `--expose-internals` |
| test-util-promisify.js | ≠0 | ≠0 | ⏭️ `--expose-internals` |
| test-util-callbackify.js | 1 | 0 | 🟡 差集：execFile 自 spawn 用位置参数（全 flag CLI 设计）；纯语义已对（含 falsy 文案修正） |
| test-util-deprecate.js | ≠0 | ≠0 | ⏭️ `--expose-internals`（`internal/util`，无 flag 时真机亦挂） |

## path

> 10f 收官：posix/win32 全按 Node `lib/path.js` 直译
> （`normalizeString` 核心 + validateString/validateObject 精确码）；
> `matchesGlob` 为手写 minimatch 子集（H 方案，142 条差分探针零分歧）。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-path-basename.js | 0 | 0 | ✅（修：多尾分隔符全剥/后缀整吞回退，见 §4 候选） |
| test-path-dirname.js | 0 | 0 | ✅（修：UNC 前导双条保留/win32.dirname 直译/`//a` 保 `//`） |
| test-path-extname.js | 0 | 0 | ✅（修：`..` 无 ext） |
| test-path-isabsolute.js | 0 | 0 | ✅ |
| test-path-posix-exists.js | 0 | 0 | ✅ |
| test-path-win32-exists.js | 0 | 0 | ✅ |
| test-path-posix-relative-on-windows.js | 0 | 0 | ✅ |
| test-path-win32-normalize-device-names.js | 0 | 0 | ✅ |
| test-path-join.js | 0 | 0 | ✅（修：空段过滤/尾斜杠/win32 首部防 UNC 误判，直译） |
| test-path-normalize.js | 0 | 0 | ✅（修：drive 相对/CVE-2024-36139/保留字，直译） |
| test-path-resolve.js | 0 | 0 | ✅（修：win32 drive 继承/尾斜杠，直译） |
| test-path-relative.js | 0 | 0 | ✅（修：win32 大小写等价/UNC，直译） |
| test-path-parse-format.js | 0 | 0 | ✅（修：parse 单遍直译/_format/validateObject 精确文案） |
| test-path-makelong.js | 0 | 0 | ✅（修：非 string 原样穿透） |
| test-path-zero-length-strings.js | 0 | 0 | ✅（修：空串 join/normalize，同直译） |
| test-path-glob.js | 0 | 0 | ✅（H 手写 minimatch 子集：60+ 探针对拍零分歧，见 §4 候选） |

## os

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-os.js | 0 | 0 | ✅（修：tmpdir 动态/env 优先级/homedir 动态/constants/priority/endian/machine/version/devNull/buffer/cidr/toPrimitive） |
| test-os-eol.js | 0 | 0 | ✅（修：EOL 非写可配 + 重定义） |
| test-os-process-priority.js | 0 | 0 | ✅（修：priority 常量/get/set/双校验/SystemError；附带修 assert.throws 对象正则） |
| test-os-constants-signals.js | 0 | 0 | ✅（signals 冻结实在化；旧偶然通过） |
| test-os-checked-function.js | ≠0 | ≠0 | ⏭️ `--expose-internals`（`internal/test/binding`） |
| test-os-fast.js | ≠0 | ≠0 | ⏭️ `--expose-internals` + V8 natives（fast API 计数） |
| test-os-homedir-no-envvar.js | 1 | 0 | ⏭️ 自 spawn 位置参数（全 flag CLI 设计） |
| test-os-userinfo-handles-getter-errors.js | 1 | 0 | ⏭️ 自 spawn `-e`（全 flag CLI 设计） |

## assert

> 行为件全修（跨域重抛/参数校验/构造器校验/rejects 双收/throws 正则）；
> message 文本子系统为书面偏离（Node diff 渲染引擎；操作符/actual/expected/code 结构一致）。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-assert.js | 1 | 0 | 🟡 差集：message 文本（diff 渲染/截断/printf 展开）；行为已对 |
| test-assert-async.js | 1 | 0 | 🟡 同上 |
| test-assert-fail.js | 1 | 0 | 🟡 同上 |
| test-assert-if-error.js | 1 | 0 | 🟡 同上 |
## timers

> 10f 收官：59 点名 = 45 ✅ + 14 ⏭️（12 件 internalBinding/expose-internals 类 + 2 件
> 自 spawn 位置参数），红 0。本轮改动主体：Timeout 真语义（unref/ref/hasRef/refresh/
> close/[Symbol.dispose]/_destroyed/this 绑定 + node 原文 delay 钳制与三态警告）、
> uncaughtException 路由、ALS/域的登记期捕获、scheduler 形态、socket.setTimeout
> 真实现；事件循环 park 唤醒集与存活判定集分家（AGENTS §4.94）。
> 偏离维持记档：`setImmediate` check 阶段近似（plan3 §4）、refresh-after-fire
> 不重臂、socket.setTimeout 无活动重置（整收口径）。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-timers-api-refs.js | 0 | 0 | ✅（修：三清同体——delete 全局后 clearInterval/clearImmediate 不二次解引用） |
| test-timers-args.js | 0 | 0 | ✅ |
| test-timers-async-store-leak.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-clear-null-does-not-throw-error.js | 0 | 0 | ✅ |
| test-timers-clear-object-does-not-throw-error.js | 0 | 0 | ✅ |
| test-timers-clear-timeout-interval-equivalent.js | 0 | 0 | ✅ |
| test-timers-clearImmediate-als.js | 0 | 0 | ✅（修：ALS capture/restore 挂载点） |
| test-timers-clearImmediate.js | 0 | 0 | ✅ |
| test-timers-destroyed.js | 0 | 0 | ✅（修：_destroyed 生命周期（clear/fire 置位，interval 例外）） |
| test-timers-dispose.js | 0 | 0 | ✅（修：Symbol.dispose/close（= clearTimeout）） |
| test-timers-fast-calls.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-immediate-promisified.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-immediate-queue-throw.js | 0 | 0 | ✅（修：uncaughtException 路由 + 域登记期捕获路由（origin/双 handler 全对）） |
| test-timers-immediate-queue.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-immediate-unref-nested-once.js | 0 | 0 | ✅（修：unref'd immediate 不续命） |
| test-timers-immediate-unref-simple.js | 0 | 0 | ✅（修：unref'd immediate 不续命） |
| test-timers-immediate-unref.js | 0 | 0 | ✅（修：immediate unref 真语义 + hasRef） |
| test-timers-immediate.js | 0 | 0 | ✅ |
| test-timers-interval-promisified.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-interval-throw.js | 0 | 0 | ✅（修：interval 抛错续排 + uncaught 双次） |
| test-timers-invalid-clear.js | 0 | 0 | ✅ |
| test-timers-linked-list.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-max-duration-warning.js | 0 | 0 | ✅ |
| test-timers-nan-duration-emit-once-per-process.js | 0 | 0 | ✅ |
| test-timers-nan-duration-warning-promises.js | 0 | 0 | ✅ |
| test-timers-nan-duration-warning.js | 1 | 0 | ⏭️ 自 spawn 位置参数（`spawnSync(execPath,[file,arg])`，全 flag CLI 设计） |
| test-timers-negative-duration-warning-emit-once-per-process.js | 0 | 0 | ✅ |
| test-timers-negative-duration-warning.js | 1 | 0 | ⏭️ 自 spawn 位置参数（`spawnSync(execPath,[file,arg])`，全 flag CLI 设计） |
| test-timers-nested.js | 0 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-next-tick.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-non-integer-delay.js | 0 | 0 | ✅（修：1.1ms 重排 + 顺序门） |
| test-timers-not-emit-duration-zero.js | 0 | 0 | ✅ |
| test-timers-now.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-ordering.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-process-tampering.js | 0 | 0 | ✅（修：process 方法 this 基（common 载入期捕获，§4.97）） |
| test-timers-promises-scheduler.js | 0 | 0 | ✅（修：Scheduler 类（ERR_ILLEGAL_CONSTRUCTOR/ERR_INVALID_THIS）+ PromiseReject 残留（§4.96）） |
| test-timers-promises.js | 0 | 0 | ✅（修：去 default——require(esm) namespace 与 .promises 同一对象） |
| test-timers-refresh-in-callback.js | 0 | 0 | ✅ |
| test-timers-refresh.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-reset-process-domain-on-throw.js | 0 | 0 | ✅（修：域 error 路由（e.domain）+ process.domain 两态（null/undefined）） |
| test-timers-same-timeout-wrong-list-deleted.js | 0 | 0 | ✅ |
| test-timers-setimmediate-infinite-loop.js | 0 | 0 | ✅ |
| test-timers-socket-timeout-removes-other-socket-unref-timer.js | 0 | 0 | ✅（修：socket.setTimeout 真实现（单发 timeout 事件不关连接，内部 timer 恒 unref）） |
| test-timers-this.js | 0 | 0 | ✅（修：回调 this=Timeout/Immediate 实例） |
| test-timers-throw-when-cb-not-function.js | 0 | 0 | ✅（修：validateCallback → ERR_INVALID_ARG_TYPE） |
| test-timers-timeout-promisified.js | 1 | 1 | ⏭️ internalBinding（`internal/test/binding`/`internal/linkedlist`，--expose-internals 类） |
| test-timers-timeout-to-interval.js | 0 | 0 | ✅ |
| test-timers-timeout-with-non-integer.js | 0 | 0 | ✅ |
| test-timers-to-primitive.js | 0 | 0 | ✅（修：字符串 id 清除（`${+t}`）） |
| test-timers-uncaught-exception.js | 0 | 0 | ✅（修：uncaught 路由（后继定时器照跑）） |
| test-timers-unenroll-unref-interval.js | 0 | 0 | ✅（修：`_onTimeout`/`_idleTimeout=-1` live 门（缺 close 补 close）） |
| test-timers-unref-throw-then-ref.js | 0 | 0 | ✅（修：uncaught（once）+ ref 恢复） |
| test-timers-unref.js | 0 | 0 | ✅（修：unref 真语义（next_wake/存活分家 + progressed 不含 unrefed，§4.94）） |
| test-timers-unrefd-interval-still-fires.js | 0 | 0 | ✅（修：next_wake（unrefed 到点须醒）） |
| test-timers-unrefed-in-beforeexit.js | 0 | 0 | ✅ |
| test-timers-unrefed-in-callback.js | 0 | 0 | ✅（修：触发期 unref 侧账 unrefed_ids（entry 已摘表，§4.94）） |
| test-timers-user-call.js | 0 | 0 | ✅（修：Reflect.apply 直调内建（回调 .call/.apply 可被补丁，§4.95）） |
| test-timers-zero-timeout.js | 0 | 0 | ✅ |
| test-timers.js | 0 | 0 | ✅（修：溢出钳 1 非上限（>2^31-1 按下一跳，原文直译）） |

## url

> 10f 收官：18 点名 = 13 ✅ + 5 ⏭️（3 件 URLPattern 整面未实现（node 26 内建 ada 系，
> 体量另案）+ 1 件自 spawn 裸文件参数 + 1 件 isURL 内部面双 1 对齐），红 0。本轮改动主体：legacy parse/format/
> resolve/resolveObject 按 node lib/url.js 逐字对齐（首尾修剪扫描器/nonHost 扫描/
> getHostname/parseHost/autoEscapeStr/noEscapeAuth 表/escapedCodes 表）、IDNA
> 校验（NFKC + ignored 软连字符 + 违禁扫描 + punycode，badIDNA 29 码点全拦）、
> pathToFileURL windows 选项全套（扩展 UNC/UNC hostname 状态机/盘符/posix 尾
> 分隔符，__encodePathSegment 按 ada file-path 表）、fileURLToPath posix 只查
> %2F、DEP0169 警告（once + isInsideNodeModules 栈走查）、urlToHttpOptions 逐字。
> 连带基建（对拍牵引）：assert.throws 函数形期望的 instanceof 门（§4.99）、
> ChildProcess exit/close 双参 (code, signal)、spawn 默认 stdio pipe、stdout/
> stderr legacy Readable 面（§4.101）、process.emitWarning 异步派发（§4.102）。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-url-domain-ascii-unicode.js | 0 | 0 | ✅（修：testmod 三参形态 test(name,{skip},fn)——选项对象此前被当 fn 收队） |
| test-url-format-whatwg.js | 0 | 0 | ✅（修：WHATWG format options Boolean 化逐件剥离） |
| test-url-format.js | 0 | 0 | ✅（修：noEscapeAuth 0x70 行抄错 + auth 代理对整体编码（encodeURIComponent 对孤立代理抛 URIError）） |
| test-url-fileurltopath.js | 0 | 0 | ✅（修：posix 路径误查 %5C——%5C 检查属 win32 专查，posix 下 file:///foo%5Cbar 合法） |
| test-url-invalid-file-url-path-input.js | 0 | 0 | ✅（修：%2F 抛 ERR_INVALID_FILE_URL_PATH + input 挂 URL 对象） |
| test-url-parse-format.js | 0 | 0 | ✅（修：IDNA 用 NFKC（规范组合回预组合 ü，xn--bcher-kva）+ escapedCodes 表 100-119 行多一空串整体错位） |
| test-url-parse-invalid-input.js | 0 | 0 | ✅（修：IDNA 拒绝族 + assert.throws 箭头校验器门（§4.99）+ spawn 默认 stdio pipe） |
| test-url-parse-query.js | 0 | 0 | ✅（修：parse 空白修剪） |
| test-url-relative.js | 0 | 0 | ✅（修：resolveObject 空源短路 `if (!source) return relative` + noLeadingSlashes 块（非斜杠协议相对路径爬升进 host）+ relative.host 抬升目标） |
| test-url-pathtofileurl.js | 0 | 0 | ✅（修：windows 选项全套 + __encodePathSegment（^|[|]~ 编码；encodeURIComponent 对 ~ 放行须手动 %7E，§4.98）+ UNC tail 反斜杠转分隔符） |
| test-url-urltooptions.js | 0 | 0 | ✅（urlToHttpOptions 逐字移植） |
| test-url-revokeobjecturl.js | 0 | 0 | ✅（revokeObjectURL/createObjectURL 缺参 ERR_MISSING_ARGS） |
| test-url-format-invalid-input.js | 0 | 0 | ✅ |
| test-url-is-url-internal.js | 1 | 1 | ⏭️ isURL 内部面（`internal/url` 同义双 1 对齐，不展开） |
| test-urlpattern.js | 1 | 1 | ⏭️ URLPattern 未实现（node 26 内建，整面另案） |
| test-urlpattern-types.js | 1 | 1 | ⏭️ URLPattern 未实现（同上） |
| test-urlpattern-invalidthis.js | 1 | 1 | ⏭️ URLPattern 未实现（同上） |
| test-url-parse-deprecation.js | 1 | 1 | ⏭️ 自 spawn 裸文件参数（fixtures/node_modules 直跑，全 flag CLI 设计；DEP0169 警告部分已绿——expectWarning 序列通过，仅 spawn 断言红） |

## diagnostics_channel

> 10f 收官：69 点名 = 37 ✅ + 2 ⏭️（`--expose-gc`）+ 30 🟡偏离，红 0（红即修或记 §4，
> 无第三种状态）。本轮改动主体：订阅者/transform 抛错改走 `__dcUncaught`
> （nextTick 内探 `uncaughtException` 监听，有则 emit——裸 throw 经本仓 microtask
> 会变 unhandled rejection，与真机对不上）+ `AsyncLocalStorage.enterWith`
> （文档化主入口，`enter` 同语义）+ `Channel[Symbol.hasInstance]` 空值 V8 文案桥。
> 偏离三类：① http/http2/net/child_process/module/udp 外的跨模块插桩（各通道事件
> 需对应模块在收发路径 publish，本仓无——http/net 对拍另案时可顺带点名）；
> ② `tracing-channel-promise-run-stores`（`await setTimeout` 后 store 保持——ALS
> 跨 await/microtask 传播不支持，`async_hooks.rs` 头注既有记档）；
> ③ `tracing-channel-promise-unhandled`（需运行时 `unhandledRejection` 事件面：
> 本仓 rejection 只在循环尾收割报 fatal，从不 emit——另案，不在 dc 切片内伪造
> 快照语义）。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-diagnostics-channel-bind-store.js | 0 | 0 | ✅（修：transform 抛错 `__dcUncaught` 路由） |
| test-diagnostics-channel-bounded-channel-run-transform-error.js | 0 | 0 | ✅ |
| test-diagnostics-channel-bounded-channel-run.js | 0 | 0 | ✅ |
| test-diagnostics-channel-bounded-channel-scope-error.js | 0 | 0 | ✅ |
| test-diagnostics-channel-bounded-channel-scope-nested.js | 0 | 0 | ✅ |
| test-diagnostics-channel-bounded-channel-scope-transform-error.js | 0 | 0 | ✅ |
| test-diagnostics-channel-bounded-channel-scope.js | 0 | 0 | ✅ |
| test-diagnostics-channel-bounded-channel.js | 0 | 0 | ✅ |
| test-diagnostics-channel-child-process.js | 1 | 0 | 🟡 偏离：需 child_process 收发插桩（`childprocess.*` 通道） |
| test-diagnostics-channel-gc-maintains-subcriptions.js | 1 | 1 | ⏭️ `--expose-gc`（`queryObjects`） |
| test-diagnostics-channel-gc-race-condition.js | 1 | 1 | ⏭️ `--expose-gc` |
| test-diagnostics-channel-has-subscribers.js | 0 | 0 | ✅ |
| test-diagnostics-channel-http-server-start.js | TIMEOUT | 0 | 🟡 偏离：需 http server 插桩（事件永不到，hang） |
| test-diagnostics-channel-http.js | TIMEOUT | 0 | 🟡 偏离：需 http client/server 双插桩 |
| test-diagnostics-channel-http2-client-stream-body-multiple-buffers-and-strings.js | 1 | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-client-stream-body-multiple-buffers.js | 1 | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-client-stream-body-no-chunks.js | 1 | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-client-stream-body-single-buffer.js | 1 | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-client-stream-body-single-string.js | 1 | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-client-stream-close-error.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-client-stream-close.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-client-stream-created.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-client-stream-error.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-client-stream-finish.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-client-stream-start.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-server-stream-close-error.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-server-stream-close.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-server-stream-created-start-timing.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-server-stream-created.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-server-stream-error.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-server-stream-finish.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-http2-server-stream-start.js | TIMEOUT | 0 | 🟡 偏离：需 http2 插桩 |
| test-diagnostics-channel-many-channels.js | 0 | 0 | ✅ |
| test-diagnostics-channel-memory-leak.js | 1 | 0 | ⏭️ 需 `--max-old-space-size` 小堆 flag（泄漏观测前提） |
| test-diagnostics-channel-module-import-error.js | 1 | 0 | 🟡 偏离：需 ESM loader 插桩（`module.import` 通道） |
| test-diagnostics-channel-module-import.js | 1 | 0 | 🟡 偏离：需 ESM loader 插桩 |
| test-diagnostics-channel-module-require-error.js | 1 | 0 | 🟡 偏离：需 CJS loader 插桩（`module.require` 通道） |
| test-diagnostics-channel-module-require.js | 1 | 0 | 🟡 偏离：需 CJS loader 插桩 |
| test-diagnostics-channel-net-client-socket-tls.js | 0 | 0 | ✅ |
| test-diagnostics-channel-net.js | TIMEOUT | 0 | 🟡 偏离：需 net 连接插桩 |
| test-diagnostics-channel-object-channel-pub-sub.js | 0 | 0 | ✅ |
| test-diagnostics-channel-process.js | 0 | 0 | ✅ |
| test-diagnostics-channel-pub-sub.js | 0 | 0 | ✅ |
| test-diagnostics-channel-run-stores-scope-transform-error.js | 0 | 0 | ✅ |
| test-diagnostics-channel-run-stores-scope.js | 0 | 0 | ✅（修：`enterWith` 别名） |
| test-diagnostics-channel-safe-subscriber-errors.js | 0 | 0 | ✅（修：订阅者抛错 `__dcUncaught` 路由） |
| test-diagnostics-channel-symbol-named.js | 0 | 0 | ✅ |
| test-diagnostics-channel-sync-unsubscribe.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-args-types.js | 0 | 0 | ✅（修：`Symbol.hasInstance` 空值 V8 文案桥） |
| test-diagnostics-channel-tracing-channel-callback-early-exit.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-callback-error.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-callback-run-stores.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-callback.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-has-subscribers.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-promise-early-exit.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-promise-error.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-promise-non-thenable.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-promise-run-stores.js | 1 | 0 | 🟡 偏离：ALS 跨 await 传播（`async_hooks.rs` 既有记档） |
| test-diagnostics-channel-tracing-channel-promise-spoofed-constructor.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-promise-thenable.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-promise-unhandled.js | 1 | 0 | 🟡 偏离：需运行时 `unhandledRejection` 事件面（另案） |
| test-diagnostics-channel-tracing-channel-promise.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-sync-early-exit.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-sync-error.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-sync-run-stores.js | 0 | 0 | ✅ |
| test-diagnostics-channel-tracing-channel-sync.js | 0 | 0 | ✅ |
| test-diagnostics-channel-udp.js | 0 | 0 | ✅ |
| test-diagnostics-channel-web-locks.js | 1 | 0 | 🟡 偏离：Web Locks 未实现（无插桩源） |
| test-diagnostics-channel-worker-threads.js | 0 | 0 | ✅ |

## trace_events

> 薄面确认（非深水）：本仓 `trace_events` 为 JS 类别集（`createTracing`/
> `getEnabledCategories`），无 V8 tracing 底座、无 `--trace-event-categories`
> CLI、无各模块插桩——与 `bun-compat.md` "类别集 JS 侧"口径一致，点名仅覆盖
> 表面形态（🟡，不逐文件展开；深件须 tracing 底座另案）。
> `test-trace-events-dynamic-enable.js` 真机侧挂（需 inspector/flag），本仓空过，
> 属"双方皆非绿"的对齐，不计 ✅。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-trace-events-api-worker-disabled.js | 0 | 0 | ✅ |
| test-trace-events-api.js | 1 | 1 | ⏭️ 需 `--expose-gc --expose-internals` flag（双 1 对齐） |
| test-trace-events-get-category-enabled-buffer.js | 1 | 1 | ⏭️ 需 `--expose-internals`（`internal/test/binding`，双 1 对齐） |
| test-trace-events-net-abstract-socket.js | 0 | 0 | ✅ |
| test-trace-events-perfetto-pftrace.js | 0 | 0 | ✅ |
| test-trace-events-dynamic-enable.js | 0 | 1 | ⏭️ 真机需 flag/底座（本仓空过，不计绿） |
| 其余 29 件（fs/http/net/v8/vm/worker/threadpool/console/`-binding`/exit/sigint 等） | ≠0 | 0 | 🟡 偏离：需 V8 tracing 底座 + CLI flags（`--trace-event-categories`、自 spawn `-e` 位置参数即全 flag 设计） + 各模块插桩 |

## dns

> 10f 收官：31 点名 = 20 ✅ + 10 ⏭️（全 `--expose-internals`）+ 1 🟡（空过，另案），红 0。本轮改动主体：
> `Resolver`/`promises.Resolver` 独立实例（Node internal/dns/utils 口径：自有
> servers/timeout/tries/maxTimeout + `_handle` 垫片 + pending 计数/setServers
> 互斥 + 代际 cancel）+ 定制 servers 经自建 UDP 单问（`hickory-proto` 编解码 +
> tokio 传输，零新依赖——hickory-resolver 的 NameServerConfig 无端口面；
> 投递即返 + 5ms 轮询收割，阻塞 native 会停转事件循环饿死 stub 分发）+
> 超时退避（无 maxTimeout 翻倍/有则截顶）+ Node 精确校验全套（name/hostname/
> callback/rrtype/servers/options/timeout/maxTimeout/ports/hints/family）+
> `lookupService`（PTR + 服务静态表）+ 错误全文 `${syscall} ${code} ${hostname}` +
> resolve4/6-DNS 化（含 ttl）+ resolveSoa/CAA 形态 + ETIMEOUT 正名。
> 偏离记档：定制路径无 CNAME 跟随（stub 口径）；系统路径端口忽略（hickory 面）；
> `perf_hooks` 文件空过（`exit` 钩另案，见下）。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-dns-cancel-reverse-lookup.js | 0 | 0 | ✅（修：cancel 即刻 ECANCELLED） |
| test-dns-channel-cancel-promise.js | 0 | 0 | ✅（修：同上，promise 形） |
| test-dns-channel-cancel.js | 0 | 0 | ✅（修：同上） |
| test-dns-channel-timeout.js | 0 | 0 | ✅（修：timeout/tries 原生透传 + 构造器校验） |
| test-dns-default-order-ipv4.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-dns-default-order-ipv6.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-dns-default-order-verbatim.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-dns-get-server.js | 0 | 0 | ✅（修：Resolver + `_handle` 垫片） |
| test-dns-lookup-promises-options-deprecated.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-dns-lookup-promises.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-dns-lookup.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-dns-lookupService-promises.js | 0 | 0 | ✅（修：lookupService + NODATA→ENOTFOUND） |
| test-dns-lookupService.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-dns-memory-error.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-dns-multi-channel.js | 0 | 0 | ✅（修：定制 servers 真查询 + 同型过滤） |
| test-dns-negative-zero.js | 0 | 0 | ✅（修：falsy options 跳过 + -0 归零） |
| test-dns-perf_hooks.js | 0 | 0 | 🟡 空过：`exit` 钩不支持致 exit 断言永不执行（另案；dns entries 亦无） |
| test-dns-promises-exists.js | 0 | 0 | ✅ |
| test-dns-resolve-promises.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-dns-resolveany-bad-ancount.js | 0 | 0 | ✅（修：坏包 EBADRESP/超时 ETIMEOUT 双形） |
| test-dns-resolveany-ttl-overflow.js | 0 | 0 | ✅（修：64KB 收包 + TTL 透传，257 条） |
| test-dns-resolveany.js | 0 | 0 | ✅（修：ANY 单问 + A/AAAA-ttl/SOA/CAA 形态） |
| test-dns-resolvens-typeerror.js | 0 | 0 | ✅（修：同步校验 + name/callback 精确码） |
| test-dns-resolver-max-timeout.js | 0 | 0 | ✅（修：退避/截顶，3113ms vs 517ms） |
| test-dns-resolvesrv-econnrefused.js | 0 | 0 | ✅（修：SRV 收发 + 同型过滤） |
| test-dns-resolvesrv.js | 0 | 0 | ✅ |
| test-dns-set-default-order.js | 1 | 1 | ⏭️ `--expose-internals` |
| test-dns-setlocaladdress.js | 0 | 0 | ✅（修：setLocalAddress 真机六形） |
| test-dns-setserver-when-querying.js | 0 | 0 | ✅（修：pending 互斥 ERR_DNS_SET_SERVERS_FAILED） |
| test-dns-setservers-type-check.js | 0 | 0 | ✅（修：servers/servers[i] 精确码 + 双 Resolver） |
| test-dns.js | 0 | 0 | ✅（修：setServers 全套/IP 表/端口/lookup 家族/错误全文/ttl） |


## vm

> 10f 收官：100 点名 = 47 ✅ + 27 ⏭️（24 件 `--experimental-vm-modules` 家族 +
> 3 件 `--expose-gc`（`globalThis.gc`），双 1 对齐）+ 26 🟡 偏离（逐件理由见下），
> 红 0（真机同条件亦红的全对齐）。本轮改动主体：沙箱簿记 WeakMap 化（ctx id 从
> 沙箱自有键撤进 `__vmBookkeeping`，ownkeys×3/definer-interception 的"沙箱键集
> 不变"口径）+ symbol 键同步与描述符保形（`__vmStageAndDefine` 暂存位过域 +
> 目标域内 defineProperty 落定；非默认数据描述符 writable/enumerable/configurable
> 保形）+ syncOut 收紧（源端访问器键不读不写——getter/setter mustCall 精确计数、
> 目标只读数据跳过、自指键（`window`）不回写、`globalThis`/`global` 永不同步）+
> 错误原物透传（native take→`vm_last_error` 槽暂存→恢复 pending 读 error_info→
> `__wjs_vm_take_error` 取原物：跨域 `instanceof SyntaxError`、非对象 `throw`
> 原样；信封重建只作兜底）+ node 文案桥四族（undeclared-variable/read-only/
> redefine/proxy 不变量 + `__recv` Received 描述）+ 信封栈前缀（`filename:line\n
> 源行\ncaret\n\n`，displayErrors `startsWith` 点名）+ jobqueue SEGV 根修（跨域
> 微任务先取执行 global 进 AutoRealm 再 `RunJSMicroTask`——SM 内部队 runJobs
> 同款，DEBUG assert；script-after-evaluate 139 → 1）+ CJS 顶层 return
> （`script_goal_probe` 双值分类 + `cjs_goal_probe` 裸文本复核（包络形禁用）+
> `load_cjs_js`（`with_commonjs`）require 分流，§4.59 姊妹）+ options 校验面
> （lineOffset/columnOffset 类型/范围分报、timeout 正数、cachedData Bufferish
> 类型门、createContext name/origin、runInNewContext contextName/contextOrigin、
> Script 方法层拒 null/'bad'/42）+ compileFunction 体级声明位预检（表达式包络被
> `});` 提前闭合吸收面）+ wrapper 真机形（`function (p) {\n…\n}`，toString/name）
> + contextExtensions 文案逐字 + 杂项（process toStringTag、"an vm.Context" 冠词
> 怪癖、zlib 空输入断言按真机 Z_BUF_ERROR 修正）。
> 偏离记档（26 件）：① 引擎不可比 7 件——解析器恢复策略（basic，V8/SM 报错
> token 不同）、lineOffset/columnOffset 栈帧未布线（context.js，另案）、
> measure-memory 契约（本仓 reject vs 真机 resolve ×2）、字节码/sourcemap/
> codegen 面（cached-data/source-map-url/codegen，既有记档）；② copy 模型固有
> 9 件（拦截器/活绑定/原型链方法语义，jsdom 级工程另案：global-property-{
> enumerator,interceptors,prototype}/harmony-symbols/proxies/symbols/
> property-not-on-sandbox/proxy-sandbox-property-query/property-definer-partial-update）；
> ③ 微任务模式 2 件（本仓恒排空 vs 真机 per-context 队列不排；异步回写无
> syncOut 点：script-after-evaluate/context-async-script）；④ 自 spawn/stdio
> 阵列面（sigint×2）+ execFile 自身（api-handles-getter-errors）+
> `--experimental-vm-modules` 缺省文案面（dynamic-import-callback×2，真机无
> flag 走校验文案，本仓 rejection 形）共 5 件；⑤ timeout 家族 3 件（timeout
> 只校验不执行，既有记档；escape ×2 记偏离，test-vm-timeout.js 本仓 TIMEOUT
> 单列）。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-vm-access-process-env.js | 0 | 0 | ✅ |
| test-vm-api-handles-getter-errors.js | 1 | 0 | 🟡 偏离：execFile 自身（自 spawn 面，全 flag CLI 设计） |
| test-vm-attributes-property-not-on-sandbox.js | 0 | 0 | ✅ |
| test-vm-basic.js | 1 | 0 | 🟡 偏离：解析器恢复策略（V8 `Unexpected token '}'` vs SM `expected expression, got ')'`——报错 token 都不同，跨引擎不可比） |
| test-vm-cached-data.js | 1 | 0 | 🟡 偏离：字节码面不做（cachedData 产物校验，既有记档） |
| test-vm-codegen.js | 1 | 0 | 🟡 偏离：codegen 面不做（既有记档） |
| test-vm-context-async-script.js | 1 | 0 | 🟡 偏离：微任务模式（异步回写无 syncOut 点） |
| test-vm-context-dont-contextify.js | 0 | 0 | ✅ |
| test-vm-context-property-forwarding.js | 0 | 0 | ✅ |
| test-vm-context.js | 1 | 0 | 🟡 偏离：lineOffset/columnOffset 栈帧偏移未布线（晚场重构不值，另案） |
| test-vm-create-and-run-in-context.js | 1 | 1 | ⏭️ 需 `--expose-gc`（`globalThis.gc`，双 1 对齐） |
| test-vm-create-context-accessors.js | 0 | 0 | ✅ |
| test-vm-create-context-arg.js | 0 | 0 | ✅ |
| test-vm-create-context-circular-reference.js | 0 | 0 | ✅ |
| test-vm-createcacheddata.js | 0 | 0 | ✅ |
| test-vm-cross-context.js | 0 | 0 | ✅ |
| test-vm-data-property-writable.js | 0 | 0 | ✅ |
| test-vm-deleting-property.js | 0 | 0 | ✅ |
| test-vm-dynamic-import-callback-missing-flag.js | 1 | 0 | 🟡 偏离：`--experimental-vm-modules` 缺省文案面（真机无 flag 走校验文案，本仓 rejection 形） |
| test-vm-function-declaration.js | 0 | 0 | ✅ |
| test-vm-function-redefinition.js | 0 | 0 | ✅ |
| test-vm-getters.js | 0 | 0 | ✅ |
| test-vm-global-assignment.js | 0 | 0 | ✅ |
| test-vm-global-configurable-properties.js | 0 | 0 | ✅ |
| test-vm-global-contextual-store.js | 0 | 0 | ✅ |
| test-vm-global-define-property.js | 0 | 0 | ✅ |
| test-vm-global-get-own.js | 0 | 0 | ✅ |
| test-vm-global-identity.js | 0 | 0 | ✅ |
| test-vm-global-non-writable-properties.js | 0 | 0 | ✅ |
| test-vm-global-property-enumerator.js | 1 | 0 | 🟡 偏离：copy 模型固有（拦截器/活绑定，jsdom 级工程另案） |
| test-vm-global-property-interceptors.js | 1 | 0 | 🟡 偏离：copy 模型固有（同上） |
| test-vm-global-property-prototype.js | 1 | 0 | 🟡 偏离：copy 模型固有（同上） |
| test-vm-global-restricted-property.js | 0 | 0 | ✅ |
| test-vm-global-setter.js | 0 | 0 | ✅ |
| test-vm-harmony-symbols.js | 1 | 0 | 🟡 偏离：copy 模型固有（原型链方法/活绑定） |
| test-vm-indexed-properties.js | 0 | 0 | ✅ |
| test-vm-inherited_properties.js | 0 | 0 | ✅ |
| test-vm-is-context.js | 0 | 0 | ✅ |
| test-vm-low-stack-space.js | 0 | 0 | ✅ |
| test-vm-measure-memory-lazy.js | 1 | 1 | ⏭️ 需 `--expose-gc`（同上） |
| test-vm-measure-memory-multi-context.js | 1 | 0 | 🟡 偏离：measure-memory 契约（本仓 reject vs 真机 resolve） |
| test-vm-measure-memory.js | 1 | 0 | 🟡 偏离：measure-memory 契约（同上） |
| test-vm-module-after-evaluate.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（双 1 对齐） |
| test-vm-module-basic.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-cached-data.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-dynamic-import-promise.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-dynamic-import.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-dynamic-namespace.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-errors.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-evaluate-source-text-module.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-evaluate-synthethic-module-rejection.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-evaluate-synthethic-module.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-evaluate-while-evaluating.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-hasasyncgraph.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-hastoplevelawait.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-import-meta.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-instantiate.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-link-shared-deps.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-link.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-linkmodulerequests-circular.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-linkmodulerequests-deep.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-linkmodulerequests.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-modulerequests.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-reevaluate.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-module-synthetic.js | 1 | 1 | ⏭️ `--experimental-vm-modules`（同上） |
| test-vm-new-script-new-context.js | 0 | 0 | ✅ |
| test-vm-new-script-this-context.js | 0 | 0 | ✅ |
| test-vm-no-dynamic-import-callback.js | 1 | 0 | 🟡 偏离：`--experimental-vm-modules` 缺省文案面（同 missing-flag） |
| test-vm-not-strict.js | 0 | 0 | ✅ |
| test-vm-options-validation.js | 0 | 0 | ✅ |
| test-vm-ownkeys.js | 0 | 0 | ✅ |
| test-vm-ownpropertynames.js | 0 | 0 | ✅ |
| test-vm-ownpropertysymbols.js | 0 | 0 | ✅ |
| test-vm-parse-abort-on-uncaught-exception.js | 0 | 0 | ✅（直跑稳定绿；classify 并发负载下偶抖，计绿） |
| test-vm-preserves-property.js | 0 | 0 | ✅ |
| test-vm-property-definer-interception.js | 0 | 0 | ✅ |
| test-vm-property-definer-partial-update.js | 1 | 0 | 🟡 偏离：copy 模型固有（definer 部分更新语义） |
| test-vm-property-not-on-sandbox.js | 1 | 0 | 🟡 偏离：copy 模型固有 |
| test-vm-proxies.js | 1 | 0 | 🟡 偏离：copy 模型固有（原型链方法） |
| test-vm-proxy-failure-CP.js | 0 | 0 | ✅ |
| test-vm-proxy-sandbox-property-query.js | 1 | 0 | 🟡 偏离：copy 模型固有 |
| test-vm-run-in-new-context.js | 1 | 1 | ⏭️ 需 `--expose-gc`（同上） |
| test-vm-script-after-evaluate.js | 1 | 0 | 🟡 偏离：微任务模式（本仓恒排空 vs 真机 per-context 队列不排，mustNotCall 被调） |
| test-vm-script-throw-in-tostring.js | 0 | 0 | ✅ |
| test-vm-set-property-proxy.js | 0 | 0 | ✅ |
| test-vm-set-proto-null-on-globalthis.js | 0 | 0 | ✅ |
| test-vm-sigint-existing-handler.js | 1 | 0 | 🟡 偏离：自 spawn（stdio 阵列面，全 flag CLI 设计） |
| test-vm-sigint.js | 1 | 0 | 🟡 偏离：自 spawn（同上） |
| test-vm-source-map-url.js | 1 | 0 | 🟡 偏离：sourcemap 面不做（既有记档） |
| test-vm-static-this.js | 0 | 0 | ✅ |
| test-vm-strict-assign.js | 0 | 0 | ✅ |
| test-vm-strict-mode.js | 0 | 0 | ✅ |
| test-vm-symbols.js | 1 | 0 | 🟡 偏离：copy 模型固有（原型链方法） |
| test-vm-syntax-error-message.js | 0 | 0 | ✅ |
| test-vm-syntax-error-stderr.js | 0 | 0 | ✅ |
| test-vm-timeout-escape-promise-2.js | 1 | 0 | 🟡 偏离：timeout 只校验不执行（既有记档） |
| test-vm-timeout-escape-promise-module.js | 1 | 1 | ⏭️ `--experimental-vm-modules` 家族 + timeout escape 语义（双 1 对齐） |
| test-vm-timeout-escape-promise.js | 1 | 0 | 🟡 偏离：timeout 只校验不执行（同上） |
| test-vm-timeout.js | TIMEOUT | 0 | 🟡 偏离：timeout 只校验不执行（本仓 TIMEOUT，单列） |
| test-vm-util-lazy-properties.js | 0 | 0 | ✅ |
