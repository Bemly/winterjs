//! `node:internal/zip/extra-fields`（Node lib/internal/zip/extra-fields.js 逐字内嵌，MIT）。
pub const SOURCE: &str = r#"// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/zip/extra-fields.js.
import errors from 'node:internal/errors';
import bufferMod from 'node:buffer';
import constants from 'node:internal/zip/constants';
import binary from 'node:internal/zip/binary';
const {
  codes: {
    ERR_ZIP_INVALID_ARCHIVE: { HideStackFramesError: ERR_ZIP_INVALID_ARCHIVE },
  },
} = errors;
const { Buffer } = bufferMod;
const {
  EMPTY_BUFFER,
  ZIP64_EXTRA_ID,
  EXTRA_ID_NTFS,
  EXTRA_ID_EXT_TIMESTAMP,
  EXTRA_ID_UNIX_OLD,
} = constants;
const { readSafeUint64 } = binary;

function parseZip64Extra(extra, want) {
  const wanted =
    want.uncompressedSize ||
    want.compressedSize ||
    want.localFileHeaderOffset ||
    want.diskNumber;
  if (!wanted) return {};
  let pos = 0;
  while (pos + 4 <= extra.length) {
    const id = extra.readUInt16LE(pos);
    const size = extra.readUInt16LE(pos + 2);
    if (pos + 4 + size > extra.length) {
      throw new ERR_ZIP_INVALID_ARCHIVE('extra field is malformed');
    }
    if (id === ZIP64_EXTRA_ID) {
      const result = {};
      const body = pos + 4;
      const end = body + size;
      const wantedSize =
        (want.uncompressedSize ? 8 : 0) +
        (want.compressedSize ? 8 : 0) +
        (want.localFileHeaderOffset ? 8 : 0) +
        (want.diskNumber ? 4 : 0);
      const packed = size === wantedSize;
      let cursor = body;
      const take = (fullLayoutOffset, bytes) => {
        const at = packed ? cursor : body + fullLayoutOffset;
        if (at + bytes > end) {
          throw new ERR_ZIP_INVALID_ARCHIVE(
            'the Zip64 extended information extra field is truncated');
        }
        cursor += bytes;
        return bytes === 8 ? readSafeUint64(extra, at) : extra.readUInt32LE(at);
      };
      if (want.uncompressedSize) result.uncompressedSize = take(0, 8);
      if (want.compressedSize) result.compressedSize = take(8, 8);
      if (want.localFileHeaderOffset) result.localFileHeaderOffset = take(16, 8);
      if (want.diskNumber) result.diskNumber = take(24, 4);
      return result;
    }
    pos += 4 + size;
  }
  throw new ERR_ZIP_INVALID_ARCHIVE(
    'a field is 0xFFFFFFFF but the Zip64 extended information extra field is missing');
}

function forEachExtraField(extra, cb) {
  let pos = 0;
  while (pos + 4 <= extra.length) {
    const id = extra.readUInt16LE(pos);
    const size = extra.readUInt16LE(pos + 2);
    if (pos + 4 + size > extra.length) break;
    cb(id, extra.subarray(pos + 4, pos + 4 + size));
    pos += 4 + size;
  }
}

function parseNtfsMtime(body) {
  let result = null;
  let pos = 4;
  while (pos + 4 <= body.length) {
    const tag = body.readUInt16LE(pos);
    const size = body.readUInt16LE(pos + 2);
    if (pos + 4 + size > body.length) break;
    if (tag === 1 && size >= 8) {
      const ticks = body.readBigUInt64LE(pos + 4);
      result = new Date(Number(ticks / 10000n) - 11644473600000);
    }
    pos += 4 + size;
  }
  return result;
}

function parseExtTimestampMtime(body) {
  if (body.length < 5 || (body[0] & 1) === 0) return null;
  return new Date(body.readInt32LE(1) * 1000);
}

function parseUnixOldMtime(body) {
  if (body.length < 8) return null;
  return new Date(body.readInt32LE(4) * 1000);
}

function extraFieldMtime(...extras) {
  let ntfs = null;
  let ext = null;
  let unix = null;
  for (const extra of extras) {
    if (!extra?.length) continue;
    forEachExtraField(extra, (id, body) => {
      if (id === EXTRA_ID_NTFS) ntfs ??= parseNtfsMtime(body);
      else if (id === EXTRA_ID_EXT_TIMESTAMP) ext ??= parseExtTimestampMtime(body);
      else if (id === EXTRA_ID_UNIX_OLD) unix ??= parseUnixOldMtime(body);
    });
  }
  return ntfs ?? ext ?? unix;
}

function stripZip64Extra(extra) {
  if (!extra.length) return EMPTY_BUFFER;
  const parts = [];
  let total = 0;
  forEachExtraField(extra, (id, body) => {
    if (id === ZIP64_EXTRA_ID) return;
    const record = Buffer.allocUnsafe(4 + body.length);
    record.writeUInt16LE(id, 0);
    record.writeUInt16LE(body.length, 2);
    body.copy(record, 4);
    parts.push(record);
    total += record.length;
  });
  if (parts.length === 0) return EMPTY_BUFFER;
  return Buffer.concat(parts, total);
}

function buildExtTimestampExtra(seconds) {
  const buffer = Buffer.allocUnsafe(9);
  buffer.writeUInt16LE(EXTRA_ID_EXT_TIMESTAMP, 0);
  buffer.writeUInt16LE(5, 2);
  buffer.writeUInt8(0x01, 4);
  buffer.writeInt32LE(seconds, 5);
  return buffer;
}

export default {
  parseZip64Extra,
  forEachExtraField,
  extraFieldMtime,
  stripZip64Extra,
  buildExtTimestampExtra,
};
export { parseZip64Extra, forEachExtraField, extraFieldMtime, stripZip64Extra, buildExtTimestampExtra };
"#;
