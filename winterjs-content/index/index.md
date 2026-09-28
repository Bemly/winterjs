---
title: "WinterJS"
slug: WinterJS/index
---

The **`WinterJS`** global object is the winterjs runtime namespace for
non-standard, runtime-specific APIs. Web standards stay on their own globals
(`fetch`, `URL`, `storage`); Node compatibility lives under `node:` modules.

## Syntax

```js
WinterJS.version
WinterJS.storage
WinterJS.localStorage
WinterJS.fs
WinterJS.memory
WinterJS.alloc
WinterJS.unsafeAlloc
WinterJS.CompressionStream
WinterJS.DecompressionStream
WinterJS.semver
WinterJS.yaml
WinterJS.jsonc
WinterJS.ip
WinterJS.shlex
WinterJS.spdx
WinterJS.qrcode
```

### Parameters

- `version`
  - : The current winterjs version string.
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
  - : Same class as global `CompressionStream` (plus winterjs `zstd` format).
- `DecompressionStream`
  - : Same class as global `DecompressionStream` (plus winterjs `zstd` format).
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
