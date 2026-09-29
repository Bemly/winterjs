//! `node:internal/zip/compression`（Node lib/internal/zip/compression.js 逐字内嵌，MIT）。
pub const SOURCE: &str = r#"// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/zip/compression.js.
import errors from 'node:internal/errors';
import bufferMod from 'node:buffer';
import constants from 'node:internal/zip/constants';
import zlibMod from 'node:zlib';
import streamMod from 'node:stream';
const {
  codes: {
    ERR_ZIP_ENTRY_CORRUPT: { HideStackFramesError: ERR_ZIP_ENTRY_CORRUPT },
    ERR_ZIP_ENTRY_TOO_LARGE: { HideStackFramesError: ERR_ZIP_ENTRY_TOO_LARGE },
    ERR_ZIP_UNSUPPORTED_FEATURE: { HideStackFramesError: ERR_ZIP_UNSUPPORTED_FEATURE },
  },
} = errors;
const { kMaxLength } = bufferMod;
const { compose } = streamMod;
const { FLAG_ENCRYPTED, METHOD_STORE, METHOD_DEFLATE, METHOD_ZSTD } = constants;

let zlib;
function lazyZlib() {
  zlib ??= zlibMod.default ?? zlibMod;
  return zlib;
}

function deflateRawAsync(buffer) {
  return new Promise((resolve, reject) => {
    lazyZlib().deflateRaw(buffer, (err, result) => {
      if (err) reject(err);
      else resolve(result);
    });
  });
}

function inflateRawAsync(buffer, options) {
  return new Promise((resolve, reject) => {
    lazyZlib().inflateRaw(buffer, options, (err, result) => {
      if (err) reject(err);
      else resolve(result);
    });
  });
}

function pumpThroughTransform(source, transform) {
  return compose(source, transform);
}

function deflateRawStream(source) {
  return pumpThroughTransform(source, lazyZlib().createDeflateRaw());
}

function inflateRawStream(source) {
  return pumpThroughTransform(source, lazyZlib().createInflateRaw());
}

function zstdCompressAsync(buffer) {
  return new Promise((resolve, reject) => {
    lazyZlib().zstdCompress(buffer, (err, result) => {
      if (err) reject(err);
      else resolve(result);
    });
  });
}

function zstdDecompressAsync(buffer, options) {
  return new Promise((resolve, reject) => {
    lazyZlib().zstdDecompress(buffer, options, (err, result) => {
      if (err) reject(err);
      else resolve(result);
    });
  });
}

function zstdCompressStream(source) {
  return pumpThroughTransform(source, lazyZlib().createZstdCompress());
}

function zstdDecompressStream(source) {
  return pumpThroughTransform(source, lazyZlib().createZstdDecompress());
}

function deflateRawSync(buffer) {
  return lazyZlib().deflateRawSync(buffer);
}

function zstdCompressSync(buffer) {
  return lazyZlib().zstdCompressSync(buffer);
}

function assertDecodable(info, options) {
  if (info.flags & FLAG_ENCRYPTED) {
    throw new ERR_ZIP_UNSUPPORTED_FEATURE(
      `entry ${JSON.stringify(info.name)} is encrypted`);
  }
  if (info.method !== METHOD_STORE && info.method !== METHOD_DEFLATE && info.method !== METHOD_ZSTD) {
    throw new ERR_ZIP_UNSUPPORTED_FEATURE(
      `entry ${JSON.stringify(info.name)} uses compression method ${info.method}`);
  }
  if (options?.maxSize !== undefined && info.uncompressedSize > options.maxSize) {
    throw new ERR_ZIP_ENTRY_TOO_LARGE(
      `entry ${JSON.stringify(info.name)} declares ${info.uncompressedSize} bytes, ` +
      `exceeding the ${options.maxSize} byte limit`);
  }
}

function outputCap(info, options) {
  return Math.min(
    info.uncompressedSize + 1, options?.maxSize ?? kMaxLength, kMaxLength);
}

function rethrowDecodeFailure(err, info, method) {
  if (err?.code === 'ERR_BUFFER_TOO_LARGE') {
    throw new ERR_ZIP_ENTRY_CORRUPT(
      `entry ${JSON.stringify(info.name)} ` +
      `${method === METHOD_DEFLATE ? 'inflates' : 'decompresses'} beyond its ` +
      `declared size of ${info.uncompressedSize} bytes`);
  }
  throw new ERR_ZIP_ENTRY_CORRUPT(
    `entry ${JSON.stringify(info.name)} failed to ` +
    `${method === METHOD_DEFLATE ? 'inflate' : 'decompress'}: ${err.message}`);
}

