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

> 10f 收官：18 点名 = 13 ✅ + 5 ⏭️（3 件 URLPattern 整面 + 1 件自 spawn
> 裸文件参数 + 1 件 isURL 内部面双 1 对齐），红 0。URLPattern 专项收官
> （plan3 §5，2026-09-21）：`urlpattern` 0.6 直引（Deno 官方，2026-09-11
> 已采购未接线）+ Rust 桥（parse/test/exec 三 natives + 会话注册表）+
> prelude 真类（WebIDL 重载分流）+ `node:url` 具名/default 双导出——
> 18 点名 = **16 ✅ + 2 ⏭️**，红 0。本轮改动主体：legacy parse/format/
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
| test-urlpattern.js | 0 | 0 | ✅（专项：结果键序/分量键序/inputs 回填） |
| test-urlpattern-types.js | 0 | 0 | ✅（专项：重载矩阵全码全信：CONSTRUCT_CALL/ARG_TYPE/URL_PATTERN/OPERATION_FAILED） |
| test-urlpattern-invalidthis.js | 0 | 0 | ✅（专项：九 getter + test/exec 双品牌门 `Illegal invocation`） |
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

## stream

> 10f 收官（本轮）：250 点名 = 190 ✅ + 50 ⏭️（全 `--expose-internals`）+ 10 🟡 偏离，
> 红 0（真机同条件亦红的全对齐）。本轮改动主体：**process.nextTick 原生队列**
> （node 双层调度语义——同步期入队的 tick 先于微任务、微任务期入队的 tick 等整轮
> 微任务排空后跑（V8 checkpoint 原子性）；queueMicrotask 同队列 FIFO 无法表达，
> compose/pipeline post-loop throw 全族对拍现形；pump 内 RunJobs 前后各收割一轮，
> 回调抛错走 uncaughtException 路由——无监听 fatal，有监听分发即吞）+
> QueuingStrategy 双全局（ByteLength/Count：hwm 原型 getter、size 全实例共享、
> ERR_MISSING_OPTION 校验，真机逐项对拍）+ compose `onclose` 初始 null（undefined
> 会把 _destroy 回调永久搁浅——composed 流 error/close 双丢）+ stdout/stderr
> Socket 形 EE 表面（pipe 的 dest.on/emit('pipe') 接得住；事件面空转偏差记档）+
> fs.createReadStream 换真 ReadStream（open/ready/data(Buffer)/end/close 事件序 +
> path/start/end/autoClose；sync 底座偏差记档：'open' fd 恒 null、事件于首个
> _read 前派发）+ Writable.prototype.pipe 的 ERR_STREAM_CANNOT_PIPE（真机口径，
> prelude 既有）。
> 偏离记档（10 件）：自 spawn 3 件（spawnPromisified/execPath 子进程）；TIMEOUT 3 件
> （pipeline/pipeline-http2/readable-async-iterators——边角流形/混接另案）；断言级
> 4 件（consumers unhandled rejection ×6、preprocess 内容差、promises ENOENT 形态、
> toWeb BYOB 异常路径）——均另案。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-stream-auto-destroy.js、test-stream-await-drain-writers-in-synchronously-recursion-write.js、test-stream-backpressure.js、test-stream-big-packet.js、test-stream-big-push.js、test-stream-catch-rejections.js、test-stream-compose.js、test-stream-construct.js、test-stream-decoder-objectmode.js、test-stream-destroy-event-order.js、test-stream-destroy.js、test-stream-drop-take.js、test-stream-duplex-destroy.js、test-stream-duplex-end.js、test-stream-duplex-from.js、test-stream-duplex-props.js、test-stream-duplex-readable-end.js、test-stream-duplex-readable-writable.js、test-stream-duplex-writable-finished.js、test-stream-duplex.js、test-stream-duplexpair.js、test-stream-end-of-streams.js、test-stream-end-paused.js、test-stream-err-multiple-callback-construction.js、test-stream-error-once.js、test-stream-event-names.js、test-stream-events-prepend.js、test-stream-filter.js、test-stream-flatMap.js、test-stream-forEach.js、test-stream-inheritance.js、test-stream-ispaused.js、test-stream-map.js、test-stream-objectmode-undefined.js、test-stream-once-readable-pipe.js、test-stream-passthrough-drain.js、test-stream-pipe-after-end.js、test-stream-pipe-await-drain-manual-resume.js、test-stream-pipe-await-drain-push-while-write.js、test-stream-pipe-await-drain.js、test-stream-pipe-cleanup-pause.js、test-stream-pipe-cleanup.js、test-stream-pipe-deadlock.js、test-stream-pipe-error-handling.js、test-stream-pipe-event.js、test-stream-pipe-flow-after-unpipe.js、test-stream-pipe-flow.js、test-stream-pipe-manual-resume.js、test-stream-pipe-multiple-destinations-error.js、test-stream-pipe-multiple-pipes.js、test-stream-pipe-needDrain.js、test-stream-pipe-objectmode-to-non-objectmode.js、test-stream-pipe-same-destination-twice.js、test-stream-pipe-unpipe-streams.js、test-stream-pipe-without-listenerCount.js、test-stream-pipeline-async-iterator.js、test-stream-pipeline-duplex.js、test-stream-pipeline-queued-end-in-destroy.js、test-stream-pipeline-with-empty-string.js、test-stream-push-order.js、test-stream-push-strings.js、test-stream-readable-aborted.js、test-stream-readable-add-chunk-during-data.js、test-stream-readable-async-iter-half-open-duplex.js、test-stream-readable-constructor-set-methods.js、test-stream-readable-data.js、test-stream-readable-default-encoding.js、test-stream-readable-destroy.js、test-stream-readable-didRead.js、test-stream-readable-dispose.js、test-stream-readable-emit-readable-short-stream.js、test-stream-readable-emittedReadable.js、test-stream-readable-end-destroyed.js、test-stream-readable-ended.js、test-stream-readable-error-end.js、test-stream-readable-event.js、test-stream-readable-flow-recursion.js、test-stream-readable-from-web-termination.js、test-stream-readable-hwm-0-async.js、test-stream-readable-hwm-0-no-flow-data.js、test-stream-readable-hwm-0.js、test-stream-readable-infinite-read.js、test-stream-readable-invalid-chunk.js、test-stream-readable-needReadable.js、test-stream-readable-next-no-null.js、test-stream-readable-no-unneeded-readable.js、test-stream-readable-object-multi-push-async.js、test-stream-readable-pause-and-resume.js、test-stream-readable-readable-one.js、test-stream-readable-readable-then-resume.js、test-stream-readable-readable.js、test-stream-readable-reading-readingMore.js、test-stream-readable-resume-hwm.js、test-stream-readable-resumeScheduled.js、test-stream-readable-setEncoding-existing-buffers.js、test-stream-readable-setEncoding-null.js、test-stream-readable-to-web-termination-byob.js、test-stream-readable-to-web-termination.js、test-stream-readable-unshift.js、test-stream-readable-with-unimplemented-_read.js、test-stream-readableListening-state.js、test-stream-reduce.js、test-stream-set-default-hwm.js、test-stream-toArray.js、test-stream-toWeb-allows-server-response.js、test-stream-transform-callback-twice.js、test-stream-transform-constructor-set-methods.js、test-stream-transform-destroy.js、test-stream-transform-final-sync.js、test-stream-transform-final.js、test-stream-transform-flush-data.js、test-stream-transform-hwm0.js、test-stream-transform-objectmode-falsey-value.js、test-stream-transform-split-highwatermark.js、test-stream-transform-split-objectmode.js、test-stream-typedarray.js、test-stream-uint8array.js、test-stream-unpipe-event.js、test-stream-unshift-empty-chunk.js、test-stream-unshift-read-race.js、test-stream-writable-aborted.js、test-stream-writable-change-default-encoding.js、test-stream-writable-clear-buffer.js、test-stream-writable-constructor-set-methods.js、test-stream-writable-decoded-encoding.js、test-stream-writable-destroy.js、test-stream-writable-end-cb-error.js、test-stream-writable-end-multiple.js、test-stream-writable-ended-state.js、test-stream-writable-final-async.js、test-stream-writable-final-destroy.js、test-stream-writable-final-throw.js、test-stream-writable-finish-destroyed.js、test-stream-writable-finished-state.js、test-stream-writable-finished.js、test-stream-writable-invalid-chunk.js、test-stream-writable-needdrain-state.js、test-stream-writable-null.js、test-stream-writable-properties.js、test-stream-writable-samecb-singletick.js、test-stream-writable-writable.js、test-stream-writable-write-cb-error.js、test-stream-writable-write-cb-twice.js、test-stream-writable-write-error.js、test-stream-writable-write-writev-finish.js、test-stream-writableState-ending.js、test-stream-writableState-uncorked-bufferedRequestCount.js、test-stream-write-destroy.js、test-stream-write-drain.js、test-stream-write-final.js、test-stream-writev.js、test-stream2-base64-single-char-read-end.js、test-stream2-compatibility.js、test-stream2-decode-partial.js、test-stream2-finish-pipe.js、test-stream2-httpclient-response-end.js、test-stream2-objects.js、test-stream2-pipe-error-handling.js、test-stream2-pipe-error-once-listener.js、test-stream2-push.js、test-stream2-read-correct-num-bytes-in-utf8.js、test-stream2-read-sync-stack.js、test-stream2-readable-empty-buffer-no-eof.js、test-stream2-readable-legacy-drain.js、test-stream2-readable-non-empty-end.js、test-stream2-readable-wrap-destroy.js、test-stream2-readable-wrap-empty.js、test-stream2-readable-wrap-error.js、test-stream2-readable-wrap-proxy-methods.js、test-stream2-readable-wrap.js、test-stream2-set-encoding.js、test-stream2-transform.js、test-stream2-unpipe-drain.js、test-stream2-unpipe-leak.js、test-stream3-cork-end.js、test-stream3-cork-uncork.js、test-stream3-pause-then-read.js、test-stream3-pipeline-async-iterator.js、test-streams-highwatermark.js | 0 | 0 | ✅ |
| test-stream-pipe-error-unhandled.js | 0 | 0 | ✅（修：nextTick 原生队列（uncaughtException 路由）） |
| test-stream-pipeline-listeners.js | 0 | 0 | ✅（修：nextTick 原生队列（同上）） |
| test-stream-pipeline-uncaught.js | 0 | 0 | ✅（修：nextTick 原生队列（同上）） |
| test-stream-readable-compose.js | 0 | 0 | ✅（修：compose onclose 初始 null + nextTick 原生队列） |
| test-stream-readable-strategy-option.js | 0 | 0 | ✅（修：QueuingStrategy 双全局（真机口径）） |
| test-stream-readable-unpipe-resume.js | 0 | 0 | ✅（修：fs.ReadStream 换真 Readable（事件序真机口径）） |
| test-stream-writable-end-cb-uncaught.js | 0 | 0 | ✅（修：nextTick 原生队列（同上）） |
| test-stream2-basic.js | 0 | 0 | ✅（修：nextTick 原生队列（§4.80 逐条 rooting 消 139）） |
| test-stream2-finish-pipe-error.js | 0 | 0 | ✅（修：nextTick 原生队列（同上）） |
| test-stream2-large-read-stall.js | 0 | 0 | ✅（修：nextTick 原生队列（同上）） |
| test-stream2-writable.js | 0 | 0 | ✅（修：stdout/stderr EE 表面（pipe dest 接线）） |
| test-stream-add-abort-signal.js、test-stream-base-prototype-accessors-enumerability.js、test-stream-finished-async-local-storage.js、test-stream-finished-bindAsyncResource-path.js、test-stream-finished-default-path.js、test-stream-finished.js、test-stream-iter-broadcast-backpressure.js、test-stream-iter-broadcast-basic.js、test-stream-iter-broadcast-coverage.js、test-stream-iter-broadcast-from.js、test-stream-iter-consumers-bytes.js、test-stream-iter-consumers-merge.js、test-stream-iter-consumers-tap.js、test-stream-iter-consumers-text.js、test-stream-iter-cross-realm.js、test-stream-iter-duplex.js、test-stream-iter-from-async.js、test-stream-iter-from-coverage.js、test-stream-iter-from-sync.js、test-stream-iter-from-writable-cache-options.js、test-stream-iter-namespace.js、test-stream-iter-pipeto-edge.js、test-stream-iter-pipeto-signal.js、test-stream-iter-pipeto-writev.js、test-stream-iter-pipeto.js、test-stream-iter-pull-async.js、test-stream-iter-pull-sync.js、test-stream-iter-push-backpressure.js、test-stream-iter-push-basic.js、test-stream-iter-push-writer.js、test-stream-iter-readable-interop.js、test-stream-iter-share-async.js、test-stream-iter-share-coverage.js、test-stream-iter-share-from.js、test-stream-iter-share-sync.js、test-stream-iter-sharedarraybuffer.js、test-stream-iter-to-readable.js、test-stream-iter-transform-compat.js、test-stream-iter-transform-coverage.js、test-stream-iter-transform-errors.js、test-stream-iter-transform-output.js、test-stream-iter-transform-params.js、test-stream-iter-transform-roundtrip.js、test-stream-iter-transform-sync.js、test-stream-iter-validation.js、test-stream-iter-writable-from.js、test-stream-iter-writable-interop.js、test-stream-wrap-drain.js、test-stream-wrap-encoding.js、test-stream-wrap.js | 1 | 1 | ⏭️ `--expose-internals`（真机同条件亦红，双 1 对齐） |
| test-stream-consumers.js | 1 | 0 | 🟡 偏离：streamConsumers 边角语义 unhandled rejection ×6——另案 |
| test-stream-iter-disabled.js | 1 | 0 | 🟡 偏离：自 spawn（spawnPromisified 全套件） |
| test-stream-iter-readable-interop-disabled.js | 1 | 0 | 🟡 偏离：自 spawn（同上） |
| test-stream-pipeline-http2.js | TIMEOUT | 0 | 🟡 偏离：TIMEOUT：h2 管线混流形（h2 面另案） |
| test-stream-pipeline-process.js | 1 | 0 | 🟡 偏离：自 spawn（execPath 子进程） |
| test-stream-pipeline.js | TIMEOUT | 0 | 🟡 偏离：TIMEOUT：pipeline 边角流形（destroyed/iterable 混接）仍挂——另案 |
| test-stream-preprocess.js | 1 | 0 | 🟡 偏离：断言 'abc…' 内容差（preprocess 管线时序）——另案 |
| test-stream-promises.js | 1 | 0 | 🟡 偏离：ENOENT 预期形态差（promises 错误路由）——另案 |
| test-stream-readable-async-iterators.js | TIMEOUT | 0 | 🟡 偏离：TIMEOUT：async iterator 边角——另案 |
| test-stream-readable-to-web-byob.js | 1 | 0 | 🟡 偏离：toWeb BYOB 缺异常路径——另案 |

