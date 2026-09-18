//! `node:internal/zip/header-builders`（Node lib/internal/zip/header-builders.js 逐字内嵌，MIT）。
pub const SOURCE: &str = r#"// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/zip/header-builders.js.
import errors from 'node:internal/errors';
import bufferMod from 'node:buffer';
import constants from 'node:internal/zip/constants';
import dos from 'node:internal/zip/dos';
import binary from 'node:internal/zip/binary';
import extraFields from 'node:internal/zip/extra-fields';
const {
  codes: {
    ERR_ZIP_ENTRY_TOO_LARGE: { HideStackFramesError: ERR_ZIP_ENTRY_TOO_LARGE },
  },
} = errors;
const { Buffer } = bufferMod;
const {
  EMPTY_BUFFER,
  SIG_LOCAL_FILE_HEADER,
  SIG_DATA_DESCRIPTOR,
  SIG_CENTRAL_FILE_HEADER,
  SIG_ZIP64_EOCD_RECORD,
  SIG_ZIP64_EOCD_LOCATOR,
  SIG_EOCD,
  MADE_BY_UNIX,
  ZIP64_EXTRA_ID,
  SENTINEL16,
  SENTINEL32,
  FLAG_DATA_DESCRIPTOR,
  METHOD_ZSTD,
  VERSION_DEFAULT,
  VERSION_ZIP64,
  VERSION_ZSTD,
} = constants;
const { encodeDosDateTime } = dos;
const { writeSafeUint64 } = binary;
const { buildExtTimestampExtra } = extraFields;

function versionNeeded(meta, zip64) {
  return Math.max(
    zip64 ? VERSION_ZIP64 : VERSION_DEFAULT,
    meta.method === METHOD_ZSTD ? VERSION_ZSTD : 0);
}

function checkExtraLength(extraLength) {
  if (extraLength > SENTINEL16) {
    throw new ERR_ZIP_ENTRY_TOO_LARGE(
      'the entry extra fields must not exceed 65535 bytes');
  }
}

function buildLocalHeader(meta) {
  const streaming = (meta.flags & FLAG_DATA_DESCRIPTOR) !== 0;
  const zip64 =
    streaming ||
    meta.compressedSize >= SENTINEL32 ||
    meta.uncompressedSize >= SENTINEL32;
  const ts = meta.extendedMtime !== null ? buildExtTimestampExtra(meta.extendedMtime) : EMPTY_BUFFER;
  const extraLength = (zip64 ? 20 : 0) + ts.length + meta.extra.length;
  checkExtraLength(extraLength);
  const buffer = Buffer.allocUnsafe(30 + meta.name.length + extraLength);
  buffer.writeUInt32LE(SIG_LOCAL_FILE_HEADER, 0);
  buffer.writeUInt16LE(versionNeeded(meta, zip64), 4);
  buffer.writeUInt16LE(meta.flags, 6);
  buffer.writeUInt16LE(meta.method, 8);
  const { time, date } = encodeDosDateTime(meta.modified);
  buffer.writeUInt16LE(time, 10);
  buffer.writeUInt16LE(date, 12);
  buffer.writeUInt32LE(streaming ? 0 : meta.crc, 14);
  buffer.writeUInt32LE(zip64 ? SENTINEL32 : meta.compressedSize, 18);
  buffer.writeUInt32LE(zip64 ? SENTINEL32 : meta.uncompressedSize, 22);
  buffer.writeUInt16LE(meta.name.length, 26);
  buffer.writeUInt16LE(extraLength, 28);
  meta.name.copy(buffer, 30);
  let pos = 30 + meta.name.length;
  if (zip64) {
    buffer.writeUInt16LE(ZIP64_EXTRA_ID, pos);
    buffer.writeUInt16LE(16, pos + 2);
    writeSafeUint64(buffer, pos + 4, streaming ? 0 : meta.uncompressedSize);
    writeSafeUint64(buffer, pos + 12, streaming ? 0 : meta.compressedSize);
    pos += 20;
  }
  ts.copy(buffer, pos);
  pos += ts.length;
  meta.extra.copy(buffer, pos);
  return buffer;
}

