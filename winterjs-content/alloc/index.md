---
title: "WinterJS.alloc"
slug: WinterJS/alloc
---

The **`WinterJS.alloc()`** method allocates a zero-filled `Uint8Array` of the
given size (garbage-collected, capped at 64MB). Memory is managed for you;
there is nothing to free.

## Syntax

```js
WinterJS.alloc(4)
```

### Parameters

- `size`
  - : A non-negative integer within 64MB.
