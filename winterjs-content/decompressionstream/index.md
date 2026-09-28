---
title: "WinterJS.DecompressionStream"
slug: WinterJS/DecompressionStream
---

The **`WinterJS.DecompressionStream`** property is the same class as the
global `DecompressionStream`. Formats `gzip`, `deflate`, `deflate-raw` follow
the Web standard; `zstd` is a winterjs extension (full decode).

## Syntax

```js
new WinterJS.DecompressionStream("zstd")
```

### Parameters

- `format`
  - : One of `gzip`, `deflate`, `deflate-raw`, `zstd`.
