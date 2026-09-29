---
title: "Bun.hash"
slug: Bun/hash
---

Hash a string or array buffer using Wyhash
This is not a cryptographic hash function.

## Syntax

```ts
const hash: (( data: string | ArrayBufferView | ArrayBuffer | SharedArrayBuffer, seed?: number | bigint, ) => number | bigint) & Hash;
```

### Parameters

- `data`
  - : The data to hash.
- `seed`
  - : The seed to use.
