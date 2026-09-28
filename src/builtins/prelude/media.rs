//! WinterJS.media JS 面（prelude 分域；拼接顺序见 mod.rs，namespace 之后）。
pub const MEDIA_JS: &str = r#"
{
  const __wjs_media_table = [
    { name: "mp3", kind: "audio", mime: "audio/mpeg", decode: true, encode: false },
    { name: "wav", kind: "audio", mime: "audio/wav", decode: true, encode: false },
    { name: "flac", kind: "audio", mime: "audio/flac", decode: true, encode: false },
    { name: "ogg", kind: "audio", mime: "audio/ogg", decode: true, encode: false },
    { name: "m4a", kind: "audio", mime: "audio/mp4", decode: true, encode: false },
    { name: "aac", kind: "audio", mime: "audio/aac", decode: true, encode: false },
    { name: "aiff", kind: "audio", mime: "audio/aiff", decode: true, encode: false },
    { name: "caf", kind: "audio", mime: "audio/x-caf", decode: true, encode: false },
    { name: "alac", kind: "audio", mime: "audio/mp4", decode: true, encode: false },
    { name: "mka", kind: "audio", mime: "audio/x-matroska", decode: true, encode: false },
    { name: "av1", kind: "video", mime: "video/av1", decode: false, encode: true },
    { name: "mp4", kind: "container", mime: "video/mp4", decode: true, encode: false },
  ];
  const __wjs_media_alias = { mp4a: "m4a", vorbis: "ogg", riff: "wav" };
  const __wjs_media_by_name = (f) => {
    const n = String(f).trim().toLowerCase();
    return __wjs_media_table.find((r) => r.name === n || r.name === (__wjs_media_alias[n] || ""));
  };
  // v1 取舍（文档记录）：解码全量进内存（134M f32 上限，流式另案）；动画首帧；
  // mp4 只做 demux（mux 跟进项）；play 后台线程即返，id 显式 stop。
  globalThis.WinterJS.media = {
    formats() { return __wjs_media_table.map((r) => ({ ...r })); },
    audioInfo(bytes, format) {
      if (!(bytes instanceof Uint8Array)) throw new TypeError("WinterJS.media.audioInfo requires Uint8Array bytes");
      return JSON.parse(__wjs_media_audio_info(bytes, format === undefined ? undefined : String(format)));
    },
    decodeAudio(bytes, format) {
      if (!(bytes instanceof Uint8Array)) throw new TypeError("WinterJS.media.decodeAudio requires Uint8Array bytes");
      let fmt = format === undefined ? undefined : String(format);
      const info = this.audioInfo(bytes, fmt);
      const u8 = __wjs_media_audio_pcm(bytes, fmt);
      const data = new Float32Array(u8.buffer, u8.byteOffset, u8.byteLength / 4);
      return { format: info.format, codec: info.codec, sampleRate: info.sampleRate, channels: info.channels, duration: info.duration, title: info.title, artist: info.artist, data };
    },
    play(pcm, options) {
      if (!pcm || !(pcm.data instanceof Float32Array)) {
        throw new TypeError("WinterJS.media.play requires { data: Float32Array, sampleRate, channels }");
      }
      const o = options || {};
      const id = Number(__wjs_media_play(pcm.data, pcm.sampleRate, pcm.channels, o.volume === undefined ? 1 : o.volume));
      return id;
    },
    stop(id) { return __wjs_media_stop(id) === "true"; },
    videoEncode(frames, options) {
      if (!frames || !(frames.data instanceof Uint8Array)) {
        throw new TypeError("WinterJS.media.videoEncode requires { data: Uint8Array (RGBA8 concat), width, height, count }");
      }
      const o = options || {};
      return __wjs_media_video_encode(frames.width, frames.height, frames.data, frames.count, JSON.stringify(o));
    },
    mp4Info(bytes) {
      if (!(bytes instanceof Uint8Array)) throw new TypeError("WinterJS.media.mp4Info requires Uint8Array bytes");
      return JSON.parse(__wjs_media_mp4info(bytes));
    },
    mp4Samples(bytes, track, limit) {
      if (!(bytes instanceof Uint8Array)) throw new TypeError("WinterJS.media.mp4Samples requires Uint8Array bytes");
      return JSON.parse(__wjs_media_mp4samples(bytes, track === undefined ? undefined : track, limit === undefined ? 1000 : limit));
    },
    mp4Sample(bytes, track, index) {
      if (!(bytes instanceof Uint8Array)) throw new TypeError("WinterJS.media.mp4Sample requires Uint8Array bytes");
      return __wjs_media_mp4sample(bytes, track, index);
    },
  };
}
"#;