## fs

> 三轮对拍（2026-09-17，263 件，`/tmp/wjs-10f-fs*.txt`）：
> 同绿 39 → 54 → 96 → **123**；双红 76 → 73 → 28 → **28**；DIFF 148 → 136 → 139 → **112**。
> 双红为真机同条件亦红（fixture/环境），对齐不算欠账。
>
> **G4 轮（2026-09-19，欠账 validators 尾件）**：全量 `test-fs-*` 355 件
> 166 绿（净 +20：constants/stat-bigint/stat/statfs/readfile/rename-type-check/
> null-bytes/options-immutable/mkdir-mode-mask/rmdir-throws/truncate/
> timestamp-parsing/lchmod/lchown×2/fchown/utimes/y2K38/append-file-sync/
> write-file-sync/write-file/roundtrip 主干）。核心修正：utimes 族秒口径
> （真机量纲）、lchown/lchmod/lutimes/_toUnixTimestamp 新面、null-byte 全
> API（含 URL %00）、__fdCb 值优先校验 + 孤儿 promise 根除、latin1 字节
> 直映、WriteStream 真 open。残簇另案：cp ~33 / write-stream ~13 /
> promises-file-handle ~12 / read-stream ~8 / roundtrip 末段 async_hooks
> FSREQCALLBACK 资源面（AGENTS §4.149）。

### 已修（每项经真机 26.8.2 对拍）

- **WriteStream 真类**（write/end/finish/close 事件序 + bytesWritten/flags/autoClose）
  + ReadStream/WriteStream Proxy 自 new 形（node legacy：`fs.ReadStream(file)` 可调）。
- **read/write 全形态**（node lib/fs.js 同构）：`read(fd,cb)`/`read(fd,params,cb)`/
  `read(fd,buf,options,cb)`/6 参全形；write 字符串三形 + options 形；返回
  `{bytesRead|bytesWritten, buffer}`；`util.promisify` args 符号
  （`Symbol.for('nodejs.util.promisify.customArgs')` 跨模块桥 + fs.read/write/exists
  挂名单，`promisify(fs.exists)` → boolean）。
- **FileHandle 全家**：read/write params+options 形、`{bytesRead,buffer}` 返回、
  close 幂等（缓存 promise 二次 close 不抛不重发）、chown/fchown/readv/writev/
  `createReadStream/createWriteStream`（fd 为 FileHandle 时走 handle 方法——
  node streams.js FileHandleOperations 同构）、fh close 事件毁流、writeFile 流/
  同步+异步可迭代/encoding/signal abort（宏任务写位——nextTick 检查点先于写）。
- **fs 流 fd 形**：`fs.createReadStream(null, {fd})`（path 可 null）、start/end 记档。
- **mkdtempDisposable**（sync + promises）：`{path, remove, [Symbol.dispose|asyncDispose]}`、
  remove 锁创建期绝对路径（chdir 隔离）、promises 版 remove 为 async（assert.rejects 契约）、
  mkdtemp 后缀 6 随机 alnum（libuv 口径）。
- **参数校验族全面对齐**（node validators 口径，code+message 逐字）：
  path（Buffer/URL 收录）、fd（int32 正数域）、mode（number/string/范围 0..2^32）、
  `options.recursive`（"property" 文案）、callback；错误消息 node 形状
  （`CODE: <uv msg>, <syscall> '<path>'`，errno→uv 消息表）；
  `__fsErr` 对 JS 侧已带 code 的错误直通（`__fsCall` 闭包内校验错误不得重包 UNKNOWN）。
- **chown/fchown/lchown 落地**（std chown 安全路线 + libc fchown/lchown 边界）。
- **mkdir recursive**：返回首建路径、目录外 EEXIST、父为文件 ENOTDIR、
  recursive 布尔校验、fs_err 去 raw errno。
- **fd 形 readFile/writeFile/appendFile**（current position 语义、只读 fd EBADF、
  signal abort）；readdir `encoding:'buffer'`、readlink/readdir/realpath/mkdtemp/watch/
  streams 编码校验（`is invalid encoding` 逐字）。

### 剩余红项（DIFF 112 + 双红 28，均另案或记档）

- **Missing expected / unexpected throw 长尾**（~25 件）：open/opendir/mkdtemp-prefix/
  non-number 逐 API 校验缺口，沿本轮 validators 族续补；lchmod/lchown 真 syscall 面。
- **hang 9 件**：promises-watch/watch-encoding/watch-recursive、read-stream-pos、
  readfile-utf8-fast-path——watch 事件流 + fast-path 另案。
- **pipe 系 4 件**（readfile-pipe/eof）：stdin 管道读形，另案。
- **watch-ignore-glob 系 6 件**：node 26 glob ignore 语义，另案。
- **"test is not a function" 4 件**（flush 系）：writeFile/appendFile flush 选项，另案。
- **unhandled-rej 16 件**：readdir-buffer hex 断言、promises-appendfile、dispose 收尾等
  尾部件，沿簇续修。
- **双红 28 件**：真机同红（fixture 依赖/内部面），对齐。

四轮（2026-09-19，validators 族 + unhandled-rej 簇）：open/opendir/mkdtemp×2/
non-number/readdir-buffer/promises-appendfile/dispose/mkdtempDisposable×2
**10/10 转绿**（真机逐件）+ `require('fs').mkdtempDisposableSync` 具名导出
（node 26 双名并存；`phase10f_mkdtemp_disposable_sync_cjs_export`）。连带：
assert.throws 对象校验器逐键 deepStrictEqual（node expectedException 口径，
ERR_* 码括号仅限 ERR_ 前缀——errno 系 name 恒裸 Error）。剩余红项维持上记
分类（validators 尾件/watch hang/pipe/glob/flush 各簇另案）。

五轮（2026-09-20，G8 watch 轮）：watch 域 46 件 **SAME0 40 + SAME1 3，
DIFF 3**（fs.glob ×2 记档另案 + enoent-after-deletion 间歇性超时另查）。
转绿 30 件：ignore 全形态 ×9（含递归 `**` + 相对路径）、StatWatcher 单例 EE +
异步 stop + 零 Stats 首轮（stop-sync/async、watchfile-ref-unref、watchfile）、
FSWatcher ref/unref + 异步 close（watch-ref-unref）、encoding（hex/buffer）、
promises.watch ×7（迭代 + 8 组校验 + abort + ignore 五面）、assert-leaks
（_getActiveHandles）、realpath-pipe（exit 首码赢）、recursive-watch-file
（Create 二判据）/watch.js/enoent-after-deletion（前沿防抖 + 首轮 return 收口），
flush 选项三面（write/append/stream，校验逐字；套件待 node:test）。
根修三件：静默窗持续写饿死→前沿触发（AGENTS §4.152）、notify 线程 TLS 错表→
分类移分发侧（§4.153）、首轮 return 吞真变迁（§4.155）；附 exit 首码赢（§4.154）、
stat 真 unref。黑盒 fs 18/18 + 冒烟 5/5。

六轮（2026-09-21，fs 残簇 cp/streams 轮）：cp/write/read/handle 129 件
**70 SAME/59 DIFF → 114 SAME/15 DIFF**（三提交：e2f0d28 校验族/6412095 链接分发/
b2a473e 流续命）。转绿 44 件：cp validateCpOptions 逐字 + 错误码（EISDIR/EINVAL/
EEXIST/DIR_TO_NON_DIR/NON_DIR_TO_DIR/INCOMPATIBLE_PAIR）+ lstat 分发/onLink
（verbatim/srcIsDir 门/换链）+ 缺失父目录创建 + stat 去 fs_err（ENAMETOOLONG）；
流 getOptions/path-undefined/close 补齐/autoClose 访问器/autoDestroy 随 autoClose/
fd-null/open 递延/_final 递延/patch-open honored；promises.readFile 信号让步；
FileHandle.readLines/readFile-2GiB 门；AbortSignal symbol 双轨穿透 Proxy。
残 15 全部分类：cp async-filter ×1 ✅ 2026-09-21 转绿（__cpAsync 逐项 await +
同步校验前置，cp 79/79 全同；附 readdirSync recursive 下钻补齐）/socket ×2
✅ 2026-09-21 转绿（UDS listen 改同步 bind， task 异步绑 ENOENT 竞态根除；
AGENTS §4.165）/read-pos ×1 ✅ 2026-09-21 转绿（live 跟随 setImmediate 重查）/
write-err ×1 ✅ 2026-09-21 转绿（WriteStream 增量经默认导出 + 写 position 跟踪 +
close 不等回调；附带修 autoclose-option/change-open；AGENTS §4.166）/
write-patch-open ×1（早绿，child 域修复顺带）；expose-internals ×3（跳过类）；
pull/writer ×3（需 stream/iter+zlib/iter 新模块，另轮）；read-worker ×1
（worker fd 移交，G6 残件同族）；eagain/flush ×2（node:test mock，runner 深度）。
 黑盒 fs 20/20（含新增 phase10f_fs_stream_lifetime）+ 冒烟 5/5。

 七轮（2026-09-21，fs 读流轮）：cp/write/read/handle 129 件 **114 SAME/15 DIFF →
 126 SAME/3 DIFF**（118 SAME0 + 8 SAME1）。转绿 12 件：read-stream-throw-type-error
 （end:Infinity 放行 + NaN/小数/负数/超 MAX_SAFE 分流 OUT_OF_RANGE + start>end 精确
 文案）/read-stream.js（同校验 + start/end 暴露 + 真 fd/open 数值 + 缺失文件异步
 error + bytesRead 累加 + fd 定位读 + fifo 单开）/read-stream-inherit（同上全套）/
 read-stream-encoding（encoding 透传基类 StringDecoder + WriteStream encoding 即
 默认编码）/read-stream-pos（live 跟随，已绿复核）/cp socket×2 + async-filter +
 write-err（上轮已绿，本轮复核仍绿）/promises-file-handle-read（length===0 先于
 空 buffer 检查）。残 3 全另案：read-worker ×1（worker fd 移交，G6 残件同族）/
 eagain + flush ×2（node:test mock，runner 深度）。根修三件：fifo 双 open 死锁→
 经已开 fd 全量读（AGENTS §4.167，sample 实锤卡 open(2)）/仅 error 监听够不着懒
 open→A 段微任务先开（§4.169）/旧 SAME1 系 fixtures 缺失掩盖（§4.168）。
 黑盒 fs 26/26（新增 offsets_and_props/read_write_encoding/fh_read_empty/
 fifo_end 四件）+ 全量 `cargo test` 21 target 0 失败 + 冒烟 5/5。

## net

