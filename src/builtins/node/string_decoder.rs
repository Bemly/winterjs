//! `node:string_decoder`（Node `lib/string_decoder.js` 公开面，MIT）。
//!
//! Node 用 internalBinding('string_decoder') 原生解码器；本仓为纯 JS 重写：
//! - utf8：全局 TextDecoder 流式（引擎缓存截断序列，invalid → U+FFFD），
//!   尾部截断长度用预扫描（utf8TailLen）计算，供 lastChar/lastTotal 遗产面；
//! - utf16le/ucs2/latin1/binary/ascii：单/双字节直映（无截断态）；
//! - hex：逐字节 hex；base64：3 字节组缓存，end() 补齐 padding（残组 → U+FFFD
//!   语义不存在，Node 对 base64 end() 是直接编码剩余字节）。
//! 遗产面 lastNeed/lastTotal/lastChar 语义保持（kMissingBytes/kBufferedBytes）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/string_decoder.js (pure-JS; see module docs).
import { normalizeEncoding } from 'node:internal/util';
import errors from 'node:internal/errors';
const {
  codes: {
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
    ERR_INVALID_THIS: { HideStackFramesError: ERR_INVALID_THIS },
    ERR_UNKNOWN_ENCODING: { HideStackFramesError: ERR_UNKNOWN_ENCODING },
  },
} = errors;

// 编码 id（legacy 面用）：0 utf8 / 1 utf16le / 2 ascii / 3 latin1 / 4 hex / 5 base64
const encodingsMap = {
  utf8: 0, utf16le: 1, ucs2: 1, ascii: 2, latin1: 3, binary: 3, hex: 4, base64: 5,
};

// 尾部不完整序列字节数（0-3）：从尾向前找首字节，按其期望长度判定截断。
function utf8TailLen(view) {
  const n = view.length;
  if (n === 0) return 0;
  let i = n - 1;
  let back = 0;
  while (i >= 0 && back < 4) {
    const b = view[i];
    if ((b & 0xC0) !== 0x80) {
      let expect;
      if (b < 0x80) expect = 1;
      else if (b < 0xE0) expect = 2;
      else if (b < 0xF0) expect = 3;
      else if (b < 0xF5) expect = 4;
      else expect = 1; // 非法首字节：引擎按 FFFD 单字节处理
      return expect > back + 1 ? back + 1 : 0;
    }
    i--;
    back++;
  }
  return 0;
}

function StringDecoder(encoding) {
  this.encoding = normalizeEncoding(encoding ?? 'utf8');
  if (this.encoding === undefined) {
    throw new ERR_UNKNOWN_ENCODING(encoding);
  }
  this.__wjsId = encodingsMap[this.encoding];
  if (this.__wjsId === undefined) {
    throw new ERR_UNKNOWN_ENCODING(encoding);
  }
  this.__wjsDecoder = undefined;       // TextDecoder（utf8 流式）
  this.__wjsStore = new Uint8Array(4); // 截断字节缓存（lastChar 面）
  this.__wjsStored = 0;                // kBufferedBytes
  this.__wjsMissing = 0;               // kMissingBytes（utf8 序列还差的字节数）
  this.__wjsBase64Buf = undefined;     // base64 残组（1-2 字节）
}

StringDecoder.prototype.write = function write(buf) {
  if (typeof buf === 'string') return buf;
  if (!ArrayBuffer.isView(buf)) {
    throw new ERR_INVALID_ARG_TYPE('buf', ['Buffer', 'TypedArray', 'DataView'], buf);
  }
  if (this.__wjsId === undefined) {
    throw new ERR_INVALID_THIS('StringDecoder');
  }
  const view = buf instanceof Uint8Array ? buf :
    new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
  switch (this.__wjsId) {
    case 0: return this.__wjsWriteUtf8(view);
    case 1: return this.__wjsWriteWide(view);
    case 2: return this.__wjsWriteSingle(view, true);
    case 3: return this.__wjsWriteSingle(view, false);
    case 4: return __wjs_bufEncode(view, 'hex');
    case 5: return this.__wjsWriteBase64(view);
  }
  throw new ERR_UNKNOWN_ENCODING(this.encoding);
};

