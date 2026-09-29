---
title: "WinterJS2.CompressionStream"
slug: WinterJS2/CompressionStream
---

The **`WinterJS2.CompressionStream`** property is the same class as the global
`CompressionStream`. Formats `gzip`, `deflate`, `deflate-raw` follow the Web
standard; `zstd` is a winterjs2 extension (encodes at Fastest).

## Syntax

```js
new WinterJS2.CompressionStream("zstd")
```

### Parameters

- `format`
  - : One of `gzip`, `deflate`, `deflate-raw`, `zstd`.