> 五轮对拍（2026-09-17，157 件，`/tmp/wjs-10f-net*.txt`）：
> 同绿 30 → 45 → 66 → 71 → 84 → **86**；DIFF 117 → 88 → 79 → 77 → 61 → **58**
> （net11：2 worker 低并发复核，零回归）。
> 一～四轮（UDS 全链/BoundSocket/blockList+lookup/write 语义/exit 派发/fd 真值/
> listen 校验族/错误形状/server 回指/getConnections/localFamily/bufferSize/
> finish/半开透传/write 校验/resetAndDestroy）见各提交；本轮（五轮）为
> Socket/Server 可观测表面（`tests/node/net.rs::phase10f_net_socket_surface`）。
>
> ### 已修（每项经真机 26.8.2 对拍）
>
> - **_handle 生命周期**（真机 fresh 即 null）：构造 null → `__realConnect`/
>   `__attachConn`/`__attachUds` 建桩 → `destroy`/`__ev-close` 置空；无柄期
>   setNoDelay/setKeepAlive 只缓存不转发（after-close/`cli-close` 套件）。
> - **close(hadError)**：`__hadError` 由 `destroy(err)`/`__ev error` 置位，
>   `__ev close` 透传布尔（reconnect/after-close 套件点名 `false`）。
> - **销后噪声吞派发**：`__ev error` 见 `destroyed` 即吞（`__hadError` 照记；
>   双块并发 1/3 flaky 根除，见 AGENTS §4.125 坑四）。
> - **柄关 vs 柄空双形**（异步 error 事件，非同步抛）：`_handle.close()` 后写 →
>   `Error('write EBADF')`（win 系 EPIPE；`close()` 只标旗 + 延迟 destroy，
>   同步 destroy 会使两态坍缩）；已连接 `_handle = null` 后写 →
>   `ERR_SOCKET_CLOSED('Socket is closed')`；destroyed 面沿用 `__writeErr`。
> - **pipe 最小面**（`socket.pipe(socket)` 回显，write-connect-write 套件）+
>   `unpipe`/`_unrefTimer` 空桩。
> - **TOS**：`setTypeOfService` 校验三文案逐字（ARG_TYPE helper 形 ×2、
>   OUT_OF_RANGE "must be an integer"/"must be >= 0 && <= 255"），链式返回；
>   `getTypeOfService` 读缓存（连接前设置生效）。
> - **setKeepAlive**：位置四参 + 选项对象双形；ms→s 下取整；缺省位转发
>   undefined（非 null）；四元组全同跳过转发（keepalive-interval-count/
>   server-keepalive 套件）。
> - **Server**：`keepAlive`/`keepAliveInitialDelay` 存自身（默认 false/0）；
>   `_handle.onconnection` 接线（`__ev connection` 经此进入，套件可包装观测；
>   keepAlive 经 clientHandle + 已接受 socket 双落）；已接受 socket 预置去重缓存。
> - **autoSelectFamily 存值**：`get/setDefaultAutoSelectFamily`（默认 true；
>   Happy Eyeballs 连接侧未实现，见下）。
>
> ### 剩余红项（DIFF 61，分类）
>
> - **TIMEOUT 9 件**：async-iter/max-connections×2/throttle/write-after-end-nt/
>   bytes-stats/abort-controller/ipv6/listen-handle-cluster/listen-twice——
>   背压/最大连接/中止语义另案。
> - **Happy Eyeballs 3 件**：autoselectfamily-default（::1→127.0.0.1 回落，需
>   `all:true` + 竞速）+ socket-connect-invalid-autoselectfamily×2（校验族）——另案。
> - **worker 投递 3 件**：socket/server-transfer-worker×3（Socket 不可 clone）——另案。
> - **校验族长尾**（~12 件）：boundsocket/connect-options-port/write-arguments/
>   transfer-guards/socket-constructor/server-options/server-listen-options×2/
>   localerror/options-lookup/server-call-listen-multiple——"Missing expected/
>   unexpected throw" 逐 API 续补。
> - **server close/listen 时序**（~10 件）：listen-close-server×2/drop-connections/
>   pause-on-connect/server-blocklist/close-before-lookup/server-pause 等——另案。
> - **远端地址 2 件**：remote-address/-port（六轮已修：客户端发布时序 +
>   服务端 accept 回填本就齐；见上补记）。
> - **大串 2 字节差 1 件**：large-string（40962 vs 40960）——分包边界另案。
> - **环境/双红**：autoselectfamily-commandline-option（w=0 n=1，真机自红）、
>   child-process-connect-reset（spawn ipc 不支持）、dns-error（bogus 域名线）、
>   listen-after-destroying-stdin（stdin.destroy 缺口）等。
>
> 六轮补记（2026-09-17，`phase10f_net_remote_surface`，net11 实测）：
> 远端面发布时序（连接完成前 remote* 全 undefined，完成后回填地址/端口/
> 地址族；remote-address 双件修）+ `connect(addressObj)` 取 address 键
> （ready-without-cb 套件连通；`ready` 事件本身暂不发射，见 AGENTS §4.126）。
> 58 DIFF 均为既有分类（校验族长尾/server 时序/TIMEOUT/Happy Eyeballs/
> worker 投递/大串 2 字节差/环境双红），large-string 单跑同错（确定性残留，
> 非回归）。
>
> 七轮（2026-09-19，校验族长尾 + listen 面）：boundsocket/connect-options-port/
> write-arguments/transfer-guards/socket-constructor/server-options/
> server-listen-options×2/localerror/options-lookup/call-listen-multiple
> **11/11 转绿**（真机逐件）；write(undefined) 翻转
> ERR_INVALID_ARG_TYPE（真机仅 null 走 NULL_VALUES，§4.65）；listen 面三修
> （数字字符串端口 TCP 分流/ALREADY_LISTEN 同步守卫/柄随 close-error 双出口
> 清——AGENTS §4.138），`phase10f_net_listen_surface` 落盒。剩余红项维持
> 下记分类（TIMEOUT/Happy Eyeballs/worker 投递/large-string/环境双红）。
>
> 八轮（2026-09-19，G6 欠账轮）：**13 件转绿**——large-string（分包多字节：
> setEncoding 换持久 StringDecoder，40962→40960）/ allow-half-open-async-iter
> （end(cb) 挂 finish 非 close）/ write-after-end-nt（对端 FIN+本地 end 后写
> → EPIPE 'ended by the other party'，cb/error 均下一 tick）/ connect-abort-
> controller（Socket 构造器 signal 分支补齐 + abort 触发的 destroy 推
> microtask + 直调 addEventListener 入 `__etAdd` 侧表供 listenerCount 读）/
> connect-options-ipv6（lookup opts 透传请求 family）/ autoselectfamily-default
> （HE 串行回落：all:true 拉全地址 + `__heOnErr` 钩吞中间失败 + close 后复位
> 重试 + 保留 __pendW；attemptTimeout 竞速记档）/ HE 校验族 ×2
> （autoSelectFamily boolean 门 + attemptTimeout int[1,60000]→OUT_OF_RANGE、
> setDefault 钳 [10,60000]）/ max-connections×2 + bytes-stats（G2 server
> 选项面已顺手修绿，欠账表滞后）。全量 `test-net-*` 159 件对拍：116 绿
> （+13）/ SAME1 14 / 仅我们红 29（stash 旧二进制红集逐一相同，零回归）；
> black-box 223 全绿。AGENTS §4.148。
>
> **G6 残件（6 件，infra 级，逐件定性）**：throttle（需 native 读门控 +
> 写 EAGAIN 流控——JS 层 pause 不停内核读，npauses>1 无法满足）/
> listen-handle-in-cluster-1 + listen-twice（cluster 协议：
> internalMessage/NODE_CLUSTER 消息 + process.disconnect，非 net 面）/
> socket·server-transfer-worker×3（跨线程 fd 移交底座：net_detach + 信封
> 携带 token + worker 侧 adopt 重建——可骑 BoundSocket holdToken 机制，
> 独立轮）。其余红项为既有校验族/环境 DIFF 池（better-error-messages×2/
> dns-error/reuseport/onread-static-buffer/socket-timeout 系等，与旧
> 二进制红集一致）。

## zlib

> 首轮对拍（2026-09-17，83 件，`/tmp/wjs-10f-zlib2.txt`，2 worker）：
> SAME0=34 + SAME1=13，DIFF=36。二轮（2026-09-18，Zip 归档面）：17 Zip 件中
> 15 全绿 + hardening 转绿 + security-hardening 仅剩 FIFO 自 spawn 环境项
> （`spawnSync -e`，全 flag CLI 设计 ⏭️），见下。
> 首轮两批共修 9 件：流收尾与内部小面 7 件
> （destroy/close-after-error/sync-no-event/invalid-input/zero-byte/
> reset-during-write/brotli-flush-invalid-kind，`phase10f_zlib_stream_teardown`）
> + flush-flags 构造期选项校验与 write-after-close（`phase10f_zlib_flush_opts`，
> 后者为 close 改撕毁的附带修复），单文件逐个实测 exit=0，余约 27 件。
>
> ### 已修（每项经真机 26.8.2 对拍）
>
> - **轮子 framing 双修**（Rust，逐字节对齐）：brotli 去显式 `flush()`
>   （flush 先吐非终结同步块，空输入 3B/非空头尾多包；drop 的 FINISH 即完整
>   终结——空输入 1B、非空与真机逐字节一致）；ruzstd 去 `hash` 特性
>   （content_checksum 给每帧补 4B，空帧 13B；Node/libzstd 默认无校验 9B）。
> - **流收尾面**（纯 JS）：`_handle`（开流 stub 对象，close/destroy 置空）+
>   `_closed`（构造 false，`_destroy`/close 同步置 true）+ `_destroy` 覆写透传 +
>   `close(cb)` 改撕毁语义（真机：不落数据、无 finish/end，只有 close+cb；
>   已销毁则只等 close）+ `reset()` 双形（分发中抛原文、已关闭抛
>   ERR_INTERNAL_ASSERTION，均真机实测）+ `_processChunk`（含 `_chunkSize`/
>   `_outOffset` 越界门）+ `_handle.reset()` 分发中抛原文。
> - **flush kind 逐族校验**：zlib {0,4,5} / brotli {0,1,2,3} / zstd {0,1,2}；
>   undefined/NaN/函数直通；非 number → ARG_TYPE；越界 → OUT_OF_RANGE；
>   另补 `ZSTD_e_continue/flush/end` 常量（0/1/2）。
>
> ### 二轮已修（2026-09-18，Zip 归档面，零新依赖）
>
> - **internal/zip 13 件逐字移植**（constants/binary/dos/extra-fields/
>   content-size/compression/headers/header-builders/fs-util/entry/archive/
>   buffer/file + zlib 接线 + 实验警告使用时一次）：`ZipEntry.create/read`/
>   `createZipArchive(+Sync)`/`ZipBuffer`/`ZipFile`/`zipFiles`/`crc`/zip64/
>   注释放置/安全加固全链；zstd-93 经既有 ruzstd（Fastest），deflate 经 flate2。
> - **附带修**：`inflateRaw`/`zstdDecompress`（Sync + 回调）补 `maxOutputLength`
>   背stop（既有 brotli 独有；Zip 伪造小头回退到"produced…expected"而非
>   "inflates beyond…"现形，hardening 套件钉住）。
> - 回归：`tests/node/zlib.rs::phase10f_zlib_zip_archive`（正常/报错/边界）。
>   套件侧 17 Zip 件 15 全绿 + hardening 30/30；security-hardening 仅剩 FIFO
>   自 spawn（`-e`，全 flag 设计 ⏭️）。
>
> ### 三轮已修（2026-09-19，G9-3 欠账轮收官）
>
> - **增量语义 6 件转绿**（G9-1 Rust 状态机 + G9-2 JS 流类接线，见 plan3 §5）。
> - **G9-3 尾件 4 件转绿**（本轮）：
>   - **dictionary**：raw 族字典流 "repeated call with bad state"——RawInflate
>     改构造期主动 `set_dictionary`（zlib 语义：raw 无 FDICT 头，字典必须在
>     首次 inflate 前设）；zlib 族被动 NEED_DICT 恢复保留（dictionary-fail
>     套件文案依赖）；Buffer/ArrayBuffer/Uint8Array/DataView 四源 + reset
>     组合全过。模块单测 zdbg 4 件 + 黑盒 `phase10g_zlib_dict_pledged_webstream`。
>   - **brotli-dictionary**：一次性压缩改走引擎收口（dict/quality/错误口径
>     单点化；切前字节对比——同 quality 下与裸 native 逐字节一致）；
>     字典严格校验 `__zDictBytes`（'string' 是合法数据输入但非法字典，
>     ERR_INVALID_ARG_TYPE）。
>   - **zstd-pledged-src-size**：`pledgedSrcSize` 全校验面（'1'/null→ARG_TYPE、
>     NaN/±Infinity/非整数/负/MAX_SAFE+1→OUT_OF_RANGE）+ 引擎终检
>     （mismatch→`ZSTD_error_srcSize_wrong`，`err.errno=72`）+
>     `constants.ZSTD_error_*` 28 项真机逐项入库。
>   - **type-error**：Web `CompressionStream`/`DecompressionStream` 落地
>     （prelude 全局 + node:stream/web 导出）：4 格式 roundtrip、尾垃圾
>     readable TypeError（pipeTo cancel 语义——错误必须 ctrl.error，
>     write 拒绝走 cancel 链读不到，AGENTS §4.147）、format 枚举 TypeError
>     文案逐字、proto 链独立（不继承 TransformStream）、toStringTag。
>   套件点名 6/6 全绿；全量 82 件对拍零回归（11 件 SAME1 双红 + zip 自 spawn
>   2 件 + brotli-16GB 资源门控，均 node 同红或既有 ⏭️）。AGENTS §4.145-147。
>
> ### 剩余红项（SAME1 双红 + 既有 ⏭️，无欠账）
>
> - 11 件 SAME1 双红（node 26.8.2 同红：internals/fs 环境面——test-zlib.js/
>   zstd.js/params.js/from-gzip.js 系等）；`test-zlib-zip-internals.js`
>   node 红、本仓绿（反向）；zip-experimental-warning / zip-security-hardening
>   （自 spawn `-e`，全 flag CLI 设计 ⏭️）；brotli-16GB（16GB 无背压解压
>   OOM abort，本机资源门控 ⏭️，属流式分块/背压欠账，net/http 同族另案）。
> - 增量语义 6 件与杂项 4 件（brotli-dictionary/zstd-pledged-src-size/
>   type-error/增量解码面）已全部转绿，见上两轮记录。

