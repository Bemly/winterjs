//! Buffer 类本体与构造族（from/copy/compare/fill/swap）（prelude 分域；拼接顺序见 mod.rs）。
pub const BUFFER_CLASS_JS: &str = r#"
// ---- Buffer 类本体（lib/buffer.js 原文）----
class __wjs2_bufFastBuffer extends Uint8Array {}
let __wjs2_bufWarned = false;
function __wjs2_bufShowFlaggedDeprecation() {
  if (__wjs2_bufWarned) return;
  // isInsideNodeModules(3) 的栈走查近似（SM 栈格式；DEP0169 同法）
  const saved = Error.stackTraceLimit;
  Error.stackTraceLimit = 5;
  const stack = new Error().stack || '';
  Error.stackTraceLimit = saved;
  const frames = stack.split('\n').slice(1, 5);
  // node lib/buffer.js 口径：--pending-deprecation 下 node_modules 内同样告警。
  const pending = Array.isArray(globalThis.__wjs2_nodeCompat) &&
    globalThis.__wjs2_nodeCompat.includes('--pending-deprecation');
  if (!pending && frames.some((f) => f.includes('node_modules'))) return;
  __wjs2_bufWarned = true;
  try {
    process.emitWarning(
      'Buffer() is deprecated due to security and usability issues. ' +
      'Please use the Buffer.alloc(), Buffer.allocUnsafe(), or Buffer.from() ' +
      'methods instead.', 'DeprecationWarning', 'DEP0005');
  } catch { }
}
function Buffer(arg, encodingOrOffset, length) {
  __wjs2_bufShowFlaggedDeprecation();
  if (typeof arg === 'number') {
    if (typeof encodingOrOffset === 'string') {
      throw __wjs2_bufArgTypeErr('string', 'string', arg);
    }
    return Buffer.alloc(arg);
  }
  return Buffer.from(arg, encodingOrOffset, length);
}
Object.defineProperty(Buffer, Symbol.species, {
  enumerable: false,
  configurable: true,
  get() { return __wjs2_bufFastBuffer; },
});
Object.setPrototypeOf(Buffer, Uint8Array);
Buffer.prototype = __wjs2_bufFastBuffer.prototype;
Buffer.prototype.constructor = Buffer;
Buffer.poolSize = 64 * 1024;
// 10f：小串池化（Node lib/buffer.js fromStringFast 口径：< poolSize/2 走池，
// 8 字节对齐，满即新池；`a.buffer === b.buffer` 套件门）。池 AB 进
// `__wjs2_bufPooled`（WeakSet，全局暴露供 worker  transfer 拒收），
// `ArrayBuffer.prototype.transfer` 对池内 AB 抛 TypeError（真机同款不可转移）。
let __wjs2_bufPoolAB = new ArrayBuffer(Buffer.poolSize);
const __wjs2_bufPooled = new WeakSet([__wjs2_bufPoolAB]);
globalThis.__wjs2_bufPooled = __wjs2_bufPooled;
let __wjs2_bufPoolOffset = 0;
function __wjs2_bufPoolAlign() {
  if (__wjs2_bufPoolOffset & 0x7) __wjs2_bufPoolOffset = (__wjs2_bufPoolOffset + 7) & ~7;
}
{
  const __wjs2_bufOrigTransfer = ArrayBuffer.prototype.transfer;
  Object.defineProperty(ArrayBuffer.prototype, 'transfer', {
    value: function() {
      if (__wjs2_bufPooled.has(this)) {
        throw new TypeError('Cannot transfer a pooled Buffer ArrayBuffer');
      }
      return __wjs2_bufOrigTransfer.call(this);
    },
    writable: true, configurable: true,
  });
}
function __wjs2_bufToInteger(n, defaultVal) {
  n = +n;
  if (!Number.isNaN(n) && n >= Number.MIN_SAFE_INTEGER && n <= Number.MAX_SAFE_INTEGER) {
    return ((n % 1) === 0 ? n : Math.floor(n));
  }
  return defaultVal;
}
function __wjs2_bufCopyImpl(source, target, targetStart, sourceStart, sourceEnd) {
  if (!ArrayBuffer.isView(source))
    throw __wjs2_bufArgTypeErr('source', ['Buffer', 'Uint8Array'], source);
  if (!ArrayBuffer.isView(target))
    throw __wjs2_bufArgTypeErr('target', ['Buffer', 'Uint8Array'], target);
  if (targetStart === undefined) {
    targetStart = 0;
  } else {
    targetStart = Number.isInteger(targetStart) ? targetStart : __wjs2_bufToInteger(targetStart, 0);
    if (targetStart < 0) throw __wjs2_bufRangeErr('targetStart', '>= 0', targetStart);
  }
  if (sourceStart === undefined) {
    sourceStart = 0;
  } else {
    sourceStart = Number.isInteger(sourceStart) ? sourceStart : __wjs2_bufToInteger(sourceStart, 0);
    if (sourceStart < 0 || sourceStart > source.byteLength)
      throw __wjs2_bufRangeErr('sourceStart', `>= 0 && <= ${source.byteLength}`, sourceStart);
  }
  if (sourceEnd === undefined) {
    sourceEnd = source.byteLength;
  } else {
    sourceEnd = Number.isInteger(sourceEnd) ? sourceEnd : __wjs2_bufToInteger(sourceEnd, 0);
    if (sourceEnd < 0) throw __wjs2_bufRangeErr('sourceEnd', '>= 0', sourceEnd);
  }
  if (targetStart >= target.byteLength || sourceStart >= sourceEnd)
    return 0;
  return __wjs2_bufCopyActual(source, target, targetStart, sourceStart, sourceEnd);
}
function __wjs2_bufCopyActual(source, target, targetStart, sourceStart, sourceEnd) {
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
    return __wjs2_bufFromString(value, encodingOrOffset);
  if (typeof value === 'object' && value !== null) {
    if (__wjs2_bufIsAnyAB(value)) {
      // 10f：伪 AB（原型链伪造、无内部槽，如 `Object.setPrototypeOf(AB, ArrayBuffer)`）
      // 须拒为 ERR_INVALID_ARG_TYPE（V8 IsArrayBuffer 品牌检查口径）；直接进
      // FromArrayBuffer 会在引擎内抛 incompatible 文案，断言对不上。
      let branded = true;
      try { void value.byteLength; } catch { branded = false; }
      if (branded)
        return __wjs2_bufFromArrayBuffer(value, encodingOrOffset, length);
      // 落空到尾部统一 invalid-arg（`an instance of AB`，__wjs2_bufSpecificType 口径）
    } else {
      const valueOf = value.valueOf && value.valueOf();
      if (valueOf != null && valueOf !== value &&
          (typeof valueOf === 'string' || typeof valueOf === 'object')) {
        return from(valueOf, encodingOrOffset, length);
      }
      const b = __wjs2_bufFromObject(value);
      if (b) return b;
      if (typeof value[Symbol.toPrimitive] === 'function') {
        const primitive = value[Symbol.toPrimitive]('string');
        if (typeof primitive === 'string') {
          return __wjs2_bufFromString(primitive, encodingOrOffset);
        }
      }
    }
  }
  throw __wjs2_bufArgTypeErr(
    'first argument',
    ['string', 'Buffer', 'ArrayBuffer', 'Array', 'Array-like Object'],
    value,
  );
};
Buffer.copyBytesFrom = function copyBytesFrom(view, offset, length) {
  if (!ArrayBuffer.isView(view) || view instanceof DataView) {
    throw __wjs2_bufArgTypeErr('view', ['TypedArray'], view);
  }
  const viewLength = view.length;
  if (viewLength === 0) return new __wjs2_bufFastBuffer();
  let start = 0;
  let end = viewLength;
  if (offset !== undefined) {
    __wjs2_bufValidateInteger(offset, 'offset', 0);
    if (offset >= viewLength) return new __wjs2_bufFastBuffer();
    start = offset;
  }
  if (length !== undefined) {
    __wjs2_bufValidateInteger(length, 'length', 0);
    end = Math.min(start + length, viewLength);
  }
  if (end <= start) return new __wjs2_bufFastBuffer();
  const viewByteLength = view.byteLength;
  const elementSize = viewByteLength / viewLength;
  const srcByteOffset = view.byteOffset + start * elementSize;
  const srcByteLength = (end - start) * elementSize;
  return __wjs2_bufFromArrayLike(new Uint8Array(view.buffer, srcByteOffset, srcByteLength));
};
const __wjs2_bufOf = (...items) => {
  const len = items.length;
  const newObj = new __wjs2_bufFastBuffer(len);
  for (let k = 0; k < len; k++) newObj[k] = items[k];
  return newObj;
};
Buffer.of = __wjs2_bufOf;
Buffer.alloc = function alloc(size, fill, encoding) {
  __wjs2_bufValidateNumber(size, 'size', 0, kMaxLength);
  if (fill !== undefined && fill !== 0 && size > 0) {
    const buf = new __wjs2_bufFastBuffer(size);
    return __wjs2_bufFill(buf, fill, 0, buf.length, encoding);
  }
  return new __wjs2_bufFastBuffer(size);
};
Buffer.allocUnsafe = function allocUnsafe(size) {
  // alignment 形参不做（无 O_DIRECT 场景；真机 26 有，记档）
  __wjs2_bufValidateNumber(size, 'size', 0, kMaxLength);
  return size <= 0 ? new __wjs2_bufFastBuffer() : new __wjs2_bufFastBuffer(size);
};
Buffer.allocUnsafeSlow = function allocUnsafeSlow(size) {
  __wjs2_bufValidateNumber(size, 'size', 0, kMaxLength);
  return size <= 0 ? new __wjs2_bufFastBuffer() : new __wjs2_bufFastBuffer(size);
};
function __wjs2_bufFromStringFast(string, ops) {
  const length = ops.byteLength(string);
  // 池路径（小串共享池 AB；actual 按写入实长推进，与 Node fromStringFast 同口径）
  if (length > 0 && length < (Buffer.poolSize >>> 1)) {
    __wjs2_bufPoolAlign();
    if (length > __wjs2_bufPoolAB.byteLength - __wjs2_bufPoolOffset) {
      __wjs2_bufPoolAB = new ArrayBuffer(Buffer.poolSize);
      __wjs2_bufPooled.add(__wjs2_bufPoolAB);
      __wjs2_bufPoolOffset = 0;
    }
    const scratch = new Uint8Array(__wjs2_bufPoolAB);
    const actual = ops.write(scratch, string, __wjs2_bufPoolOffset, length);
    const b = new __wjs2_bufFastBuffer(__wjs2_bufPoolAB, __wjs2_bufPoolOffset, actual);
    __wjs2_bufPoolOffset += actual;
    return b;
  }
  const buf = Buffer.allocUnsafeSlow(length);
  const actual = ops.write(buf, string, 0, length);
  return actual < length ? new __wjs2_bufFastBuffer(buf.buffer, 0, actual) : buf;
}
function __wjs2_bufFromString(string, encoding) {
  let ops;
  if (!encoding || encoding === 'utf8' || typeof encoding !== 'string') {
    ops = __wjs2_bufEncodingOps.utf8;
  } else {
    ops = __wjs2_bufGetEncodingOps(encoding);
    if (ops === undefined) throw __wjs2_bufEncErr(encoding);
  }
  return string.length === 0 ? new __wjs2_bufFastBuffer() : __wjs2_bufFromStringFast(string, ops);
}
function __wjs2_bufFromArrayBuffer(obj, byteOffset, length) {
  if (byteOffset === undefined) {
    byteOffset = 0;
  } else {
    byteOffset = +byteOffset;
    if (Number.isNaN(byteOffset)) byteOffset = 0;
  }
  const maxLength = obj.byteLength - byteOffset;
  if (maxLength < 0) throw __wjs2_bufOobErr('offset');
  if (length !== undefined) {
    length = +length;
    if (length > 0) {
      if (length > maxLength) throw __wjs2_bufOobErr('length');
    } else {
      length = 0;
    }
  }
  return new __wjs2_bufFastBuffer(obj, byteOffset, length);
}
function __wjs2_bufFromArrayLike(obj) {
  const { length } = obj;
  if (length <= 0) return new __wjs2_bufFastBuffer();
  return new __wjs2_bufFastBuffer(obj);
}
function __wjs2_bufFromObject(obj) {
  if (obj.length !== undefined || (obj.buffer != null && __wjs2_bufIsAnyAB(obj.buffer))) {
    if (typeof obj.length !== 'number') {
      return new __wjs2_bufFastBuffer();
    }
    return __wjs2_bufFromArrayLike(obj);
  }
  if (obj.type === 'Buffer' && Array.isArray(obj.data)) {
    return __wjs2_bufFromArrayLike(obj.data);
  }
}
Buffer.isBuffer = function isBuffer(b) {
  return b instanceof Buffer;
};
Buffer.compare = function compare(buf1, buf2) {
  if (!__wjs2_bufIsU8(buf1)) throw __wjs2_bufArgTypeErr('buf1', ['Buffer', 'Uint8Array'], buf1);
  if (!__wjs2_bufIsU8(buf2)) throw __wjs2_bufArgTypeErr('buf2', ['Buffer', 'Uint8Array'], buf2);
  if (buf1 === buf2) return 0;
  return __wjs2_bufCompare(buf1, buf2);
};
function __wjs2_bufIsU8(v) { return v instanceof Uint8Array; }
function __wjs2_bufCompare(a, b) {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  }
  return a.length === b.length ? 0 : (a.length < b.length ? -1 : 1);
}
Buffer.isEncoding = function isEncoding(encoding) {
  return typeof encoding === 'string' && encoding.length !== 0 &&
         __wjs2_bufNormalizeEncoding(encoding) !== undefined;
};
Buffer.concat = function concat(list, length) {
  __wjs2_bufValidateArray(list, 'list');
  if (list.length === 0) return new __wjs2_bufFastBuffer();
  if (length === undefined) {
    length = 0;
    for (let i = 0; i < list.length; i++) {
      const buf = list[i];
      if (!__wjs2_bufIsU8(buf)) {
        throw __wjs2_bufArgTypeErr(`list[${i}]`, ['Buffer', 'Uint8Array'], buf);
      }
      length += buf.byteLength;
    }
    const buffer = length <= 0 ? new __wjs2_bufFastBuffer() : new __wjs2_bufFastBuffer(length);
    let pos = 0;
    for (let i = 0; i < list.length; i++) {
      const buf = list[i];
      buffer.set(buf, pos);
      pos += buf.byteLength;
    }
    return buffer;
  }
  __wjs2_bufValidateInteger(length, 'length', 0);
  for (let i = 0; i < list.length; i++) {
    if (!__wjs2_bufIsU8(list[i])) {
      throw __wjs2_bufArgTypeErr(`list[${i}]`, ['Buffer', 'Uint8Array'], list[i]);
    }
  }
  const buffer = length <= 0 ? new __wjs2_bufFastBuffer() : new __wjs2_bufFastBuffer(length);
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
    __wjs2_bufTAFill.call(buffer, 0, pos, length);
  }
  return buffer;
};
function __wjs2_bufByteLengthUtf8(string) { return new TextEncoder().encode(string).length; }
function __wjs2_bufByteLength(string, encoding) {
  if (typeof string !== 'string') {
    if (ArrayBuffer.isView(string) || __wjs2_bufIsAnyAB(string)) {
      try {
        return string.byteLength;
      } catch {
        return 0; // detached 视空
      }
    }
    throw __wjs2_bufArgTypeErr('string', ['string', 'Buffer', 'ArrayBuffer'], string);
  }
  const len = string.length;
  if (len === 0) return 0;
  if (!encoding || encoding === 'utf8') {
    return __wjs2_bufByteLengthUtf8(string);
  }
  if (encoding === 'ascii') {
    return len;
  }
  const ops = __wjs2_bufGetEncodingOps(encoding);
  if (ops === undefined) {
    return __wjs2_bufByteLengthUtf8(string);
  }
  return ops.byteLength(string);
}
Buffer.byteLength = __wjs2_bufByteLength;
Buffer.prototype.copy = function copy(target, targetStart, sourceStart, sourceEnd) {
  return __wjs2_bufCopyImpl(this, target, targetStart, sourceStart, sourceEnd);
};
Buffer.prototype.toString = function toString(encoding, start, end) {
  if (arguments.length === 0) {
    return __wjs2_bufUtf8Slice(this, 0, this.length);
  }
  const bufferLength = this.length;
  if (start <= 0) start = 0;
  else if (start >= bufferLength) return '';
  else start = Math.trunc(start) || 0;
  if (end === undefined || end > bufferLength) end = bufferLength;
  else end = Math.trunc(end) || 0;
  if (end <= start) return '';
  if (encoding === undefined) return __wjs2_bufUtf8Slice(this, start, end);
  const ops = __wjs2_bufGetEncodingOps(encoding);
  if (ops === undefined) throw __wjs2_bufEncErr(encoding);
  return ops.slice(this, start, end);
};
Buffer.prototype.equals = function equals(otherBuffer) {
  if (!__wjs2_bufIsU8(otherBuffer)) {
    throw __wjs2_bufArgTypeErr('otherBuffer', ['Buffer', 'Uint8Array'], otherBuffer);
  }
  if (this === otherBuffer) return true;
  const len = this.byteLength;
  if (len !== otherBuffer.byteLength) return false;
  return len === 0 || __wjs2_bufCompare(this, otherBuffer) === 0;
};
let INSPECT_MAX_BYTES = 50;
const __wjs2_bufCustomInspect = Symbol.for('nodejs.util.inspect.custom');
Buffer.prototype[__wjs2_bufCustomInspect] = function inspect(recurseTimes, ctx) {
  const max = INSPECT_MAX_BYTES;
  const actualMax = Math.min(max, this.length);
  const remaining = this.length - max;
  let str = __wjs2_bufHexSlice(this, 0, actualMax).replace(/(.{2})/g, '$1 ').trim();
  if (remaining > 0) str += ` ... ${remaining} more byte${remaining > 1 ? 's' : ''}`;
  // Inspect special properties as well, if possible（lib/buffer.js extras 段）。
  if (ctx && typeof globalThis.__wjs2_inspect === 'function') {
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
        .map((key) => `${key}: ${globalThis.__wjs2_inspect(obj[key], { ...ctx, breakLength: Infinity, compact: true })}`)
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
Buffer.prototype.inspect = Buffer.prototype[__wjs2_bufCustomInspect];
function __wjs2_bufCompareOffset(source, target, targetStart, sourceStart, targetEnd, sourceEnd) {
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
  if (!__wjs2_bufIsU8(target)) {
    throw __wjs2_bufArgTypeErr('target', ['Buffer', 'Uint8Array'], target);
  }
  if (arguments.length === 1) return __wjs2_bufCompare(this, target);
  if (targetStart === undefined) targetStart = 0;
  else __wjs2_bufValidateOffset(targetStart, 'targetStart');
  if (targetEnd === undefined) targetEnd = target.length;
  else __wjs2_bufValidateOffset(targetEnd, 'targetEnd', 0, target.length);
  if (sourceStart === undefined) sourceStart = 0;
  else __wjs2_bufValidateOffset(sourceStart, 'sourceStart');
  if (sourceEnd === undefined) sourceEnd = this.length;
  else __wjs2_bufValidateOffset(sourceEnd, 'sourceEnd', 0, this.length);
  if (sourceStart >= sourceEnd) return (targetStart >= targetEnd ? 0 : -1);
  if (targetStart >= targetEnd) return 1;
  return __wjs2_bufCompareOffset(this, target, targetStart, sourceStart, targetEnd, sourceEnd);
};
function __wjs2_bufBidirectionalIndexOf(buffer, val, byteOffset, end, encoding, dir) {
  __wjs2_bufValidateBuffer(buffer);
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
    return __wjs2_bufIndexOfNumber(buffer, val >>> 0, byteOffset, dir, end);
  }
  let ops;
  if (encoding === undefined) ops = __wjs2_bufEncodingOps.utf8;
  else ops = __wjs2_bufGetEncodingOps(encoding);
  if (typeof val === 'string') {
    if (ops === undefined) throw __wjs2_bufEncErr(encoding);
    return ops.indexOf(buffer, val, byteOffset, dir, end);
  }
  if (__wjs2_bufIsU8(val)) {
    // node indexOfBuffer：needle 按给定 encoding 重编码（'ucs2' 把 'f' 编成
    // [0x66,0x00] 两字节——奇尾字节补零，非丢弃）
    if (ops !== undefined && ops.encoding === 'utf16le') {
      const out = new Uint8Array(val.length + (val.length % 2));
      for (let i = 0; i < val.length; i++) out[i] = val[i];
      return __wjs2_bufIndexOfBytes(buffer, out, byteOffset, dir, end, 2);
    }
    if (ops !== undefined && ops.encoding !== 'utf8') {
      const reencoded = __wjs2_bufEncodeStr(ops.slice(val, 0, val.length), ops);
      return __wjs2_bufIndexOfBytes(buffer, reencoded, byteOffset, dir, end);
    }
    return __wjs2_bufIndexOfBytes(buffer, val, byteOffset, dir, end);
  }
  throw __wjs2_bufArgTypeErr('value', ['number', 'string', 'Buffer', 'Uint8Array'], val);
}
Buffer.prototype.indexOf = function indexOf(val, offset, end, encoding) {
  if (typeof end === 'string') {
    encoding = end;
    end = this.length;
  } else if (end === undefined) {
    end = this.length;
  }
  return __wjs2_bufBidirectionalIndexOf(this, val, offset, end, encoding, true);
};
Buffer.prototype.lastIndexOf = function lastIndexOf(val, offset, end, encoding) {
  if (typeof end === 'string') {
    encoding = end;
    end = this.length;
  } else if (end === undefined) {
    end = this.length;
  }
  return __wjs2_bufBidirectionalIndexOf(this, val, offset, end, encoding, false);
};
Buffer.prototype.includes = function includes(val, offset, end, encoding) {
  if (typeof end === 'string') {
    encoding = end;
    end = this.length;
  } else if (end === undefined) {
    end = this.length;
  }
  return __wjs2_bufBidirectionalIndexOf(this, val, offset, end, encoding, true) !== -1;
};
function __wjs2_bufValidateOffset(value, name, min = 0, max = kMaxLength) {
  __wjs2_bufValidateInteger(value, name, min, max);
}
function __wjs2_bufFill(buf, value, offset, end, encoding) {
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
    const normalizedEncoding = __wjs2_bufNormalizeEncoding(encoding);
    if (normalizedEncoding === undefined) {
      __wjs2_bufValidateString(encoding, 'encoding');
      throw __wjs2_bufEncErr(encoding);
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
    __wjs2_bufValidateOffset(offset, 'offset');
    if (end === undefined) {
      end = buf.length;
    } else {
      __wjs2_bufValidateOffset(end, 'end', 0, buf.length);
    }
    if (offset >= end) return buf;
  }
  if (typeof value === 'number') {
    const byteLen = buf.byteLength;
    const fillLength = end - offset;
    if (offset > end || fillLength + offset > byteLen) throw __wjs2_bufOobErr();
    __wjs2_bufTAFill.call(buf, value, offset, end);
  } else {
    const res = __wjs2_bufBindingFill(buf, value, offset, end, encoding);
    if (res < 0) {
      if (res === -1) throw __wjs2_bufArgValueErr('value', value);
      throw __wjs2_bufOobErr();
    }
  }
  return buf;
}
function __wjs2_bufBindingFill(buf, value, offset, end, encoding) {
  let bytes;
  if (typeof value === 'string') {
    const ops = __wjs2_bufGetEncodingOps(encoding === undefined ? 'utf8' : encoding);
    if (ops === undefined) return -1;
    const tmp = new __wjs2_bufFastBuffer(ops.byteLength(value) || 1);
    const actual = ops.write(tmp, value, 0, tmp.length);
    bytes = tmp.subarray(0, actual);
  } else if (__wjs2_bufIsU8(value)) {
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
  return __wjs2_bufFill(this, value, offset, end, encoding);
};
Buffer.prototype.write = function write(string, offset, length, encoding) {
  const bufferLength = this.length;
  if (offset === undefined) {
    return __wjs2_bufUtf8Write(this, string, 0, bufferLength);
  }
  if (length === undefined && typeof offset === 'string') {
    encoding = offset;
    length = bufferLength;
    offset = 0;
  } else {
    __wjs2_bufValidateOffset(offset, 'offset', 0, bufferLength);
    const remaining = bufferLength - offset;
    if (length === undefined) {
      length = remaining;
    } else if (typeof length === 'string') {
      encoding = length;
      length = remaining;
    } else {
      __wjs2_bufValidateOffset(length, 'length', 0, bufferLength);
      if (length > remaining) length = remaining;
    }
  }
  if (!encoding || encoding === 'utf8') return __wjs2_bufUtf8Write(this, string, offset, length);
  if (encoding === 'ascii') return __wjs2_bufAsciiWrite(this, string, offset, length);
  const ops = __wjs2_bufGetEncodingOps(encoding);
  if (ops === undefined) throw __wjs2_bufEncErr(encoding);
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
function __wjs2_bufAdjustOffset(offset, length) {
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
  start = __wjs2_bufAdjustOffset(start, srcLength);
  end = end !== undefined ? __wjs2_bufAdjustOffset(end, srcLength) : srcLength;
  const newLength = end > start ? end - start : 0;
  return new __wjs2_bufFastBuffer(this.buffer, this.byteOffset + start, newLength);
};
Buffer.prototype.slice = function slice(start, end) {
  return this.subarray(start, end);
};
function __wjs2_bufSwap(b, n, m) {
  const i = b[n];
  b[n] = b[m];
  b[m] = i;
}
Buffer.prototype.swap16 = function swap16() {
  const len = this.length;
  if (len % 2 !== 0) throw __wjs2_bufSizeErr('16-bits');
  for (let i = 0; i < len; i += 2) __wjs2_bufSwap(this, i, i + 1);
  return this;
};
Buffer.prototype.swap32 = function swap32() {
  const len = this.length;
  if (len % 4 !== 0) throw __wjs2_bufSizeErr('32-bits');
  for (let i = 0; i < len; i += 4) {
    __wjs2_bufSwap(this, i, i + 3);
    __wjs2_bufSwap(this, i + 1, i + 2);
  }
  return this;
};
Buffer.prototype.swap64 = function swap64() {
  const len = this.length;
  if (len % 8 !== 0) throw __wjs2_bufSizeErr('64-bits');
  for (let i = 0; i < len; i += 8) {
    __wjs2_bufSwap(this, i, i + 7);
    __wjs2_bufSwap(this, i + 1, i + 6);
    __wjs2_bufSwap(this, i + 2, i + 5);
    __wjs2_bufSwap(this, i + 3, i + 4);
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
Buffer.prototype.readBigUInt64LE = __wjs2_bufReadBigUInt64LE;
Buffer.prototype.readBigUInt64BE = __wjs2_bufReadBigUInt64BE;
Buffer.prototype.readBigUint64LE = __wjs2_bufReadBigUInt64LE;
Buffer.prototype.readBigUint64BE = __wjs2_bufReadBigUInt64BE;
Buffer.prototype.readBigInt64LE = __wjs2_bufReadBigInt64LE;
Buffer.prototype.readBigInt64BE = __wjs2_bufReadBigInt64BE;
Buffer.prototype.writeBigUInt64LE = function (value, offset = 0) {
  return __wjs2_bufWriteBigU64LE(this, value, offset, 0n, 0xffffffffffffffffn);
};
Buffer.prototype.writeBigUInt64BE = function (value, offset = 0) {
  return __wjs2_bufWriteBigU64BE(this, value, offset, 0n, 0xffffffffffffffffn);
};
Buffer.prototype.writeBigUint64LE = Buffer.prototype.writeBigUInt64LE;
Buffer.prototype.writeBigUint64BE = Buffer.prototype.writeBigUInt64BE;
Buffer.prototype.writeBigInt64LE = function (value, offset = 0) {
  return __wjs2_bufWriteBigU64LE(this, value, offset, -0x8000000000000000n, 0x7fffffffffffffffn);
};
Buffer.prototype.writeBigInt64BE = function (value, offset = 0) {
  return __wjs2_bufWriteBigU64BE(this, value, offset, -0x8000000000000000n, 0x7fffffffffffffffn);
};
Buffer.prototype.readUIntLE = __wjs2_bufReadUIntLE;
Buffer.prototype.readUInt32LE = function (offset) { return __wjs2_bufReadUInt32LE(this, offset); };
Buffer.prototype.readUInt16LE = function (offset) { return __wjs2_bufReadUInt16LE(this, offset); };
Buffer.prototype.readUInt8 = function (offset) { return __wjs2_bufReadUInt8(this, offset); };
Buffer.prototype.readUIntBE = __wjs2_bufReadUIntBE;
Buffer.prototype.readUInt32BE = function (offset) { return __wjs2_bufReadUInt32BE(this, offset); };
Buffer.prototype.readUInt16BE = function (offset) { return __wjs2_bufReadUInt16BE(this, offset); };
Buffer.prototype.readUintLE = __wjs2_bufReadUIntLE;
Buffer.prototype.readUint32LE = Buffer.prototype.readUInt32LE;
Buffer.prototype.readUint16LE = Buffer.prototype.readUInt16LE;
Buffer.prototype.readUint8 = Buffer.prototype.readUInt8;
Buffer.prototype.readUintBE = __wjs2_bufReadUIntBE;
Buffer.prototype.readUint32BE = Buffer.prototype.readUInt32BE;
Buffer.prototype.readUint16BE = Buffer.prototype.readUInt16BE;
Buffer.prototype.readIntLE = __wjs2_bufReadIntLE;
Buffer.prototype.readInt32LE = function (offset) { return __wjs2_bufReadInt32LE(this, offset); };
Buffer.prototype.readInt16LE = function (offset) { return __wjs2_bufReadInt16LE(this, offset); };
Buffer.prototype.readInt8 = function (offset) { return __wjs2_bufReadInt8(this, offset); };
Buffer.prototype.readIntBE = __wjs2_bufReadIntBE;
Buffer.prototype.readInt32BE = function (offset) { return __wjs2_bufReadInt32BE(this, offset); };
Buffer.prototype.readInt16BE = function (offset) { return __wjs2_bufReadInt16BE(this, offset); };
Buffer.prototype.writeUIntLE = __wjs2_bufWriteUIntLE;
Buffer.prototype.writeUInt32LE = function (value, offset = 0) { return __wjs2_bufWriteU32LE(this, value, offset, 0, 0xffffffff); };
Buffer.prototype.writeUInt16LE = function (value, offset = 0) { return __wjs2_bufWriteU16LE(this, value, offset, 0, 0xffff); };
Buffer.prototype.writeUInt8 = function (value, offset = 0) { return __wjs2_bufWriteU8(this, value, offset, 0, 0xff); };
Buffer.prototype.writeUIntBE = __wjs2_bufWriteUIntBE;
Buffer.prototype.writeUInt32BE = function (value, offset = 0) { return __wjs2_bufWriteU32BE(this, value, offset, 0, 0xffffffff); };
Buffer.prototype.writeUInt16BE = function (value, offset = 0) { return __wjs2_bufWriteU16BE(this, value, offset, 0, 0xffff); };
Buffer.prototype.writeUintLE = __wjs2_bufWriteUIntLE;
Buffer.prototype.writeUint32LE = Buffer.prototype.writeUInt32LE;
Buffer.prototype.writeUint16LE = Buffer.prototype.writeUInt16LE;
Buffer.prototype.writeUint8 = Buffer.prototype.writeUInt8;
Buffer.prototype.writeUintBE = __wjs2_bufWriteUIntBE;
Buffer.prototype.writeUint32BE = Buffer.prototype.writeUInt32BE;
Buffer.prototype.writeUint16BE = Buffer.prototype.writeUInt16BE;
Buffer.prototype.writeIntLE = __wjs2_bufWriteIntLE;
function __wjs2_bufWriteIntBE(value, offset, byteLength) {
  if (byteLength === 6) return __wjs2_bufWriteU48BE(this, value, offset, -0x800000000000, 0x7fffffffffff);
  if (byteLength === 5) return __wjs2_bufWriteU40BE(this, value, offset, -0x8000000000, 0x7fffffffff);
  if (byteLength === 3) return __wjs2_bufWriteU24BE(this, value, offset, -0x800000, 0x7fffff);
  if (byteLength === 4) return __wjs2_bufWriteU32BE(this, value, offset, -0x80000000, 0x7fffffff);
  if (byteLength === 2) return __wjs2_bufWriteU16BE(this, value, offset, -0x8000, 0x7fff);
  if (byteLength === 1) return __wjs2_bufWriteU8(this, value, offset, -0x80, 0x7f);
  __wjs2_bufBoundsError(byteLength, 6, 'byteLength');
}
function __wjs2_bufWriteIntLE(value, offset, byteLength) {
  if (byteLength === 6) return __wjs2_bufWriteU48LE(this, value, offset, -0x800000000000, 0x7fffffffffff);
  if (byteLength === 5) return __wjs2_bufWriteU40LE(this, value, offset, -0x8000000000, 0x7fffffffff);
  if (byteLength === 3) return __wjs2_bufWriteU24LE(this, value, offset, -0x800000, 0x7fffff);
  if (byteLength === 4) return __wjs2_bufWriteU32LE(this, value, offset, -0x80000000, 0x7fffffff);
  if (byteLength === 2) return __wjs2_bufWriteU16LE(this, value, offset, -0x8000, 0x7fff);
  if (byteLength === 1) return __wjs2_bufWriteU8(this, value, offset, -0x80, 0x7f);
  __wjs2_bufBoundsError(byteLength, 6, 'byteLength');
}
Buffer.prototype.writeInt32LE = function (value, offset = 0) { return __wjs2_bufWriteU32LE(this, value, offset, -0x80000000, 0x7fffffff); };
Buffer.prototype.writeInt16LE = function (value, offset = 0) { return __wjs2_bufWriteU16LE(this, value, offset, -0x8000, 0x7fff); };
Buffer.prototype.writeInt8 = function (value, offset = 0) { return __wjs2_bufWriteU8(this, value, offset, -0x80, 0x7f); };
Buffer.prototype.writeIntBE = __wjs2_bufWriteIntBE;
Buffer.prototype.writeInt32BE = function (value, offset = 0) { return __wjs2_bufWriteU32BE(this, value, offset, -0x80000000, 0x7fffffff); };
Buffer.prototype.writeInt16BE = function (value, offset = 0) { return __wjs2_bufWriteU16BE(this, value, offset, -0x8000, 0x7fff); };
Buffer.prototype.readFloatLE = __wjs2_bufReadFloatLE;
Buffer.prototype.readFloatBE = __wjs2_bufReadFloatBE;
Buffer.prototype.readDoubleLE = __wjs2_bufReadDoubleLE;
Buffer.prototype.readDoubleBE = __wjs2_bufReadDoubleBE;
Buffer.prototype.writeFloatLE = __wjs2_bufWriteFloatLE;
Buffer.prototype.writeFloatBE = __wjs2_bufWriteFloatBE;
Buffer.prototype.writeDoubleLE = __wjs2_bufWriteDoubleLE;
Buffer.prototype.writeDoubleBE = __wjs2_bufWriteDoubleBE;
Buffer.prototype.asciiWrite = function (string, offset, length) { return __wjs2_bufAsciiWrite(this, string, offset, length); };
Buffer.prototype.base64Write = function (string, offset, length) { return __wjs2_bufBase64Write(this, string, offset, length); };
Buffer.prototype.base64urlWrite = function (string, offset, length) { return __wjs2_bufBase64urlWrite(this, string, offset, length); };
Buffer.prototype.latin1Write = function (string, offset, length) { return __wjs2_bufLatin1Write(this, string, offset, length); };
Buffer.prototype.hexWrite = function (string, offset, length) { return __wjs2_bufHexWrite(this, string, offset, length); };
Buffer.prototype.ucs2Write = function (string, offset, length) { return __wjs2_bufUcs2Write(this, string, offset, length); };
Buffer.prototype.utf8Write = function (string, offset, length) { return __wjs2_bufUtf8Write(this, string, offset, length); };
Buffer.prototype.asciiSlice = function (start, end) { return __wjs2_bufAsciiSlice(this, start, end); };
Buffer.prototype.base64Slice = function (start, end) { return __wjs2_bufB64Slice(this, start, end, false); };
Buffer.prototype.base64urlSlice = function (start, end) { return __wjs2_bufB64Slice(this, start, end, true); };
Buffer.prototype.latin1Slice = function (start, end) { return __wjs2_bufLatin1Slice(this, start, end); };
Buffer.prototype.hexSlice = function (start, end) { return __wjs2_bufHexSlice(this, start, end); };
Buffer.prototype.ucs2Slice = function (start, end) { return __wjs2_bufUcs2Slice(this, start, end); };
Buffer.prototype.utf8Slice = function (start, end) { return __wjs2_bufUtf8Slice(this, start, end); };
"#;
