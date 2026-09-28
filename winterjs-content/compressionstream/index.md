---
title: "WinterJS.CompressionStream"
slug: WinterJS/CompressionStream
---

The **`WinterJS.CompressionStream`** property is the same class as the global
`CompressionStream`. Formats `gzip`, `deflate`, `deflate-raw` follow the Web
standard; `zstd` is a winterjs extension (encodes at Fastest).

## Syntax

```js
new WinterJS.CompressionStream("zstd")
```

### Parameters

- `format`
  - : One of `gzip`, `deflate`, `deflate-raw`, `zstd`.