## child_process

> 首轮对拍（2026-09-17，110 件，`/tmp/wjs-10f-child3.txt`，2 worker，
> 含 `test/fixtures` 稀疏检出——无 fixtures 时 SAME1 虚高 18→8）：
> SAME0=12 + SAME1=8，DIFF=90。本轮（同步族口径，
> `tests/node/child.rs::phase10f_child_sync_surface`）修 10 件：
> spawnsync-validation-errors/timeout/input/maxbuf/spawnsync/args/env +
> execfilesync-maxbuf/execsync-maxbuf/spawn-argv0（单文件逐个实测 exit=0），
> 余约 80 件。
>
> ### 二轮（2026-09-19，exec/abort/stdio 面）
>
> - **exec 族**：callback 可缺席（返回 live child）、callback 非函数 ARG_TYPE、
>   execFile 的 shell 透传、execvp 预检 ENOENT 异步回调（pid undefined）、
>   timeout/killSignal 在 execFile 层、exec 编码透传、promisify.custom 的
>   customPromiseExecFunction 逐字形（promise.child + 同步抛不吞）。
> - **AbortSignal 中断面**：spawn/fork 预中止（线程不起、error(AbortError) +
>   exit/close(null, killSignal) 合成）、signal 非法型校验、abortcontroller
>   套件 `{ name, cause: DOMException }` 逐键 deepStrictEqual（assert.throws
>   对象校验器从 `==` 升级）。
> - **stdio 面**：stdout/stderr legacy Readable（setEncoding/on('data')）、
>   stdin legacy Writable（真机 Socket 写半部：write/end 直调——旧 Web
>   WritableStream 形退役，§4.65 翻转）、fork 非 silent stdio 三面恒 null
>   （真机逐项；§4.139）、fork silent 管形流（pipe/unpipe 形状在，数据面
>   偏差记档）。
> - **连带根修**（§4.137，AGENTS）：eval/模块入口失败 + 开着句柄 = 事件循环
>   永不收割的 hang——eval wrapper rejection 重抛 + entry reactions 挂载 +
>   event_loop unhandled 表检查点（`phase10f_entry_failure_open_handle_exit`，
>   修前 alarm 打不到头）。
> - 对拍：exec-encoding/exec-timeout-kill/exec-timeout-expire 转绿；
>   `phase10f_child_exec_shell_self_and_timeout`/`spawn_abort_and_surface`/
>   `stdin_legacy_and_fork_silent` 三 phase 落盒。
> - 剩余红项（下轮）：exec-maxbuf 的多字节截断面（2026-09-21 实测与真机一致，
>   早先 1/7 flake 后稳定，销账）、test-child-process-stdio 校验长尾
>   （spawn-typeerror/stdio 流转交已修，销账）、fork/IPC handle 传递
>   （底座另案，收敛为 4 件出局，见三轮）、windows 专属。
>
> ### 已修（每项经真机 26.8.2 对拍）
>
> - **自举翻译**（全 flag 铁律所迫，CLI 禁 `-e` 别名）：子进程即自身
>   （`file === execPath`）时 argv 映射——`-e X` → `--eval X`、裸文件 →
>   `--run <file>`；shell 串首（`"<execPath>" -e/-p/-pe`，`$NODE` token 同理）
>   同规则改写；他家二进制原样透传（`cat` 等真程序不受影响）。
> - **同步族错误形状**：`syscall` 带命令（`spawnSync <file>`）、`error.message`
>   `spawnSync <file> <CODE>`、`error.path`、errno 表（ENOENT -2/EACCES -13/
>   ENOBUFS -55/ETIMEDOUT -60，余 -4094）、`error.spawnargs` 为参数数组、
>   起 spaw 失败 `pid` 为 0（非 -1）、失败 `output` 为 null（成功才
>   `[null, stdout, stderr]`）。
> - **缺省 Buffer**：spawnSync/execSync/execFileSync 缺省回真 Buffer
>   （deepStrictEqual 裸 Uint8Array 即不等）；exec **异步**缺省仍 utf8——
>   三处缺省各不同，禁想当然统一（旧测试两处伪语义已翻转）。
> - **选项校验族**：cwd/argv0（string）、detached/windowsHide/
>   windowsVerbatimArguments（boolean）、shell（boolean/string）、uid/gid
>   （非负整数，值忽略）、timeout（非负整数）、maxBuffer（非负数/Infinity
>   不限/小数下取整）、killSignal（类型先行 ARG_TYPE，再查 os.signals 表落空
>   即 ERR_UNKNOWN_SIGNAL）、input（string/Buffer/视图/ArrayBuffer）。
> - **killSignal 落地**：kill_signo 数字通道（libc 直杀，免枚举表）+
>   kill_signame 回显；缺省 SIGTERM 并阻塞 wait（旧 SIGKILL 直杀为伪语义，
>   旧测试已翻转）；detached 组杀同信号；argv0 经 unix arg0（+ runtime 取真
>   argv[0]，自举回显对拍）。
> - **管道死锁根修**：同步等待与读出并发（读出线程先行；等退出后才读 =
>   1MB+ 输出永挂，maxbuf 套件现形）。`maxBuffer: Infinity` 即不限。
> - **stdio 透传起步**：inherit 即继承不捕获（spawnchild 链路必需）；
>   ignore/pipe 按旧路；校验面另案。
> - **ChildProcess.emit**（exit/close/error/spawn 四位，经访问器 wrap）
>   + `args=null` 不吞 opts（四处同修，含 async）。
>
> ### 三轮（2026-09-21，child 尾件收官：作用域 76 件 SAME0=54/SAME1=6/DIFF=16 → DIFF=4）
>
> - **参数归一逐字**（spawn-typeerror 转绿）：spawn file/args/options
>   （validateString/空 ARG_VALUE/纯对象回落/显式 null 与数组 ARG_TYPE）+
>   uid/gid validateInt32（非数 ARG_TYPE、非整数/超 int32 RANGE，逐字节对真机）+
>   execFile normalizeExecFileArgs 重写（args 留位/options 数组拒收/callback
>   带 Received）+ fork（缺席即[]/纯对象回落/余下非数组 ARG_TYPE-Array、
>   options 数组拒收）；黑盒 `phase10f_child_spawn_arg_validation`。
> - **kill 转绿**：legacy 流 end/close 单监听亦起泵 + kill(0) 存在性短路
>   （`Signal::try_from(0)` 落空 SIGKILL 误杀，改 `kill(target, None)`；
>   killed=true 语义保留）+ stdin 轮询投递（`__wjs_stdin_poll` nix safe
>   非阻塞读三态 + 首监听起 refed 轮询，EOF 自停；TTY 归 REPL）+
>   stdout/stderr 逐次 flush（Rust 块缓冲致常驻进程输出滞留）。
>   黑盒 `phase10f_child_kill_stdin_surface`。
> - **stdio 流转交三件**（merge/reuse/pipe-dataflow 转绿）：数组流对象元按位
>   搭桥（stdin 位 data/end 转入、stdout/stderr 位只转 data 不转 end）+
>   `_handle.readStart` 兼容桩 + 流 end 递延 close + 进程 close 记录迟挂重放。
>   黑盒 `phase10f_child_stdio_stream_handoff`。
> - **fork env 透传**（net-reuseport 转绿兼 fork 炸弹根除）：`__normForkOpts`
>   存 env 拷贝 → `new Worker(..., { env: o.env })`；net `__doListen`
>   reusePort 直通 direct 路径（BoundSocket-adopt 早有，补齐）。
>   黑盒 `phase10f_child_fork_env_and_internal`。
> - **internalMessage 分流**（internal 转绿）：`cmd` 首段 `NODE_` 即内部消息
>   面（+ 事件位注册/off/removeAllListeners 全表）。
> - 另：exec-maxbuf 早先 flake（负载下 1/7）后 7/7 稳定，多字节截断口径
>   与真机一致（str slice 字符数），不另修。
>
> ### 剩余红项（4 件，均为 handle 传递，出局：Bun 🟡 IPC 缺口同缺 + 拍板维持）
>
> - send-keep-open（live socket 跨会话写）、server-close（socket 作 stdio、
>   另 hang）、recv-handle/send-returns-boolean（4 元 stdio + 句柄 +  backlog
>   记账）。底座为同进程线程，token 共享理论可行，但跨会话状态机/GC
>   风险高且系拍板出局项，不做。
>
> ### 剩余红项（约 80，分簇；2026-09-21 三轮后收敛为上之 4 件，以下为历史存档）
>
> - **fork/IPC 约 25 件**（多 TIMEOUT）：handle 传递（send/dgram/net-server
>   共享）、高级序列化、ipc-next-tick——线程底座 IPC 语义另案。
> - **async 中断面**：AbortSignal（execFile/exec abort/预中断/非法型、
>   spawn-controller、fork-abort）——独立特征轮。
> - **async 句柄面**：silent/stdin（stdio 句柄 write/pipe）、exit-code/cwd
>   （事件序）、server-close 等——live 句柄轮。
> - **exec 字符串族**：spawnsync-shell（DEP0190 + `-pe` shell 串映射 + 平台 mock）、
>   exec-encoding/timeout 系列——随 async/exec 轮。
> - **零散**：argv0 message 全文（已顺带对齐）、windows 专属、dgram-reuseport 等。

## crypto

