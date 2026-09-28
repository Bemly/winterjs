// WinterJS.media: audio decode/play + AV1 encode + MP4 demux (all offline).
// WinterJS.media：音频解码/放音 + AV1 编码 + MP4 解复用（全离线）。
// Run / 运行: winterjs --run sample/media/basics.js
const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);

// Synth 1s 440Hz mono 8kHz WAV in-JS (44-byte header + i16 PCM).
function synthWav() {
  const rate = 8000, n = 8000;
  const buf = new Uint8Array(44 + n * 2);
  const dv = new DataView(buf.buffer);
  const wstr = (o, s) => { for (let i = 0; i < s.length; i++) dv.setUint8(o + i, s.charCodeAt(i)); };
  wstr(0, 'RIFF'); dv.setUint32(4, 36 + n * 2, true); wstr(8, 'WAVE');
  wstr(12, 'fmt '); dv.setUint32(16, 16, true); dv.setUint16(20, 1, true);
  dv.setUint16(22, 1, true); dv.setUint32(24, rate, true); dv.setUint32(28, rate * 2, true);
  dv.setUint16(32, 2, true); dv.setUint16(34, 16, true);
  wstr(36, 'data'); dv.setUint32(40, n * 2, true);
  for (let i = 0; i < n; i++) dv.setInt16(44 + i * 2, Math.round(Math.sin(i * 440 * 2 * Math.PI / rate) * 30000), true);
  return { bytes: buf, rate, n };
}

const wav = synthWav();
const info = WinterJS.media.audioInfo(wav.bytes, 'wav');
console.log('[media] info:', info.format === 'wav' && info.codec === 'pcm' && info.sampleRate === 8000 && info.channels === 1);
const dec = WinterJS.media.decodeAudio(wav.bytes, 'wav');
console.log('[media] decode:', dec.data.length === 8000 && dec.data.constructor.name === 'Float32Array');
// Sine energy check: mean absolute value well above silence.
let e = 0;
for (let i = 0; i < dec.data.length; i++) e += Math.abs(dec.data[i]);
console.log('[media] energy:', e / dec.data.length > 0.2);

// Playback is best-effort (headless CI has no device): id number or clean error.
try {
  const id = WinterJS.media.play({ data: dec.data.slice(0, 800), sampleRate: 8000, channels: 1 }, { volume: 0 });
  console.log('[media] play:', typeof id === 'number' && WinterJS.media.stop(id) === true);
} catch (err) {
  console.log('[media] play:', String(err.message).includes('no audio output'));
}

// AV1: 2 frames 16x16, fastest preset.
const w = 16, h = 16;
const frame = new Uint8Array(w * h * 4).fill(128);
const frames = new Uint8Array(frame.length * 2);
frames.set(frame, 0); frames.set(frame, frame.length);
const ivf = WinterJS.media.videoEncode({ data: frames, width: w, height: h, count: 2 }, { speed: 10, quantizer: 200 });
console.log('[media] av1:', String.fromCharCode(...ivf.slice(0, 4)) === 'DKIF' && WinterJS.media.formats().find((f) => f.name === 'av1').encode);

console.log('[media] formats:', WinterJS.media.formats().filter((f) => f.decode).map((f) => f.name).join(','));
