---
title: "WinterJS.media.videoEncode"
slug: WinterJS/media-videoencode
---

The **`WinterJS.media.videoEncode()`** static method encodes RGBA8 frames to
an AV1 IVF file (`Uint8Array`). Frames convert to YUV420 (BT.601 full range);
dimensions must be even and within 16..4096, 1..256 frames, 16M pixels total.

## Syntax

```js
WinterJS.media.videoEncode({ data, width, height, count }, options)
```

### Parameters

- `frames`
  - : `{ data: Uint8Array, width, height, count }` with concatenated RGBA8 frames.
- `options`
  - : `speed` 0..10 (default 8), `quantizer` 0..255 (default 100), `fps` 1..120 (default 30).