> 首轮对拍（2026-09-17，133 件，`/tmp/wjs-10f-crypto1/2.txt`，2 worker）：
> 同绿 17 → **24**（+7，见下）；二轮（`/tmp/wjs-10f-crypto3-8.txt`）同绿 24 → **38**
> （+14，见下）；SAME1=8（双边同码）；DIFF 108 → **87**。
>
> ### 三轮已修（2026-09-18，x448 采购落地）
>
> - **x448 全链**（用户拍板引 `x448 =0.14.0-pre.12` + `static_secrets`，
>   零新增传递依赖，见 `docs/dependencies3.md` §5）：三 native（生成即 RFC 7748
>   clamp、PublicKey 派生、DH 经 `x448()` 自带低阶点检查）+ OKP DER 层通用化
>   三档（32/56/57B 统一公式，X448=1.3.101.111）+ JWK crv X448 双向 + raw
>   private/public 导入导出（四 OKP 键通用；缺 asymmetricKeyType → ARG_TYPE、
>   类型错/坏长 → Invalid key data、raw-public 建私钥 → format 无效、非 OKP →
>   INCOMPATIBLE，真机 26 逐项）+ diffieHellman 低阶点抛
>   `ERR_OSSL_FAILED_DURING_DERIVATION`（`error:1C8000A4` 文案逐字）+ sign/verify
>   无原语门 + 证书 SPKI 头 + `generateKeyPairSync('nope')` 改真机原文
>   （`The argument 'type' must be a supported key type.`，旧自编文案退役）。
> - 回归：`tests/node/crypto.rs::phase10f_crypto_x448_parity`（19 断言，含
>   fixture PEM 逐字/JWK deepStrictEqual/DH 双侧一致/低阶点/x25519 无回归）；
>   单测 `x448_dh_cross_checks`（RFC 7748 向量 + 全零 None）。
> - 套件侧：`test-crypto-key-objects.js` x448 行断言经 fixture 探针逐项验绿
>   （PEM 逐字/JWK/raw 段）；整文件仍红在 RSA pkcs1 段（313 行族，见剩余簇）
>   —— 对拍计数不变，阻塞项消一块。
>
> ### 首轮已修（每项经真机 26.8.2 对拍）
>
> ### 已修（每项经真机 26.8.2 对拍）
>
> - **call-without-new**：Hash/Hmac/Cipheriv/Decipheriv/ECDH/DiffieHellman 可无
>   new 调用（内类改 `XImpl` + 同名包装函数 + prototype 接线；`instanceof`/
>   方法面不变）+ `DiffieHellmanGroup` 新导出；Hash/Hmac 附 DEP0179/DEP0181
>   一次性警告（`deprecate()` once 语义，真机 `length 2/name deprecated` 伪装不抄）。
> - **randomUUID/v7 选项校验**：非对象 → ARG_TYPE（null/undefined special-case），
>   disableEntropyCache 非布尔 → ARG_TYPE（真机文案逐字）。
> - **摘要别名**：dss1→sha1、ripemd→ripemd160（JS 表 + Rust `norm_hash`）+
>   sha224 原生（RustCrypto `sha2::Sha224`，零新依赖；update/finalize/copy
>   三 match + `crypto_hash_norm_table` 单测）。
> - **'buffer' 编码**：digest/DH-getters 系 'buffer' 即回 Buffer；Hmac 二次
>   digest 形态（undefined/'buffer' 精确小写回空 Buffer，其余回 ""；Hash 系恒抛
>   FINALIZED，真机逐项对拍）。
> - **最小流式鸭子面**：Hash/Hmac/Cipheriv/Decipheriv 加 write/end/read/
>   readableLength（cipher 系 end 须拼 update-head + final-tail，丢 head 即少块；
>   真 Duplex 的 pipe 等另案）。
> - **ECB 三档**：Rust `CipherJob::Ecb`（`aes` 轮子块直调，`[u8; 16]` 中转，
>   零新依赖）+ 流式 update/final（解密 autopad 扣尾块）+ iv 规则（undefined
>   真机文案/null 视为空/ECB 仅空）+ nid 418/422/426 + getCipherInfo 去
>   ivLength 键 + `cipher_params_table` 单测。
> - **DH 数值形**：构造期经 `generatePrimeSync` 同步生成 + 字符串 generator
>   编码位忽略（旧 `Number("buffer")=NaN` 记档修）。
> - **校验文案**：`__needStr` null/undefined 的 Received 无 type 前缀；Hmac
>   参数名 "hmac"；`__needCipher` 非串先报 ARG_TYPE；`__outBuf` encoding 先
>   `String()` 显式转（用户 toString 抛错透传，坏编码串仍回 Buffer）；
>   超长 update（≥2^31-1）无码错（nodejs/node#45757，精确边界）。
> - **outputLength 全套**：`__checkOutputLength`（非数 ARG_TYPE/非整数或越界
>   OUT_OF_RANGE，真机逐字，0..2^32-1）+ 非 XOF 须恰为摘要长（NOT_XOF 原文）
>   + `copy(options)` 改长（XOF 经新 native `__wjs_crypto_hash_set_len`；
>   无参 copy 回默认长，真机口径）。
> - 回归：`tests/node/crypto.rs::phase10f_crypto_round1_parity`（40 断言，
>   正常/报错/边界三件）。
>
> ### 二轮已修（+14：dh×3、RSA×6、导入导出×5）
>
> - **DH 组与 flavor**：modp1/modp2 素数（真机逐字节；手抄丢字节即环错，
>   改脚本精确替换）+ Group 自立原型（constructor 归 Group、setters 置
>   undefined，余法继承）+ generator 字节语义（三参；`'02'` hex 解 2，
>   `'02'` 裸串即 0x3032，真机逐项）+ 数值 `getPrime('buffer')` 即回 Buffer。
> - **RSA 位长**：下限 512（真机口径；subtle 面 JS 门不动）+ `asymmetricKeyDetails`
>   （modulusLength + publicExponent；导入键从材料现算）+ 位长过小真机原文。
> - **KeyObject 四层链**：Secret/Public/Private/AsymmetricKeyObject（真机原型
>   形状）+ WeakMap 状态（实例零自有属性；131 处 `x.__foo` 文本零改动，经原型
>   访问器）+ 原生品牌（`isKeyObject`/getters 全认品牌；`instanceof` 保持纯原型
>   语义，15 处内部点改显式 `__isKeyObject`）+ `equals`/`from`/`toStringTag`/
>   构造校验/secret 无参裸回。
> - **pkcs1 双向**：导出（RSAPublicKey/RSAPrivateKey 内层提取 + 标签）+ 导入
>   （9/2 整数按内容区分；公钥料作私钥报 DECODER 原文）+ 公钥侧私钥材料派生
>   （`publicEncrypt(privPem)` 同款）+ 导出 type 矩阵（RSA 公 pkcs1/spki、
>   私 pkcs1/pkcs8）+ ESM 具名导出补齐（Hash 等 10 名，真机导出表口径）。
> - **加密 PEM**：EVP_BytesToKey/MD5 + AES-CBC（零新依赖；双向真机交叉）+
>   缺口令/错口令 openssl 3.x 原文（套件点名；`__pemDecode` 头行感知）。
> - **sign/verify 补齐**：callback 异步形；验签形态错回 false（ed/RSA/EC/DSA；
>   空签名套件点名）；x25519 无原语错（摘要校验之前）；RSA 钥短错真机原文；
>   `rsa_sign` panic 根修（`sign`→先验长度，139 转可读错）；Sign/Verify 类
>   rest 串改输出/签名编码 + key-options 整体透传（passphrase 同）。
> - **编解码面**：`hash()` 的 'buffer' 编码；`readFileSync` hex/base64 解码；
>   证书提 SPKI；key/der 的 `encoding: 'hex'`（key 串 + data 串双解码）；
>   `createPublicKey` 私钥材料一律派生；JWK 私钥规则（缺 d/坏参 INVALID_JWK
>   真机逐字）；显式 type 门（priv+spki 等）；空串 DECODER 原文。
> - **混合 OAEP**：`mgf1Hash` 校验 + JS 编解码（几何经真机预言机定案：种子长
>   取 oaep 哈希长；自交 + 双向真机交叉）+ 私钥加密/公钥解密反向 natives
>   （OAEP-SHA1/v1.5）+ NO_PADDING 裸 RSA + 默认 padding 按方向（加解密
>   OAEP/签式 v1.5）+ OAEP 校验矩阵（oaepHash/oaepLabel 真机逐字）+
>   解密失败/超长真机原文。
> - 回归：`tests/node/crypto.rs::phase10f_crypto_round2_parity`（34 断言）；
>   旧伪语义翻转（copy 默认长/DH 数值形/shake 负长；§4.65/§4.82 再进宫）。
>
> ### 四轮已修（2026-09-18，key-objects 阻塞簇）
>
> - **导出门矩阵**：format 门（secret∈{undefined,'buffer','jwk'}；非对称∈
>   {'pem','der','jwk','raw-private','raw-public'}，其余 ARG_VALUE）+ type 门
>   （未知/缺 type→ARG_VALUE；public+pkcs8/sec1、private+spki→ARG_VALUE；
>   pkcs1 非 RSA、sec1 非 EC 私钥→INCOMPATIBLE；真机 26 逐项）+ RSA pkcs1/
>   EC sec1 导出（sec1 头 `307702010104200f` 逐字节）+ typeless 旧断言翻转
>   （mlkem `mk-pem` 等，§4.65/§4.82）。
> - **EC raw**：raw-private=定长标量、raw-public=`04||X||Y`（P-256 32/65B；
>   别名 'P-256' 同收，'secp256r1' 拒 INVALID_CURVE；坏点/压缩形/尺寸错位
>   逐项）+ 派生收口（raw-private 建公钥由私钥派生）。
> - **details/JWK**：asymmetricKeyDetails（EC→OpenSSL 名、OKP→{}、DSA→
>   {modulusLength,divisorLength}）+ OKP/EC JWK 校验矩阵（x/y 对派生点、
>   缺 d/crv 非法曲线等，真机逐项）+ DSA 无 JWK 面（导出
>   JWK_UNSUPPORTED、导入 INVALID_JWK；旧 `d-jwk` 伪语义翻转）。
> - 回归：`tests/node/crypto.rs::phase10f_crypto_round4_parity`（50+ 断言）；
>   坑见 AGENTS §4.133（`Uint8Array.equals` 两处）。
>
> ### 五轮已修（2026-09-18，raw 门收敛 → `key-objects-raw.js` 全绿）
>
> - **私钥加密门最前**：`passphrase` 有即仅 pem/der 放行，其余
>   （jwk/raw-*/未知/缺 format）一律 INCOMPATIBLE `... does not support
>   encryption`（`'banana'`/`undefined` 逐字；公钥/secret 侧忽略；
>   cipher 单给忽略——jwk 旧 `||cipher` 收窄，附带修）。
> - **raw-seed**：format 门收编；导出 ml 私钥走既有 seed natives、
>   余下一律 INCOMPATIBLE（公钥侧 kind 门先判 ARG_VALUE）；导入同走 akt
>   链后 INCOMPATIBLE（ml 见下）。
> - **字符串导入拒收**：raw 导入字符串 key 即 ARG_TYPE（28 码点截断规则
>   实测：超长截前 25 + `...`）；`createPrivateKey/PublicKey` 的 encoding
>   预解码限 pem/der 系（raw 系先解码即洗白，门永不触发）。
> - **ml raw**：公钥 raw-public = SPKI BIT STRING 裸料（768→1184B 等）；
>   私钥 raw-seed = PKCS#8 种子（kem 64/dsa 32，既有 natives）；导入按集验长
>   （错位 ARG_VALUE 'Invalid key data'，料回包 DER 存，derive 全通）；
>   raw-private 一律 INCOMPATIBLE；展开形 PKCS#8
>   （OCTET{ SEQ{ OCTET(seed), … } }，`ml_dsa_44_private.pem` 指纹）进
>   `mlkem_pkcs8_seed`（Rust 单测钉住）。
> - **slh 装载**（128f/192f 指纹 OID，余集仍 Invalid）：SPKI/PKCS#8 试解链
>   + raw 尺寸门（pub 32/48、priv 64/96；raw-seed 私钥 INCOMPATIBLE）。
> - **DH 装载**：PKCS#8 dhpublicnumber OID 建 KeyObject（p/g 不校验，
>   raw 门只认 keyType）。
> - **EC 压缩点**：导出 `type: compressed/uncompressed`（缺省非压缩；
>   非法 type ARG_VALUE inspect 口径；raw-private 无视 type）+ 导入
>   02/03（新 native `__wjs_ec_import_compressed`，轮子内解压+上曲线校验，
>   p256/p384/p521/k256 全直引零新增）/06/07 直通（坏前缀/错长 ARG_VALUE）；
>   压缩输出与真机逐字节一致（P-256/P-384 实测）。
> - 回归：`tests/node/crypto.rs::phase10f_crypto_raw_seed_parity`（30 断言）；
>   套件侧 `test-crypto-key-objects-raw.js` 由 DIFF 转 SAME0；
>   坑见 AGENTS §4.134。
>
> ### 六轮已修（2026-09-18，PSS 全链 + 加密 PEM + DSA 装载宽容 + 入口门）
>
> - **RSA-PSS 装载**：PKCS#8/SPKI 的 rsassaPss alg 直判 + 归一化 plain-RSA 存料
>   （轮子只见 plain-RSA；params 缺席即无约束，空/显式 params 落 detail；
>   未知哈希/mgf/非法 trailer 即拒解）+ details 约束面（有即透传）+
>   导出回贴（stash params 原文/缺席裸 OID，round-trip 逐字节稳定；生成键同形）+
>   JWK 拒收（UNSUPPORTED_KEY_TYPE）+ Details 生成键裸 OID 回贴。
> - **PSS-SHA1**：`sha1_010` 改名直引（用户拍板，lock 内已有零新增，
>   见 dependencies.md；MD5 无 0.10 可引维持不支持）+ JS 门放行 SHA-1
>   （MD5 维持 NOT_SUPPORTED）+ sign/verify 缺省 salt 取键约束值。
> - **PSS 约束执行**：salt 下限先、digest 后（套件 1007 行钉序）+
>   真机码逐字（DIGEST_NOT_ALLOWED/PSS_SALTLEN_TOO_SMALL）。
> - **MGF1 自动切换**：手组 EMSA-PSS（既有 `__dgstBytes/__mgf1Bytes/
>   __wjs_rsa_raw`，零新 native；RFC4055 §3.1/3.3）+ 双向真机交叉
>   （自签互验；侧信道非恒定时间记档）。
> - **PBES2 解密**：`ENCRYPTED PRIVATE KEY`（PBKDF2 + AES-CBC/DES-EDE3，
>   既有 kdf/cipher natives；缺口令/超 1024B→INTERRUPTED，解密失败→
>   BAD_DECRYPT）+ 展开形 PKCS#8 进 `mlkem_pkcs8_seed`。
> - **DSA 装载宽容**：轮子拒收非标准尺寸（1088/160）时装载期纯解析建对象
>   （真机装载宽容；`__cryptErr` 全大写门窄匹配两条 DataError 文案）。
> - **入口门**：`key.format`/`key.type` 未知值门（inspect 形 Received；
>   type 门仅 DER 系）+ JWK key 非对象门（28 码点截断同 raw 门）+
>   `?? key` 回退退役（null/undefined 直透）+ 私钥 passphrase 缺 cipher 门 +
>   `getCurves` 去 OKP 名（真机/自家 ECDH 一致；`ec`+OKP 指引文案退役，
>   改 INVALID_CURVE；自家 `curves` 断言翻转）。
> - 回归：`tests/node/crypto.rs::phase10f_crypto_pss_gates`（16 断言）；
>   套件侧 `key-objects.js` 余唯一红块（JWK-unsupported-curve，能力偏离下记）。
>   坑见 AGENTS §4.135。
>
> ### 剩余红项（分簇，均下轮或另案）
>
> - **key-objects.js**：六轮后余唯一 JWK-unsupported-curve 块（`assert(namedCurve)`
>   空值：本仓仅 NIST 四曲线，`getCurves` 无 JWK 不可表示曲线——能力偏离，
>   非语义缺口；同文件其余块全绿）。
>   （恒定位 `313:53` 系 assert 内部抛点，非套件位置——二分截断定位）。
> - **校验长尾**（~15）：Missing expected / unexpected throw 逐 API 续补。
> - **密钥导入零散**（pkcs1-pub 显式形、pub+pkcs8 wrong-tag 等 type 门全表），下轮。
> - **真流式面**（hasher pipe/dest.on/tls.Server 无 new/stdio 桩），另案。
> - **subtle 面**（RSA-OAEP SHA-1 等 WebCrypto 差集），另案（`oaep-zero-length`）。
> - **零散**：scrypt 钥长、raw-public 导出、ECDH getters 的 'buffer'、
>   outputEncoding 对象形等，下轮。

