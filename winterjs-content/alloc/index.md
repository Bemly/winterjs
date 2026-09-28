---
title: "WinterJS.alloc"
slug: WinterJS/alloc
---

The **`WinterJS.alloc()`** method allocates a zero-filled `Uint8Array` of the
given size (GC-managed, capped at 64MB). It is the controlled allocation face;
no raw pointers leave the runtime.

## Syntax

```js
WinterJS.alloc(4)
```

### Parameters

- `size`
  - : A non-negative integer within 64MB.
