//! Buffer 定长整数/浮点读写（prelude 分域；拼接顺序见 mod.rs）。
pub const BUFFER_INT_JS: &str = r#"
// ---- 定长整数/浮点读写（lib/internal/buffer.js 逐字；错误消息真机口径）----
function __wjs_bufCheckBounds(buf, offset, byteLength) {
  __wjs_bufValidateNumber(offset, 'offset');
  if (buf[offset] === undefined || buf[offset + byteLength] === undefined)
    __wjs_bufBoundsError(offset, buf.length - (byteLength + 1));
}
function __wjs_bufCheckInt(value, min, max, buf, offset, byteLength) {
  if (value > max || value < min) {
    const n = typeof min === 'bigint' ? 'n' : '';
    let range;
    if (byteLength > 3) {
      if (min === 0 || min === 0n) {
        range = `>= 0${n} and < 2${n} ** ${(byteLength + 1) * 8}${n}`;
      } else {
        range = `>= -(2${n} ** ${(byteLength + 1) * 8 - 1}${n}) and < 2${n} ** ${(byteLength + 1) * 8 - 1}${n}`;
      }
    } else {
      range = `>= ${min}${n} and <= ${max}${n}`;
    }
    throw __wjs_bufRangeErr('value', range, value);
  }
  __wjs_bufCheckBounds(buf, offset, byteLength);
}
function __wjs_bufBoundsError(value, length, type) {
  if (Math.floor(value) !== value) {
    __wjs_bufValidateNumber(value, type);
    throw __wjs_bufRangeErr(type || 'offset', 'an integer', value);
  }
  if (length < 0)
    throw __wjs_bufOobErr();
  throw __wjs_bufRangeErr(type || 'offset', `>= ${type ? 1 : 0} and <= ${length}`, value);
}
function __wjs_bufReadBigUInt64LE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined)
    __wjs_bufBoundsError(offset, this.length - 8);
  const lo = first + this[++offset] * 2 ** 8 + this[++offset] * 2 ** 16 + this[++offset] * 2 ** 24;
  const hi = this[++offset] + this[++offset] * 2 ** 8 + this[++offset] * 2 ** 16 + last * 2 ** 24;
  return BigInt(lo) + (BigInt(hi) << 32n);
}
function __wjs_bufReadBigUInt64BE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined)
    __wjs_bufBoundsError(offset, this.length - 8);
  const hi = first * 2 ** 24 + this[++offset] * 2 ** 16 + this[++offset] * 2 ** 8 + this[++offset];
  const lo = this[++offset] * 2 ** 24 + this[++offset] * 2 ** 16 + this[++offset] * 2 ** 8 + last;
  return (BigInt(hi) << 32n) + BigInt(lo);
}
function __wjs_bufReadBigInt64LE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined)
    __wjs_bufBoundsError(offset, this.length - 8);
  const val = this[offset + 4] +
    this[offset + 5] * 2 ** 8 +
    this[offset + 6] * 2 ** 16 +
    (last << 24); // Overflow
  return (BigInt(val) << 32n) +
    BigInt(first +
    this[++offset] * 2 ** 8 +
    this[++offset] * 2 ** 16 +
    this[++offset] * 2 ** 24);
}
function __wjs_bufReadBigInt64BE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined)
    __wjs_bufBoundsError(offset, this.length - 8);
  const val = (first << 24) + // Overflow
    this[++offset] * 2 ** 16 +
    this[++offset] * 2 ** 8 +
    this[++offset];
  return (BigInt(val) << 32n) +
    BigInt(this[++offset] * 2 ** 24 +
    this[++offset] * 2 ** 16 +
    this[++offset] * 2 ** 8 +
    last);
}
function __wjs_bufReadUIntLE(offset, byteLength) {
  if (offset === undefined) throw __wjs_bufArgTypeErr('offset', 'number', offset);
  if (byteLength === 6) return __wjs_bufReadUInt48LE(this, offset);
  if (byteLength === 5) return __wjs_bufReadUInt40LE(this, offset);
  if (byteLength === 3) return __wjs_bufReadUInt24LE(this, offset);
  if (byteLength === 4) return __wjs_bufReadUInt32LE(this, offset);
  if (byteLength === 2) return __wjs_bufReadUInt16LE(this, offset);
  if (byteLength === 1) return __wjs_bufReadUInt8(this, offset);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufReadUInt48LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 5];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 6);
  return first + buf[++offset] * 2 ** 8 + buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 24 +
    (buf[++offset] + last * 2 ** 8) * 2 ** 32;
}
function __wjs_bufReadUInt40LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 4];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 5);
  return first + buf[++offset] * 2 ** 8 + buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 24 + last * 2 ** 32;
}
function __wjs_bufReadUInt32LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 4);
  return first + buf[++offset] * 2 ** 8 + buf[++offset] * 2 ** 16 + last * 2 ** 24;
}
function __wjs_bufReadUInt24LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 2];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 3);
  return first + buf[++offset] * 2 ** 8 + last * 2 ** 16;
}
function __wjs_bufReadUInt16LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 1];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 2);
  return first + last * 2 ** 8;
}
function __wjs_bufReadUInt8(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const val = buf[offset];
  if (val === undefined) __wjs_bufBoundsError(offset, buf.length - 1);
  return val;
}
function __wjs_bufReadUIntBE(offset, byteLength) {
  if (offset === undefined) throw __wjs_bufArgTypeErr('offset', 'number', offset);
  if (byteLength === 6) return __wjs_bufReadUInt48BE(this, offset);
  if (byteLength === 5) return __wjs_bufReadUInt40BE(this, offset);
  if (byteLength === 3) return __wjs_bufReadUInt24BE(this, offset);
  if (byteLength === 4) return __wjs_bufReadUInt32BE(this, offset);
  if (byteLength === 2) return __wjs_bufReadUInt16BE(this, offset);
  if (byteLength === 1) return __wjs_bufReadUInt8(this, offset);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufReadUInt48BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 5];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 6);
  return (first * 2 ** 8 + buf[++offset]) * 2 ** 32 + buf[++offset] * 2 ** 24 +
    buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadUInt40BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 4];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 5);
  return first * 2 ** 32 + buf[++offset] * 2 ** 24 + buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadUInt32BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 4);
  return first * 2 ** 24 + buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadUInt24BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 2];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 3);
  return first * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadUInt16BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 1];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 2);
  return first * 2 ** 8 + last;
}
function __wjs_bufReadIntLE(offset, byteLength) {
  if (offset === undefined) throw __wjs_bufArgTypeErr('offset', 'number', offset);
  if (byteLength === 6) return __wjs_bufReadInt48LE(this, offset);
  if (byteLength === 5) return __wjs_bufReadInt40LE(this, offset);
  if (byteLength === 3) return __wjs_bufReadInt24LE(this, offset);
  if (byteLength === 4) return __wjs_bufReadInt32LE(this, offset);
  if (byteLength === 2) return __wjs_bufReadInt16LE(this, offset);
  if (byteLength === 1) return __wjs_bufReadInt8(this, offset);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufReadInt48LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 5];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 6);
  const val = buf[offset + 4] + last * 2 ** 8;
  return (val | (val & 2 ** 15) * 0x1fffe) * 2 ** 32 + first + buf[++offset] * 2 ** 8 +
    buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 24;
}
function __wjs_bufReadInt40LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 4];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 5);
  return (last | (last & 2 ** 7) * 0x1fffffe) * 2 ** 32 + first + buf[++offset] * 2 ** 8 +
    buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 24;
}
function __wjs_bufReadInt32LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 4);
  return first + buf[++offset] * 2 ** 8 + buf[++offset] * 2 ** 16 + (last << 24);
}
function __wjs_bufReadInt24LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 2];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 3);
  const val = first + buf[++offset] * 2 ** 8 + last * 2 ** 16;
  return val | (val & 2 ** 23) * 0x1fe;
}
function __wjs_bufReadInt16LE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 1];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 2);
  const val = first + last * 2 ** 8;
  return val | (val & 2 ** 15) * 0x1fffe;
}
function __wjs_bufReadInt8(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const val = buf[offset];
  if (val === undefined) __wjs_bufBoundsError(offset, buf.length - 1);
  return val | (val & 2 ** 7) * 0x1fffffe;
}
function __wjs_bufReadIntBE(offset, byteLength) {
  if (offset === undefined) throw __wjs_bufArgTypeErr('offset', 'number', offset);
  if (byteLength === 6) return __wjs_bufReadInt48BE(this, offset);
  if (byteLength === 5) return __wjs_bufReadInt40BE(this, offset);
  if (byteLength === 3) return __wjs_bufReadInt24BE(this, offset);
  if (byteLength === 4) return __wjs_bufReadInt32BE(this, offset);
  if (byteLength === 2) return __wjs_bufReadInt16BE(this, offset);
  if (byteLength === 1) return __wjs_bufReadInt8(this, offset);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufReadInt48BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 5];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 6);
  const val = buf[++offset] + first * 2 ** 8;
  return (val | (val & 2 ** 15) * 0x1fffe) * 2 ** 32 + buf[++offset] * 2 ** 24 +
    buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadInt40BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 4];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 5);
  return (first | (first & 2 ** 7) * 0x1fffffe) * 2 ** 32 + buf[++offset] * 2 ** 24 +
    buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadInt32BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 4);
  return (first << 24) + buf[++offset] * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
}
function __wjs_bufReadInt24BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 2];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 3);
  const val = first * 2 ** 16 + buf[++offset] * 2 ** 8 + last;
  return val | (val & 2 ** 23) * 0x1fe;
}
function __wjs_bufReadInt16BE(buf, offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = buf[offset];
  const last = buf[offset + 1];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, buf.length - 2);
  const val = first * 2 ** 8 + last;
  return val | (val & 2 ** 15) * 0x1fffe;
}
// 浮点（float32Array 转换板，原文同款）
const __wjs_bufF32 = new Float32Array(1);
const __wjs_bufU8F32 = new Uint8Array(__wjs_bufF32.buffer);
const __wjs_bufF64 = new Float64Array(1);
const __wjs_bufU8F64 = new Uint8Array(__wjs_bufF64.buffer);
__wjs_bufF32[0] = -1;
function __wjs_bufReadFloatLE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, this.length - 4);
  __wjs_bufU8F32[0] = first;
  __wjs_bufU8F32[1] = this[++offset];
  __wjs_bufU8F32[2] = this[++offset];
  __wjs_bufU8F32[3] = last;
  return __wjs_bufF32[0];
}
function __wjs_bufReadFloatBE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 3];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, this.length - 4);
  __wjs_bufU8F32[3] = first;
  __wjs_bufU8F32[2] = this[++offset];
  __wjs_bufU8F32[1] = this[++offset];
  __wjs_bufU8F32[0] = last;
  return __wjs_bufF32[0];
}
function __wjs_bufReadDoubleLE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, this.length - 8);
  __wjs_bufU8F64[0] = first;
  __wjs_bufU8F64[1] = this[++offset];
  __wjs_bufU8F64[2] = this[++offset];
  __wjs_bufU8F64[3] = this[++offset];
  __wjs_bufU8F64[4] = this[++offset];
  __wjs_bufU8F64[5] = this[++offset];
  __wjs_bufU8F64[6] = this[++offset];
  __wjs_bufU8F64[7] = last;
  return __wjs_bufF64[0];
}
function __wjs_bufReadDoubleBE(offset = 0) {
  __wjs_bufValidateNumber(offset, 'offset');
  const first = this[offset];
  const last = this[offset + 7];
  if (first === undefined || last === undefined) __wjs_bufBoundsError(offset, this.length - 8);
  __wjs_bufU8F64[7] = first;
  __wjs_bufU8F64[6] = this[++offset];
  __wjs_bufU8F64[5] = this[++offset];
  __wjs_bufU8F64[4] = this[++offset];
  __wjs_bufU8F64[3] = this[++offset];
  __wjs_bufU8F64[2] = this[++offset];
  __wjs_bufU8F64[1] = this[++offset];
  __wjs_bufU8F64[0] = last;
  return __wjs_bufF64[0];
}
function __wjs_bufWriteBigU64LE(buf, value, offset, min, max) {
  __wjs_bufCheckInt(value, min, max, buf, offset, 7);
  let lo = Number(value & 0xffffffffn);
  buf[offset++] = lo;
  lo = lo >> 8;
  buf[offset++] = lo;
  lo = lo >> 8;
  buf[offset++] = lo;
  lo = lo >> 8;
  buf[offset++] = lo;
  let hi = Number(value >> 32n & 0xffffffffn);
  buf[offset++] = hi;
  hi = hi >> 8;
  buf[offset++] = hi;
  hi = hi >> 8;
  buf[offset++] = hi;
  hi = hi >> 8;
  buf[offset++] = hi;
  return offset;
}
function __wjs_bufWriteBigU64BE(buf, value, offset, min, max) {
  __wjs_bufCheckInt(value, min, max, buf, offset, 7);
  let lo = Number(value & 0xffffffffn);
  buf[offset + 7] = lo;
  lo = lo >> 8;
  buf[offset + 6] = lo;
  lo = lo >> 8;
  buf[offset + 5] = lo;
  lo = lo >> 8;
  buf[offset + 4] = lo;
  let hi = Number(value >> 32n & 0xffffffffn);
  buf[offset + 3] = hi;
  hi = hi >> 8;
  buf[offset + 2] = hi;
  hi = hi >> 8;
  buf[offset + 1] = hi;
  hi = hi >> 8;
  buf[offset] = hi;
  return offset + 8;
}
function __wjs_bufWriteUIntLE(value, offset, byteLength) {
  if (byteLength === 6) return __wjs_bufWriteU48LE(this, value, offset, 0, 0xffffffffffff);
  if (byteLength === 5) return __wjs_bufWriteU40LE(this, value, offset, 0, 0xffffffffff);
  if (byteLength === 3) return __wjs_bufWriteU24LE(this, value, offset, 0, 0xffffff);
  if (byteLength === 4) return __wjs_bufWriteU32LE(this, value, offset, 0, 0xffffffff);
  if (byteLength === 2) return __wjs_bufWriteU16LE(this, value, offset, 0, 0xffff);
  if (byteLength === 1) return __wjs_bufWriteU8(this, value, offset, 0, 0xff);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufWriteU48LE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 5);
  const newVal = Math.floor(value * 2 ** -32);
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  buf[offset++] = newVal;
  buf[offset++] = (newVal >>> 8);
  return offset;
}
function __wjs_bufWriteU40LE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 4);
  const newVal = value;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  buf[offset++] = Math.floor(newVal * 2 ** -32);
  return offset;
}
function __wjs_bufWriteU32LE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 3);
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  return offset;
}
function __wjs_bufWriteU24LE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 2);
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  value = value >>> 8;
  buf[offset++] = value;
  return offset;
}
function __wjs_bufWriteU16LE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 1);
  buf[offset++] = value;
  buf[offset++] = (value >>> 8);
  return offset;
}
function __wjs_bufWriteU8(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufValidateNumber(offset, 'offset');
  if (value > max || value < min) {
    throw __wjs_bufRangeErr('value', `>= ${min} and <= ${max}`, value);
  }
  if (buf[offset] === undefined) __wjs_bufBoundsError(offset, buf.length - 1);
  buf[offset] = value;
  return offset + 1;
}
function __wjs_bufWriteUIntBE(value, offset, byteLength) {
  if (byteLength === 6) return __wjs_bufWriteU48BE(this, value, offset, 0, 0xffffffffffff);
  if (byteLength === 5) return __wjs_bufWriteU40BE(this, value, offset, 0, 0xffffffffff);
  if (byteLength === 3) return __wjs_bufWriteU24BE(this, value, offset, 0, 0xffffff);
  if (byteLength === 4) return __wjs_bufWriteU32BE(this, value, offset, 0, 0xffffffff);
  if (byteLength === 2) return __wjs_bufWriteU16BE(this, value, offset, 0, 0xffff);
  if (byteLength === 1) return __wjs_bufWriteU8(this, value, offset, 0, 0xff);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufWriteU48BE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 5);
  const newVal = Math.floor(value * 2 ** -32);
  buf[offset++] = (newVal >>> 8);
  buf[offset++] = newVal;
  buf[offset + 3] = value;
  value = value >>> 8;
  buf[offset + 2] = value;
  value = value >>> 8;
  buf[offset + 1] = value;
  value = value >>> 8;
  buf[offset] = value;
  return offset + 4;
}
function __wjs_bufWriteU40BE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 4);
  buf[offset++] = Math.floor(value * 2 ** -32);
  buf[offset + 3] = value;
  value = value >>> 8;
  buf[offset + 2] = value;
  value = value >>> 8;
  buf[offset + 1] = value;
  value = value >>> 8;
  buf[offset] = value;
  return offset + 4;
}
function __wjs_bufWriteU32BE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 3);
  buf[offset + 3] = value;
  value = value >>> 8;
  buf[offset + 2] = value;
  value = value >>> 8;
  buf[offset + 1] = value;
  value = value >>> 8;
  buf[offset] = value;
  return offset + 4;
}
function __wjs_bufWriteU24BE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 2);
  buf[offset + 2] = value;
  value = value >>> 8;
  buf[offset + 1] = value;
  value = value >>> 8;
  buf[offset] = value;
  return offset + 3;
}
function __wjs_bufWriteU16BE(buf, value, offset, min, max) {
  value = +value;
  __wjs_bufCheckInt(value, min, max, buf, offset, 1);
  buf[offset++] = (value >>> 8);
  buf[offset++] = value;
  return offset;
}
function __wjs_bufWriteFloatLE(val, offset = 0) {
  val = +val;
  __wjs_bufCheckBounds(this, offset, 3);
  __wjs_bufF32[0] = val;
  this[offset++] = __wjs_bufU8F32[0];
  this[offset++] = __wjs_bufU8F32[1];
  this[offset++] = __wjs_bufU8F32[2];
  this[offset++] = __wjs_bufU8F32[3];
  return offset;
}
function __wjs_bufWriteFloatBE(val, offset = 0) {
  val = +val;
  __wjs_bufCheckBounds(this, offset, 3);
  __wjs_bufF32[0] = val;
  this[offset++] = __wjs_bufU8F32[3];
  this[offset++] = __wjs_bufU8F32[2];
  this[offset++] = __wjs_bufU8F32[1];
  this[offset++] = __wjs_bufU8F32[0];
  return offset;
}
function __wjs_bufWriteDoubleLE(val, offset = 0) {
  val = +val;
  __wjs_bufCheckBounds(this, offset, 7);
  __wjs_bufF64[0] = val;
  this[offset++] = __wjs_bufU8F64[0];
  this[offset++] = __wjs_bufU8F64[1];
  this[offset++] = __wjs_bufU8F64[2];
  this[offset++] = __wjs_bufU8F64[3];
  this[offset++] = __wjs_bufU8F64[4];
  this[offset++] = __wjs_bufU8F64[5];
  this[offset++] = __wjs_bufU8F64[6];
  this[offset++] = __wjs_bufU8F64[7];
  return offset;
}
function __wjs_bufWriteDoubleBE(val, offset = 0) {
  val = +val;
  __wjs_bufCheckBounds(this, offset, 7);
  __wjs_bufF64[0] = val;
  this[offset++] = __wjs_bufU8F64[7];
  this[offset++] = __wjs_bufU8F64[6];
  this[offset++] = __wjs_bufU8F64[5];
  this[offset++] = __wjs_bufU8F64[4];
  this[offset++] = __wjs_bufU8F64[3];
  this[offset++] = __wjs_bufU8F64[2];
  this[offset++] = __wjs_bufU8F64[1];
  this[offset++] = __wjs_bufU8F64[0];
  return offset;
}
// write 静态包装（internal/buffer.js utf8Write 等的越界校验口径）
function __wjs_bufUtf8Write(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufUtf8WriteStatic(buf, string, offset, length);
}
function __wjs_bufAsciiWrite(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufAsciiWriteStatic(buf, string, offset, length);
}
function __wjs_bufLatin1Write(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufLatin1WriteStatic(buf, string, offset, length);
}
function __wjs_bufUcs2Write(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufUcs2WriteStatic(buf, string, offset, length);
}
function __wjs_bufHexWrite(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufHexWriteStatic(buf, string, offset, length);
}
function __wjs_bufBase64Write(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufB64WriteStatic(buf, string, offset, length, false);
}
function __wjs_bufBase64urlWrite(buf, string, offset = 0, length) {
  offset = Number(offset);
  if (offset < 0 || offset > buf.byteLength) throw __wjs_bufOobErr('offset');
  if (length === undefined) length = buf.byteLength - offset;
  else length = Number(length);
  if (length < 0 || length > buf.byteLength - offset) throw __wjs_bufOobErr('length');
  return __wjs_bufB64WriteStatic(buf, string, offset, length, true);
}
// slice 静态（utf8 用 TextDecoder；ascii 掩 0x7F，10f 真机口径）
function __wjs_bufUtf8Slice(u8, start, end) {
  // 预检先于 decode（utf8 串长 ≤ 字节数；>1GB decode 会撞 SM 崩溃面，
  // 不给引擎机会）——node kStringMaxLength 字节等同口径的保守近似
  if (end - start > kStringMaxLength) {
    const e = new Error(`Cannot create a string longer than ${kStringMaxLength} characters`);
    e.code = 'ERR_STRING_TOO_LONG';
    throw e;
  }
  const s = new TextDecoder().decode(u8.subarray(start, end));
  if (s.length > kStringMaxLength) {
    // node 引擎级字串上限在 SM 更高，按 node 口径在 slice 层模拟
    // （E('ERR_STRING_TOO_LONG', 'Cannot create a string longer than %s characters', Error)）
    const e = new Error(`Cannot create a string longer than ${kStringMaxLength} characters`);
    e.code = 'ERR_STRING_TOO_LONG';
    throw e;
  }
  return s;
}
function __wjs_bufAsciiSlice(u8, start, end) {
  let s = '';
  for (let i = start; i < end; i++) s += String.fromCharCode(u8[i] & 0x7F);
  return s;
}
function __wjs_bufLatin1Slice(u8, start, end) {
  let s = '';
  for (let i = start; i < end; i++) s += String.fromCharCode(u8[i]);
  return s;
}
function __wjs_bufUcs2Slice(u8, start, end) {
  let s = '';
  for (let i = start; i + 1 < end; i += 2) s += String.fromCharCode(u8[i] | (u8[i + 1] << 8));
  return s;
}
function __wjs_bufHexSlice(u8, start, end) {
  let s = '';
  for (let i = start; i < end; i++) s += u8[i].toString(16).padStart(2, '0');
  return s;
}
// indexOf 族（C++ SearchString 的 JS 重实现）
function __wjs_bufIndexOfNumber(buf, val, byteOffset, dir, end) {
  val = val >>> 0;
  val = val & 0xFF;
  const len = buf.length;
  const limit = Math.min(end === undefined ? len : end, len);
  if (dir) {
    // 负偏移 = 距尾偏移（node SearchString 口径；越界收敛到 0）
    if (byteOffset < 0) byteOffset = Math.max(len + byteOffset, 0);
    if (byteOffset >= limit) return -1;
    for (let i = byteOffset; i < limit; i++) {
      if (buf[i] === val) return i;
    }
    return -1;
  }
  if (byteOffset < 0) {
    byteOffset = len + byteOffset;
    if (byteOffset < 0) return -1;
  }
  let i = Math.min(byteOffset, limit - 1);
  for (; i >= 0; i--) {
    if (buf[i] === val) return i;
  }
  return -1;
}
function __wjs_bufIndexOfBytes(buf, needle, byteOffset, dir, end, align = 1) {
  const len = buf.length;
  const nlen = needle.length;
  if (nlen === 0) {
    // 空 needle 钳到搜索上限 end（套件 "clamp to search_end" 门）
    const lim0 = Math.min(end === undefined ? len : end, len);
    return Math.min(Math.max(byteOffset, 0), lim0);
  }
  const limit = Math.min(end === undefined ? len : end, len);
  if (dir) {
    let i = byteOffset < 0 ? Math.max(len + byteOffset, 0) : Math.max(byteOffset, 0);
    // utf16le 对齐搜索（node C++ 口径：只在偶字节偏移命中，套件 allChars 门）
    if (align === 2 && i % 2 !== 0) i++;
    for (; i <= limit - nlen; i += align) {
      let ok = true;
      for (let j = 0; j < nlen; j++) {
        if (buf[i + j] !== needle[j]) { ok = false; break; }
      }
      if (ok) return i;
    }
    return -1;
  }
  if (byteOffset < 0) {
    byteOffset = len + byteOffset;
    if (byteOffset < 0) return -1;
  }
  let i = Math.min(byteOffset, limit - nlen);
  if (align === 2 && i % 2 !== 0) i--;
  for (; i >= 0; i -= align) {
    let ok = true;
    for (let j = 0; j < nlen; j++) {
      if (buf[i + j] !== needle[j]) { ok = false; break; }
    }
    if (ok) return i;
  }
  return -1;
}
function __wjs_bufIndexOfString(buf, val, byteOffset, enc, dir, end) {
  const ops = __wjs_bufEncodingOps[enc];
  return __wjs_bufIndexOfBytes(buf, __wjs_bufEncodeStr(val, ops), byteOffset, dir, end,
                               ops.encoding === 'utf16le' ? 2 : 1);
}
function __wjs_bufEncodeStr(str, ops) {
  const tmp = new __wjs_bufFastBuffer(ops.byteLength(str) || 1);
  const actual = ops.write(tmp, str, 0, tmp.length);
  return tmp.subarray(0, actual);
}
"#;
