//! Buffer encodingOps 表（prelude 分域；拼接顺序见 mod.rs）。
pub const BUFFER_OPS_JS: &str = r#"
// ---- encodingOps（lib/buffer.js 原文结构）----
function __wjs_bufByteLengthUtf8(string) {
  return new TextEncoder().encode(string).length;
}
const __wjs_bufEncodingOps = {
  utf8: {
    encoding: 'utf8',
    byteLength: __wjs_bufByteLengthUtf8,
    write: __wjs_bufUtf8Write,
    slice: __wjs_bufUtf8Slice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfString(buf, val, byteOffset, 'utf8', dir, end),
  },
  ucs2: {
    encoding: 'utf16le',
    byteLength: (string) => string.length * 2,
    write: __wjs_bufUcs2Write,
    slice: __wjs_bufUcs2Slice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfString(buf, val, byteOffset, 'utf16le', dir, end),
  },
  utf16le: {
    encoding: 'utf16le',
    byteLength: (string) => string.length * 2,
    write: __wjs_bufUcs2Write,
    slice: __wjs_bufUcs2Slice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfString(buf, val, byteOffset, 'utf16le', dir, end),
  },
  latin1: {
    encoding: 'latin1',
    byteLength: (string) => string.length,
    write: __wjs_bufLatin1Write,
    slice: __wjs_bufLatin1Slice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfString(buf, val, byteOffset, 'latin1', dir, end),
  },
  ascii: {
    encoding: 'ascii',
    byteLength: (string) => string.length,
    write: __wjs_bufAsciiWrite,
    slice: __wjs_bufAsciiSlice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfBytes(buf, __wjs_bufEncodeStr(val, __wjs_bufEncodingOps.ascii), byteOffset, dir, end),
  },
  base64: {
    encoding: 'base64',
    byteLength: (string) => __wjs_bufB64ByteLength(string, string.length),
    write: __wjs_bufBase64Write,
    slice: (u8, start, end) => __wjs_bufB64Slice(u8, start, end, false),
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfBytes(buf, __wjs_bufEncodeStr(val, __wjs_bufEncodingOps.base64), byteOffset, dir, end),
  },
  base64url: {
    encoding: 'base64url',
    byteLength: (string) => __wjs_bufB64ByteLength(string, string.length),
    write: __wjs_bufBase64urlWrite,
    slice: (u8, start, end) => __wjs_bufB64Slice(u8, start, end, true),
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfBytes(buf, __wjs_bufEncodeStr(val, __wjs_bufEncodingOps.base64url), byteOffset, dir, end),
  },
  hex: {
    encoding: 'hex',
    byteLength: (string) => string.length >>> 1,
    write: __wjs_bufHexWrite,
    slice: __wjs_bufHexSlice,
    indexOf: (buf, val, byteOffset, dir, end) =>
      __wjs_bufIndexOfBytes(buf, __wjs_bufEncodeStr(val, __wjs_bufEncodingOps.hex), byteOffset, dir, end),
  },
};
function __wjs_bufGetEncodingOps(encoding) {
  encoding += '';
  switch (encoding.length) {
    case 4:
      if (encoding === 'utf8') return __wjs_bufEncodingOps.utf8;
      if (encoding === 'ucs2') return __wjs_bufEncodingOps.ucs2;
      encoding = encoding.toLowerCase();
      if (encoding === 'utf8') return __wjs_bufEncodingOps.utf8;
      if (encoding === 'ucs2') return __wjs_bufEncodingOps.ucs2;
      break;
    case 5:
      if (encoding === 'utf-8') return __wjs_bufEncodingOps.utf8;
      if (encoding === 'ascii') return __wjs_bufEncodingOps.ascii;
      if (encoding === 'ucs-2') return __wjs_bufEncodingOps.ucs2;
      encoding = encoding.toLowerCase();
      if (encoding === 'utf-8') return __wjs_bufEncodingOps.utf8;
      if (encoding === 'ascii') return __wjs_bufEncodingOps.ascii;
      if (encoding === 'ucs-2') return __wjs_bufEncodingOps.ucs2;
      break;
    case 7:
      if (encoding === 'utf16le' || encoding.toLowerCase() === 'utf16le')
        return __wjs_bufEncodingOps.utf16le;
      break;
    case 8:
      if (encoding === 'utf-16le' || encoding.toLowerCase() === 'utf-16le')
        return __wjs_bufEncodingOps.utf16le;
      break;
    case 6:
      if (encoding === 'latin1' || encoding === 'binary') return __wjs_bufEncodingOps.latin1;
      if (encoding === 'base64') return __wjs_bufEncodingOps.base64;
      encoding = encoding.toLowerCase();
      if (encoding === 'latin1' || encoding === 'binary') return __wjs_bufEncodingOps.latin1;
      if (encoding === 'base64') return __wjs_bufEncodingOps.base64;
      break;
    case 3:
      if (encoding === 'hex' || encoding.toLowerCase() === 'hex') return __wjs_bufEncodingOps.hex;
      break;
    case 9:
      if (encoding === 'base64url' || encoding.toLowerCase() === 'base64url')
        return __wjs_bufEncodingOps.base64url;
      break;
  }
}
"#;
