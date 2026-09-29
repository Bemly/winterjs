---
title: "WinterJS.media.play"
slug: WinterJS/media-play
---

The **`WinterJS.media.play()`** static method plays f32 PCM on the default
output device without blocking (background thread, numeric id back).
`WinterJS.media.stop(id)` drops the player (`false` for unknown ids).

## Syntax

```js
WinterJS.media.play({ data, sampleRate, channels }, options)
WinterJS.media.stop(id)
```

### Parameters

- `pcm`
  - : `{ data: Float32Array, sampleRate, channels }` (1..32 channels).
- `options`
  - : `volume` (finite number >= 0, default 1).