StringDecoder.prototype.end = function end(buf) {
  const ret = buf === undefined ? '' : this.write(buf);
  let flushed = '';
  switch (this.__wjsId) {
    case 0: {
      // 截断序列由 TextDecoder 收尾 flush（WHATWG finalize → U+FFFD），
      // 手工再补会双份；这里只清遗产面状态。
      if (this.__wjsDecoder !== undefined) {
        flushed = this.__wjsDecoder.decode();
        this.__wjsDecoder = undefined;
      }
      this.__wjsStored = 0;
      this.__wjsMissing = 0;
      break;
    }
    case 1: {
      // utf16le：奇数截断尾字节 → flush U+FFFD（Node 语义）
      if (this.__wjsStored > 0) {
        flushed = '\uFFFD';
        this.__wjsStored = 0;
      }
      break;
    }
    case 5: {
      const rest = this.__wjsBase64Buf;
      if (rest !== undefined) {
        flushed = __wjs_bufEncode(rest, 'base64'); // btoa 自动补 padding
        this.__wjsBase64Buf = undefined;
        this.__wjsStored = 0;
      }
      break;
    }
  }
  return ret + flushed;
};

StringDecoder.prototype.__wjsWriteUtf8 = function (view) {
  if (this.__wjsDecoder === undefined) {
    this.__wjsDecoder = new TextDecoder('utf-8', { fatal: false });
  }
  // 引擎流式路径自己缓存截断序列——全量 feed；
  // 预扫描只为 lastChar/lastTotal 遗产面记录截断字节。
  const out = this.__wjsDecoder.decode(view, { stream: true });
  const tail = utf8TailLen(view);
  this.__wjsStored = tail;
  this.__wjsMissing = tail === 0 ? 0 : tail;
  for (let i = 0; i < tail; i++) this.__wjsStore[i] = view[view.length - tail + i];
  return out;
};

StringDecoder.prototype.__wjsWriteWide = function (view) {
  // utf16le：奇数截断尾字节交引擎外缓存——本仓 __wjs_bufEncode 逐对解码时
  // 丢弃孤立尾字节；为 keep lastChar 面，尾字节记入 store 并在 end() flush FFFD。
  let full = view;
  if (view.length % 2 === 1) {
    const head = view.subarray(0, view.length - 1);
    this.__wjsStore[0] = view[view.length - 1];
    this.__wjsStored = 1;
    full = head;
  } else {
    this.__wjsStored = 0;
  }
  return __wjs_bufEncode(full, 'utf16le');
};

StringDecoder.prototype.__wjsWriteSingle = function (view, ascii) {
  this.__wjsStored = 0;
  let s = '';
  for (let i = 0; i < view.length; i += 0x8000) {
    s += ascii
      ? String.fromCharCode(...Array.from(view.subarray(i, i + 0x8000), (b) => b & 0x7F))
      : String.fromCharCode(...view.subarray(i, i + 0x8000));
  }
  return s;
};

StringDecoder.prototype.__wjsWriteBase64 = function (view) {
  // 3 字节组缓存：完整组编码，残组缓存到 end()
  const buf = this.__wjsBase64Buf;
  const all = buf === undefined ? view : Buffer.concat([buf, view]);
  const completeLen = all.length - (all.length % 3);
  if (completeLen === 0) {
    if (all.length > 0) {
      this.__wjsBase64Buf = Buffer.from(all);
      this.__wjsStored = all.length;
    }
    return '';
  }
  const out = __wjs_bufEncode(all.subarray(0, completeLen), 'base64');
  const rest = all.subarray(completeLen);
  if (rest.length > 0) {
    this.__wjsBase64Buf = Buffer.from(rest);
    this.__wjsStored = rest.length;
  } else {
    this.__wjsBase64Buf = undefined;
    this.__wjsStored = 0;
  }
  return out;
};

// 遗产面（lastNeed/lastTotal/lastChar）
Object.defineProperties(StringDecoder.prototype, {
  lastChar: {
    __proto__: null,
    configurable: true,
    enumerable: true,
    get() {
      return this.__wjsStore.subarray(0, this.__wjsStored);
    },
  },
  lastNeed: {
    __proto__: null,
    configurable: true,
    enumerable: true,
    get() {
      return this.__wjsMissing;
    },
  },
  lastTotal: {
    __proto__: null,
    configurable: true,
    enumerable: true,
    get() {
      return this.__wjsStored + this.__wjsMissing;
    },
  },
});

export { StringDecoder };
export default { StringDecoder };
"#;