function __zipCrc32Native(data, seed) {
  if (typeof __wjs2_zlib_crc32 === 'function') {
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    return __wjs2_zlib_crc32(bytes, seed >>> 0);
  }
  return lazyZlib().crc32(data, seed);
}

function checkDecoded(data, info, verify) {
  if (data.length !== info.uncompressedSize) {
    throw new ERR_ZIP_ENTRY_CORRUPT(
      `entry ${JSON.stringify(info.name)} produced ${data.length} bytes, expected ` +
      `${info.uncompressedSize}`);
  }
  if (verify && __zipCrc32Native(data, 0) !== info.crc32) {
    throw new ERR_ZIP_ENTRY_CORRUPT(
      `entry ${JSON.stringify(info.name)} failed CRC-32 verification`);
  }
}

async function* decodeMemberStream(source, info, options) {
  assertDecodable(info, options);
  const verify = options?.verify !== false;
  let produced = 0;
  let state = 0;
  const decoded = info.method === METHOD_DEFLATE ? inflateRawStream(source) :
    info.method === METHOD_ZSTD ? zstdDecompressStream(source) : source;
  for await (const chunk of decoded) {
    produced += chunk.length;
    if (produced > info.uncompressedSize) {
      throw new ERR_ZIP_ENTRY_CORRUPT(
        `entry ${JSON.stringify(info.name)} inflates beyond its declared size of ` +
        `${info.uncompressedSize} bytes`);
    }
    if (verify) state = __zipCrc32Native(chunk, state);
    yield chunk;
  }
  if (produced !== info.uncompressedSize) {
    throw new ERR_ZIP_ENTRY_CORRUPT(
      `entry ${JSON.stringify(info.name)} is truncated: got ${produced} of ` +
      `${info.uncompressedSize} bytes`);
  }
  if (verify && state !== info.crc32) {
    throw new ERR_ZIP_ENTRY_CORRUPT(
      `entry ${JSON.stringify(info.name)} failed CRC-32 verification`);
  }
}

async function decodeMemberAsync(compressed, info, options) {
  assertDecodable(info, options);
  const cap = outputCap(info, options);
  let data;
  if (info.method === METHOD_DEFLATE) {
    try {
      data = await inflateRawAsync(compressed, { maxOutputLength: cap });
    } catch (err) {
      rethrowDecodeFailure(err, info, METHOD_DEFLATE);
    }
  } else if (info.method === METHOD_ZSTD) {
    try {
      data = await zstdDecompressAsync(compressed, { maxOutputLength: cap });
    } catch (err) {
      rethrowDecodeFailure(err, info, METHOD_ZSTD);
    }
  } else {
    data = compressed;
  }
  checkDecoded(data, info, options?.verify !== false);
  return data;
}

function decodeMemberSync(compressed, info, options) {
  assertDecodable(info, options);
  const cap = outputCap(info, options);
  let data;
  if (info.method === METHOD_DEFLATE) {
    try {
      data = lazyZlib().inflateRawSync(compressed, { maxOutputLength: cap });
    } catch (err) {
      rethrowDecodeFailure(err, info, METHOD_DEFLATE);
    }
  } else if (info.method === METHOD_ZSTD) {
    try {
      data = lazyZlib().zstdDecompressSync(compressed, { maxOutputLength: cap });
    } catch (err) {
      rethrowDecodeFailure(err, info, METHOD_ZSTD);
    }
  } else {
    data = compressed;
  }
  checkDecoded(data, info, options?.verify !== false);
  return data;
}

export default {
  deflateRawAsync,
  deflateRawSync,
  zstdCompressAsync,
  zstdCompressSync,
  deflateRawStream,
  zstdCompressStream,
  decodeMemberStream,
  decodeMemberAsync,
  decodeMemberSync,
};
export {
  deflateRawAsync,
  deflateRawSync,
  zstdCompressAsync,
  zstdCompressSync,
  deflateRawStream,
  zstdCompressStream,
  decodeMemberStream,
  decodeMemberAsync,
  decodeMemberSync,
};
"#;
