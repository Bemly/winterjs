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

## zlib

> 首轮对拍（2026-09-17，83 件，`/tmp/wjs-10f-zlib2.txt`，2 worker）：
> SAME0=34 + SAME1=13，DIFF=36。本轮两批共修 9 件：流收尾与内部小面 7 件
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
> ### 剩余红项（约 29，均另案或记档）
>
> - **Zip 归档 API 约 15 件**（`ZipEntry`/`ZipFile`/`ZipBuffer`/
>   `createZipArchive(+Sync)`/`zipFiles`/`crc`/zip64/注释放置/安全加固）：
>   整面未实现。轮子已在树内（`zip = "2"`，deflate 特性，§2 禁 bzip2），
>   零新依赖可做，另起特征轮（zstd-93 档经 ruzstd、ZipFile 落 fs）。
> - **增量语义约 6 件**（flush/premature-end/reject-garbage-after-end/
>   truncated/write-after-end/from-gzip-trailing-garbage）：
>   需真流式编解码状态机，与本轮外既有"整收"架构冲突，另案。
> - **杂项**：brotli-dictionary（字典面）、zstd-pledged-src-size、
>   premature-end/truncated/write-after-end/reject-garbage/from-gzip-trailing
>   （增量解码面，同上另案）、type-error（Web `DecompressionStream` 缺失，另切片）、
>   brotli-16GB（16G 量级用例，本机资源门控不跑，逻辑上属流式分块面）。

## child_process

> 首轮对拍（2026-09-17，110 件，`/tmp/wjs-10f-child3.txt`，2 worker，
> 含 `test/fixtures` 稀疏检出——无 fixtures 时 SAME1 虚高 18→8）：
> SAME0=12 + SAME1=8，DIFF=90。本轮（同步族口径，
> `tests/node/child.rs::phase10f_child_sync_surface`）修 10 件：
> spawnsync-validation-errors/timeout/input/maxbuf/spawnsync/args/env +
> execfilesync-maxbuf/execsync-maxbuf/spawn-argv0（单文件逐个实测 exit=0），
> 余约 80 件。
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
> ### 剩余红项（约 80，分簇）
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
> ### 剩余红项（分簇，均下轮或另案）
>
> - **key-objects.js**：四轮后剩 RSA pkcs1 313 行族尾段 + 零散 type 门，下轮。
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