## http

> 两轮对拍（2026-09-18，404 件，`/tmp/wjs-10f-http1/2.txt`）：
> 同绿 95 → **125**（+30）；SAME1=13（双边同码）；DIFF 296 → **266**。
> 首轮 DIFF 聚类：TIMEOUT 97 / createConnection 缺失 13（假 socket 用例全灭于
> ECONNREFUSED:80）/ req.setTimeout 等方法面 ~100 / server timeout 三件套
> 属性缺失 13+ / 400 Bad Request 未发射 6+ / 内部别名 `_http_*` 等。
> 回归：`tests/node/http.rs::phase10f_http_parity_round1`（13 组面，
> 正常/报错/边界）。
>
> ### 首轮已修（每项经真机 26.8.2 对拍）
>
> - **Agent 函数化（no-new）**：`http.Agent({...})` 裸调用合法（keepalive-client/
>   free/override 套件）；BaseAgent/子类同改（函数 + `__init` + 原型链挂 EE）。
> - **createConnection 钩**：agent 级（node lib/_http_agent.js 口径，同步回值/cb
>   双形态，settled 旗防双取）+ request 级 `options.createConnection` 绕 agent；
>   假 Duplex 黑洞 socket 全链（client-readable/generic-streams 套件）。
> - **Agent 键位统一 getName 形**：`host:port:localAddress(:family)`（缺省位仍带
>   分隔冒号，agent-getname 套件）；池键/release/acquire/addRequest 全同键；
>   ClientRequest 缺省 host 127.0.0.1 → `localhost`（node 口径，键位一致性）。
> - **addRequest 落地**：freeSockets 直投复用 / 建连 / maxSockets 排队三路
>   （agent-uninitialized 套件：外部直塞 freeSockets + addRequest）。
> - **server timeout 三件套**：`requestTimeout`(300000)/`headersTimeout`
>   (min(60000, requestTimeout))/`keepAliveTimeout`(5000)/`keepAliveTimeoutBuffer`
>   (1000) 选项持久化 + `validateInteger` 形校验 + headersTimeout > requestTimeout
>   即 ERR_OUT_OF_RANGE；`server.timeout`/`setTimeout(msecs, cb)`（per-socket 单发
>   idle timer + data 到达重臂，'timeout'(socket) 透传）。
> - **408/400 发射**：头未齐过 headersTimeout → `HTTP/1.1 408 Request Timeout`
>   精确字节 + 销毁；体在途过 requestTimeout 同；请求行/头行校验（RFC token 形
>   + 头行无冒号即拒）失败 → `HTTP/1.1 400 Bad Request` 精确字节（管线残渣
>   "hello world\r\n" 在行终结即 400，不等 \r\n\r\n——blank-header 套件）。
>   消息期计时器不因部分数据重置（interrupted/delayed 系套件）；ka 计时器只在
>   响应完成后臂（体齐响应未完时挂 ka 会误杀在途响应）。
> - **IncomingMessage.setTimeout**（转发 socket）+ 客户端 `res.socket`/
>   `res.req`/服务端 `req.res` 回填 + `res.req = req` 双向。
> - **状态行无短语**：`HTTP/1.1 200\r\n` 合法，statusMessage 空串
>   （response-status-message 套件）。
> - **flushHeaders 双侧**：req 连通即发头/未连通记 `__forceHead` 位（连通时
>   `__tryFlush` 兜住）；res 立即 chunked 头 + holdback 首块刷出。
> - **ClientRequest 面**：`setNoDelay`/`setSocketKeepAlive`（deferToConnect 语义，
>   pending 位 attach 落地）/`setTimeout`（once('timeout') + socket 转发）/`clearTimeout`/
>   `getPort`/`getHost`/`'socket'` 事件 + `options.timeout` 存储；`.port` 移出自有
>   属性（真机 `req.port === undefined`，取值走 getPort）。
> - **OutgoingMessage 独立可用**：基类 `_write` 缓冲不落盘（cb 不调，writableLength
>   保持——node outputData 口径）+ `_implicitHeader` 桩；`assignSocket` +
>   ERR_HTTP_SOCKET_ASSIGNED 双拒（构造首参是 req 形对象非 socket，非 socket
>   一律不入 `__sock`）。
> - **头+首块合并写**：`res.end(data)` 头未发时 head+body 合并为一次 socket
>   write（node `_send` 合并口径；standalone 套件断言单 chunk 以 body 结尾）。
> - **closeIdleConnections/closeAllConnections**（空闲=无在途 req）。
> - **path 校验**：控制字符/空格即 ERR_UNESCAPED_CHARACTERS（node
>   INVALID_PATH_REGEX 同款，errors 表补码）。
>
> ### 剩余红项（DIFF 266，分簇）
>
> - **TIMEOUT 110 件**：expect-continue/upgrade 深件/trailer/管线背压/
>   max-connections 系——多数需流式深化，另轮。
> - **校验长尾 ~30**：`Missing expected exception`/`throws: unexpected throw`/
>   write-after-destroy 时序等逐 API 续补。
> - **假 socket 深件 2**：`socket.push`（net.Socket 非 Duplex，read-in-error/
>   header-overflow）——需 net.Socket 流式化，另轮。
> - **内部别名**：`_http_common`/`_http_server`/`_http_agent`（HTTPParser 内省面，
>   2+ 件）——映射无谓（我们无 llhttp），记档偏离。
> - **ECONNREFUSED localhost:80 残 7 件**：createConnection cb 形/timeout-option
>   交叉——逐个另查。
> - **chunk 限深 2 件**：chunk-extensions-limit/extensions 总量限（llhttp
>   计数语义）。
> - ** flakes**：full-response 并行跑偶发（单跑 rc=0，重负载族 §4.126）。
>
> ### G11 TIMEOUT 轮增量（2026-09-22，18 提交；http 域黑盒 17/17 + 冒烟 5/5）
>
> > - **请求超时全家**：`setTimeout` 补 `timeoutCb` 武装、`defer-to-connect`
> >   （socket 事件见构造期值、connect 后见覆写值）、请求级覆盖 agent 级、
> >   finish 后 noop、keepSocketAlive 可覆写 + 池超时自毁、onTimeout 单例、
> >   `net Socket.setTimeout` 发布 `.timeout`、先入池后发 free、回池清请求级超时。
> > - **101 摘池 + Trailer 校验**：upgrade 先摘 agent 池再发事件（req close 异步随后）；
> >   非 chunked 带 Trailer 同步抛 `ERR_HTTP_TRAILER_INVALID`（自动 chunked 合法）。
> > - **管线面**：前导空行连发、残缺头 requestTimeout 期 408、超 max 回 503 +
> >   `dropRequest`、升级双侧 parser 释放、毁后写丢弃、`_last` FIN 递延一轮
> >   （GET 确定性 503 根因）。
> > - **流出面**：`_write` 异步回（背压）、end 重复语义（ALREADY_FINISHED/
> >   WRITE_AFTER_END，不毒化 errored，抛错回滚 `__userEnded`）、capture 接线 +
> >   destroy 透传、空闲判定补 `st.res`。
> > - **abort 级联**：双侧 `aborted` + `ECONNRESET('aborted')`（有监听才发 error 且递延）。
> > - **1xx/头形态/agent**：writeInformation/Processing/EarlyHints；数组形 headers/
> >   setHost/原拼写/noDefaults；maxHeadersCount 接收截断（双侧 null）；
> >   默认 keep-alive 修正；回池门（close 响应不入池）+ 排队续行；池键无残留 +
> >   取消递延；`socket ready` 事件解禁（connect 后同步）；setTimeout 门控改
> >   `res.readableEnded`（前版误用请求 finish，响应中恒 noop）。
> > - **转 SAME**：client/agent 超时全家 14 件 + upgrade-agent/de-chunked-trailer +
> >   pipelining/upgrade-parser/max-requests/incoming-destroy + outgoing-finish/
> >   end-multiple/end-types/capture + aborted 全块 + information×2/early-hints×2 +
> >   automatic-headers/drop-requests + dont-set-default×3 + max-headers-count +
> >   get-pipeline-problem + abort-queued/get-pipeline（约 40 件）。
> > - **计数**：http-only TIMEOUT 82→**63**、DIFF 105→**100**
> >  （sweep 05:47–06:32，跨 06:09/06:28 两次构建，混二进制仅当趋势；终局需干净重扫）。
> > - **未闭环**：drain-writable-length（需 eager-parse + 无 socket 排队架构，另轮深水）；
> >   server-keep-alive-timeout 6/6 hang（done 内 destroy + server.close 同 tick 撞
> >   native 收尾，ServerClose 丢失）；偶发 park 错过唤醒（0% CPU parked，看门狗不触发，
> >   疑与上同源，需 Rust 侧深入）。
>
> ### G11 半开双杀（2026-09-22，5 提交；AGENTS §4.186）
>
> > - **写端等读端死锁**：`Close` 命令只 shutdown 写端、`Close` 事件等读端 EOF——
> >   对端半开永不 FIN 即死锁（`__sockets` 残留 1）。修法：写端收 `Close` 即发
> >   `Close`（`close_once` 防双发），不等读端。
> > - **半开续命**：服务端干净后半开客户端仍续命（真机 k7/k9/k12 实证照常退出，
> >   `ref()` 也留不住）。修法：`NetEntry.holding` 位 + `net_halfhold` native，
> >   `__ev end` 内 allowHalfOpen 递延一轮 microtask，稳定半开才摘续命；
> >   `net_open` 只数 `refed && holding`（`net_halfhold_balance` 单测 + 黑盒
> >   `phase11_net_halfopen_releases_loop`，8s unref 守卫回归只红不挂）。
> > - **转 SAME**：`server-keep-alive-timeout`（修前 TIMEOUT）+
> >   `server-close-idle-wait-response`（附带）；`server-request-timeout-keepalive`
> >   真机自挂（node 142，超跑分 alarm），非我方回归；dd3/kadbg 偶发 park 未复现
> >   （4/4 确定性触发 kaT）；`drain-writable-length` 仍 TIMEOUT——outputData
> >   缓冲模型（writableLength/writableNeedDrain/drain 门控），G3 既定另轮专项。
>
> ### G11 upgrade 轮（2026-09-22，3 提交 + 黑盒；AGENTS §4.187）
>
> > - **判定门 + 三形态**：connection token + Upgrade 头双全（advertise case2/3）；
> >   无回调无监听回落 request、回调放行无监听销毁、回调否决 request；
> >   `shouldUpgradeCallback` 真/假/抛（抛经 nextTick 交付 uncaught）。
> > - **体路由**：升级后 CL/chunked 增量泵续喂 req（`__feedUpgraded`），体完转
> >   socket；`upgradeHead` 恒 Buffer；native 改 `__srvFeed` 直调（去双发）；
> >   接管即摘服务端监听（去占数吞 spill）；spill 守卫防重入递归。
> > - **迟挂不丢**：socket `data` 先暂存后冲刷 + `newListener` 递延（保序）；
> >   `destroy(err)` 改 `emitErrorNT` 异步（同步抛压成异常）。
> > - **对形 headers**：客户端 `[[k,v]]` 与扁平双形同发头。
> > - **转 SAME**：upgrade 6 件（advertise/client/server-callback/large-body×2/
> >   body-error；修前 5 TIMEOUT + 1 DIFF）；黑盒
> >   `tests/node/http/upgrade.rs::phase11_http_upgrade_faces`（15s unref 守卫）。
>
> ### G11 头面 batch5（2026-09-22；AGENTS §4.188）
>
> > - **校验门三件**：数字头名 HTTP_TOKEN（typeof 先判，set/append 双侧）/
> >   奇长 writeHead 数组 ARG_VALUE 'headers'/已发头再 writeHead 即 HEADERS_SENT。
> > - **拼写与短语**：writeHead 恒覆写拼写（首写优先仅 setHeader 之间）/
> >   未知码短语 'unknown'（属性与 wire 双处）。
> > - **数组与对形**：同键对逐行保留（首触覆写、再触累积）/
> >   对形 `[[k,v]]` 归一扁平（坏对/超长元真机逐项）。
> > - **Host 头**：显式 defaultPort 比较（缺席恒拼 `:80`）+ IPv6 双冒号加框。
> > - **拒写**：`rejectNonStandardBodyWrites`（缺省 false）+ 新码
> >   `ERR_HTTP_BODY_NOT_ALLOWED`（1xx/204/304/HEAD，空串亦抛；检查在
> >   write/end 包装层，禁入 `_write`）。
> > - **转 SAME**：write-head×2/write-head-after-set-header/head-throw/
> >   host-header-ipv6-fail/set-trailers（修前 4 TIMEOUT + 2 DIFF）+
> >   旧 11 件头面（validators/value-relaxed/mutable/multiple/remove-stays/
> >   setheaders/distinct-proto/automatic/client-array/dont-default/host）；
> >   28 件头面对拍 SAME0（`header-overflow` 的 `socket.push` 系既定另轮）；
> >   黑盒 `tests/node/http/surface.rs::phase11_http_header_face_batch5`。
>
> ### G11 TIMEOUT 深水第一铲（2026-09-22；AGENTS §4.189）
>
> > - **host/auth**：url.parse 对象 `hostname` 优先（源码 400 行）/
> >   `options.auth` 补 Basic（源码 551 行，显式头恒赢）/
> >   URL userinfo 进 auth（decode 双侧）+ IPv6 去框。
> > - **CONNECT**：authority-form 不补斜杠（源码 293-295 行，OPTIONS * 同免）/
> >   Host 取 path 本体（源码 546 行）/隧道 detach（两端 end:1 其余 0 +
> >   `_httpMessage` null + 摘池 + req destroyed/close；server FIN 守卫，
> >   升级形不动）。
> > - **小面**：server timeout 监听进 `if`/socket HWM 65536 对齐/
> >   基类 setTimeout（事件实参）+ `req.protocol`。
> > - **转 SAME0**：url.parse×5/auth×2/CONNECT×3（default-host-header/connect/
> >   connect-req-res）/outgoing-settimeout（11 件）；黑盒
> >   `tests/node/http/surface.rs::phase11_http_timeout_deep_host_auth_connect`。
> > - **未闭环**：`outgoing-properties`（HWM 已对齐，余 `writableLength`
> >   `len+8` 记账 = G3 outputData 专项）+ handler 抛吞进 400 通道即静默 hang
> >  （真机 crash，另立单元）。
>
> ### G11 TIMEOUT 深水第二铲（2026-09-22；AGENTS §4.190）
>
> > - **server 选项类**：`IncomingMessage/ServerResponse` 存 server + 请求期当
> >   构造器（真机无校验；子类透传；裸 `Server()` 本就可调）。
> > - **建连选项透传**：`net.createConnection` 首参对象进 `new Socket`
> >   （readableHWM 等生效）；socket 读写 HWM 缺省双 65536（背压默认同改，
> >   零回归）+ 双 getter；客户端 res 跟 socket HWM 走。
> > - **转 SAME0**：server-options-incoming-message/
> >   server-options-server-response/incoming-message-options（3 件）；黑盒
> >   `tests/node/http/surface.rs::phase11_http_server_options_surface`。
>
> ### G11 splitting 一件（2026-09-22；AGENTS §4.191）
>
> > - `ERR_INVALID_CHAR` 加可选 `field`（`["key"]` 后缀；无参回裸文案）+
> >   `__checkOutboundHeaderValue` 全调用点传键（set/append/writeHead 三路）。
> > - **转 SAME0**：response-splitting（修前 DIFF）；附带 6 件零回归；黑盒
> >   `tests/node/http/surface.rs::phase11_http_invalid_char_key`。
>
> ### G11 response 双件（2026-09-22；AGENTS §4.192）
>
> > - **write-after-end**：`write()` 包装层先行拦截（自发 error + 回 false，
> >   不进基类不置 errored；终结块走正常 `_final`；end 优先于拒写旗）。
> > - **状态码门**：E 注册 `%s` RangeError + 传原值（`|0` 后判，对象走 inspect）。
> > - **转 SAME0**：res-write-after-end/response-statuscode（修前双 TIMEOUT；
> >   head-throw 零回归）；黑盒
> >   `tests/node/http/surface.rs::phase11_http_response_gates`。
> > - **未竟**：response-cork（流控手术另单元）。
>
> ### G11 收尾轮（2026-09-23；AGENTS §4.193）
>
> > - **cork 面**：res.cork()/uncork() = 流机构 cork 滞留 + socket.cork()
> >   镜像（writableCorked 双侧恒等）+ socket cork 真计数/writableCorked/
> >   `_writableState` HWM 桩 + res 写机构 HWM 跟 socket 可写侧（drain-cork
> >   的 1000 背压）+ end 强制全开（机构与 socket 双侧置 1 再 uncork）。
> >   偏差：uncork 尾flush node 合并单帧、本仓逐块成帧（字节流恒等）。
> > - **写粒度**：chunked 四发 hex/CRLF/体/CRLF（hex 不含 CRLF——双 CRLF 曾
> >   致整流错位）+ 首发头拼 hex/体（_header prepend）+ 终结独立 + end 无数据
> >   chunked 头拼终结一发；response-cork 的 socket.write spy 恰 5。
> > - **uncaught 双向**：解析错（__parseErr 旗）走 destroy 通道、用户 throw
> >   nextTick 重抛（response 监听 + request handler 双位；400 通道只收解析错）。
> > - **小面**：`res._send('')` 冲头 + holdback 补帧；`sendDate=false` 不补
> >   Date；ClientRequest options 入口 null-proto 拷贝；客户端响应头超限
> >   静默截断（服务端维持 HPE 抛错）；客户端拒多 CL（HPE_UNEXPECTED_
> >   CONTENT_LENGTH 'Duplicate Content-Length'）。
> > - **转 SAME0**：response-cork/response-drain-cork/outgoing-end-cork/
> >   uncaught-from-request-callback/test-http-1.0/null-prototype-options/
> >   max-headers-count/response-multi-content-length（8 件）；
> >   request-timeout-keepalive 实为绿（15s sweep alarm 误判，25s 双边绿）。
> >   黑盒 `phase11_http_cork_faces` + `phase11_http_uncaught_throws`。
> > - **终局 serial 重扫**（sweep4，2026-09-23，25s alarm，干净二进制）：
> >   409 件 SAME0=294/SAME1=0/DIFF=102/TIMEOUT=13（TIMEOUT 全为 wjs=142/node=0）。
> >
> > ### 基建轮（2026-09-23；AGENTS §4.194）
> >
> > > - **socket.push**：可读侧注入（与 native 到包同 ingest；push(null)=可读
> > >   EOF，无传输即收尾 close）→ read-in-error/header-overflow 转绿。
> > > - **服务端 HPE 化**：方法增量匹配（首字节 A-Z + METHODS 前缀候选，分叉即
> > >   偏移；7 探针）+ TE+CL/重 CL 门 + 溢出 HPE + 默认分支 socket-error 递送
> > >   （有用户监听才 destroy(e)）+ rawPacket 当片补齐 → 5 件转绿 +
> > >   socket-error-listeners hang 根除。
> > > - **writableLength 记账**：dry-run 计数 + 落盘递减 + _final 兜底清零
> > >   （131/139 与真机逐字同）→ outgoing-properties 转绿。
> > > - **eager-parse 队列**：后继 res.socket null + 写停靠 + finish 轮转 +
> > >   drain 门控 + socket 写队列（writableLength/HWM/drain）→
> > >   outgoing-drain-writable-length 转绿，附带 1.0-keep-alive/
> > >   pipeline-flood/pipeline-outgoing-destroy/catch-uncaughtexception。
> > > - **终局重扫**（sweep6，同口径）：409 件 SAME0=307/SAME1=0/DIFF=91/
> > >   TIMEOUT=11（+13，零新增红项）。残：reuse-drained（process.report 另域）。
> > - **残件定性**：drain-writable-length/outgoing-properties（outputData
> >   记账 + writableLength 合成 getter）与 header-overflow/read-in-error
> >   （socket.push）同根 → **net.Socket 写侧流式化 + eager-parse outgoing
> >   队列**基建另轮；execPath spawn ~18（CLI 全 flag 铁律，需拍板）；
> >   parser 内省 ~4（记档偏离）；余散件逐套件记档。

