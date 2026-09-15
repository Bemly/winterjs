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

> 10f 收官（除 glob）：posix/win32 六件全按 Node `lib/path.js` 直译
> （`normalizeString` 核心 + validateString/validateObject 精确码）；
> `matchesGlob` 需 `internal/fs/glob`，另切片。

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
| test-path-glob.js | 1 | 0 | 🟡 差集：`matchesGlob` 未实现（需 `internal/fs/glob`，另切片） |

## assert

> 行为件全修（跨域重抛/参数校验/构造器校验/rejects 双收/throws 正则）；
> message 文本子系统为书面偏离（Node diff 渲染引擎；操作符/actual/expected/code 结构一致）。

| 文件 | winterjs | node | 结论 |
|---|---|---|---|
| test-assert.js | 1 | 0 | 🟡 差集：message 文本（diff 渲染/截断/printf 展开）；行为已对 |
| test-assert-async.js | 1 | 0 | 🟡 同上 |
| test-assert-fail.js | 1 | 0 | 🟡 同上 |
| test-assert-if-error.js | 1 | 0 | 🟡 同上 |
