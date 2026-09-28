---
layout: docs
title: API 参考
lang: zh
stub: api
permalink: /zh/api/
---
# winterjs API 参考

> 体例对标 https://nodejs.org/docs/latest/api/ ：每模块一节，标注稳定性、
> 给出导入形态、链接样例，并列出**已知偏离**（2026-09-27 经
> `winterjs --run sample/...` 逐项实测）。English version: [API reference](../en/api/).

稳定性：`稳定` = 跟随 Node 语义；`实验` = 可用但在演进；`桥接` = 有意裁剪的面（下文注明）。

可运行索引：每行对应 `sample/<领域>/` 下的样例。

## Web 全局（免导入）

| 全局 | 稳定性 | 样例 | 说明 |
|---|---|---|---|
| `console` | 稳定 | `sample/web/console.js`、`sample/web/performance.js` | 控制台输出：`time/timeEnd/count/assert/dir`；`node:console` 导出自定义流的 `Console` 类 |
| `setTimeout/clearTimeout/setInterval/clearInterval/setImmediate/clearImmediate/queueMicrotask` | 稳定 | `sample/web/timers.js` | 定时器与任务调度：`setImmediate` 不钳 1ms；`nextTick` 走原生队列 |
| `structuredClone` | 实验 | `sample/web/structured-clone.js` | 作用：对象深拷贝。**偏离**：仅纯数据——Date/Map/Set/正则/类型化数组/ArrayBuffer 回来都是普通对象；无 `transfer` 剥离 |
| `storage` / `localStorage` | 稳定 | `sample/storage/basics.js` | WinterCG 自有 KV（turso 单文件，`--storage-path`，默认 `./winterjs-storage.db`）；异步 `get/set/delete/has/keys/clear/size` + 同步 Web Storage 垫片；值 JSON 可序列化 + `Uint8Array`；经 `-b/--db` 查看 |
| `fs` / `WinterJS.fs` | 稳定 | `sample/wfs/basics.js` | 自有文件面，与 `node:fs` 分离（直用 `fs-err`）；异步 `readFile/readTextFile/writeFile/writeTextFile/stat/mkdir/readdir/remove/rename/copyFile/exists` |
| `WinterJS.memory/alloc/unsafe*` | 稳定 | `sample/mem/basics.js` | `memory()` 看 rss+分配器；`alloc(n)` GC 托管零填；`unsafeAlloc/Write/Read/Size/Free/List` 手动 id 堆，需 `--allow-ffi` |
| `URL/URLSearchParams/URLPattern` | 稳定 | `sample/web/url.js` | URL 解析与构造（WHATWG）；legacy `url.parse/format` 在 `node:url` |
| `TextEncoder/TextDecoder/atob/btoa` | 稳定 | `sample/web/url.js` | 文本编解码：支持 `fatal:true` |
| `Blob/File` | 稳定 | `sample/web/blob-file.js` | 二进制对象：`slice/text/arrayBuffer` |
| `fetch/Request/Response/Headers` | 稳定 | `sample/web/fetch.js` | 网络请求：`data:` URL 离线可用；流式体走 Web Streams |
| `ReadableStream/WritableStream/TransformStream/ByteLengthQueuingStrategy/CountQueuingStrategy` | 稳定 | `sample/web/streams.js` | 流式数据处理：支持 `for await`；**偏离**：`new Response(可读流)` 体不接受——用 reader 收集 |
| `CompressionStream/DecompressionStream` | 稳定 | `sample/web/compression.js` | 流式压缩：`gzip` + `deflate` 已验往返 |
| `crypto.getRandomValues/randomUUID/subtle` | 稳定 | `sample/web/webcrypto.js` | 密码学：摘要/AES-GCM/HMAC；`importKey` 需完整 `{name, hash}` 参数 |
| `Event/EventTarget/CustomEvent/MessageEvent/CloseEvent/AbortController/AbortSignal` | 稳定 | `sample/web/events.js` | |
| `DOMException` | 稳定 | `sample/web/events.js` | 具名错误（`AbortError` 码 20） |
| `performance` | 稳定 | `sample/web/performance.js` | 高精度计时：`now` / `timeOrigin`；`node:perf_hooks` 同源再导出 |
| `Buffer` | 稳定 | `sample/web/buffer.js` | 作用：二进制缓冲。**偏离**：无 `isAscii/isUtf8/transcode`；仅 `from(string)` 小串池化；`allocUnsafe` 恒零填 |
| `WebSocket (client)` | 实验 | `sample/web/websocket.js` | 客户端 WebSocket：构造形态与常量离线可验； live 回声走 `--serve`，见 `sample/serve-hello/` |
| `process/require/module/__dirname/__filename` | 稳定 | `sample/module/main.mjs`、`sample/process/basics.js` | 进程对象与模块互操作：CJS + ESM；支持 `require(esm)` |

