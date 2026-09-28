---
title: "WinterJS.media"
slug: WinterJS/media
---

The **`WinterJS.media`** object decodes audio to f32 PCM, plays PCM through
the default output device, encodes AV1 video, and demuxes MP4. Audio rides
`symphonia`, playback rides `rodio`, AV1 rides `rav1e`, MP4 rides `mp4-rs`.

## Syntax

```js
WinterJS.media.audioInfo(bytes)
WinterJS.media.decodeAudio(bytes)
WinterJS.media.play({ data, sampleRate, channels })
WinterJS.media.videoEncode({ data, width, height, count })
WinterJS.media.mp4Info(bytes)
```

### Parameters

- `bytes`
  - : A `Uint8Array` holding the media file.
