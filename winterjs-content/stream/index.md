---
title: "WinterJS.stream"
slug: WinterJS/stream
---

The **`WinterJS.stream`** property provides `pipeline` over Web streams (backpressure included).

## Syntax

```js
await WinterJS.stream.pipeline(rs, ws)
```
### Parameters

- `streams`
  - : Readable + transforms + Writable.
