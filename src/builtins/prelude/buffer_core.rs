//! Buffer 基础（错误工厂/kMaxLength/TA 原生/base64 头）（prelude 分域；拼接顺序见 mod.rs）。
pub const BUFFER_CORE_JS: &str = r#"
// ---- Buffer 全局（10f：node lib/buffer.js v26.8.2 + lib/internal/buffer.js 逐字移植，MIT）----
const __wjs_bufTAFill = Uint8Array.prototype.fill; // 原生 fill（Buffer.prototype.fill 会遮蔽，内部一律走它）
// native 片（slice/write 静态、_compare、indexOf 族）以纯 JS 重实现；
// pool 不做（直接分配，Buffer.poolSize 仅作值）；allocUnsafe 恒零填（无未初始化
// 内存暴露，记档）；kMaxLength/kStringMaxLength 取引擎实测边界（SM
// MaxStringLength=2^30-2；node 26 64-bit kMaxLength=MAX_SAFE_INTEGER）。
// 错误工厂自含（prelude 不能 import node:internal/errors；消息逐字对齐，
// 实现摘自 errors.rs 同源移植）。__wjs_bufDecode/Encode 为 string_decoder
// 依赖的全局 helper，原样保留。
const kMaxLength = 9007199254740991; // node 26 64-bit 实测 = MAX_SAFE_INTEGER
const kStringMaxLength = 1073741822; // SM MaxStringLength = 2^30 - 2（实测探针钉住，
                                     // 套件门：repeat(MAX+1) 须抛、repeat(MAX) 须过）
