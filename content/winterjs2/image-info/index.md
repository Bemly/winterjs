---
title: "WinterJS2.image.info"
slug: WinterJS2/image-info
---

The **`WinterJS2.image.info()`** static method reads `{ format, width, height,
mime }` without decoding pixels.

## Syntax

```js
WinterJS2.image.info(bytes)
WinterJS2.image.info(bytes, format)
```

### Parameters

- `bytes`
  - : A `Uint8Array` holding the encoded file.
- `format`
  - : Optional format name; sniffed when omitted.
