---
title: "WinterJS.image.info"
slug: WinterJS/image-info
---

The **`WinterJS.image.info()`** static method reads `{ format, width, height,
mime }` without decoding pixels.

## Syntax

```js
WinterJS.image.info(bytes)
WinterJS.image.info(bytes, format)
```

### Parameters

- `bytes`
  - : A `Uint8Array` holding the encoded file.
- `format`
  - : Optional format name; sniffed when omitted.
