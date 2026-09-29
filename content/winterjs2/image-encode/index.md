---
title: "WinterJS2.image.encode"
slug: WinterJS2/image-encode
---

The **`WinterJS2.image.encode()`** static method encodes RGBA8 pixels to an
image file (`Uint8Array`). Format support follows the underlying libraries:
`svg`/`jxl`/`dds` have no encoder. Quality options pass straight through.

## Syntax

```js
WinterJS2.image.encode({ data, width, height }, format)
WinterJS2.image.encode({ data, width, height }, "jpeg", { quality: 90 })
WinterJS2.image.encode({ data, width, height }, "png", { compression: "best", filter: "paeth" })
```

### Parameters

- `image`
  - : `{ data: Uint8Array, width, height }` with `data.length === width * height * 4`.
- `format`
  - : Target format name.
- `options`
  - : `jpeg`: `quality` 1..100. `png`: `compression` (`default`/`fast`/`best`/`uncompressed`/1..9) + `filter` (`none`/`sub`/`up`/`avg`/`paeth`/`adaptive`). `gif`: `speed` 1..30 + `repeat` (0 = infinite). `pnm`: `subtype` (`ppm`/`pgm`/`pbm`/`pam`) + `encoding` (`binary`/`ascii`). `webp` is lossless-only.
