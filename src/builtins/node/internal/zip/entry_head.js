'use strict';
import * as __m0 from 'node:buffer';
import * as __m1 from 'node:internal/errors';
import * as __m2 from 'node:internal/util/types';
import * as __m3 from 'node:internal/validators';
import * as __m4 from 'node:internal/zip/binary';
import * as __m5 from 'node:internal/zip/compression';
import * as __m6 from 'node:internal/zip/constants';
import * as __m7 from 'node:internal/zip/content-size';
import * as __m8 from 'node:internal/zip/dos';
import * as __m9 from 'node:internal/zip/extra-fields';
import * as __m10 from 'node:internal/zip/fs-util';
import * as __m11 from 'node:internal/zip/header-builders';
import * as __m12 from 'node:internal/zip/headers';

const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'buffer': return __m0.default ?? __m0;
    case 'internal/errors': return __m1.default ?? __m1;
    case 'internal/util/types': return __m2.default ?? __m2;
    case 'internal/validators': return __m3.default ?? __m3;
    case 'internal/zip/binary': return __m4.default ?? __m4;
    case 'internal/zip/compression': return __m5.default ?? __m5;
    case 'internal/zip/constants': return __m6.default ?? __m6;
    case 'internal/zip/content-size': return __m7.default ?? __m7;
    case 'internal/zip/dos': return __m8.default ?? __m8;
    case 'internal/zip/extra-fields': return __m9.default ?? __m9;
    case 'internal/zip/fs-util': return __m10.default ?? __m10;
    case 'internal/zip/header-builders': return __m11.default ?? __m11;
    case 'internal/zip/headers': return __m12.default ?? __m12;
    default: throw new Error('unmapped internal require: ' + spec);
  }
}
const module = { exports: { __proto__: null } };



// `ZipEntry`: a single archive member. Reads (buffered and streaming),
// (de)serialization, and the `create()`/`createSync()`/`createStream()`/
// `createSymlink()` builders, plus the `createEntryMeta()` helper that
// normalizes their options into the internal metadata record.

import { primordials } from 'node:internal/primordials';
const { ArrayPrototypePush, ArrayPrototypeSlice, ArrayPrototypeSort, BigInt, Date, DateNow, FunctionPrototypeCall, JSONStringify, Map, MapPrototypeClear, MapPrototypeDelete, MapPrototypeEntries, MapPrototypeGet, MapPrototypeGetSize, MapPrototypeHas, MapPrototypeKeys, MapPrototypeSet, MathFloor, MathMax, MathMin, Number, NumberIsInteger, NumberIsNaN, NumberMAX_SAFE_INTEGER, Promise, PromisePrototypeThen, PromiseResolve, StringFromCharCode, StringPrototypeEndsWith, Symbol, SymbolAsyncDispose, SymbolAsyncIterator, SymbolDispose, SymbolIterator, SymbolToStringTag } = primordials;

const {
  codes: {
    ERR_INVALID_ARG_TYPE,
    ERR_INVALID_ARG_VALUE,
    ERR_INVALID_STATE,
    ERR_ZIP_ENTRY_TOO_LARGE,
    ERR_ZIP_INVALID_ARCHIVE,
    ERR_ZIP_UNSUPPORTED_FEATURE,
  },
} = require('internal/errors');
const {
  validateInteger,
  validateString,
  validateUint32,
} = require('internal/validators');
const {
  isDate,
  isUint8Array,
} = require('internal/util/types');
const { Buffer, kMaxLength } = require('buffer');
function crc32Native(data, seed) { if (typeof __wjs_zlib_crc32 === 'function') { const bytes = data instanceof Uint8Array ? data : new Uint8Array(data.buffer, data.byteOffset, data.byteLength); return __wjs_zlib_crc32(bytes, seed >>> 0); } throw new Error('crc32 native unavailable'); }
const {
  EMPTY_BUFFER,
  SIG_LOCAL_FILE_HEADER,
  SENTINEL16,
  FLAG_DATA_DESCRIPTOR,
  FLAG_UTF8,
  MADE_BY_UNIX,
  METHOD_STORE,
  METHOD_DEFLATE,
  METHOD_ZSTD,
  S_IFREG,
  S_IFDIR,
  S_IFLNK,
  S_IFMT,
  READ_CHUNK_SIZE,
  kFinalize,
  kPromote,
} = require('internal/zip/constants');
const {
  toBuffer,
  validateArchiveRange,
} = require('internal/zip/binary');
const {
  extraFieldMtime,
  stripZip64Extra,
} = require('internal/zip/extra-fields');
const {
  decodeZipName,
  decodeZipText,
} = require('internal/zip/dos');
const {
  CentralFileHeader,
  LocalFileHeader,
  assertConsistentLocalHeader,
  findArchiveEnd,
} = require('internal/zip/headers');
const {
  buildLocalHeader,
  buildCentralHeader,
  buildDataDescriptor64,
} = require('internal/zip/header-builders');
const {
  deflateRawAsync,
  deflateRawSync,
  zstdCompressAsync,
  zstdCompressSync,
  deflateRawStream,
  zstdCompressStream,
  decodeMemberStream,
  decodeMemberAsync,
  decodeMemberSync,
} = require('internal/zip/compression');
const {
  readFdFully,
  readFdFullySync,
} = require('internal/zip/fs-util');
const { getMaxZipContentSize } = require('internal/zip/content-size');