> ### G11 createConnection 错误路由（2026-09-23；AGENTS §4.203）
>
> > - 请求级 createConnection 的 `oncreate` 只认 socket 不认 err——async
> >   cb 错被吞（请求既不挂 socket 也不发 error，promise 永悬）；sync throw
> >   靠异常穿透构造器侥幸 reject。修法（真机 _http_client.js 591-607 行
> >   逐字）：err 臂 `process.nextTick(() => this.emit("error", err))`
> >   （无监听经 EE 落 uncaught）+ sync throw try/catch 收进同路（永不同步
> >   抛出构造器）+ settled 门前置防双投（Both1/2 cb+return 双形）。
> > - **转 SAME0**：test-http-createConnection（修前 TIMEOUT，插桩六块
> >   定位 async 错误块）；黑盒
> >   `tests/node/http/parity.rs::phase11_http_create_connection_error_routing`
> >   （async/sync/uncaught 三形）。http/net/https 三域 52 绿 + node 域
> >   285 绿零回归。
> > - 对拍 mapper（AGENTS §4.202-①）同日落地：本套件零输出挂死即靠块标记
> >   插桩定位；mapper 覆盖 uncaught 形，TIMEOUT 件仍走插桩。

## https

> 同 http 域随行（帧层共用，67 件）：同绿 8 → **13**（+5：Agent no-new/
> getName TLS 全键/server timeout 三件套随行）；SAME1=2；DIFF 57 → **52**。
> 剩余红项多为 TLS 深件：pfx（2）、createSecureContext（缺 API）、SNI 回调、
> session reuse（getSession/getTicketKeys）、`res.socket.getSession`/
> `socket.authorized` 深链、unix socket self-signed（ECONNREFUSED）、
> checkServerIdentity 面等——随 tls 域深化另轮；`https.Agent.getName` 已按
> node lib/https.js 23 字段全键落地（agent-getname ✅）。

## http2

> 首轮点名（2026-09-18，276 件，`/tmp/wjs-10f-http1/2.txt`）：
> 同绿 7 → **8**；SAME1=30；DIFF 239 → **238**。
> 主簇（~105 件）：compat 层 `Http2ServerRequest/Response` 为 EventEmitter 薄壳
> （整收口径直发 data/end），缺 resume/setEncoding/pipe/背压全流面——与 10b
> http 流式化同型工程，另轮；次簇：`stream.respond/respondWithFile` 等
> server 流面（~11）、settings/priority/ALPN 校验面、TIMEOUT（背压/内存限）。
> h2c/HTTP3 既有黑盒全绿无回归（`tests/node/http2.rs`）。
>
> > ### 流式化已修（2026-09-18，头/体分离）
> >
> > - **ChanBody**：头经 oneshot、体经 `H2RespondData/End/Reset` 增量下发
> >   （早到体块 outbox 回放；`poll_recv(cx)` 注册 waker，`try_recv` 空转饿死已修；
> >   `done` 初值有体即 false）；客户端上传同通道（`H2OpenTrailers` 补 trailer 帧）。
> > - **compat 错误码**：`ERR_HTTP2_HEADERS_SENT/INVALID_STREAM/
> >   NO_SOCKET_MANIPULATION/INVALID_HEADER_VALUE/PUSH_DISABLED/
> >   INVALID_HTTP_TOKEN`（node 文案逐字）+ socket 代理（`connecting` 自有遮蔽）。
> > - **Duplex 只读**：`ClientHttp2Stream extends Duplex` 后 `closed/destroyed`
> >   禁赋值（state 位图），`close()` 以 `destroyed` 判幂等（坑见 AGENTS §4.133）。
> > - 回归：`tests/node/http2.rs::phase10f_http2_streaming`（分块写/POST 回显/
> >   trailer 往返/HEADERS_SENT/空体，正常+报错+边界）。

## worker