function buildCentralHeader(meta, localOffset) {
  const zip64Streaming = (meta.flags & FLAG_DATA_DESCRIPTOR) !== 0;
  const u64 = meta.uncompressedSize >= SENTINEL32;
  const c64 = meta.compressedSize >= SENTINEL32;
  const o64 = localOffset >= SENTINEL32;
  const zip64Fields = (u64 ? 1 : 0) + (c64 ? 1 : 0) + (o64 ? 1 : 0);
  const ts = meta.extendedMtime !== null ? buildExtTimestampExtra(meta.extendedMtime) : EMPTY_BUFFER;
  const extraLength = (zip64Fields ? 4 + 8 * zip64Fields : 0) + ts.length + meta.extra.length;
  checkExtraLength(extraLength);
  const zip64 = zip64Streaming || zip64Fields > 0;
  const version = versionNeeded(meta, zip64);
  const buffer = Buffer.allocUnsafe(
    46 + meta.name.length + extraLength + meta.comment.length);
  buffer.writeUInt32LE(SIG_CENTRAL_FILE_HEADER, 0);
  buffer.writeUInt16LE((meta.madeBy << 8) | version, 4);
  buffer.writeUInt16LE(version, 6);
  buffer.writeUInt16LE(meta.flags, 8);
  buffer.writeUInt16LE(meta.method, 10);
  const { time, date } = encodeDosDateTime(meta.modified);
  buffer.writeUInt16LE(time, 12);
  buffer.writeUInt16LE(date, 14);
  buffer.writeUInt32LE(meta.crc, 16);
  buffer.writeUInt32LE(c64 ? SENTINEL32 : meta.compressedSize, 20);
  buffer.writeUInt32LE(u64 ? SENTINEL32 : meta.uncompressedSize, 24);
  buffer.writeUInt16LE(meta.name.length, 28);
  buffer.writeUInt16LE(extraLength, 30);
  buffer.writeUInt16LE(meta.comment.length, 32);
  buffer.writeUInt16LE(0, 34);
  buffer.writeUInt16LE(meta.internal, 36);
  buffer.writeUInt32LE(meta.external, 38);
  buffer.writeUInt32LE(o64 ? SENTINEL32 : localOffset, 42);
  meta.name.copy(buffer, 46);
  let pos = 46 + meta.name.length;
  if (zip64Fields) {
    buffer.writeUInt16LE(ZIP64_EXTRA_ID, pos);
    buffer.writeUInt16LE(8 * zip64Fields, pos + 2);
    pos += 4;
    if (u64) {
      writeSafeUint64(buffer, pos, meta.uncompressedSize);
      pos += 8;
    }
    if (c64) {
      writeSafeUint64(buffer, pos, meta.compressedSize);
      pos += 8;
    }
    if (o64) {
      writeSafeUint64(buffer, pos, localOffset);
      pos += 8;
    }
  }
  ts.copy(buffer, pos);
  pos += ts.length;
  meta.extra.copy(buffer, pos);
  pos += meta.extra.length;
  meta.comment.copy(buffer, pos);
  return buffer;
}

function buildDataDescriptor64(crc, compressedSize, uncompressedSize) {
  const buffer = Buffer.allocUnsafe(24);
  buffer.writeUInt32LE(SIG_DATA_DESCRIPTOR, 0);
  buffer.writeUInt32LE(crc, 4);
  writeSafeUint64(buffer, 8, compressedSize);
  writeSafeUint64(buffer, 16, uncompressedSize);
  return buffer;
}

function buildEndOfCentralDirectory(count, size, offset, comment) {
  const buffer = Buffer.allocUnsafe(22 + comment.length);
  buffer.writeUInt32LE(SIG_EOCD, 0);
  buffer.writeUInt16LE(0, 4);
  buffer.writeUInt16LE(0, 6);
  buffer.writeUInt16LE(Math.min(count, SENTINEL16), 8);
  buffer.writeUInt16LE(Math.min(count, SENTINEL16), 10);
  buffer.writeUInt32LE(Math.min(size, SENTINEL32), 12);
  buffer.writeUInt32LE(Math.min(offset, SENTINEL32), 16);
  buffer.writeUInt16LE(comment.length, 20);
  comment.copy(buffer, 22);
  return buffer;
}

function buildZip64EndRecord(count, size, offset) {
  const buffer = Buffer.allocUnsafe(56);
  buffer.writeUInt32LE(SIG_ZIP64_EOCD_RECORD, 0);
  writeSafeUint64(buffer, 4, 44);
  buffer.writeUInt16LE((MADE_BY_UNIX << 8) | VERSION_ZIP64, 12);
  buffer.writeUInt16LE(VERSION_ZIP64, 14);
  buffer.writeUInt32LE(0, 16);
  buffer.writeUInt32LE(0, 20);
  writeSafeUint64(buffer, 24, count);
  writeSafeUint64(buffer, 32, count);
  writeSafeUint64(buffer, 40, size);
  writeSafeUint64(buffer, 48, offset);
  return buffer;
}

function buildZip64EndLocator(recordOffset) {
  const buffer = Buffer.allocUnsafe(20);
  buffer.writeUInt32LE(SIG_ZIP64_EOCD_LOCATOR, 0);
  buffer.writeUInt32LE(0, 4);
  writeSafeUint64(buffer, 8, recordOffset);
  buffer.writeUInt32LE(1, 16);
  return buffer;
}

function buildArchiveTrailer(count, centralDirectorySize, centralDirectoryOffset, comment) {
  const chunks = [];
  const zip64 =
    count >= SENTINEL16 ||
    centralDirectoryOffset >= SENTINEL32 ||
    centralDirectorySize >= SENTINEL32;
  if (zip64) {
    const recordOffset = centralDirectoryOffset + centralDirectorySize;
    chunks.push(buildZip64EndRecord(count, centralDirectorySize, centralDirectoryOffset));
    chunks.push(buildZip64EndLocator(recordOffset));
  }
  chunks.push(
    buildEndOfCentralDirectory(count, centralDirectorySize, centralDirectoryOffset, comment));
  return chunks;
}

export default {
  buildLocalHeader,
  buildCentralHeader,
  buildDataDescriptor64,
  buildEndOfCentralDirectory,
  buildZip64EndRecord,
  buildZip64EndLocator,
  buildArchiveTrailer,
};
export {
  buildLocalHeader,
  buildCentralHeader,
  buildDataDescriptor64,
  buildEndOfCentralDirectory,
  buildZip64EndRecord,
  buildZip64EndLocator,
  buildArchiveTrailer,
};
"#;
