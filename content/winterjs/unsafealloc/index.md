---
title: "WinterJS.unsafeAlloc"
slug: WinterJS/unsafeAlloc
---

The **`WinterJS.unsafeAlloc()`** method allocates a manual byte block and
returns its id (the runtime keeps the bytes; your code only sees the id).
Operate it with `unsafeWrite` / `unsafeRead` / `unsafeSize`, release with
`unsafeFree`, list with `unsafeList`. Requires `--allow-ffi`; use-after-free,
double-free and out-of-bounds are readable errors.

## Syntax

```js
const id = WinterJS.unsafeAlloc(8)
```

### Parameters

- `size`
  - : A non-negative integer within 64MB.
