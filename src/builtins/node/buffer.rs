//! `node:buffer`——全局 Buffer（prelude 逐字移植实现，见 builtins/mod.rs 头注）
//! 的模块面。Node `lib/buffer.js` exports 原文口径：Buffer/transcode/isUtf8/
//! isAscii/kMaxLength/kStringMaxLength/btoa/atob + constants/INSPECT_MAX_BYTES
//! (getter/setter)/File/Blob。SlowBuffer 已移除（真机 26 `typeof undefined`，§4.65）。
pub const SOURCE: &str = r#"
const Buffer = globalThis.Buffer;
const api = globalThis.__wjs2_bufApi;

const constants = {
  MAX_LENGTH: api.kMaxLength,
  MAX_STRING_LENGTH: api.kStringMaxLength,
};

const buffer = {
  Buffer,
  transcode: api.transcode,
  isUtf8: api.isUtf8,
  isAscii: api.isAscii,
  kMaxLength: api.kMaxLength,
  kStringMaxLength: api.kStringMaxLength,
  btoa: api.btoa,
  atob: api.atob,
  constants,
  File: globalThis.File,
  Blob: globalThis.Blob,
};
// 10f：具名 `INSPECT_MAX_BYTES` 真机 26 可 import（值为 50）；ESM live 性经
// 默认对象 setter 回写（import 侧只读，真机同款）。
export let INSPECT_MAX_BYTES = api.INSPECT_MAX_BYTES;
Object.defineProperty(buffer, 'INSPECT_MAX_BYTES', {
  enumerable: true,
  configurable: true,
  get() { return INSPECT_MAX_BYTES; },
  set(val) { api.INSPECT_MAX_BYTES = val; INSPECT_MAX_BYTES = api.INSPECT_MAX_BYTES; },
});

export default buffer;
export { Buffer, constants };
export const transcode = api.transcode;
export const isUtf8 = api.isUtf8;
export const isAscii = api.isAscii;
// kMaxLength/kStringMaxLength 可劫持（zlib kmaxlength 套件改模块导出触发；
// Node lib/buffer.js 同款可写，const 导出赋值即抛，真机口径为准）。
export let kMaxLength = api.kMaxLength;
Object.defineProperty(buffer, 'kMaxLength', {
  enumerable: true,
  configurable: true,
  get() { return kMaxLength; },
  set(val) { kMaxLength = val; },
});
export let kStringMaxLength = api.kStringMaxLength;
Object.defineProperty(buffer, 'kStringMaxLength', {
  enumerable: true,
  configurable: true,
  get() { return kStringMaxLength; },
  set(val) { kStringMaxLength = val; },
});
export const btoa = api.btoa;
export const atob = api.atob;
export const File = globalThis.File;
export const Blob = globalThis.Blob;
"#;