(() => {
function __wjs_bufFormatList(array, type = 'and') {
  switch (array.length) {
    case 0: return '';
    case 1: return `${array[0]}`;
    case 2: return `${array[0]} ${type} ${array[1]}`;
    case 3: return `${array[0]}, ${array[1]}, ${type} ${array[2]}`;
    default: return `${array.slice(0, -1).join(', ')}, ${type} ${array[array.length - 1]}`;
  }
}
// node util.inspect 最小替身（错误消息 Received 兜底；depth=-1 不展开嵌套）
function __wjs_bufInspect(value, depth = -1) {
  if (value === null) return 'null';
  if (value === undefined) return 'undefined';
  const t = typeof value;
  if (t === 'string') {
    if (value.length > 28) value = value.slice(0, 25) + '...';
    return `'${value}'`;
  }
  if (t === 'number' || t === 'boolean' || t === 'bigint' || t === 'symbol') return String(value);
  if (t === 'function') return value.name ? `[Function: ${value.name}]` : '[Function (anonymous)]';
  if (t !== 'object') return String(value);
  const ctor = value.constructor?.name;
  if (Array.isArray(value)) {
    if (depth === -1) return `[Array(${value.length})]`;
    const items = value.slice(0, 7).map((v) => __wjs_bufInspect(v, depth - 1));
    if (value.length > 7) items.push(`... ${value.length - 7} more item${value.length - 7 > 1 ? 's' : ''}`);
    return `[ ${items.join(', ')} ]`;
  }
  if (value instanceof Error) {
    return ctor === 'Error' ? (value.stack || String(value)).split('\n')[0] : `${ctor || 'Error'}: ${value.message}`;
  }
  if (value instanceof Date) return isNaN(value.getTime()) ? 'Invalid Date' : value.toISOString();
  const keys = Object.keys(value);
  if (depth === -1) {
    if (keys.length > 0) return '[Object]';
    if (Object.getPrototypeOf(value) === null) return '[Object: null prototype] {}';
    return ctor === 'Object' || ctor === undefined ? '{}' : `${ctor} {}`;
  }
  const proto = Object.getPrototypeOf(value);
  const head = proto === null ? '[Object: null prototype] ' : (ctor === 'Object' || ctor === undefined ? '' : `${ctor} `);
  if (keys.length === 0) return `${head}{}`;
  const parts = keys.slice(0, 7).map((k) => `${k}: ${__wjs_bufInspect(value[k], depth - 1)}`);
  if (keys.length > 7) parts.push(`... ${keys.length - 7} more item${keys.length - 7 > 1 ? 's' : ''}`);
  return `${head}{ ${parts.join(', ')} }`;
}
function __wjs_bufSpecificType(value) {
  if (value === null) return 'null';
  if (value === undefined) return 'undefined';
  const type = typeof value;
  switch (type) {
    case 'bigint': return `type bigint (${value}n)`;
    case 'number':
      if (value === 0) {
        return 1 / value === -Infinity ? 'type number (-0)' : 'type number (0)';
      } else if (value !== value) {
        return 'type number (NaN)';
      } else if (value === Infinity) {
        return 'type number (Infinity)';
      } else if (value === -Infinity) {
        return 'type number (-Infinity)';
      }
      return `type number (${value})`;
    case 'boolean': return value ? 'type boolean (true)' : 'type boolean (false)';
    case 'symbol': return `type symbol (${String(value)})`;
    case 'function': return `function ${value.name}`;
    case 'object': {
      const name = value.constructor?.name;
      if (typeof name === 'string' && name !== '') return `an instance of ${name}`;
      return `${__wjs_bufInspect(value)}`;
    }
    case 'string':
      if (value.length > 28) value = `${value.slice(0, 25)}...`;
      if (value.indexOf("'") === -1) return `type string ('${value}')`;
      return `type string (${JSON.stringify(value)})`;
    default: {
      let inspected = __wjs_bufInspect(value, 0);
      if (inspected.length > 28) inspected = `${inspected.slice(0, 25)}...`;
      return `type ${type} (${inspected})`;
    }
  }
}
// ERR_INVALID_ARG_TYPE（errors.rs 同源；prelude 自含）
// 跨 realm ArrayBuffer 品牌检查（vm.runInNewContext 产物 instanceof 不可靠；
// Object.prototype.toString tag 全 realm 稳定）
function __wjs_bufIsAnyAB(v) {
  if (v instanceof ArrayBuffer) return true;
  if (typeof SharedArrayBuffer !== 'undefined' && v instanceof SharedArrayBuffer) return true;
  const tag = Object.prototype.toString.call(v);
  return tag === '[object ArrayBuffer]' || tag === '[object SharedArrayBuffer]';
}
// detached 容错视图（detached 视空；isAscii/isUtf8 套件口径）
function __wjs_bufAsU8(v) {
  try {
    if (v instanceof ArrayBuffer || Object.prototype.toString.call(v) === '[object ArrayBuffer]') return new Uint8Array(v);
    if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  } catch {
    return new Uint8Array(0);
  }
  return null;
}
const __wjs_bufKTypes = ['string', 'function', 'number', 'object', 'Function', 'Object', 'boolean', 'bigint', 'symbol'];
const __wjs_bufClassRegExp = /^[A-Z][a-zA-Z0-9]*$/;
function __wjs_bufArgTypeErr(name, expected, actual) {
  if (!Array.isArray(expected)) expected = [expected];
  let msg = 'The ';
  if (name.endsWith(' argument')) {
    msg += `${name} `;
  } else {
    msg += `"${name}" ${name.includes('.') ? 'property' : 'argument'} `;
  }
  msg += 'must be ';
  const types = [];
  const instances = [];
  const other = [];
  for (const value of expected) {
    if (__wjs_bufKTypes.includes(value)) types.push(value.toLowerCase());
    else if (__wjs_bufClassRegExp.test(value)) instances.push(value);
    else other.push(value);
  }
  if (instances.length > 0) {
    const pos = types.indexOf('object');
    if (pos !== -1) {
      types.splice(pos, 1);
      instances.push('Object');
    }
  }
  if (types.length > 0) {
    msg += `${types.length > 1 ? 'one of type' : 'of type'} ${__wjs_bufFormatList(types, 'or')}`;
    if (instances.length > 0 || other.length > 0) msg += ' or ';
  }
  if (instances.length > 0) {
    msg += `an instance of ${__wjs_bufFormatList(instances, 'or')}`;
    if (other.length > 0) msg += ' or ';
  }
  if (other.length > 0) {
    if (other.length > 1) {
      msg += `one of ${__wjs_bufFormatList(other, 'or')}`;
    } else {
      if (other[0].toLowerCase() !== other[0]) msg += 'an ';
      msg += `${other[0]}`;
    }
  }
  msg += `. Received ${__wjs_bufSpecificType(actual)}`;
  const e = new TypeError(msg);
  e.code = 'ERR_INVALID_ARG_TYPE';
  return e;
}
function __wjs_bufNumSep(val) {
  let res = '';
  let i = val.length;
  const start = val[0] === '-' ? 1 : 0;
  for (; i >= start + 4; i -= 3) res = `_${val.slice(i - 3, i)}${res}`;
  return `${val.slice(0, i)}${res}`;
}
function __wjs_bufRangeErr(str, range, input, replaceDefault = false) {
  let msg = replaceDefault ? str : `The value of "${str}" is out of range.`;
  let received;
  if (Number.isInteger(input) && Math.abs(input) > 2 ** 32) {
    received = __wjs_bufNumSep(String(input));
  } else if (typeof input === 'bigint') {
    received = String(input);
    if (input > 2n ** 32n || input < -(2n ** 32n)) received = __wjs_bufNumSep(received);
    received += 'n';
  } else {
    received = __wjs_bufInspect(input);
  }
  msg += ` It must be ${range}. Received ${received}`;
  const e = new RangeError(msg);
  e.code = 'ERR_OUT_OF_RANGE';
  return e;
}
function __wjs_bufOobErr(name = undefined) {
  const msg = name ? `"${name}" is outside of buffer bounds`
                   : 'Attempt to access memory outside buffer bounds';
  const e = new RangeError(msg);
  e.code = 'ERR_BUFFER_OUT_OF_BOUNDS';
  return e;
}
function __wjs_bufArgValueErr(name, value, reason = 'is invalid') {
  let inspected = __wjs_bufInspect(value);
  if (inspected.length > 128) inspected = `${inspected.slice(0, 128)}...`;
  const type = name.includes('.') ? 'property' : 'argument';
  const e = new TypeError(`The ${type} '${name}' ${reason}. Received ${inspected}`);
  e.code = 'ERR_INVALID_ARG_VALUE';
  return e;
}
function __wjs_bufEncErr(encoding) {
  const e = new TypeError(`Unknown encoding: ${encoding}`);
  e.code = 'ERR_UNKNOWN_ENCODING';
  return e;
}
function __wjs_bufSizeErr(bits) {
  const e = new RangeError(`Buffer size must be a multiple of ${bits}`);
  e.code = 'ERR_INVALID_BUFFER_SIZE';
  return e;
}
function __wjs_bufMissingArgsErr(...args) {
  let msg = 'The ';
  const wrapped = args.map((a) => Array.isArray(a) ? a.map((x) => `"${x}"`).join(' or ') : `"${a}"`);
  msg += `${__wjs_bufFormatList(wrapped)} argument${args.length > 1 ? 's' : ''} must be specified`;
  const e = new TypeError(msg);
  e.code = 'ERR_MISSING_ARGS';
  return e;
}
// validators（validators.js 原文口径）
function __wjs_bufValidateNumber(value, name, min = undefined, max) {
  if (typeof value !== 'number') throw __wjs_bufArgTypeErr(name, 'number', value);
  if ((min != null && value < min) || (max != null && value > max) ||
      ((min != null || max != null) && Number.isNaN(value))) {
    throw __wjs_bufRangeErr(
      name,
      `${min != null ? `>= ${min}` : ''}${min != null && max != null ? ' && ' : ''}${max != null ? `<= ${max}` : ''}`,
      value);
  }
}
function __wjs_bufValidateInteger(value, name, min = -Number.MAX_SAFE_INTEGER, max = Number.MAX_SAFE_INTEGER) {
  if (typeof value !== 'number') throw __wjs_bufArgTypeErr(name, 'number', value);
  if (!Number.isInteger(value)) throw __wjs_bufRangeErr(name, 'an integer', value);
  if (value < min || value > max) throw __wjs_bufRangeErr(name, `>= ${min} && <= ${max}`, value);
}
function __wjs_bufValidateString(value, name) {
  if (typeof value !== 'string') throw __wjs_bufArgTypeErr(name, 'string', value);
}
function __wjs_bufValidateArray(value, name, minLength = 0) {
  if (!Array.isArray(value)) throw __wjs_bufArgTypeErr(name, 'Array', value);
  if (value.length < minLength) {
    throw __wjs_bufArgValueErr(name, value, `must have a length of at least ${minLength}`);
  }
}
function __wjs_bufValidateBuffer(buffer, name = 'buffer') {
  if (!ArrayBuffer.isView(buffer)) {
    throw __wjs_bufArgTypeErr(name, ['Buffer', 'TypedArray', 'DataView'], buffer);
  }
}
// normalizeEncoding（internal/util.js 原文）
function __wjs_bufNormalizeEncoding(enc) {
  if (enc == null || enc === 'utf8' || enc === 'utf-8') return 'utf8';
  return __wjs_bufSlowCases(enc);
}
function __wjs_bufSlowCases(enc) {
  switch (enc.length) {
    case 4:
      if (enc === 'UTF8') return 'utf8';
      if (enc === 'ucs2' || enc === 'UCS2') return 'utf16le';
      enc = enc.toLowerCase();
      if (enc === 'utf8') return 'utf8';
      if (enc === 'ucs2') return 'utf16le';
      break;
    case 3:
      if (enc === 'hex' || enc === 'HEX' || enc.toLowerCase() === 'hex') return 'hex';
      break;
    case 5:
      if (enc === 'ascii') return 'ascii';
      if (enc === 'ucs-2') return 'utf16le';
      if (enc === 'UTF-8') return 'utf8';
      if (enc === 'ASCII') return 'ascii';
      if (enc === 'UCS-2') return 'utf16le';
      enc = enc.toLowerCase();
      if (enc === 'utf-8') return 'utf8';
      if (enc === 'ascii') return 'ascii';
      if (enc === 'ucs-2') return 'utf16le';
      break;
    case 6:
      if (enc === 'base64') return 'base64';
      if (enc === 'latin1' || enc === 'binary') return 'latin1';
      if (enc === 'BASE64') return 'base64';
      if (enc === 'LATIN1' || enc === 'BINARY') return 'latin1';
      enc = enc.toLowerCase();
      if (enc === 'base64') return 'base64';
      if (enc === 'latin1' || enc === 'binary') return 'latin1';
      break;
    case 7:
      if (enc === 'utf16le' || enc === 'UTF16LE' || enc.toLowerCase() === 'utf16le') return 'utf16le';
      break;
    case 8:
      if (enc === 'utf-16le' || enc === 'UTF-16LE' || enc.toLowerCase() === 'utf-16le') return 'utf16le';
      break;
    case 9:
      if (enc === 'base64url' || enc === 'BASE64URL' || enc.toLowerCase() === 'base64url') return 'base64url';
      break;
    default:
      if (enc === '') return 'utf8';
  }
}
function __wjs_bufDecode(str, enc) {
  enc = String(enc || "utf8").toLowerCase().replace(/[-_]/g, "");
  if (enc === "utf8" || enc === "utf-8") return new TextEncoder().encode(str);
  if (enc === "hex") {
    const s = String(str).replace(/\s+/g, "");
    if (s.length % 2 !== 0) throw new TypeError("Invalid hex string");
    const out = new Uint8Array(s.length / 2);
    for (let i = 0; i < out.length; i++) {
      const v = parseInt(s.slice(i * 2, i * 2 + 2), 16);
      if (Number.isNaN(v)) throw new TypeError("Invalid hex string");
      out[i] = v;
    }
    return out;
  }
  if (enc === "base64" || enc === "base64url") {
    let s = String(str).replace(/-/g, "+").replace(/_/g, "/");
    while (s.length % 4) s += "=";
    const bin = atob(s);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }
  if (enc === "latin1" || enc === "binary") {
    const s = String(str);
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i) & 255;
    return out;
  }
  if (enc === "ascii") {
    // node 实测：ascii 写/解码不掩码（读侧 __wjs_bufEncode 掩 0x7F）
    const s = String(str);
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i) & 255;
    return out;
  }
  if (enc === "ucs2" || enc === "utf16le" || enc === "utf16") {
    const s = String(str);
    const out = new Uint8Array(s.length * 2);
    for (let i = 0; i < s.length; i++) {
      const c = s.charCodeAt(i);
      out[i * 2] = c & 255; out[i * 2 + 1] = (c >> 8) & 255;
    }
    return out;
  }
  throw new TypeError(`Unknown encoding: ${enc}`);
}
function __wjs_bufEncode(u8, enc) {
  enc = String(enc || "utf8").toLowerCase().replace(/[-_]/g, "");
  if (enc === "utf8" || enc === "utf-8") return new TextDecoder().decode(u8);
  if (enc === "hex") return [...u8].map((x) => x.toString(16).padStart(2, "0")).join("");
  if (enc === "base64") {
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return btoa(s);
  }
  if (enc === "base64url") {
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }
  if (enc === "latin1" || enc === "binary") {
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return s;
  }
  if (enc === "ascii") {
    // 10f：ascii 解码掩 0x7F（真机口径；旧实现与 latin1 同形漏掩，套件 fuzz 点名）。
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) {
      const part = u8.subarray(i, i + 0x8000);
      const masked = new Uint8Array(part.length);
      for (let j = 0; j < part.length; j++) masked[j] = part[j] & 127;
      s += String.fromCharCode(...masked);
    }
    return s;
  }
  if (enc === "ucs2" || enc === "utf16le" || enc === "utf16") {
    let s = "";
    for (let i = 0; i + 1 < u8.length; i += 2) s += String.fromCharCode(u8[i] | (u8[i + 1] << 8));
    return s;
  }
  throw new TypeError(`Unknown encoding: ${enc}`);
}
// ---- encoding write/slice 静态实现（node C++ binding 的 JS 重实现）----
function __wjs_bufUtf8WriteStatic(buf, string, offset, length) {
  const bytes = new TextEncoder().encode(string);
  let k = Math.min(bytes.length, length);
  // 截断必须落在字符边界（下一字节须为 lead byte，套件 "split char" 门）
  while (k > 0 && k < bytes.length && (bytes[k] & 0xC0) === 0x80) k--;
  if (k > 0) buf.set(bytes.subarray(0, k), offset);
  return k;
}
function __wjs_bufAsciiWriteStatic(buf, string, offset, length) {
  // node 实测：ascii 写不掩码（'über' → [0xFC]）；只有读/slice 掩 0x7F
  let n = 0;
  const L = string.length;
  for (; n < length && n < L; n++) buf[offset + n] = string.charCodeAt(n) & 0xFF;
  return n;
}
function __wjs_bufLatin1WriteStatic(buf, string, offset, length) {
  let n = 0;
  const L = string.length;
  for (; n < length && n < L; n++) buf[offset + n] = string.charCodeAt(n) & 0xFF;
  return n;
}
function __wjs_bufUcs2WriteStatic(buf, string, offset, length) {
  let n = 0;
  const L = string.length;
  for (let i = 0; i < L && n + 1 < length; i++) {
    const c = string.charCodeAt(i);
    buf[offset + n] = c & 255;
    buf[offset + n + 1] = (c >> 8) & 255;
    n += 2;
  }
  return n;
}
function __wjs_bufHexVal(c) {
  if (c >= 48 && c <= 57) return c - 48;
  if (c >= 97 && c <= 102) return c - 87;
  if (c >= 65 && c <= 70) return c - 55;
  return -1;
}
function __wjs_bufHexWriteStatic(buf, string, offset, length) {
  let n = 0;
  for (let i = 0; i + 1 < string.length && n < length; i += 2) {
    const a = __wjs_bufHexVal(string.charCodeAt(i));
    const b = __wjs_bufHexVal(string.charCodeAt(i + 1));
    if (a === -1 || b === -1) break;
    buf[offset + n++] = a * 16 + b;
  }
  return n;
}
const __wjs_bufB64Std = new Int8Array(128).fill(-1);
{
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  for (let i = 0; i < chars.length; i++) __wjs_bufB64Std[chars.charCodeAt(i)] = i;
}
const __wjs_bufB64Url = new Int8Array(128).fill(-1);
{
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
  for (let i = 0; i < chars.length; i++) __wjs_bufB64Url[chars.charCodeAt(i)] = i;
}
const __wjs_bufB64Lenient = new Int8Array(128).fill(-1);
{
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/-_';
  for (let i = 0; i < chars.length; i++) {
    const c = chars.charCodeAt(i);
    if (c === 0x2B || c === 0x2D) __wjs_bufB64Lenient[c] = 62;      // '+' '-' → 62
    else if (c === 0x2F || c === 0x5F) __wjs_bufB64Lenient[c] = 63; // '/' '_' → 63
    else __wjs_bufB64Lenient[c] = i % 64;
  }
}
function __wjs_bufB64WriteStatic(buf, string, offset, length, url) {
  // node simdutf 口径：解码双字母表均收（'base64' 也接受 -_，反之亦然）；
  // 只有输出（slice）按 flag 选字母表/是否补 padding
  const dec = __wjs_bufB64Lenient;
  let n = 0;
  let carry = -1; // 组内进度：-1 组头；0..2 = 已攒字节数
  let group = 0;
  for (let i = 0; i < string.length && n < length; i++) {
    const c = string.charCodeAt(i);
    if (c === 0x3D) break; // '=' 终止
    if (c >= 128) continue; // 非 ASCII 忽略（node simdutf 忽略无效字符）
    const v = dec[c];
    if (v === -1) continue;
    group = (group << 6) | v;
    carry++;
    if (carry === 3) {
      buf[offset + n++] = (group >> 16) & 255;
      if (n < length) buf[offset + n++] = (group >> 8) & 255;
      if (n < length) buf[offset + n++] = group & 255;
      carry = -1;
      group = 0;
    }
  }
  if (carry === 1 && n < length) buf[offset + n++] = (group >> 4) & 255;
  else if (carry === 2 && n < length) {
    buf[offset + n++] = (group >> 10) & 255;
    if (n < length) buf[offset + n++] = (group >> 2) & 255;
  }
  return n;
}
function __wjs_bufB64Slice(u8, start, end, url) {
  const chars = url
    ? 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_'
    : 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  let s = '';
  for (let i = start; i < end; i += 3) {
    const b0 = u8[i];
    const b1 = i + 1 < end ? u8[i + 1] : 0;
    const b2 = i + 2 < end ? u8[i + 2] : 0;
    const n = Math.min(3, end - i);
    s += chars[b0 >> 2];
    s += chars[((b0 & 3) << 4) | (b1 >> 4)];
    if (n > 1) s += chars[((b1 & 15) << 2) | (b2 >> 6)]; else if (!url) s += '=';
    if (n > 2) s += chars[b2 & 63]; else if (!url) s += '=';
  }
  return s;
}
function __wjs_bufB64ByteLength(str, bytes) {
  if (str.charCodeAt(bytes - 1) === 0x3D) bytes--;
  if (bytes > 1 && str.charCodeAt(bytes - 1) === 0x3D) bytes--;
  return (bytes * 3) >>> 2;
}
"#;
