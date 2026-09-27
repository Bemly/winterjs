[![winterjs logo](assets/logo.avif)](https://github.com/Bemly/winterjs)

# winterjs ❄️

[中文版](./README.zh.md) · [Docs site](https://winterjs.bemly.moe/) · [Samples](./sample/) · [Changelog](./docs/plan3-journal.md)

*winterjs is a **Bun-like JavaScript runtime on Mozilla SpiderMonkey** — one binary that runs JS files, `package.json` scripts, tests, linters and static/dynamic HTTP services, with `node:` compatibility tracking **Bun's height**.*

```bash
./target/debug/winterjs --run sample/http/server-client.js
./target/debug/winterjs --eval 'await (await fetch("data:text/plain,hi")).text()'  # → hi
```

> Note: winterjs shares only the "Winter" name with [wasmerio/winterjs](https://github.com/wasmerio/winterjs)
> (a WinterCG server, now deprecated). This project is a general-purpose JS runtime
> in the Bun/Node lane — rebuilt from scratch on `servo/mozjs`, no server framework inside.

## Quick start

```bash
export SDKROOT="$(xcrun --show-sdk-path)"          # macOS, every new shell
export LIBCLANG_PATH="/opt/homebrew/opt/llvm/lib" # for bindgen
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
cargo build
./target/debug/winterjs --eval '40 + 2'            # → 42
```

5-minute path: [English](https://winterjs.bemly.moe/#/en/quickstart) / [中文](https://winterjs.bemly.moe/#/zh/quickstart) ·
50 runnable examples in [`sample/`](./sample/) (every API area, all offline-capable).

## Usage

One invocation runs **exactly one action**; modifiers only work with their action
(`--port` → `--serve`, `--filter` → `--test`, `--schema` → `--config`):

| Flag | Effect |
|---|---|
| `-r/--run <file\|script>` | Run a JS file or a `package.json` script (JS bins re-execute through winterjs, zero node) |
| `-e/--eval <code>` | Evaluate inline JS, print the completion value |
| `-t/--test [paths]` | Run test files (auto-discovery, `--filter/--watch`) |
| `-s/--serve [dir]` | Serve static + JS `fetch` handler + WebSocket over H1/H2/H3 |
| `--repl` | Interactive REPL |
| `-a/--add`, `-i/--install`, `-p/--publish`, `--login`, `-u/--upgrade`, `-I/--init` | Package lifecycle (npm registry) |
| `--lint`, `-f/--fmt` | Forward to oxlint/oxfmt |
| `-c/--config`, `--completions`, `-m/--man`, `-v`, `-l/--lang` | Config/help/i18n/logging |

Full reference: [CLI (EN)](https://winterjs.bemly.moe/#/en/cli) / [CLI (中文)](https://winterjs.bemly.moe/#/zh/cli).

## How winterjs works

winterjs links **Mozilla SpiderMonkey** (`mozjs =0.26.0`, Gecko 153, pinned) through
`servo/mozjs` and implements everything else — event loop, loader, Web/Node
builtins, `node:` shims — in **pure Rust**. `unsafe` lives only at the mozjs
boundary (rooting, `AutoRealm`, FFI); JS runs on a dedicated thread and Rust
sides talk to it through message queues, never by sharing `&mut JSContext`.

## `node:` API compatibility (Bun height)

Goal: everything in Bun's bundled node test list works; semantics follow Node
(`lib/` source + `test/parallel` assertions). Per-module status, samples and
**known deviations** live in the [API reference](https://winterjs.bemly.moe/#/en/api):

| Area | Status | Notes |
|---|---|---|
| `fs/net/http/https/http2/tls/dgram/dns` | ✅ Stable | streaming bodies, keep-alive, H2C, UDP loopback |
| `crypto/zlib/buffer/stream/events/timers` | ✅ Stable | AEAD ciphers, brotli, WHATWG streams |
| `child_process/cluster/worker_threads/vm/module/test` | ✅ Stable | thread-based cluster/workers |
| `sqlite` (`node:` + `bun:sqlite`), `quic`, `readline/repl/tty` | ✅ / 🔶 | `quic` handshake on loopback times out (tracked) |
| `v8/inspector/trace_events/domain` | 🔶 Bridge | intentionally reduced (heap numbers are engine-specific) |
| `wasi`, `sea` | ❌ | out of scope by design |

Web globals (`fetch`, `URL`, `TextEncoder`, Web Streams, WebCrypto, `WebSocket`,
`structuredClone`, …) ship alongside — see the API reference.

## Limitations

* Cross-engine numbers are not comparable (`v8` heap stats, `allocUnsafe` is zero-filled).
* `structuredClone` covers plain data (Date/Map/Set come back as plain objects).
* `node:quic` session handshake on loopback times out; `node:https` currently
  dispatches `request` twice per connection (guard idempotently).
* `wasi` / `sea` will not be implemented.

## Developing

```bash
cargo build                        # ~25s full debug (mozjs uses a prebuilt static lib)
./target/debug/winterjs --eval '40 + 2'   # smoke (5 canonical one-liners in AGENTS.md §3)
cargo nextest run --profile strict # full suite (~2 min)
bash scripts/check-lines.sh        # every .rs / src JS ≤ 1000 lines
```

Working conventions: [AGENTS.md](./AGENTS.md) · progress: [`docs/plan3.md`](./docs/plan3.md)
(Chinese) · pitfalls: [`docs/pitfalls.md`](./docs/pitfalls.md).

## License

MPL-2.0. Vendored third-party JS keeps its MIT headers.