## node: 模块

| 模块 | 稳定性 | 样例 | 说明 |
|---|---|---|---|
| `node:assert`、`node:assert/strict` | 稳定 | `sample/assert/basics.js` | 断言：ok/equal/deepEqual/throws/rejects |
| `node:async_hooks` | 稳定 | `sample/observe/basics.js` | `AsyncLocalStorage` 跨定时器透传 |
| `node:buffer` | 稳定 | `sample/web/buffer.js` | 与全局 `Buffer` 同一类 |
| `node:child_process` | 稳定 | `sample/child-process/spawn.js` | 子进程：经 `process.execPath` 自举（跨平台） |
| `node:cluster` | 稳定 | `sample/cluster/primary-worker.js` | 多进程（线程底座）：fork/message/disconnect/exit |
| `node:console` | 稳定 | `sample/web/console.js` | 控制台类：`Console`（自定义流） |
| `node:crypto` | 稳定 | `sample/crypto/hash.js`、`cipher.js`、`keys.js` | 密码学全家：哈希/HMAC/AES-GCM+CCM/ChaCha20/ECDSA/Ed25519/ECDH/随机数；CCM 需 `{authTagLength}` |
| `node:dgram` | 稳定 | `sample/dgram/udp.js` | UDP：udp4 回声；支持组播/connect |
| `node:diagnostics_channel` | 稳定 | `sample/observe/basics.js` | 诊断通道：发布订阅 |
| `node:dns`、`node:dns/promises` | 稳定 | `sample/dns/lookup.js` | 域名解析：`lookup(localhost)` 离线可用；全 resolver 读系统 DNS |
| `node:domain` | 桥接 | `sample/observe/basics.js` | 错误域（遗留）：仅同步路由；异步不路由（书面记档） |
| `node:events` | 稳定 | `sample/events/emitter.js` | 事件发射器：on/once/off/error/上限 |
| `node:fs`、`node:fs/promises` | 稳定 | `sample/fs/read-write.js`、`directory.js`、`streams-watch.js` | 文件系统：同步/回调/Promise/流/监听全家 |
| `node:http` | 稳定 | `sample/http/server-client.js` | HTTP 服务与客户端：keep-alive 复用、流式体 |
| `node:http2` | 稳定 | `sample/http2/h2c.js` | HTTP/2：h2c 兼容服务 + 客户端会话 |
| `node:https` | 稳定 | `sample/https/get.js` | **偏离**：单请求目前派发两次 `request` 监听——请幂等守卫，样例内有写法 |
| `node:inspector`、`node:inspector/promises` | 桥接 | `sample/inspector/basics.js` | `open/url/close` 生命周期；无线上调试通道（`url()` 恒 undefined） |
| `node:module` | 稳定 | `sample/module/main.mjs` + `helper.cjs` | 模块加载：`createRequire`、内建 require |
| `node:net` | 稳定 | `sample/net/tcp.js` | TCP 网络：TCP 回声；`isIP/isIPv4/isIPv6` |
| `node:os` | 稳定 | `sample/os/info.js` | 系统信息：平台/架构/CPU/内存/用户/时长/网卡 |
| `node:path`、`node:path/posix`、`node:path/win32` | 稳定 | `sample/path/basics.js` | 路径处理：join/resolve/parse/relative/extname |
| `node:perf_hooks` | 稳定 | `sample/observe/basics.js` | 性能计时：`performance` 对象 |
| `node:process` | 稳定 | `sample/process/basics.js` | 进程控制：参数/环境/版本/高精度时间/nextTick |
| `node:punycode` | 稳定 | `sample/codecs/basics.js` | 域名编码：toASCII/toUnicode/ucs2 |
| `node:querystring` | 稳定 | `sample/codecs/basics.js` | 查询串：parse/stringify/escape |
| `node:quic` | 实验 | — | 仅 `QuicEndpoint/listen/connect` 形态面；本构建回环握手超时（另案追查） |
| `node:readline` | 稳定 | `sample/readline/basics.js` | 交互行读取：内存流驱动，无需 TTY |
| `node:repl` | 稳定 | `sample/repl/basics.js` | 交互式求值：内存流驱动（免 TTY）；`Recoverable` |
| `bun:ffi` | 实验 | `sample/bun-ffi/strlen.js` | 原生库调用：`dlopen` 调 libc `strlen`（darwin/linux 守卫）；`ptr`/`CString` 地址模型 |
| `node:sqlite` / `bun:sqlite` | 稳定 | `sample/sqlite/basics.js` | 内建数据库：`DatabaseSync` / `Database`，内存库已验 |
| `node:stream` (+`/promises`, `/consumers`, `/web`) | 稳定 | `sample/stream/basics.js`、`extras.js` | 数据流：四类流/pipeline/finished/text/json |
| `node:string_decoder` | 稳定 | `sample/codecs/basics.js` | 跨包多字节解码：split 无乱码 |
| `node:sys` | 稳定 | `sample/util/basics.js` | 废弃别名（同实例）：与 `node:util` 同实例 |
| `node:test` | 稳定 | `sample/test-runner/basics.js` | 内置测试：describe/it，`--run` 与 `--test` 下都跑 |
| `node:timers`、`node:timers/promises` | 稳定 | `sample/url-timers/basics.js` | 定时器：回调 + Promise 双形态 |
| `node:tls`、`node:_tls_wrap` | 稳定 | `sample/tls/server-client.js` | TLS 传输层：样例自签证书；`_tls_wrap` 报 DEP0192 |
| `node:trace_events` | 桥接 | `sample/trace-events/basics.js` | 性能追踪：`createTracing/enable/disable/getEnabledCategories` |
| `node:tty` | 稳定 | `sample/observe/basics.js` | 终端：`isatty/ReadStream/WriteStream` |
| `node:url` (legacy) | 稳定 | `sample/url-timers/basics.js` | 传统 URL：parse/format/resolve/`Url` 类 |
| `node:util`、`node:util/types` | 稳定 | `sample/util/basics.js` | 工具函数：format/inspect/promisify/isMap/isPromise |
| `node:v8` | 桥接 | `sample/observe/basics.js` | 引擎桥（有意裁剪）：**仅 `startupSnapshot`**；堆统计/序列化缺席（引擎口径不可比） |
| `node:vm` | 稳定 | `sample/vm/basics.js` | 沙箱虚拟机：Script/上下文/compileFunction；宿主全局不泄漏 |
| `node:worker_threads` | 稳定 | `sample/worker-threads/main.mjs` + `helper.mjs` | 工作线程：postMessage/terminate |
| `node:zlib` | 稳定 | `sample/zlib/basics.js` | 压缩解压：gzip/deflate/brotli/crc32 |

## 不做（设计如此）

* `wasi`、`sea` —— 无对等计划。
* N-API 原生 addon（`.node`）可经 napi 面加载，但针对 winterjs 头写 C addon 不在样例范围。

## 错误与警告

* 未捕获错误按 node 形渲染（`文件:行`、源码行、`^`、`名: 消息`、`at` 栈），exit=1；`throw <非 Error>` 打印值本身。
* `process.emitWarning` 异步派发（nextTick），与 Node 一致。
