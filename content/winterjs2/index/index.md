---
title: "WinterJS2"
slug: WinterJS2/index
---

The **`WinterJS2`** global object is the winterjs2 runtime namespace for
non-standard, runtime-specific APIs. Web standards stay on their own globals
(`fetch`, `URL`, `storage`); Node compatibility lives under `node:` modules.

## Syntax

```js
WinterJS2.version
WinterJS2.storage
WinterJS2.localStorage
WinterJS2.fs
WinterJS2.memory
WinterJS2.alloc
WinterJS2.unsafeAlloc
WinterJS2.CompressionStream
WinterJS2.DecompressionStream
WinterJS2.semver
WinterJS2.yaml
WinterJS2.jsonc
WinterJS2.ip
WinterJS2.shlex
WinterJS2.spdx
WinterJS2.qrcode
WinterJS2.shell
WinterJS2.hex
WinterJS2.time
WinterJS2.retry
WinterJS2.graph
WinterJS2.git
WinterJS2.oauth
WinterJS2.transpile
WinterJS2.log
WinterJS2.mime
WinterJS2.cookie
WinterJS2.httpdate
WinterJS2.assert
WinterJS2.util
WinterJS2.punycode
WinterJS2.tcp
WinterJS2.udp
WinterJS2.dns
WinterJS2.tls
WinterJS2.command
WinterJS2.terminal
WinterJS2.repl
WinterJS2.cluster
WinterJS2.test
WinterJS2.vm
WinterJS2.os
WinterJS2.path
WinterJS2.db
WinterJS2.inspect
WinterJS2.tty
WinterJS2.stream
WinterJS2.serve
WinterJS2.diagnostics
WinterJS2.domain
WinterJS2.trace
WinterJS2.AsyncLocalStorage
WinterJS2.quic
WinterJS2.crypto
WinterJS2.ffi
```

### Parameters

- `version`
  - : The current winterjs2 version string.
- `storage`
  - : The WinterCG async KV store for the current project.
- `localStorage`
  - : The sync Web Storage shim (same object as global `localStorage`).
- `fs`
  - : The own file surface (same object as global `fs`), separate from `node:fs`.
- `memory`
  - : Observable memory info (`rss`, allocator kind).
- `alloc`
  - : Controlled zero-filled `Uint8Array` allocation.
- `unsafeAlloc`
  - : Manual id-heap allocation behind `--allow-ffi`.
- `CompressionStream`
  - : Same class as global `CompressionStream` (plus winterjs2 `zstd` format).
- `DecompressionStream`
  - : Same class as global `DecompressionStream` (plus winterjs2 `zstd` format).
- `semver`
  - : npm-semantics version utilities (`valid/parse/satisfies/compare`).
- `yaml`
  - : YAML `parse`/`stringify` (first document).
- `jsonc`
  - : Comment-tolerant JSON `parse`.
- `ip`
  - : IP/CIDR utilities (`isNet/isAddr/contains/parse`).
- `shlex`
  - : Shell command-line `split`.
- `spdx`
  - : SPDX license-expression check.
- `qrcode`
  - : Terminal QR art for a short text.
- `shell`
  - : Shell-word `expand` (`~`, `$VAR`; env-gated).
- `hex`
  - : Hex `encode`/`decode`.
- `time`
  - : Clock (`now/parse/format`, jiff).
- `retry`
  - : Backoff `delay` + async `run` helper.
- `graph`
  - : Directed/undirected graph heap.
- `git`
  - : Read-only `revParse`/`log`.
- `oauth`
  - : Authorize URL + PKCE + token exchange.
- `transpile`
  - : Same oxc pipeline the loader runs.
- `log`
  - : Structured log (`debug/info/warn/error`).
- `mime`
  - : File-name MIME lookup.
- `cookie`
  - : Cookie `parse`/`serialize`.
- `httpdate`
  - : IMF date `parse`/`format`.
- `assert`
  - : Structural assertions (`deepEqual` ignores prototypes).
- `util`
  - : `format`/`inspect` (same port node:util rides).
- `punycode`
  - : Punycode core shared with `node:punycode`.
- `tcp/udp/dns/tls`
  - : Promise sockets, datagrams, resolvers and TLS clients.
- `command/terminal/repl`
  - : Child processes plus line-editing and REPL faces.
- `cluster/test/vm`
  - : Worker cluster, test runner and script evaluation.
- `os/path/db/inspect/tty`
  - : OS facts, paths, embedded SQL, evaluator and TTY check.
- `stream/serve/diagnostics/domain/trace/AsyncLocalStorage/quic/crypto/ffi`
  - : Streams, serving, diagnostics and system utilities.
