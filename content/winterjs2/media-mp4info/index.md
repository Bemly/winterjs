---
title: "WinterJS2.media.mp4Info"
slug: WinterJS2/media-mp4info
---

The **`WinterJS2.media.mp4Info()`** static method demuxes an MP4 file to
`{ tracks: [{ id, kind, codec, timescale, duration, sampleCount,
totalBytes }] }`. Sample bytes come from `mp4Samples` (metadata) +
`mp4Sample` (bytes by track + index).

## Syntax

```js
WinterJS2.media.mp4Info(bytes)
WinterJS2.media.mp4Samples(bytes, track, limit)
WinterJS2.media.mp4Sample(bytes, track, index)
```

### Parameters

- `bytes`
  - : A `Uint8Array` holding the MP4 file.
- `track`
  - : Track id (`mp4Samples` omits it for all tracks).
- `limit`
  - : Max samples listed (default 1000, max 100000).
- `index`
  - : Zero-based sample index within the track.
