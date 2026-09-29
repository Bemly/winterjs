// TextDecoder: bytes to strings, with fatal mode.
// TextDecoder：字节解成字符串，支持 fatal 严格模式。
// Run / 运行: winterjs --run sample/web/textdecoder.js
const bytes = new TextEncoder().encode('hi-中文');
console.log('[textdecoder] decoded:', new TextDecoder('utf-8').decode(bytes) === 'hi-中文');
console.log('[textdecoder] fatal ok:', new TextDecoder('utf-8', { fatal: true }).decode(new TextEncoder().encode('ok')) === 'ok');
console.log('[textdecoder] fatal-throws:', (() => { try { new TextDecoder('utf-8', { fatal: true }).decode(new Uint8Array([0xff])); return false; } catch { return true; } })());
