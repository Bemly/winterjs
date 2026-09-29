---
title: "WinterJS2.DecompressionStream"
slug: WinterJS2/DecompressionStream
---

The **`WinterJS2.DecompressionStream`** property is the same class as the
global `DecompressionStream`. Formats `gzip`, `deflate`, `deflate-raw` follow
the Web standard; `zstd` is a winterjs2 extension (full decode).

## Syntax

```js
new WinterJS2.DecompressionStream("zstd")
```

### Parameters

- `format`
  - : One of `gzip`, `deflate`, `deflate-raw`, `zstd`.
