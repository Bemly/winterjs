# winterjs API Reference

> Style follows https://nodejs.org/docs/latest/api/ : one section per module,
> stability marker, import form, sample link, and **known deviations** (verified
> against `winterjs --run sample/...`, 2026-09-27). 中文版见 [api.zh.md](./api.zh.md).

Stability: `Stable` = tracks Node semantics; `Experimental` = present but
evolving; `Bridge` = intentionally reduced surface (documented below).

Quick runnable index: every row has a sample under `sample/<area>/`.

## Web globals (no import)

| Global | Stability | Sample | Notes |
|---|---|---|---|
| `console` (+`console.time/timeEnd/count/assert/dir`) | Stable | `sample/web/console.js`, `sample/web/performance.js` | `node:console` exports `Console` class over custom streams |
| `setTimeout/clearTimeout/setInterval/clearInterval/setImmediate/clearImmediate/queueMicrotask` | Stable | `sample/web/timers.js` | `setImmediate` is not clamped to 1ms; `nextTick` uses its native queue |
| `structuredClone` | Experimental | `sample/web/structured-clone.js` | **Deviation**: plain data only — Date/Map/Set/RegExp/TypedArray/ArrayBuffer come back as plain objects; no `transfer` detach |
| `URL/URLSearchParams/URLPattern` | Stable | `sample/web/url.js` | WHATWG; legacy `url.parse/format` lives in `node:url` |
| `TextEncoder/TextDecoder/atob/btoa` | Stable | `sample/web/url.js` | `fatal:true` supported |
| `Blob/File` | Stable | `sample/web/blob-file.js` | `slice/text/arrayBuffer` |
| `fetch/Request/Response/Headers` | Stable | `sample/web/fetch.js` | `data:` URLs work offline; streaming bodies via Web Streams |
| `ReadableStream/WritableStream/TransformStream/ByteLengthQueuingStrategy/CountQueuingStrategy` | Stable | `sample/web/streams.js` | `for await` consumption; **Deviation**: `new Response(readableStream)` body not accepted — collect via reader |
| `CompressionStream/DecompressionStream` | Stable | `sample/web/compression.js` | `gzip` + `deflate` verified round-trips |
| `crypto.getRandomValues/randomUUID/subtle` | Stable | `sample/web/webcrypto.js` | digest/AES-GCM/HMAC; `importKey` needs full `{name, hash}` params |
| `Event/EventTarget/CustomEvent/MessageEvent/CloseEvent/AbortController/AbortSignal` | Stable | `sample/web/events.js` | |
| `performance` (+`performance.now/timeOrigin`) | Stable | `sample/web/performance.js` | `node:perf_hooks` re-exports + extras |
| `Buffer` | Stable | `sample/web/buffer.js` | **Deviation**: `isAscii/isUtf8/transcode` absent; pool covers `from(string)` small strings; `allocUnsafe` is zero-filled |
| `WebSocket` (client) | Experimental | `sample/web/websocket.js` | constructor shape + constants offline; live echo via `--serve` handler, see `sample/serve-hello/` |
| `process/require/module/__dirname/__filename` | Stable | `sample/module/main.mjs`, `sample/process/basics.js` | CJS + ESM interop; `require(esm)` supported |

## node: modules

