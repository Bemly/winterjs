---
title: "WinterJS2.alloc"
slug: WinterJS2/alloc
---

The **`WinterJS2.alloc()`** method allocates a zero-filled `Uint8Array` of the
given size (garbage-collected, capped at 64MB). Memory is managed for you;
there is nothing to free.

## Syntax

```js
WinterJS2.alloc(4)
```

### Parameters

- `size`
  - : A non-negative integer within 64MB.
