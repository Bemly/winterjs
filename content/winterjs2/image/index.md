---
title: "WinterJS2.image"
slug: WinterJS2/image
---

The **`WinterJS2.image`** object decodes images to RGBA8 pixels and encodes
RGBA8 pixels back to bytes. Raster formats ride the `image` crate, `svg` is
rasterized with `resvg`, `jxl` is decoded with `jxl-oxide`.

## Syntax

```js
WinterJS2.image.info(bytes)
WinterJS2.image.decode(bytes, format?, scale?)
WinterJS2.image.encode({ data, width, height }, format, options?)
WinterJS2.image.formats()
```

### Parameters

- `bytes`
  - : A `Uint8Array` holding the encoded file.
- `format`
  - : `png`/`jpeg`/`gif`/`webp`/`tiff`/`tga`/`bmp`/`ico`/`hdr`/`exr`/`pnm`/`farbfeld`/`qoi`/`svg`/`jxl` (`jpg`/`tif`/`ff`/`svgz` aliases work).
- `scale`
  - : SVG rasterization scale within (0, 32].