| Module | Stability | Sample | Notes |
|---|---|---|---|
| `node:assert`, `node:assert/strict` | Stable | `sample/assert/basics.js` | ok/equal/deepEqual/throws/rejects |
| `node:async_hooks` | Stable | `sample/observe/basics.js` | `AsyncLocalStorage` run/getStore across timers |
| `node:buffer` | Stable | `sample/web/buffer.js` | same class as global `Buffer` |
| `node:child_process` | Stable | `sample/child-process/spawn.js` | `spawnSync/execFileSync` via `process.execPath` (cross-platform) |
| `node:cluster` | Stable | `sample/cluster/primary-worker.js` | thread-based; fork/message/disconnect/exit |
| `node:console` | Stable | `sample/web/console.js` | `Console` class |
| `node:crypto` | Stable | `sample/crypto/hash.js`, `sample/crypto/cipher.js`, `sample/crypto/keys.js` | hash/HMAC/AES-GCM+CCM/ChaCha20/ECDSA/Ed25519/ECDH/random; CCM needs `{authTagLength}` |
| `node:dgram` | Stable | `sample/dgram/udp.js` | udp4 echo; multicast/connect supported |
| `node:diagnostics_channel` | Stable | `sample/observe/basics.js` | channel publish/subscribe |
| `node:dns`, `node:dns/promises` | Stable | `sample/dns/lookup.js` | `lookup(localhost)` offline; full resolver via system DNS |
| `node:domain` | Bridge | `sample/observe/basics.js` | sync routing only; async not routed (documented) |
| `node:events` | Stable | `sample/events/emitter.js` | on/once/off/error/maxListeners |
| `node:fs`, `node:fs/promises` | Stable | `sample/fs/read-write.js`, `sample/fs/directory.js`, `sample/fs/streams-watch.js` | sync/callback/promise/stream/watch faces |
| `node:http` | Stable | `sample/http/server-client.js` | keep-alive agent, streaming bodies |
| `node:http2` | Stable | `sample/http2/h2c.js` | h2c compat server + client session |
| `node:https` | Stable | `sample/https/get.js` | **Deviation**: `request` listener currently fires twice per request — guard idempotently; sample shows the guard |
| `node:inspector`, `node:inspector/promises` | Bridge | — | `Session/open/url`; no live debugging wire |
| `node:module` | Stable | `sample/module/main.mjs` + `helper.cjs` | `createRequire`, builtin require |
| `node:net` | Stable | `sample/net/tcp.js` | TCP echo; `isIP/isIPv4/isIPv6` |
| `node:os` | Stable | `sample/os/info.js` | platform/arch/cpus/mem/user/uptime/net |
| `node:path`, `node:path/posix`, `node:path/win32` | Stable | `sample/path/basics.js` | |
| `node:perf_hooks` | Stable | `sample/observe/basics.js` | `performance` object |
| `node:process` | Stable | `sample/process/basics.js` | argv/env/cwd/versions/hrtime/nextTick |
| `node:punycode` | Stable | `sample/codecs/basics.js` | toASCII/toUnicode/ucs2 |
| `node:querystring` | Stable | `sample/codecs/basics.js` | parse/stringify/escape |
| `node:quic` | Experimental | — | `QuicEndpoint/listen/connect`; needs UDP loopback |
| `node:readline` | Stable | `sample/readline/basics.js` | stream-backed, no TTY needed |
| `node:repl` | Stable | — | `REPLServer/start/Recoverable` over Interface |
| `node:sqlite` / `bun:sqlite` | Stable | `sample/sqlite/basics.js` | `DatabaseSync` / `Database`, in-memory verified |
| `node:stream` (+`/promises`, `/consumers`, `/web`) | Stable | `sample/stream/basics.js`, `sample/stream/extras.js` | Readable/Writable/Transform/pipeline/finished/text/json |
| `node:string_decoder` | Stable | `sample/codecs/basics.js` | split multibyte decode |
| `node:sys` | Stable | `sample/util/basics.js` | deprecated alias, same instance as `node:util` |
| `node:test` | Stable | `sample/test-runner/basics.js` | describe/it, runs under `--run` and `--test` |
| `node:timers`, `node:timers/promises` | Stable | `sample/url-timers/basics.js` | callback + promise faces |
| `node:tls`, `node:_tls_wrap` | Stable | `sample/tls/server-client.js` | fixture cert; `_tls_wrap` emits DEP0192 |
| `node:trace_events` | Bridge | — | `createTracing/getEnabledCategories` |
| `node:tty` | Stable | `sample/observe/basics.js` | `isatty/ReadStream/WriteStream` |
| `node:url` (legacy) | Stable | `sample/url-timers/basics.js` | parse/format/resolve/`Url` class |
| `node:util`, `node:util/types` | Stable | `sample/util/basics.js` | format/inspect/promisify/isMap/isPromise |
| `node:v8` | Bridge | `sample/observe/basics.js` | **only `startupSnapshot`**; heap/serializer intentionally absent (engine-specific) |
| `node:vm` | Stable | `sample/vm/basics.js` | Script/context/compileFunction; host globals not leaked |
| `node:worker_threads` | Stable | `sample/worker-threads/main.mjs` + `helper.mjs` | postMessage/terminate |
| `node:zlib` | Stable | `sample/zlib/basics.js` | gzip/deflate/brotli/crc32 |

## Not supported (by design)

* `wasi`, `sea` — no equivalent planned.
* N-API native addons (`.node`) load via the napi surface, but writing C
  addons against winterjs headers is out of scope for these samples.

## Errors & warnings

* Uncaught errors render Node-style (`file:line`, source line, `^`,
  `Name: message`, `at` stack) with exit 1; `throw <non-Error>` prints the value.
* `process.emitWarning` dispatches asynchronously (nextTick), matching Node.
