---
title: "WinterJS2.media"
slug: WinterJS2/media
---

The **`WinterJS2.media`** object decodes audio to f32 PCM, plays PCM through
the default output device, encodes AV1 video, and demuxes MP4. Audio rides
`symphonia`, playback rides `rodio`, AV1 rides `rav1e`, MP4 rides `mp4-rs`.

## Syntax

```js
WinterJS2.media.audioInfo(bytes)
WinterJS2.media.decodeAudio(bytes)
WinterJS2.media.play({ data, sampleRate, channels })
WinterJS2.media.videoEncode({ data, width, height, count })
WinterJS2.media.mp4Info(bytes)
```

### Parameters

- `bytes`
  - : A `Uint8Array` holding the media file.
