---
title: "WinterJS.media.decodeAudio"
slug: WinterJS/media-audiodecode
---

The **`WinterJS.media.decodeAudio()`** static method decodes an entire audio
file to `{ format, codec, sampleRate, channels, duration, title, artist,
data }`, where `data` is interleaved f32 PCM (`Float32Array`). First audio
track only; files over 134M samples need streaming decode (deferred).

## Syntax

```js
WinterJS.media.decodeAudio(bytes)
WinterJS.media.decodeAudio(bytes, format)
```

### Parameters

- `bytes`
  - : A `Uint8Array` holding the audio file.
- `format`
  - : Optional container hint (`mp3`/`wav`/`flac`/`ogg`/`m4a`/`aac`/`aiff`/`caf`/`alac`/`mka`); sniffed when omitted.