// The whole (UTC) second to record in an extended-timestamp extra field when
// `mtimeMs` cannot be represented exactly by the 2-second-resolution,
// local-time DOS date/time fields (sub-second parts and odd seconds alike),
// or null when the DOS fields suffice or the value does not fit the extra
// field's signed 32-bit Unix-seconds range (through 2038). One rule, shared
// by createEntryMeta() and #finalizeMeta() so the two cannot drift.
function extendedMtimeSeconds(mtimeMs) {
  const seconds = MathFloor(mtimeMs / 1000);
  return (mtimeMs % 2000 !== 0 && seconds >= -2147483648 && seconds <= 2147483647) ?
    seconds : null;
}

// Normalize the public builder options into the internal metadata record:
// name/comment bytes, the UTF-8 flag, the Unix mode packed into the external
// attributes (sec. 4.4.15), and the DOS/extended-timestamp fields.
function createEntryMeta(filename, options) {
  validateString(filename, 'filename');
  const name = Buffer.from(filename, 'utf8');
  if (name.length === 0) {
    throw new ERR_INVALID_ARG_VALUE('filename', filename, 'must not be empty');
  }
  if (name.length > SENTINEL16) {
    throw new ERR_ZIP_ENTRY_TOO_LARGE(
      'the entry name must not exceed 65535 bytes when encoded as UTF-8');
  }
  let comment = EMPTY_BUFFER;
  if (options?.comment !== undefined) {
    validateString(options.comment, 'options.comment');
    comment = Buffer.from(options.comment, 'utf8');
    if (comment.length > SENTINEL16) {
      throw new ERR_ZIP_ENTRY_TOO_LARGE(
        'the entry comment must not exceed 65535 bytes when encoded as UTF-8');
    }
  }
  const isSymlink = options?.symlink === true;
  const isDirectory = !isSymlink && StringPrototypeEndsWith(filename, '/');
  const mode = options?.mode ?? (isSymlink ? 0o777 : isDirectory ? 0o755 : 0o644);
  validateUint32(mode, 'options.mode');
  // Default to the current time at the DOS fields' 2-second resolution, so a
  // default entry needs no extended-timestamp extra field (see below).
  const modified = options?.modified ?? new Date(MathFloor(DateNow() / 2000) * 2000);
  if (!isDate(modified)) {
    throw new ERR_INVALID_ARG_TYPE('options.modified', 'Date', modified);
  }
  if (options?.method !== undefined &&
      options.method !== 'deflate' && options.method !== 'store' && options.method !== 'zstd') {
    throw new ERR_INVALID_ARG_VALUE(
      'options.method', options.method, "must be 'deflate', 'store', or 'zstd'");
  }
  const typeBits = isSymlink ? S_IFLNK : isDirectory ? S_IFDIR : S_IFREG;
  const unixAttrs = (typeBits | (mode & 0o7777)) & SENTINEL16;
  const external = ((unixAttrs << 16) | (isDirectory ? 0x10 : 0)) >>> 0;
  // Record the whole (UTC) second in an extended-timestamp extra field when
  // the DOS fields cannot represent the time exactly; see
  // extendedMtimeSeconds().
  const extendedMtime = extendedMtimeSeconds(modified.getTime());
  return {
    name,
    comment,
    extra: EMPTY_BUFFER,
    flags: FLAG_UTF8,
    method: 0,
    crc: 0,
    compressedSize: 0,
    uncompressedSize: 0,
    modified,
    extendedMtime,
    external,
    internal: 0,
    madeBy: MADE_BY_UNIX,
    pending: true,
  };
}

// Release one in-flight read on a shared ZipFile descriptor handle. When the
// last read settles, wake a close() that is waiting to release the fd (see
// ZipFile.close() in file.js), so a read never lands on a closed/reused fd.
function endHandleRead(handle) {
  if (--handle.reads === 0 && handle.drain !== null) {
    const drain = handle.drain;
    handle.drain = null;
    drain();
  }
}

