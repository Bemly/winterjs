//! Buffer 全局出口（bufApi/decode/encode/transfer/pool）（prelude 分域；拼接顺序见 mod.rs）。
pub const BUFFER_API_JS: &str = r#"
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
"#;
