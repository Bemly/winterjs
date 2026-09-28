---
title: "Bun.sha"
slug: Bun/sha
---

Hash input using [SHA-2 512/256](https://en.wikipedia.org/wiki/SHA-2#Comparison_of_SHA_functions)
This hashing function balances speed with cryptographic strength. It does not encrypt or decrypt data.
The implementation uses [BoringSSL](https://boringssl.googlesource.com/boringssl) (used in Chromium & Go)
The equivalent openssl command is:

## Syntax

```ts
function sha(input: Bun.StringOrBuffer, hashInto?: NodeJS.TypedArray): NodeJS.TypedArray;
```

### Parameters

- `input`
  - : `string`, `Uint8Array`, or `ArrayBuffer` to hash. `Uint8Array` or `ArrayBuffer` is faster
- `hashInto`
  - : optional `Uint8Array` to write the hash to. 32 bytes minimum.
