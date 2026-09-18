//! `node:internal/zip/binary`（Node lib/internal/zip/binary.js 逐字内嵌，MIT）。
pub const SOURCE: &str = r#"// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/zip/binary.js.
import errors from 'node:internal/errors';
import types from 'node:internal/util/types';
import bufferMod from 'node:buffer';
import constants from 'node:internal/zip/constants';
const {
  codes: {
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
    ERR_ZIP_INVALID_ARCHIVE: { HideStackFramesError: ERR_ZIP_INVALID_ARCHIVE },
  },
} = errors;
const { isAnyArrayBuffer, isArrayBufferView, isUint8Array } = types;
const { Buffer } = bufferMod;
const { BIGINT_MAX_SAFE_INTEGER } = constants;

function validateArchiveRange(buffer, offset, length, what) {
  if (
    !Number.isInteger(offset) ||
    offset < 0 ||
    !Number.isInteger(length) ||
    length < 0 ||
    offset + length > buffer.length
  ) {
    throw new ERR_ZIP_INVALID_ARCHIVE(`${what} is out of bounds`);
  }
}

function readSafeUint64(buffer, offset) {
  if (offset + 8 > buffer.length) {
    throw new ERR_ZIP_INVALID_ARCHIVE('64-bit field is out of bounds');
  }
  const value = buffer.readBigUInt64LE(offset);
  if (value > BIGINT_MAX_SAFE_INTEGER) {
    throw new ERR_ZIP_INVALID_ARCHIVE('64-bit field exceeds the safe integer range');
  }
  return Number(value);
}

function writeSafeUint64(buffer, offset, value) {
  buffer.writeBigUInt64LE(BigInt(value), offset);
}

function toBuffer(value, name) {
  if (isUint8Array(value)) {
    return Buffer.isBuffer(value) ?
      value : Buffer.from(value.buffer, value.byteOffset, value.byteLength);
  }
  if (isArrayBufferView(value)) {
    return Buffer.from(value.buffer, value.byteOffset, value.byteLength);
  }
  if (isAnyArrayBuffer(value)) {
    return Buffer.from(value);
  }
  throw new ERR_INVALID_ARG_TYPE(
    name, ['Buffer', 'TypedArray', 'DataView', 'ArrayBuffer'], value);
}

export default { validateArchiveRange, readSafeUint64, writeSafeUint64, toBuffer };
export { validateArchiveRange, readSafeUint64, writeSafeUint64, toBuffer };
"#;
