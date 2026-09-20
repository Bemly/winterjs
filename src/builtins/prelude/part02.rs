//! prelude part 02 (byte-exact slice; order matters, see prelude/mod.rs).
pub const PART_02: &str = r#"
function __wjs_bufPoolAlign() {
  if (__wjs_bufPoolOffset & 0x7) __wjs_bufPoolOffset = (__wjs_bufPoolOffset + 7) & ~7;
}
{
  const __wjs_bufOrigTransfer = ArrayBuffer.prototype.transfer;
  Object.defineProperty(ArrayBuffer.prototype, 'transfer', {
    value: function() {
      if (__wjs_bufPooled.has(this)) {
        throw new TypeError('Cannot transfer a pooled Buffer ArrayBuffer');
      }
      return __wjs_bufOrigTransfer.call(this);
    },
    writable: true, configurable: true,
  });
}
function __wjs_bufToInteger(n, defaultVal) {
  n = +n;
  if (!Number.isNaN(n) && n >= Number.MIN_SAFE_INTEGER && n <= Number.MAX_SAFE_INTEGER) {
    return ((n % 1) === 0 ? n : Math.floor(n));
  }
  return defaultVal;
}
function __wjs_bufCopyImpl(source, target, targetStart, sourceStart, sourceEnd) {
  if (!ArrayBuffer.isView(source))
    throw __wjs_bufArgTypeErr('source', ['Buffer', 'Uint8Array'], source);
  if (!ArrayBuffer.isView(target))
    throw __wjs_bufArgTypeErr('target', ['Buffer', 'Uint8Array'], target);
  if (targetStart === undefined) {
    targetStart = 0;
  } else {
    targetStart = Number.isInteger(targetStart) ? targetStart : __wjs_bufToInteger(targetStart, 0);
    if (targetStart < 0) throw __wjs_bufRangeErr('targetStart', '>= 0', targetStart);
  }
  if (sourceStart === undefined) {
    sourceStart = 0;
  } else {
    sourceStart = Number.isInteger(sourceStart) ? sourceStart : __wjs_bufToInteger(sourceStart, 0);
    if (sourceStart < 0 || sourceStart > source.byteLength)
      throw __wjs_bufRangeErr('sourceStart', `>= 0 && <= ${source.byteLength}`, sourceStart);
  }
  if (sourceEnd === undefined) {
    sourceEnd = source.byteLength;
  } else {
    sourceEnd = Number.isInteger(sourceEnd) ? sourceEnd : __wjs_bufToInteger(sourceEnd, 0);
    if (sourceEnd < 0) throw __wjs_bufRangeErr('sourceEnd', '>= 0', sourceEnd);
  }
  if (targetStart >= target.byteLength || sourceStart >= sourceEnd)
    return 0;
  return __wjs_bufCopyActual(source, target, targetStart, sourceStart, sourceEnd);
}
function __wjs_bufCopyActual(source, target, targetStart, sourceStart, sourceEnd) {
  if (sourceEnd - sourceStart > target.byteLength - targetStart)
    sourceEnd = sourceStart + target.byteLength - targetStart;
  let nb = sourceEnd - sourceStart;
  const sourceLen = source.byteLength - sourceStart;
  if (nb > sourceLen) nb = sourceLen;
  if (nb <= 0) return 0;
  // 字节级拷贝（node _copy memmove 口径；目标非 u8 时 targetStart 按元素索引
  // 换算字节偏移，如 Uint16Array——test-buffer-copy "packed into 16-bit" 门）
  const elemSize = target.length ? (target.byteLength / target.length) : 1;
  const byteStart = targetStart * elemSize;
  const room = target.byteLength - byteStart;
  if (nb > room) nb = room;
  if (nb <= 0) return 0;
  const dst = new Uint8Array(target.buffer, target.byteOffset + byteStart, nb);
  const src = new Uint8Array(source.buffer, source.byteOffset + sourceStart, nb);
  dst.set(src); // 同 buffer 时 set 按 memmove 语义
  return nb;
}
Buffer.from = function from(value, encodingOrOffset, length) {
  if (typeof value === 'string')
    return __wjs_bufFromString(value, encodingOrOffset);
  if (typeof value === 'object' && value !== null) {
    if (__wjs_bufIsAnyAB(value)) {
      // 10f：伪 AB（原型链伪造、无内部槽，如 `Object.setPrototypeOf(AB, ArrayBuffer)`）
      // 须拒为 ERR_INVALID_ARG_TYPE（V8 IsArrayBuffer 品牌检查口径）；直接进
      // FromArrayBuffer 会在引擎内抛 incompatible 文案，断言对不上。
      let branded = true;
      try { void value.byteLength; } catch { branded = false; }
      if (branded)
        return __wjs_bufFromArrayBuffer(value, encodingOrOffset, length);
      // 落空到尾部统一 invalid-arg（`an instance of AB`，__wjs_bufSpecificType 口径）
    } else {
      const valueOf = value.valueOf && value.valueOf();
      if (valueOf != null && valueOf !== value &&
          (typeof valueOf === 'string' || typeof valueOf === 'object')) {
        return from(valueOf, encodingOrOffset, length);
      }
      const b = __wjs_bufFromObject(value);
      if (b) return b;
      if (typeof value[Symbol.toPrimitive] === 'function') {
        const primitive = value[Symbol.toPrimitive]('string');
        if (typeof primitive === 'string') {
          return __wjs_bufFromString(primitive, encodingOrOffset);
        }
      }
    }
  }
  throw __wjs_bufArgTypeErr(
    'first argument',
    ['string', 'Buffer', 'ArrayBuffer', 'Array', 'Array-like Object'],
    value,
  );
};
Buffer.copyBytesFrom = function copyBytesFrom(view, offset, length) {
  if (!ArrayBuffer.isView(view) || view instanceof DataView) {
    throw __wjs_bufArgTypeErr('view', ['TypedArray'], view);
  }
  const viewLength = view.length;
  if (viewLength === 0) return new __wjs_bufFastBuffer();
  let start = 0;
  let end = viewLength;
  if (offset !== undefined) {
    __wjs_bufValidateInteger(offset, 'offset', 0);
    if (offset >= viewLength) return new __wjs_bufFastBuffer();
    start = offset;
  }
  if (length !== undefined) {
    __wjs_bufValidateInteger(length, 'length', 0);
    end = Math.min(start + length, viewLength);
  }
  if (end <= start) return new __wjs_bufFastBuffer();
  const viewByteLength = view.byteLength;
  const elementSize = viewByteLength / viewLength;
  const srcByteOffset = view.byteOffset + start * elementSize;
  const srcByteLength = (end - start) * elementSize;
  return __wjs_bufFromArrayLike(new Uint8Array(view.buffer, srcByteOffset, srcByteLength));
};
const __wjs_bufOf = (...items) => {
  const len = items.length;
  const newObj = new __wjs_bufFastBuffer(len);
  for (let k = 0; k < len; k++) newObj[k] = items[k];
  return newObj;
};
Buffer.of = __wjs_bufOf;
Buffer.alloc = function alloc(size, fill, encoding) {
  __wjs_bufValidateNumber(size, 'size', 0, kMaxLength);
  if (fill !== undefined && fill !== 0 && size > 0) {
    const buf = new __wjs_bufFastBuffer(size);
    return __wjs_bufFill(buf, fill, 0, buf.length, encoding);
  }
  return new __wjs_bufFastBuffer(size);
};
Buffer.allocUnsafe = function allocUnsafe(size) {
  // alignment 形参不做（无 O_DIRECT 场景；真机 26 有，记档）
  __wjs_bufValidateNumber(size, 'size', 0, kMaxLength);
  return size <= 0 ? new __wjs_bufFastBuffer() : new __wjs_bufFastBuffer(size);
};
Buffer.allocUnsafeSlow = function allocUnsafeSlow(size) {
  __wjs_bufValidateNumber(size, 'size', 0, kMaxLength);
  return size <= 0 ? new __wjs_bufFastBuffer() : new __wjs_bufFastBuffer(size);
};
function __wjs_bufFromStringFast(string, ops) {
  const length = ops.byteLength(string);
  // 池路径（小串共享池 AB；actual 按写入实长推进，与 Node fromStringFast 同口径）
  if (length > 0 && length < (Buffer.poolSize >>> 1)) {
    __wjs_bufPoolAlign();
    if (length > __wjs_bufPoolAB.byteLength - __wjs_bufPoolOffset) {
      __wjs_bufPoolAB = new ArrayBuffer(Buffer.poolSize);
      __wjs_bufPooled.add(__wjs_bufPoolAB);
      __wjs_bufPoolOffset = 0;
    }
    const scratch = new Uint8Array(__wjs_bufPoolAB);
    const actual = ops.write(scratch, string, __wjs_bufPoolOffset, length);
    const b = new __wjs_bufFastBuffer(__wjs_bufPoolAB, __wjs_bufPoolOffset, actual);
    __wjs_bufPoolOffset += actual;
    return b;
  }
  const buf = Buffer.allocUnsafeSlow(length);
  const actual = ops.write(buf, string, 0, length);
  return actual < length ? new __wjs_bufFastBuffer(buf.buffer, 0, actual) : buf;
}
function __wjs_bufFromString(string, encoding) {
  let ops;
  if (!encoding || encoding === 'utf8' || typeof encoding !== 'string') {
    ops = __wjs_bufEncodingOps.utf8;
  } else {
    ops = __wjs_bufGetEncodingOps(encoding);
    if (ops === undefined) throw __wjs_bufEncErr(encoding);
  }
  return string.length === 0 ? new __wjs_bufFastBuffer() : __wjs_bufFromStringFast(string, ops);
}
function __wjs_bufFromArrayBuffer(obj, byteOffset, length) {
  if (byteOffset === undefined) {
    byteOffset = 0;
  } else {
    byteOffset = +byteOffset;
    if (Number.isNaN(byteOffset)) byteOffset = 0;
  }
  const maxLength = obj.byteLength - byteOffset;
  if (maxLength < 0) throw __wjs_bufOobErr('offset');
  if (length !== undefined) {
    length = +length;
    if (length > 0) {
      if (length > maxLength) throw __wjs_bufOobErr('length');
    } else {
      length = 0;
    }
  }
  return new __wjs_bufFastBuffer(obj, byteOffset, length);
}
function __wjs_bufFromArrayLike(obj) {
  const { length } = obj;
  if (length <= 0) return new __wjs_bufFastBuffer();
  return new __wjs_bufFastBuffer(obj);
}
function __wjs_bufFromObject(obj) {
  if (obj.length !== undefined || (obj.buffer != null && __wjs_bufIsAnyAB(obj.buffer))) {
    if (typeof obj.length !== 'number') {
      return new __wjs_bufFastBuffer();
    }
    return __wjs_bufFromArrayLike(obj);
  }
  if (obj.type === 'Buffer' && Array.isArray(obj.data)) {
    return __wjs_bufFromArrayLike(obj.data);
  }
}
Buffer.isBuffer = function isBuffer(b) {
  return b instanceof Buffer;
};
Buffer.compare = function compare(buf1, buf2) {
  if (!__wjs_bufIsU8(buf1)) throw __wjs_bufArgTypeErr('buf1', ['Buffer', 'Uint8Array'], buf1);
  if (!__wjs_bufIsU8(buf2)) throw __wjs_bufArgTypeErr('buf2', ['Buffer', 'Uint8Array'], buf2);
  if (buf1 === buf2) return 0;
  return __wjs_bufCompare(buf1, buf2);
};
function __wjs_bufIsU8(v) { return v instanceof Uint8Array; }
function __wjs_bufCompare(a, b) {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  }
  return a.length === b.length ? 0 : (a.length < b.length ? -1 : 1);
}
Buffer.isEncoding = function isEncoding(encoding) {
  return typeof encoding === 'string' && encoding.length !== 0 &&
         __wjs_bufNormalizeEncoding(encoding) !== undefined;
};
Buffer.concat = function concat(list, length) {
  __wjs_bufValidateArray(list, 'list');
  if (list.length === 0) return new __wjs_bufFastBuffer();
  if (length === undefined) {
    length = 0;
    for (let i = 0; i < list.length; i++) {
      const buf = list[i];
      if (!__wjs_bufIsU8(buf)) {
        throw __wjs_bufArgTypeErr(`list[${i}]`, ['Buffer', 'Uint8Array'], buf);
      }
      length += buf.byteLength;
    }
    const buffer = length <= 0 ? new __wjs_bufFastBuffer() : new __wjs_bufFastBuffer(length);
    let pos = 0;
    for (let i = 0; i < list.length; i++) {
      const buf = list[i];
      buffer.set(buf, pos);
      pos += buf.byteLength;
    }
    return buffer;
  }
  __wjs_bufValidateInteger(length, 'length', 0);
  for (let i = 0; i < list.length; i++) {
    if (!__wjs_bufIsU8(list[i])) {
      throw __wjs_bufArgTypeErr(`list[${i}]`, ['Buffer', 'Uint8Array'], list[i]);
    }
  }
  const buffer = length <= 0 ? new __wjs_bufFastBuffer() : new __wjs_bufFastBuffer(length);
  let pos = 0;
  for (let i = 0; i < list.length; i++) {
    const buf = list[i];
    const bufLength = buf.byteLength;
    if (pos + bufLength > length) {
      buffer.set(buf.subarray(0, length - pos), pos);
      pos = length;
      break;
    }
    buffer.set(buf, pos);
    pos += bufLength;
  }
  if (pos < length) {
    __wjs_bufTAFill.call(buffer, 0, pos, length);
  }
  return buffer;
};
function __wjs_bufByteLengthUtf8(string) { return new TextEncoder().encode(string).length; }
function __wjs_bufByteLength(string, encoding) {
  if (typeof string !== 'string') {
    if (ArrayBuffer.isView(string) || __wjs_bufIsAnyAB(string)) {
      try {
        return string.byteLength;
      } catch {
        return 0; // detached 视空
      }
    }
    throw __wjs_bufArgTypeErr('string', ['string', 'Buffer', 'ArrayBuffer'], string);
  }
  const len = string.length;
  if (len === 0) return 0;
  if (!encoding || encoding === 'utf8') {
    return __wjs_bufByteLengthUtf8(string);
  }
  if (encoding === 'ascii') {
    return len;
  }
  const ops = __wjs_bufGetEncodingOps(encoding);
  if (ops === undefined) {
    return __wjs_bufByteLengthUtf8(string);
  }
  return ops.byteLength(string);
}
Buffer.byteLength = __wjs_bufByteLength;
Buffer.prototype.copy = function copy(target, targetStart, sourceStart, sourceEnd) {
  return __wjs_bufCopyImpl(this, target, targetStart, sourceStart, sourceEnd);
};
Buffer.prototype.toString = function toString(encoding, start, end) {
  if (arguments.length === 0) {
    return __wjs_bufUtf8Slice(this, 0, this.length);
  }
  const bufferLength = this.length;
  if (start <= 0) start = 0;
  else if (start >= bufferLength) return '';
  else start = Math.trunc(start) || 0;
  if (end === undefined || end > bufferLength) end = bufferLength;
  else end = Math.trunc(end) || 0;
  if (end <= start) return '';
  if (encoding === undefined) return __wjs_bufUtf8Slice(this, start, end);
  const ops = __wjs_bufGetEncodingOps(encoding);
  if (ops === undefined) throw __wjs_bufEncErr(encoding);
  return ops.slice(this, start, end);
};
Buffer.prototype.equals = function equals(otherBuffer) {
  if (!__wjs_bufIsU8(otherBuffer)) {
    throw __wjs_bufArgTypeErr('otherBuffer', ['Buffer', 'Uint8Array'], otherBuffer);
  }
  if (this === otherBuffer) return true;
  const len = this.byteLength;
  if (len !== otherBuffer.byteLength) return false;
  return len === 0 || __wjs_bufCompare(this, otherBuffer) === 0;
};
let INSPECT_MAX_BYTES = 50;
const __wjs_bufCustomInspect = Symbol.for('nodejs.util.inspect.custom');
Buffer.prototype[__wjs_bufCustomInspect] = function inspect(recurseTimes, ctx) {
  const max = INSPECT_MAX_BYTES;
  const actualMax = Math.min(max, this.length);
  const remaining = this.length - max;
  let str = __wjs_bufHexSlice(this, 0, actualMax).replace(/(.{2})/g, '$1 ').trim();
  if (remaining > 0) str += ` ... ${remaining} more byte${remaining > 1 ? 's' : ''}`;
  // Inspect special properties as well, if possible（lib/buffer.js extras 段）。
  if (ctx && typeof globalThis.__wjs_inspect === 'function') {
    let extras = false;
    const obj = { };
    Object.keys(this).forEach((key) => {
      if (/^\d+$/.test(key)) return;
      extras = true;
      obj[key] = this[key];
    });
    if (extras) {
      if (this.length !== 0) str += ', ';
      str += Object.keys(obj)
        .map((key) => `${key}: ${globalThis.__wjs_inspect(obj[key], { ...ctx, breakLength: Infinity, compact: true })}`)
        .join(', ');
    }
  }
  let constructorName = 'Buffer';
  try {
    const { constructor } = this;
    if (typeof constructor === 'function' &&
        Object.prototype.hasOwnProperty.call(constructor, 'name')) {
      constructorName = constructor.name;
    }
  } catch { }
  return `<${constructorName} ${str}>`;
};
Buffer.prototype.inspect = Buffer.prototype[__wjs_bufCustomInspect];
function __wjs_bufCompareOffset(source, target, targetStart, sourceStart, targetEnd, sourceEnd) {
  const tlen = targetEnd - targetStart;
  const slen = sourceEnd - sourceStart;
  const n = Math.min(tlen, slen);
  for (let i = 0; i < n; i++) {
    const a = source[sourceStart + i];
    const b = target[targetStart + i];
    if (a !== b) return a < b ? -1 : 1;
  }
  return slen === tlen ? 0 : (slen < tlen ? -1 : 1);
}
Buffer.prototype.compare = function compare(target, targetStart, targetEnd, sourceStart, sourceEnd) {
  if (!__wjs_bufIsU8(target)) {
    throw __wjs_bufArgTypeErr('target', ['Buffer', 'Uint8Array'], target);
  }
  if (arguments.length === 1) return __wjs_bufCompare(this, target);
  if (targetStart === undefined) targetStart = 0;
  else __wjs_bufValidateOffset(targetStart, 'targetStart');
  if (targetEnd === undefined) targetEnd = target.length;
  else __wjs_bufValidateOffset(targetEnd, 'targetEnd', 0, target.length);
  if (sourceStart === undefined) sourceStart = 0;
  else __wjs_bufValidateOffset(sourceStart, 'sourceStart');
  if (sourceEnd === undefined) sourceEnd = this.length;
  else __wjs_bufValidateOffset(sourceEnd, 'sourceEnd', 0, this.length);
  if (sourceStart >= sourceEnd) return (targetStart >= targetEnd ? 0 : -1);
  if (targetStart >= targetEnd) return 1;
  return __wjs_bufCompareOffset(this, target, targetStart, sourceStart, targetEnd, sourceEnd);
};
function __wjs_bufBidirectionalIndexOf(buffer, val, byteOffset, end, encoding, dir) {
  __wjs_bufValidateBuffer(buffer);
  if (typeof byteOffset === 'string') {
    encoding = byteOffset;
    byteOffset = undefined;
  } else if (byteOffset > 0x7fffffff) {
    byteOffset = 0x7fffffff;
  } else if (byteOffset < -0x80000000) {
    byteOffset = -0x80000000;
  }
  byteOffset = +byteOffset;
  if (Number.isNaN(byteOffset)) {
    byteOffset = dir ? 0 : (buffer.length || buffer.byteLength);
  }
  dir = !!dir;
  if (typeof val === 'number') {
    return __wjs_bufIndexOfNumber(buffer, val >>> 0, byteOffset, dir, end);
  }
  let ops;
  if (encoding === undefined) ops = __wjs_bufEncodingOps.utf8;
  else ops = __wjs_bufGetEncodingOps(encoding);
  if (typeof val === 'string') {
    if (ops === undefined) throw __wjs_bufEncErr(encoding);
    return ops.indexOf(buffer, val, byteOffset, dir, end);
  }
  if (__wjs_bufIsU8(val)) {
    // node indexOfBuffer：needle 按给定 encoding 重编码（'ucs2' 把 'f' 编成
    // [0x66,0x00] 两字节——奇尾字节补零，非丢弃）
    if (ops !== undefined && ops.encoding === 'utf16le') {
      const out = new Uint8Array(val.length + (val.length % 2));
      for (let i = 0; i < val.length; i++) out[i] = val[i];
      return __wjs_bufIndexOfBytes(buffer, out, byteOffset, dir, end, 2);
    }
    if (ops !== undefined && ops.encoding !== 'utf8') {
      const reencoded = __wjs_bufEncodeStr(ops.slice(val, 0, val.length), ops);
      return __wjs_bufIndexOfBytes(buffer, reencoded, byteOffset, dir, end);
    }
    return __wjs_bufIndexOfBytes(buffer, val, byteOffset, dir, end);
  }
  throw __wjs_bufArgTypeErr('value', ['number', 'string', 'Buffer', 'Uint8Array'], val);
}
Buffer.prototype.indexOf = function indexOf(val, offset, end, encoding) {
  if (typeof end === 'string') {
    encoding = end;
    end = this.length;
  } else if (end === undefined) {
    end = this.length;
  }
  return __wjs_bufBidirectionalIndexOf(this, val, offset, end, encoding, true);
};
Buffer.prototype.lastIndexOf = function lastIndexOf(val, offset, end, encoding) {
  if (typeof end === 'string') {
    encoding = end;
    end = this.length;
  } else if (end === undefined) {
    end = this.length;
  }
  return __wjs_bufBidirectionalIndexOf(this, val, offset, end, encoding, false);
};
Buffer.prototype.includes = function includes(val, offset, end, encoding) {
  if (typeof end === 'string') {
    encoding = end;
    end = this.length;
  } else if (end === undefined) {
    end = this.length;
  }
  return __wjs_bufBidirectionalIndexOf(this, val, offset, end, encoding, true) !== -1;
};
function __wjs_bufValidateOffset(value, name, min = 0, max = kMaxLength) {
  __wjs_bufValidateInteger(value, name, min, max);
}
function __wjs_bufFill(buf, value, offset, end, encoding) {
  if (value === undefined) value = 0; // node fill() 无参零填（bindingFill undefined 口径）
  if (typeof value === 'string') {
    if (offset === undefined || typeof offset === 'string') {
      encoding = offset;
      offset = 0;
      end = buf.length;
    } else if (typeof end === 'string') {
      encoding = end;
      end = buf.length;
    }
    const normalizedEncoding = __wjs_bufNormalizeEncoding(encoding);
    if (normalizedEncoding === undefined) {
      __wjs_bufValidateString(encoding, 'encoding');
      throw __wjs_bufEncErr(encoding);
    }
    if (value.length === 0) {
      value = 0;
    } else if (value.length === 1) {
      if (normalizedEncoding === 'utf8' || normalizedEncoding === 'ascii') {
        const code = value.charCodeAt(0);
        if (code < 128) value = code;
      } else if (normalizedEncoding === 'latin1') {
        value = value.charCodeAt(0);
      }
    }
  } else {
    encoding = undefined;
  }
  if (offset === undefined) {
    offset = 0;
    end = buf.length;
  } else {
    __wjs_bufValidateOffset(offset, 'offset');
    if (end === undefined) {
      end = buf.length;
    } else {
      __wjs_bufValidateOffset(end, 'end', 0, buf.length);
    }
    if (offset >= end) return buf;
  }
  if (typeof value === 'number') {
    const byteLen = buf.byteLength;
    const fillLength = end - offset;
    if (offset > end || fillLength + offset > byteLen) throw __wjs_bufOobErr();
    __wjs_bufTAFill.call(buf, value, offset, end);
  } else {
    const res = __wjs_bufBindingFill(buf, value, offset, end, encoding);
    if (res < 0) {
      if (res === -1) throw __wjs_bufArgValueErr('value', value);
      throw __wjs_bufOobErr();
    }
  }
  return buf;
}
function __wjs_bufBindingFill(buf, value, offset, end, encoding) {
  let bytes;
  if (typeof value === 'string') {
    const ops = __wjs_bufGetEncodingOps(encoding === undefined ? 'utf8' : encoding);
    if (ops === undefined) return -1;
    const tmp = new __wjs_bufFastBuffer(ops.byteLength(value) || 1);
    const actual = ops.write(tmp, value, 0, tmp.length);
    bytes = tmp.subarray(0, actual);
  } else if (__wjs_bufIsU8(value)) {
    if (value.length === 0) return -1;
    bytes = value;
  } else {
    return -1;
  }
  if (bytes.length === 0) return -1;
  const room = end - offset;
  if (room < bytes.length) bytes = bytes.subarray(0, room);
  buf.set(bytes, offset);
  for (let i = offset + bytes.length; i < end; i += bytes.length) {
    const n = Math.min(bytes.length, end - i);
    buf.set(bytes.subarray(0, n), i);
  }
  return 0;
}
Buffer.prototype.fill = function fill(value, offset, end, encoding) {
  return __wjs_bufFill(this, value, offset, end, encoding);
};
Buffer.prototype.write = function write(string, offset, length, encoding) {
  const bufferLength = this.length;
  if (offset === undefined) {
    return __wjs_bufUtf8Write(this, string, 0, bufferLength);
  }
  if (length === undefined && typeof offset === 'string') {
    encoding = offset;
    length = bufferLength;
    offset = 0;
  } else {
    __wjs_bufValidateOffset(offset, 'offset', 0, bufferLength);
    const remaining = bufferLength - offset;
    if (length === undefined) {
      length = remaining;
    } else if (typeof length === 'string') {
      encoding = length;
      length = remaining;
    } else {
      __wjs_bufValidateOffset(length, 'length', 0, bufferLength);
      if (length > remaining) length = remaining;
    }
  }
  if (!encoding || encoding === 'utf8') return __wjs_bufUtf8Write(this, string, offset, length);
  if (encoding === 'ascii') return __wjs_bufAsciiWrite(this, string, offset, length);
  const ops = __wjs_bufGetEncodingOps(encoding);
  if (ops === undefined) throw __wjs_bufEncErr(encoding);
  return ops.write(this, string, offset, length);
};
Buffer.prototype.toJSON = function toJSON() {
  const bufferLength = this.length;
  if (bufferLength > 0) {
    const data = new Array(bufferLength);
    for (let i = 0; i < bufferLength; ++i) data[i] = this[i];
    return { type: 'Buffer', data };
  }
  return { type: 'Buffer', data: [] };
};
function __wjs_bufAdjustOffset(offset, length) {
  offset = Math.trunc(offset);
  if (offset === 0) return 0;
  if (offset < 0) {
    offset += length;
    return offset > 0 ? offset : 0;
  }
  if (offset < length) return offset;
  return Number.isNaN(offset) ? 0 : length;
}
Buffer.prototype.subarray = function subarray(start, end) {
  const srcLength = this.length;
  start = __wjs_bufAdjustOffset(start, srcLength);
  end = end !== undefined ? __wjs_bufAdjustOffset(end, srcLength) : srcLength;
  const newLength = end > start ? end - start : 0;
  return new __wjs_bufFastBuffer(this.buffer, this.byteOffset + start, newLength);
};
Buffer.prototype.slice = function slice(start, end) {
  return this.subarray(start, end);
};
function __wjs_bufSwap(b, n, m) {
  const i = b[n];
  b[n] = b[m];
  b[m] = i;
}
Buffer.prototype.swap16 = function swap16() {
  const len = this.length;
  if (len % 2 !== 0) throw __wjs_bufSizeErr('16-bits');
  for (let i = 0; i < len; i += 2) __wjs_bufSwap(this, i, i + 1);
  return this;
};
Buffer.prototype.swap32 = function swap32() {
  const len = this.length;
  if (len % 4 !== 0) throw __wjs_bufSizeErr('32-bits');
  for (let i = 0; i < len; i += 4) {
    __wjs_bufSwap(this, i, i + 3);
    __wjs_bufSwap(this, i + 1, i + 2);
  }
  return this;
};
Buffer.prototype.swap64 = function swap64() {
  const len = this.length;
  if (len % 8 !== 0) throw __wjs_bufSizeErr('64-bits');
  for (let i = 0; i < len; i += 8) {
    __wjs_bufSwap(this, i, i + 7);
    __wjs_bufSwap(this, i + 1, i + 6);
    __wjs_bufSwap(this, i + 2, i + 5);
    __wjs_bufSwap(this, i + 3, i + 4);
  }
  return this;
};
Buffer.prototype.toLocaleString = Buffer.prototype.toString;
// parent/offset getter（lib/buffer.js 原文；prototype 上，非自有属性）
Object.defineProperty(Buffer.prototype, 'parent', {
  enumerable: true,
  get() {
    if (!(this instanceof Buffer)) return undefined;
    return this.buffer;
  },
});
Object.defineProperty(Buffer.prototype, 'offset', {
  enumerable: true,
  get() {
    if (!(this instanceof Buffer)) return undefined;
    return this.byteOffset;
  },
});
// read/write 原型方法挂载（addBufferPrototypeMethods 原文结构）
Buffer.prototype.readBigUInt64LE = __wjs_bufReadBigUInt64LE;
Buffer.prototype.readBigUInt64BE = __wjs_bufReadBigUInt64BE;
Buffer.prototype.readBigUint64LE = __wjs_bufReadBigUInt64LE;
Buffer.prototype.readBigUint64BE = __wjs_bufReadBigUInt64BE;
Buffer.prototype.readBigInt64LE = __wjs_bufReadBigInt64LE;
Buffer.prototype.readBigInt64BE = __wjs_bufReadBigInt64BE;
Buffer.prototype.writeBigUInt64LE = function (value, offset = 0) {
  return __wjs_bufWriteBigU64LE(this, value, offset, 0n, 0xffffffffffffffffn);
};
Buffer.prototype.writeBigUInt64BE = function (value, offset = 0) {
  return __wjs_bufWriteBigU64BE(this, value, offset, 0n, 0xffffffffffffffffn);
};
Buffer.prototype.writeBigUint64LE = Buffer.prototype.writeBigUInt64LE;
Buffer.prototype.writeBigUint64BE = Buffer.prototype.writeBigUInt64BE;
Buffer.prototype.writeBigInt64LE = function (value, offset = 0) {
  return __wjs_bufWriteBigU64LE(this, value, offset, -0x8000000000000000n, 0x7fffffffffffffffn);
};
Buffer.prototype.writeBigInt64BE = function (value, offset = 0) {
  return __wjs_bufWriteBigU64BE(this, value, offset, -0x8000000000000000n, 0x7fffffffffffffffn);
};
Buffer.prototype.readUIntLE = __wjs_bufReadUIntLE;
Buffer.prototype.readUInt32LE = function (offset) { return __wjs_bufReadUInt32LE(this, offset); };
Buffer.prototype.readUInt16LE = function (offset) { return __wjs_bufReadUInt16LE(this, offset); };
Buffer.prototype.readUInt8 = function (offset) { return __wjs_bufReadUInt8(this, offset); };
Buffer.prototype.readUIntBE = __wjs_bufReadUIntBE;
Buffer.prototype.readUInt32BE = function (offset) { return __wjs_bufReadUInt32BE(this, offset); };
Buffer.prototype.readUInt16BE = function (offset) { return __wjs_bufReadUInt16BE(this, offset); };
Buffer.prototype.readUintLE = __wjs_bufReadUIntLE;
Buffer.prototype.readUint32LE = Buffer.prototype.readUInt32LE;
Buffer.prototype.readUint16LE = Buffer.prototype.readUInt16LE;
Buffer.prototype.readUint8 = Buffer.prototype.readUInt8;
Buffer.prototype.readUintBE = __wjs_bufReadUIntBE;
Buffer.prototype.readUint32BE = Buffer.prototype.readUInt32BE;
Buffer.prototype.readUint16BE = Buffer.prototype.readUInt16BE;
Buffer.prototype.readIntLE = __wjs_bufReadIntLE;
Buffer.prototype.readInt32LE = function (offset) { return __wjs_bufReadInt32LE(this, offset); };
Buffer.prototype.readInt16LE = function (offset) { return __wjs_bufReadInt16LE(this, offset); };
Buffer.prototype.readInt8 = function (offset) { return __wjs_bufReadInt8(this, offset); };
Buffer.prototype.readIntBE = __wjs_bufReadIntBE;
Buffer.prototype.readInt32BE = function (offset) { return __wjs_bufReadInt32BE(this, offset); };
Buffer.prototype.readInt16BE = function (offset) { return __wjs_bufReadInt16BE(this, offset); };
Buffer.prototype.writeUIntLE = __wjs_bufWriteUIntLE;
Buffer.prototype.writeUInt32LE = function (value, offset = 0) { return __wjs_bufWriteU32LE(this, value, offset, 0, 0xffffffff); };
Buffer.prototype.writeUInt16LE = function (value, offset = 0) { return __wjs_bufWriteU16LE(this, value, offset, 0, 0xffff); };
Buffer.prototype.writeUInt8 = function (value, offset = 0) { return __wjs_bufWriteU8(this, value, offset, 0, 0xff); };
Buffer.prototype.writeUIntBE = __wjs_bufWriteUIntBE;
Buffer.prototype.writeUInt32BE = function (value, offset = 0) { return __wjs_bufWriteU32BE(this, value, offset, 0, 0xffffffff); };
Buffer.prototype.writeUInt16BE = function (value, offset = 0) { return __wjs_bufWriteU16BE(this, value, offset, 0, 0xffff); };
Buffer.prototype.writeUintLE = __wjs_bufWriteUIntLE;
Buffer.prototype.writeUint32LE = Buffer.prototype.writeUInt32LE;
Buffer.prototype.writeUint16LE = Buffer.prototype.writeUInt16LE;
Buffer.prototype.writeUint8 = Buffer.prototype.writeUInt8;
Buffer.prototype.writeUintBE = __wjs_bufWriteUIntBE;
Buffer.prototype.writeUint32BE = Buffer.prototype.writeUInt32BE;
Buffer.prototype.writeUint16BE = Buffer.prototype.writeUInt16BE;
Buffer.prototype.writeIntLE = __wjs_bufWriteIntLE;
function __wjs_bufWriteIntBE(value, offset, byteLength) {
  if (byteLength === 6) return __wjs_bufWriteU48BE(this, value, offset, -0x800000000000, 0x7fffffffffff);
  if (byteLength === 5) return __wjs_bufWriteU40BE(this, value, offset, -0x8000000000, 0x7fffffffff);
  if (byteLength === 3) return __wjs_bufWriteU24BE(this, value, offset, -0x800000, 0x7fffff);
  if (byteLength === 4) return __wjs_bufWriteU32BE(this, value, offset, -0x80000000, 0x7fffffff);
  if (byteLength === 2) return __wjs_bufWriteU16BE(this, value, offset, -0x8000, 0x7fff);
  if (byteLength === 1) return __wjs_bufWriteU8(this, value, offset, -0x80, 0x7f);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs_bufWriteIntLE(value, offset, byteLength) {
  if (byteLength === 6) return __wjs_bufWriteU48LE(this, value, offset, -0x800000000000, 0x7fffffffffff);
  if (byteLength === 5) return __wjs_bufWriteU40LE(this, value, offset, -0x8000000000, 0x7fffffffff);
  if (byteLength === 3) return __wjs_bufWriteU24LE(this, value, offset, -0x800000, 0x7fffff);
  if (byteLength === 4) return __wjs_bufWriteU32LE(this, value, offset, -0x80000000, 0x7fffffff);
  if (byteLength === 2) return __wjs_bufWriteU16LE(this, value, offset, -0x8000, 0x7fff);
  if (byteLength === 1) return __wjs_bufWriteU8(this, value, offset, -0x80, 0x7f);
  __wjs_bufBoundsError(byteLength, 6, 'byteLength');
}
Buffer.prototype.writeInt32LE = function (value, offset = 0) { return __wjs_bufWriteU32LE(this, value, offset, -0x80000000, 0x7fffffff); };
Buffer.prototype.writeInt16LE = function (value, offset = 0) { return __wjs_bufWriteU16LE(this, value, offset, -0x8000, 0x7fff); };
Buffer.prototype.writeInt8 = function (value, offset = 0) { return __wjs_bufWriteU8(this, value, offset, -0x80, 0x7f); };
Buffer.prototype.writeIntBE = __wjs_bufWriteIntBE;
Buffer.prototype.writeInt32BE = function (value, offset = 0) { return __wjs_bufWriteU32BE(this, value, offset, -0x80000000, 0x7fffffff); };
Buffer.prototype.writeInt16BE = function (value, offset = 0) { return __wjs_bufWriteU16BE(this, value, offset, -0x8000, 0x7fff); };
Buffer.prototype.readFloatLE = __wjs_bufReadFloatLE;
Buffer.prototype.readFloatBE = __wjs_bufReadFloatBE;
Buffer.prototype.readDoubleLE = __wjs_bufReadDoubleLE;
Buffer.prototype.readDoubleBE = __wjs_bufReadDoubleBE;
Buffer.prototype.writeFloatLE = __wjs_bufWriteFloatLE;
Buffer.prototype.writeFloatBE = __wjs_bufWriteFloatBE;
Buffer.prototype.writeDoubleLE = __wjs_bufWriteDoubleLE;
Buffer.prototype.writeDoubleBE = __wjs_bufWriteDoubleBE;
Buffer.prototype.asciiWrite = function (string, offset, length) { return __wjs_bufAsciiWrite(this, string, offset, length); };
Buffer.prototype.base64Write = function (string, offset, length) { return __wjs_bufBase64Write(this, string, offset, length); };
Buffer.prototype.base64urlWrite = function (string, offset, length) { return __wjs_bufBase64urlWrite(this, string, offset, length); };
Buffer.prototype.latin1Write = function (string, offset, length) { return __wjs_bufLatin1Write(this, string, offset, length); };
Buffer.prototype.hexWrite = function (string, offset, length) { return __wjs_bufHexWrite(this, string, offset, length); };
Buffer.prototype.ucs2Write = function (string, offset, length) { return __wjs_bufUcs2Write(this, string, offset, length); };
Buffer.prototype.utf8Write = function (string, offset, length) { return __wjs_bufUtf8Write(this, string, offset, length); };
Buffer.prototype.asciiSlice = function (start, end) { return __wjs_bufAsciiSlice(this, start, end); };
Buffer.prototype.base64Slice = function (start, end) { return __wjs_bufB64Slice(this, start, end, false); };
Buffer.prototype.base64urlSlice = function (start, end) { return __wjs_bufB64Slice(this, start, end, true); };
Buffer.prototype.latin1Slice = function (start, end) { return __wjs_bufLatin1Slice(this, start, end); };
Buffer.prototype.hexSlice = function (start, end) { return __wjs_bufHexSlice(this, start, end); };
Buffer.prototype.ucs2Slice = function (start, end) { return __wjs_bufUcs2Slice(this, start, end); };
Buffer.prototype.utf8Slice = function (start, end) { return __wjs_bufUtf8Slice(this, start, end); };
globalThis.__wjs_bufApi = {
  get INSPECT_MAX_BYTES() { return INSPECT_MAX_BYTES; },
  set INSPECT_MAX_BYTES(v) {
    __wjs_bufValidateNumber(v, 'INSPECT_MAX_BYTES', 0);
    INSPECT_MAX_BYTES = v;
  },
  kMaxLength,
  kStringMaxLength,
  isUtf8(input) {
    if ((ArrayBuffer.isView(input) && !(input instanceof DataView)) || __wjs_bufIsAnyAB(input)) {
      const u8 = __wjs_bufAsU8(input) ?? new Uint8Array(0);
      try {
        new TextDecoder('utf-8', { fatal: true }).decode(u8);
        return true;
      } catch {
        return false;
      }
    }
    throw __wjs_bufArgTypeErr('input', ['ArrayBuffer', 'Buffer', 'TypedArray'], input);
  },
  isAscii(input) {
    if ((ArrayBuffer.isView(input) && !(input instanceof DataView)) || __wjs_bufIsAnyAB(input)) {
      const u8 = __wjs_bufAsU8(input) ?? new Uint8Array(0);
      for (let i = 0; i < u8.length; i++) {
        if (u8[i] > 0x7f) return false;
      }
      return true;
    }
    throw __wjs_bufArgTypeErr('input', ['ArrayBuffer', 'Buffer', 'TypedArray'], input);
  },
  btoa(input) {
    if (arguments.length === 0) throw __wjs_bufMissingArgsErr('input');
    return globalThis.btoa(`${input}`);
  },
  atob(input) {
    if (arguments.length === 0) throw __wjs_bufMissingArgsErr('input');
    return globalThis.atob(`${input}`);
  },
  transcode(source, fromEncoding, toEncoding) {
    if (!__wjs_bufIsU8(source)) {
      throw __wjs_bufArgTypeErr('source', ['Buffer', 'Uint8Array'], source);
    }
    if (source.length === 0) return new __wjs_bufFastBuffer();
    fromEncoding = __wjs_bufNormalizeEncoding(fromEncoding) || fromEncoding;
    toEncoding = __wjs_bufNormalizeEncoding(toEncoding) || toEncoding;
    const fromOps = __wjs_bufGetEncodingOps(fromEncoding);
    const toOps = __wjs_bufGetEncodingOps(toEncoding);
    if (fromOps === undefined || toOps === undefined) {
      const e = new RangeError(`Unable to transcode Buffer [U_UNKNOWN_ENCODING]`);
      e.code = 'ERR_UNKNOWN_ENCODING';
      e.errno = -1;
      throw e;
    }
    const decoded = fromOps.slice(source, 0, source.length);
    return __wjs_bufFromStringFast(decoded, toOps);
  },
};

// Uint8Array 构造失败文案桥（V8 "Invalid typed array length: N" 口径；
// SM 抛自有文案，套件按 V8 插值断言；newTarget 必须透传，否则 TypedArray
// 子类化（`class X extends Uint8Array`）全灭为基类原型——10f buffer 实测）。
(() => {
  const U8 = globalThis.Uint8Array;
  globalThis.Uint8Array = new Proxy(U8, {
    construct(target, args, newTarget) {
      try {
        return Reflect.construct(target, args, newTarget);
      } catch (e) {
        throw new RangeError(`Invalid typed array length: ${args[0]}`);
      }
    },
  });
})();

// String.prototype.repeat 的 RangeError 文案桥（V8 口径："Invalid string length"/
// "Invalid count value: N"；SM 文案不同，套件正则按 V8 断言）
(() => {
  const rep = String.prototype.repeat;
  Object.defineProperty(String.prototype, 'repeat', {
    value: function (count) {
      if (typeof count === 'number' && count < 0) {
        throw new RangeError(`Invalid count value: ${count}`);
      }
      try {
        return rep.call(this, count);
      } catch (e) {
        throw e instanceof RangeError ? new RangeError('Invalid string length') : e;
      }
    },
    writable: true,
    configurable: true,
    enumerable: false,
  });
})();
globalThis.__wjs_bufDecode = __wjs_bufDecode;
globalThis.__wjs_bufEncode = __wjs_bufEncode;
globalThis.Buffer = Buffer;
})();
const __wjs_keyState = new WeakMap();
function NotSupportedError_(what) { return new Error(`NotSupportedError: unsupported ${what}`); }
function __wjs_normHash(h) {
  const s = typeof h === "string" ? h : String(h?.name ?? "");
  const up = s.trim().toUpperCase();
  const map = { "SHA-1": "SHA-1", "SHA1": "SHA-1", "SHA-256": "SHA-256", "SHA256": "SHA-256", "SHA-384": "SHA-384", "SHA384": "SHA-384", "SHA-512": "SHA-512", "SHA512": "SHA-512" };
  if (!map[up]) throw new Error(`NotSupportedError: unsupported hash '${s}'`);
  return map[up];
}
function __wjs_makeKey(alg, material, usages, extractable, kind) {
  const k = Object.create(CryptoKey.prototype);
  __wjs_keyState.set(k, { alg, material, usages, extractable, kind: kind ?? "secret" });
  return k;
}
function __wjs_keyBytes(v) {
  if (v instanceof ArrayBuffer) return new Uint8Array(v);
  if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  throw new TypeError("key data must be a BufferSource");
}
function __wjs_dataBytes(v) {
  if (typeof v === "string") return new TextEncoder().encode(v);
  return __wjs_keyBytes(v);
}
function __wjs_needUsage(st, op) {
  if (!st.usages.includes(op)) throw new Error(`InvalidAccessError: key cannot be used to ${op}`);
}
function __wjs_aesParams(algorithm) {
  const iv = __wjs_dataBytes(algorithm?.iv ?? new Uint8Array(0));
  if (iv.length !== 12) throw new Error("OperationError: AES-GCM iv must be 12 bytes");
  const aad = algorithm?.additionalData === undefined ? undefined : __wjs_dataBytes(algorithm.additionalData);
  const tagLength = algorithm?.tagLength === undefined ? 128 : Number(algorithm.tagLength);
  if (tagLength !== 128) throw new Error("NotSupportedError: only 128-bit AES-GCM tags for now");
  return { iv, aad };
}
function __wjs_b64urlEncode(u8) {
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
function __wjs_b64urlDecode(str) {
  str = String(str).replace(/-/g, "+").replace(/_/g, "/");
  while (str.length % 4) str += "=";
  const bin = atob(str);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
globalThis.CryptoKey = class CryptoKey {
  constructor() { throw new TypeError("Illegal constructor"); }
  get algorithm() { return { ...__wjs_keyState.get(this)?.alg }; }
  get extractable() { return !!__wjs_keyState.get(this)?.extractable; }
  get type() { return __wjs_keyState.get(this)?.kind ?? "secret"; }
  get usages() { return [...(__wjs_keyState.get(this)?.usages ?? [])]; }
};
function __wjs_normCurve(c) {
  const s = String(c ?? "").trim().toUpperCase().replace("_", "-");
  const map = { "P-256": "P-256", "P256": "P-256", "P-384": "P-384", "P384": "P-384", "P-521": "P-521", "P521": "P-521" };
  if (!map[s]) throw new Error(`NotSupportedError: unsupported curve '${c}' (P-256/384/521)`);
  return map[s];
}
function __wjs_rsaPubExp(v) {
  if (v === undefined) return 65537;
  if (v instanceof Uint8Array) {
    let n = 0;
    for (const b of v) n = n * 256 + b;
    return n;
  }
  return Number(v);
}
function __wjs_x_bits(algorithm, st, length) {
  const pubKey = algorithm?.public;
  const pst = __wjs_keyState.get(pubKey);
  if (!pst || pst.alg.name !== "X25519" || pst.kind === "private") {
"#;
