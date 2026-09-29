//! `node:internal/zip/dos`（Node lib/internal/zip/dos.js 逐字内嵌，MIT）。
pub const SOURCE: &str = r#"// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/zip/dos.js.
import errors from 'node:internal/errors';
import bufferMod from 'node:buffer';
import constants from 'node:internal/zip/constants';
import extraFields from 'node:internal/zip/extra-fields';
const {
  codes: {
    ERR_INVALID_ARG_VALUE: { HideStackFramesError: ERR_INVALID_ARG_VALUE },
  },
} = errors;
const { FLAG_UTF8, EXTRA_ID_UNICODE_PATH } = constants;
const { forEachExtraField } = extraFields;

function __zipCrc32(data, seed = 0) {
  if (typeof __wjs2_zlib_crc32 === 'function') {
    let bytes;
    if (typeof data === 'string') bytes = new TextEncoder().encode(data);
    else if (data instanceof Uint8Array) bytes = data;
    else bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    return __wjs2_zlib_crc32(bytes, seed >>> 0);
  }
  let crc = (seed ^ -1) >>> 0;
  const table = __zipCrc32.table ??= (() => {
    const t = new Uint32Array(256);
    for (let i = 0; i < 256; i++) {
      let c = i;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320 ^ (c >>> 1) : c >>> 1;
      t[i] = c >>> 0;
    }
    return t;
  })();
  for (let i = 0; i < data.length; i++) crc = table[(crc ^ data[i]) & 0xff] ^ (crc >>> 8);
  return (crc ^ -1) >>> 0;
}

function __zipIsUtf8(buf) {
  try {
    if (typeof Buffer !== 'undefined' && typeof Buffer.isUtf8 === 'function') return Buffer.isUtf8(buf);
    if (bufferMod && typeof bufferMod.isUtf8 === 'function') return bufferMod.isUtf8(buf);
  } catch {}
  try {
    new TextDecoder('utf-8', { fatal: true }).decode(buf);
    return true;
  } catch { return false; }
}

function decodeDosDateTime(time, date) {
  return new Date(
    ((date >>> 9) & 0x7f) + 1980,
    ((date >>> 5) & 0x0f || 1) - 1,
    (date & 0x1f) || 1,
    (time >>> 11) & 0x1f,
    (time >>> 5) & 0x3f,
    (time & 0x1f) * 2,
  );
}

function encodeDosDateTime(value) {
  const year = value.getFullYear();
  if (Number.isNaN(year)) {
    throw new ERR_INVALID_ARG_VALUE('modified', value, 'must be a valid Date');
  }
  if (year < 1980) return { time: 0, date: (1 << 5) | 1 };
  if (year > 2107) {
    return {
      time: (23 << 11) | (59 << 5) | 29,
      date: (127 << 9) | (12 << 5) | 31,
    };
  }
  const date =
    ((year - 1980) << 9) | ((value.getMonth() + 1) << 5) | value.getDate();
  const time =
    (value.getHours() << 11) |
    (value.getMinutes() << 5) |
    (value.getSeconds() >>> 1);
  return { time, date };
}

const CP437_HIGH = [
  0x00c7, 0x00fc, 0x00e9, 0x00e2, 0x00e4, 0x00e0, 0x00e5, 0x00e7,
  0x00ea, 0x00eb, 0x00e8, 0x00ef, 0x00ee, 0x00ec, 0x00c4, 0x00c5,
  0x00c9, 0x00e6, 0x00c6, 0x00f4, 0x00f6, 0x00f2, 0x00fb, 0x00f9,
  0x00ff, 0x00d6, 0x00dc, 0x00a2, 0x00a3, 0x00a5, 0x20a7, 0x0192,
  0x00e1, 0x00ed, 0x00f3, 0x00fa, 0x00f1, 0x00d1, 0x00aa, 0x00ba,
  0x00bf, 0x2310, 0x00ac, 0x00bd, 0x00bc, 0x00a1, 0x00ab, 0x00bb,
  0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x2561, 0x2562, 0x2556,
  0x2555, 0x2563, 0x2551, 0x2557, 0x255d, 0x255c, 0x255b, 0x2510,
  0x2514, 0x2534, 0x252c, 0x251c, 0x2500, 0x253c, 0x255e, 0x255f,
  0x255a, 0x2554, 0x2569, 0x2566, 0x2560, 0x2550, 0x256c, 0x2567,
  0x2568, 0x2564, 0x2565, 0x2559, 0x2558, 0x2552, 0x2553, 0x256b,
  0x256a, 0x2518, 0x250c, 0x2588, 0x2584, 0x258c, 0x2590, 0x2580,
  0x03b1, 0x00df, 0x0393, 0x03c0, 0x03a3, 0x03c3, 0x00b5, 0x03c4,
  0x03a6, 0x0398, 0x03a9, 0x03b4, 0x221e, 0x03c6, 0x03b5, 0x2229,
  0x2261, 0x00b1, 0x2265, 0x2264, 0x2320, 0x2321, 0x00f7, 0x2248,
  0x00b0, 0x2219, 0x00b7, 0x221a, 0x207f, 0x00b2, 0x25a0, 0x00a0,
];

function decodeCp437(buffer) {
  let out = '';
  for (let i = 0; i < buffer.length; i++) {
    const b = buffer[i];
    out += String.fromCharCode(b < 0x80 ? b : CP437_HIGH[b - 0x80]);
  }
  return out;
}

function unicodePathName(extra, standardNameBuffer) {
  let result = null;
  forEachExtraField(extra, (id, body) => {
    if (result !== null || id !== EXTRA_ID_UNICODE_PATH || body.length < 5) return;
    if (body[0] !== 1) return;
    if (body.readUInt32LE(1) !== __zipCrc32(standardNameBuffer, 0)) return;
    result = body.toString('utf8', 5);
  });
  return result;
}

function decodeZipName(nameBuffer, flags, extra) {
  if (extra?.length) {
    const unicode = unicodePathName(extra, nameBuffer);
    if (unicode !== null) return unicode;
  }
  return decodeZipText(nameBuffer, flags);
}

function decodeZipText(buffer, flags) {
  if ((flags & FLAG_UTF8) || __zipIsUtf8(buffer)) return buffer.toString('utf8');
  return decodeCp437(buffer);
}

export default {
  decodeDosDateTime,
  encodeDosDateTime,
  decodeZipName,
  decodeZipText,
};
export { decodeDosDateTime, encodeDosDateTime, decodeZipName, decodeZipText };
"#;
