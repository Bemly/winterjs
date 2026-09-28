---
title: "WinterJS.image.decode"
slug: WinterJS/image-decode
---

The **`WinterJS.image.decode()`** static method decodes image bytes to an
`{ format, mime, width, height, data }` object, where `data` is RGBA8 pixels.
Animations decode to their first frame; `svg` rasterizes with `resvg`
(`scale` within (0, 32], dimensions follow the scale).

## Syntax

```js
WinterJS.image.decode(bytes)
WinterJS.image.decode(bytes, format)
WinterJS.image.decode(bytes, "svg", 2)
```

### Parameters

- `bytes`
  - : A `Uint8Array` holding the encoded file.
- `format`
  - : Optional format name or alias (`jpg`/`svgz` work); sniffed when omitted (`tga` has no magic and needs it).
- `scale`
  - : SVG-only rasterization scale.