> 六轮对拍（2026-09-18，140 件，`/tmp/wjs-10f-worker1..6.txt`，2 worker）：
> 同绿 35 → 43 → 51 → 49（回退轮，次轮修复）→ 53 → **55**；SAME1=8；DIFF
> 98 → 90 → 81 → 85 → 79 → **77**。既有 9f/9i/M5 面（线程底座/端口迁移/循环
> 信封）黑盒全绿无回归（`tests/node/worker.rs`）。
>
> ### 七轮（2026-09-19，环境面 + terminate 生命周期）
>
> - **process.env 逐 worker 快照**：worker 会话带 env JSON（主会话/SHARE_ENV
>   继承真 env）；快照在 node prelude 之前落地（代理构建期就读）——
>   environmentdata 套件转绿。
> - **BC 同会话 pending 表**：`bc_pub` 同会话订阅改直投 pending（端口 pending
>   同款模型），`receiveMessageOnPort` 对 BroadcastChannel 的同步收信口 +
>   pump 逐轮派发双消费（broadcastchannel 套件同步收信件转绿）。
> - **terminate 中断钩**：worker 会话挂 `JS_AddInterruptCallback`（只读共享
>   终止旗）——忙循环斩断，idle 路径照旧走事件循环检查点；
>   terminate-interrupt 套件转绿。
> - 黑盒：`phase10f_worker_bc_surface_and_env_snapshot`/
>   `phase10f_worker_terminate_interrupt_busy_loop`（environmentdata/
>   broadcastchannel/message-event 对拍同日复核 SAME）。
> - 剩余红项分簇维持下记（terminate 深水/stdio 流面/Atomics.wait 均另轮）。
>
> ### 已修（每项经真机 26.8.2 对拍）
>
> - **二轮（错误形状批）**：`DataCloneError` 改抛真 `DOMException`
>   （constructor.name/code 25/instanceof Error 三面，transfer-self/closed 套件
>   逐字）+ transfer 逐类型 node 文案（duplicate/source port/already
>   detached）+ worker bootstrap `process.*` UNSUPPORTED_OPERATION 桩
>   （unsupported-things 套件 disabled/() 形/无 () 属性形三态）+
>   `__workerFilePath` 四路（data: URL eval/file: URL/ERR_WORKER_PATH 带 Wrap
>   提示/ARG_TYPE）+ Worker 构造校验序（eval 门→filename→env/name/execArgv）
>   + error 事件按类名还原错误类（SyntaxError 套件断
>   `err.constructor === SyntaxError`；裸文本回 Error）。连带基建：
>   `Error::Script` 加 `kind` 字段跨线程透传异常类名（`exc_name` 重构）。
> - **三轮（视图/SAB 批，TDZ 级联根因）**：view 解码 `byteLength` 当**元素数**
>   传——BPE>1 的 typed array（Int32/Float64 系）跨端全 OOB 静默丢消息，而
>   worker 内 `workerData` 常量在 class 声明区**之前**求值，解码炸掉整模块，
>   require 命中未初始化 `Worker` 绑定的 TDZ（报错文件名/行号还串到主脚本）。
>   修法：wire 存 byteLength、解码按 `BYTES_PER_ELEMENT` 折算 + SAB 品牌
>   信封（`k:"sab"`，副本语义——真共享内存需跨线程底座，记档）。
> - **四/五轮（表面批）**：全局 `MessageEvent`（undici webidl 校验文案逐字，
>   `inspect(v,{quotes:'double'})` 形值回显）+ MessagePort/BroadcastChannel
>   EventTarget 双面（'message' 系收 MessageEvent(data)，自定义类型收
>   CustomEvent(detail)，真机逐项实测）+ `threadName`（导出 + 属性 + 退出置
>   null，boot 带 name 四件套）+ `resourceLimits` 透传 + missing-main 文案
>   （`Cannot find module '<path>'`）+ error-primitive 原始值信封
>   （`__wjs_prim:{json}`：number/string/bigint/bool/null/undefined/
>   注册 Symbol 经 `Symbol.for` 还原跨线程同一性）+ `BroadcastChannel` 升
>   Web 全局 + `markAsUntransferable`/`isMarkedAsUntransferable` 具名导出
>   （标记端口/AB 拒转移且不 detach）+ BC this 品牌门（ERR_INVALID_THIS）+
>   嵌套端口克隆文案分形（"Object that needs transfer was found"）。
> - **同步收信（pending 表）**：node 的 `receiveMessageOnPort` 同步语义——
>   `port_post` 本地 pair 直入对端 Rust pending 表（纯 Rust 串，无 GC 值），
>   pump 逐轮统一派发。中试的 JS 直推+微任务/kick 两案均翻车：微任务链式
>   ping-pong 饿死定时器（infinite-message-loop），kick 经 pump 收割仍同轮
>   链式；pending 表案对齐 node 的 task 级节奏。
>
> ### 剩余红项（DIFF 77，分簇）
>
> - **TIMEOUT ~17 件**：terminate 时机/共享面/长任务生命周期——terminate
>   生命周期深化另轮。
> - **校验族 ~7 件**：`Missing expected exception`/`unexpected throw` 长尾
>   （broadcastchannel 深块/messaging/connection 面），下轮续补。
> - **stdio 流面 5 件**：worker stdout/stderr 非 Readable（fork 线程底座
>   stdio 数据面记档欠账，plan3 §4 维持）。
> - **环境/进程面 4 件**：process-env 逐 worker env 快照、beforeexit-throw
>   退出码、process-cwd/safe-getters 细节——随 terminate 轮。
> - **Atomics.wait 2 件**：worker 内 futex 等待（引擎 SM 面，待查）。
> - **跨引擎不可比/引擎口径 6 件**：memory rss、getHeapSnapshot、
>   v8.serialize/setFlagsFromString、stack-overflow 栈深——记档。
> - **自 spawn/flag 门控 3 件**：message-type-unknown（全 flag CLI 设计）、
>   init-failure（256 worker 环境门控）、wasm 系列（unhandled rejection，
>   另案）。
> - **真机对齐双红 8 件**：真机同条件亦红，对齐不算欠账。
>
> 复现：`tests/node/worker.rs::phase10f_worker_error_shape_and_event_faces` +
> `phase10f_worker_typed_view_and_sab_envelope`（修前 Int32Array 跨端静默丢/
> TDZ 级联/rc=101 级联）。

## test

> 首轮点名（2026-09-19，补 plan3 §1 划给 10f 的 parity 确认行，83 件
> `test-runner-*`，`/tmp/wjs-10f-testrun.txt`，单 worker 串行）：
> SAME0=5 + SAME1=12（双红对齐）+ **DIFF=66**。与 §1"node:test 🟡（起步）"
> 现状一致——矩阵维持 🟡，深浅据此定案：API 面是本轮欠账的主体，
> reporter 深度维持 plan2 既有记档（Bun 同 🟡）。
>
> ### DIFF 分簇（66 件）
>
> - **node:test API 面 ~35 件**：`test()`/`describe`/`it` 可调用与子测试面
>   不全（`test is not a function`/`describe needs a function` 两形为主）
>   ——run/subtest/mock 计划逐件补齐后可转绿，**下轮可修**（体量最大簇）。
> - **自 spawn `--test` CLI ~18 件**：套件以子进程跑 `node --test`/`--test-isolation`
>   （runner CLI 本身未实现 + 全 flag CLI 设计双重门）——runner CLI 落地后
>   一并评估，另案。
> - **reporter/TAP 输出面 ~13 件**：spec/tap 深度、diff 渲染（plan2 既有
>   "reporter/diff 欠账"记档维持；Bun 同 🟡）——书面偏离。
>
> SAME0（双侧全绿）：filter-warning/inspect/mock-timers-with-timeout/
> root-after-with-refed-handles/typechecking；SAME1=12 双红（coverage 系/
> source-map 系等，真机同条件亦红）。
>
> ### Slice A（2026-09-21，API 核心面）：SAME0=5→**17**（+11）、DIFF=66→**53**，
> > 零回归（旧 SAME 项无一转红）。
> > 转绿 11 件：aliases（`suite` 导出 + `test.suite/describe` 同体）/assert
> > （`t.assert` 全键 + ok 调用点源码行）/custom-assertions（`assert.register`
> > 逐字校验 + plan 计数 + `this===ctx` + 覆盖）/get-test-context（具名导出 +
> > 串行栈跨 setImmediate + 套件/测试钩子上下文）/option-precedence
> > （name/fn/plan 覆盖 + 单 options 形）/option-validation（timeout/
> > concurrency 码逐字）/subtest-after-hook（测试级 after 零子测试亦跑）/
> > tags-inheritance（校验/小写规范/父优先并集/冻结 + 一次性实验警告）/
> > test-fullname（SuiteContext 回调 + `t.test` 嵌套 + `<anonymous>`）/
> > wait-for（同步校验 + 串行轮询 + 超时 cause + 输家 timer 即清）/
> > aftereach-runtime-skip（运行时 skip + afterEach 照跑）。附带：`describe`
> > 无 fn 改 noop（真机口径）、`strictEqual` 缺省文案真机逐字、具名导出挂载
> > `test.getTestContext/test.assert`（CJS 可见性）。
> > 回归：`tests/node/testmod.rs::phase10f_test_*` 四件；坑见 AGENTS §4.178。
> > 残 53 件分簇：run 编程 API ~21（`run()` 事件流，另轮）/自 spawn CLI ~18
> > （`--test` runner CLI 未实现 + 全 flag 设计门，另案）/reporter ~13
> > （书面偏离维持）/mock 全家 3 + 校验零散（mock 轮并入）。
>
> ### Slice B1（2026-09-21，MockTracker 核心）：`node:internal/test/mock`
> > 新建（MockFunctionContext/PropertyContext/MockTracker 全家：fn/method/
> > getter/setter/property/reset/restoreAll；Proxy 壳 name/length 透传；
> > construct 原型归原函数；`times`/once 下标门逐字），`t.mock` 逐测试实例 +
> > 顶层 `mock` 导出 + 测试结束自动 `restoreAll` + **钩子归属重构**（before/
> > beforeEach/afterEach 跑在子测试身上，owner 自身只跑 `after`；钩内
> > getTestContext 见 owner 名、参数传子 ctx——真机 Test.run 口径，
> > hook.cjs 探针钉住）。
> > 对拍：mocking.js 55/56（唯一红为私有字段 V8 文案偏离，见下），其余
> > 82 件零回归；mock-timers-*/module-mocking 维持（B2/另案）。
> > 偏离记档（引擎边界）：`mocks a constructor` 末断言——V8 文案
> > `Cannot read private member #privateValue` vs SM
> > `can't access private field or method`，JS 层无拦截点（与栈格式 🟡 同类）。
> > 非 configurable 方法重定义走 V8 文案桥（预检抛，套件正则钉住）。
> > 回归：`tests/node/testmod.rs::phase10f_test_mock_*` 两件；
> > 坑见 AGENTS §4.180。
>
> ### Slice B2（2026-09-21，mock.timers）：MockTimers 落地（enable/tick/
> > setTime/reset/runAll + 小数组优先队列 + Date 替换/isMock/now/toString +
> > scheduler.wait 伪装 + AbortSignal.timeout；`node:timers` 命名空间冻结故
> > 只补全局+scheduler+Date，套件不覆盖处记档）。
> > 对拍：mock-timers-date/scheduler 双转绿（SAME0 17→**19**、DIFF 53→**51**，
> > 零回归）；mock-timers.js 維持 SAME1（`--expose-internals` 跳过类）；
> > mocking.js 維持单引擎文案红。
> > 回归：`tests/node/testmod.rs::phase10f_test_mock_timers_*` 两件；
> > 坑见 AGENTS §4.181。
>
> ### Slice C（2026-09-21，run none + 事件流）：`run({isolation:"none"})`
> > 同进程文件加载（状态快照/复原，可重入）+ 事件六件
> > （enqueue/dequeue/start/pass/fail/complete，testId 配对）+ 套件落定
> > pass/fail + 测试发现（cwd 下 `*.test.js`）+ `testTagFilters` 校验归一 +
> > plan wait 门 + only-过滤改 applyFilters 口径（祖先标记 + 父门，去批量）+
> > before 逐钩 runOnce（describe 建套件即 kick，同步前缀内联）+ 钩子/test/
> > suite 回调 `this` 绑定 + 根名 `<root>`。
> > 对拍：no-isolation ×2/enqueue-syntax-error/test-id/tags-validation
> > 五转绿（SAME0 19→**24**、DIFF 51→**46**，零回归）；process 隔离门明确
> > 拒绝（另片）。
> > 回归：`tests/node/testmod.rs::phase10f_test_run_none_and_plan_gates`；
> > 坑见 AGENTS §4.182。
>
> ### Slice D（2026-09-21，run process 隔离）：worker 线程传输（每文件独立
> > 会话；子内跑 none 并经 parentPort 逐事件回传，错误序列化 plain、父端重组；
> > NODE_TEST_CONTEXT 子端置位与父隔离；串行跑文件）+ expectFailure 布尔反转
> > （失败记 pass 带旗/通过记 expectedFailure 失败；空对象门逐字）+ 失败标注
> > （failureType 缺省 testCodeFailure）+ skip/todo 置旗语义（body 继续、skip
> > 优先、message 回显、互斥键）+ todo 静态跑 body（失败仍失败）+ run coverage
> > 选项校验。
> > 对拍：expect-error ×2/todo-skip/filetest-location 四转绿 + coverage ×2
> > 附带转绿（inspector 缺席空转部分，见下；SAME0 24→**30**、DIFF 46→**39**，
> > 零回归；run-coverage 回 SAME1）。
> > 偏离记档：coverage ×2 我侧 inspector 缺席致门控跳过（空转绿），真机实跑；
> > 待 inspector/覆盖率另案。
> > 回归：`tests/node/testmod.rs::phase10f_test_run_process_and_expect_failure`；
> > 坑见 AGENTS §4.183。
>
> ### Slice E（2026-09-21，run 语义深化）：子测试计 plan + test 超时竞速
> > （stopTest 口径）+ TestPlan wait（true 无限/数字超时/缺省即判）+ legacy
> > done 回调 + tag 过滤（精确小写 + not 前缀，OR）+ entryFile 转发戳 +
> > 子测试调用点文件归属 + 随机种子洗牌（逐字 PRNG + 延迟兄弟pending 队列）+
> > run coverage 选项校验 + 事件键互斥（skip/todo/expectFailure 仅真值在场；
> > skip 套件亦发 pass）。
> > 对拍：plan/tags-events/entry-file/randomize 四转绿（SAME0 30→**34**、
> > DIFF 39→**35**，零回归）。
> > 回归：`tests/node/testmod.rs::phase10f_test_run_semantics_*` +
> > `phase10f_test_run_tag_filter_and_randomize`；坑见 AGENTS §4.184。
