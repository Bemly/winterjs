//! `node:internal/zip/content-size`（Node lib/internal/zip/content-size.js 逐字内嵌，MIT）。
pub const SOURCE: &str = r#"// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/zip/content-size.js.
import errors from 'node:internal/errors';
const {
  codes: {
    ERR_OUT_OF_RANGE: { HideStackFramesError: ERR_OUT_OF_RANGE },
  },
} = errors;

function validateInteger(value, name, min = -9007199254740991, max = 9007199254740991) {
  if (typeof value !== 'number') throw new errors.codes.ERR_INVALID_ARG_TYPE(name, 'number', value);
  if (!Number.isInteger(value)) throw new ERR_OUT_OF_RANGE(name, 'an integer', value);
  if (value < min || value > max) {
    throw new ERR_OUT_OF_RANGE(name, `>= ${min} && <= ${max}`, value);
  }
}

const DEFAULT_MAX_ZIP_CONTENT_SIZE = 256 * 1024 * 1024;
let maxZipContentSize = DEFAULT_MAX_ZIP_CONTENT_SIZE;

function getMaxZipContentSize() {
  return maxZipContentSize;
}

function setMaxZipContentSize(size) {
  validateInteger(size, 'size', 0);
  maxZipContentSize = size;
}

export default {
  DEFAULT_MAX_ZIP_CONTENT_SIZE,
  getMaxZipContentSize,
  setMaxZipContentSize,
};
export { DEFAULT_MAX_ZIP_CONTENT_SIZE, getMaxZipContentSize, setMaxZipContentSize };
"#;
