//! `node:string_decoder`（Node `lib/string_decoder.js` 公开面，MIT）。
//!
//! Node 用 internalBinding('string_decoder') 原生解码器；本仓为纯 JS 重写：
//! - utf8：全局 TextDecoder 流式（引擎缓存截断序列，invalid → U+FFFD），
//!   尾部截断长度用预扫描（utf8TailLen）计算，供 lastChar/lastTotal 遗产面；
//! - utf16le/ucs2：hold 区模型（10f 真机口径）：尾 high 独留待配对、奇字节续 hold，
//!   解码 astral 配对；end() 刷偶部、孤字节静默丢弃；lastNeed/lastTotal/lastChar
//!   遗产面与真机一致（need = 奇1/偶hold2，4 字节零填充 lastChar）；
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
  base64url: 5, // 10f：真机收 base64url（解码面与 base64 同）
};

// 尾部截断 state（10f 逐行对齐 C++ 尾扫；旧实现把 missing/buffered 混为一数，
// 且 `<` 阈值在 F6–F8/C0–C1 边界与掩码不等价）。
function utf8TailState(view) {
  const n = view.length;
  if (n === 0) return { buffered: 0, total: 0 };
  if ((view[n - 1] & 0x80) === 0) return { buffered: 0, total: 0 };
  let buffered = 0;
  let missing = 0;
  for (let i = n - 1; ; i--) {
    buffered++;
    const b = view[i];
    if ((b & 0xC0) === 0x80) {
      if (buffered >= 4 || i === 0) { buffered = 0; break; }
    } else {
      if ((b & 0xE0) === 0xC0) missing = 2;
      else if ((b & 0xF0) === 0xE0) missing = 3;
      else if ((b & 0xF8) === 0xF0) missing = 4;
      else { buffered = 0; break; }
      if (buffered >= missing) { missing = 0; buffered = 0; }
      missing -= buffered;
      break;
    }
  }
  return { buffered, total: buffered + missing };
}

function StringDecoder(encoding) {
  this.encoding = normalizeEncoding(encoding ?? 'utf8');
  if (this.encoding === undefined) {
    throw new ERR_UNKNOWN_ENCODING(encoding);
  }
  this.__wjs2Id = encodingsMap[this.encoding];
  if (this.__wjs2Id === undefined) {
    throw new ERR_UNKNOWN_ENCODING(encoding);
  }
  this.__wjs2Decoder = undefined;       // TextDecoder（utf8 流式）
  this.__wjs2Store = new Uint8Array(4); // 截断字节缓存（lastChar 面）
  this.__wjs2Stored = 0;                // kBufferedBytes
  this.__wjs2Missing = 0;               // kMissingBytes（utf8 序列还差的字节数）
  this.__wjs2Base64Buf = undefined;     // base64 残组（1-2 字节）
}

StringDecoder.prototype.write = function write(buf) {
  if (typeof buf === 'string') return buf;
  if (!ArrayBuffer.isView(buf)) {
    throw new ERR_INVALID_ARG_TYPE('buf', ['Buffer', 'TypedArray', 'DataView'], buf);
  }
  if (this.__wjs2Id === undefined) {
    throw new ERR_INVALID_THIS('StringDecoder');
  }
  const view = buf instanceof Uint8Array ? buf :
    new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
  // 10f：V8 串长上限（`buffer.constants.MAX_STRING_LENGTH` 536870888；
  // 单写超限即抛，真机口径；多字节输出更短的过近似不纠，见注）。
  if (view.length > 536870888) {
    const err = new RangeError('Cannot create a string longer than 0x1fffffe8 characters');
    err.code = 'ERR_STRING_TOO_LONG';
    throw err;
  }
  switch (this.__wjs2Id) {
    case 0: return this.__wjs2WriteUtf8(view);
    case 1: return this.__wjs2WriteWide(view);
    case 2: return this.__wjs2WriteSingle(view, true);
    case 3: return this.__wjs2WriteSingle(view, false);
    case 4: return __wjs2_bufEncode(view, 'hex');
    case 5: return this.__wjs2WriteBase64(view);
  }
  throw new ERR_UNKNOWN_ENCODING(this.encoding);
};

StringDecoder.prototype.text = function text(buf, offset) {
  // 10f：`text(buf[, offset])` = 重置内部缓冲后按 write 语义解码（真机口径；
  // write 会续接上轮截断，text 则丢弃，套件点名）。
  if (!ArrayBuffer.isView(buf)) {
    throw new ERR_INVALID_ARG_TYPE('buf', ['Buffer', 'TypedArray', 'DataView'], buf);
  }
  this.__wjs2Decoder = undefined;
  this.__wjs2Stored = 0;
  this.__wjs2Missing = 0;
  this.__wjs2Base64Buf = undefined;
  const view = buf instanceof Uint8Array ? buf :
    new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
  const off = offset === undefined ? 0 : Number(offset) || 0;
  return this.write(view.subarray(off < 0 ? Math.max(0, view.length + off) : off));
};

StringDecoder.prototype.end = function end(buf) {  const ret = buf === undefined ? '' : this.write(buf);
  let flushed = '';
  switch (this.__wjs2Id) {
    case 0: {
      // utf8 刷 stash（10f：引擎按构造不再持有跨写字节，stash 经终结解码；
      // 旧实现 finalize 空引擎，截断永不落定）。
      if (this.__wjs2Decoder !== undefined) {
        if (this.__wjs2Stored > 0) {
          flushed = this.__wjs2Decoder.decode(this.__wjs2Store.subarray(0, this.__wjs2Stored));
        }
        this.__wjs2Decoder = undefined;
      }
      this.__wjs2Stored = 0;
      this.__wjs2Missing = 0;
      break;
    }
    case 1: {
      // utf16le flush（对齐 C++ FlushData）：先丢单个奇尾字节，再刷剩余偶部
      //（lone 代理直通）；旧实现刷 FFFD，套件点名。
      if (this.__wjs2Stored % 2 === 1) {
        this.__wjs2Stored--;
        this.__wjs2Missing--;
      }
      if (this.__wjs2Stored > 0) {
        flushed = __wjs2_decodeUtf16(this.__wjs2Store.subarray(0, this.__wjs2Stored));
        this.__wjs2Stored = 0;
        this.__wjs2Missing = 0;
      }
      break;
    }
    case 5: {
      const rest = this.__wjs2Base64Buf;
      if (rest !== undefined) {
        // 10f：base64url 输出 url-safe 无填充（真机口径）。
        flushed = __wjs2_bufEncode(rest, this.encoding === 'base64url' ? 'base64url' : 'base64');
        this.__wjs2Base64Buf = undefined;
        this.__wjs2Stored = 0;
      }
      break;
    }
  }
  return ret + flushed;
};

StringDecoder.prototype.__wjs2WriteUtf8 = function (view) {
  // utf8 真模型（10f 逐行对齐 C++ DecodeData；旧实现全量直喂引擎，
  // WHATWG 与 V8 对"截断/非法头"的持有语义不同，跨写即散）：
  // ① 有 pending 时先用新字节补齐——首个非续接字节出现即把已攒（含之前）
  // 经引擎刷出（V8 同口径，如 F6,9B+D1 → '��'）；② 只把完整前缀喂引擎，
  // 尾截断进 store（`utf8TailState`，C++ 尾扫逐行对齐）。
  // store 只增不 zero（lastChar 可见 stale 尾，真机同款，如 [D1,9B]）。
  if (this.__wjs2Decoder === undefined) {
    // R3b：BOM 不剥（真机 StringDecoder 原样返回 `\uFEFF`；WHATWG 默认剥，
    // preprocess/fs 流套件点名保留）。
    this.__wjs2Decoder = new TextDecoder('utf-8', { fatal: false, ignoreBOM: true });
  }
  let out = '';
  let data = view;
  if (this.__wjs2Missing > 0) {
    const need = this.__wjs2Missing;
    const lim = Math.min(data.length, need);
    let i = 0;
    while (i < lim && (data[i] & 0xC0) === 0x80) i++;
    if (i < lim) {
      for (let k = 0; k < i; k++) this.__wjs2Store[this.__wjs2Stored + k] = data[k];
      this.__wjs2Stored += i;
      this.__wjs2Missing = 0;
      data = data.subarray(i);
    } else {
      for (let k = 0; k < lim; k++) this.__wjs2Store[this.__wjs2Stored + k] = data[k];
      this.__wjs2Stored += lim;
      this.__wjs2Missing -= lim;
      data = data.subarray(lim);
    }
    if (this.__wjs2Missing === 0) {
      out += this.__wjs2Decoder.decode(this.__wjs2Store.subarray(0, this.__wjs2Stored), { stream: true });
      this.__wjs2Stored = 0;
    }
  }
  if (data.length === 0) return out;
  const tail = utf8TailState(data);
  const feed = data.subarray(0, data.length - tail.buffered);
  out += this.__wjs2Decoder.decode(feed, { stream: true });
  for (let k = 0; k < tail.buffered; k++) this.__wjs2Store[k] = data[data.length - tail.buffered + k];
  this.__wjs2Stored = tail.buffered;
  this.__wjs2Missing = tail.total - tail.buffered;
  return out;
};

StringDecoder.prototype.__wjs2WriteWide = function (view) {
  // utf16le 真模型（10f 逐行对齐 node `src/string_decoder.cc` UCS2 分支；
  // 旧实现两次猜错 hold 规则）：① 先用新字节补齐 pending（恰好补满才拼出
  // prepend；不够则全攒）；② 只看**本轮剩余**尾：奇→hold 1 字节，偶且末单元
  // 高代理→hold 2 字节；③ 余部解码（astral 配对）。Missing 恒由 Stored 导出
  // （0→0，奇→1，偶hold→2），与 C++ 的 (1,1)/(2,2) 一致。
  let data = view;
  let out = '';
  if (this.__wjs2Missing > 0) {
    const take = Math.min(data.length, this.__wjs2Missing);
    for (let i = 0; i < take; i++) this.__wjs2Store[this.__wjs2Stored + i] = data[i];
    this.__wjs2Stored += take;
    this.__wjs2Missing -= take;
    data = data.subarray(take);
    if (this.__wjs2Missing === 0) {
      out += __wjs2_decodeUtf16(this.__wjs2Store.subarray(0, this.__wjs2Stored));
      this.__wjs2Stored = 0;
    }
  }
  if (data.length === 0) return out;
  let buffered = 0;
  if (data.length % 2 === 1) {
    buffered = 1;
    this.__wjs2Missing = 1;
  } else if ((data[data.length - 1] & 0xFC) === 0xD8) {
    buffered = 2;
    this.__wjs2Missing = 2;
  }
  let body = data;
  if (buffered > 0) {
    body = data.subarray(0, data.length - buffered);
    this.__wjs2Store.set(data.subarray(data.length - buffered));
    this.__wjs2Stored = buffered;
  }
  out += __wjs2_decodeUtf16(body);
  return out;
};

// utf16le 解码（astral 配对；lone 代理/BMP 直通；不做有效性替换）。
function __wjs2_decodeUtf16(units) {
  let s = '';
  for (let i = 0; i + 1 < units.length; i += 2) {
    const u = units[i] | (units[i + 1] << 8);
    if (u >= 0xD800 && u <= 0xDBFF && i + 3 < units.length) {
      const v = units[i + 2] | (units[i + 3] << 8);
      if (v >= 0xDC00 && v <= 0xDFFF) {
        s += String.fromCodePoint(0x10000 + ((u - 0xD800) << 10) + (v - 0xDC00));
        i += 2;
        continue;
      }
    }
    s += String.fromCharCode(u);
  }
  return s;
}

StringDecoder.prototype.__wjs2WriteSingle = function (view, ascii) {
  this.__wjs2Stored = 0;
  let s = '';
  for (let i = 0; i < view.length; i += 0x8000) {
    s += ascii
      ? String.fromCharCode(...Array.from(view.subarray(i, i + 0x8000), (b) => b & 0x7F))
      : String.fromCharCode(...view.subarray(i, i + 0x8000));
  }
  return s;
};

StringDecoder.prototype.__wjs2WriteBase64 = function (view) {
  // 3 字节组缓存：完整组编码，残组缓存到 end()
  const buf = this.__wjs2Base64Buf;
  const all = buf === undefined ? view : Buffer.concat([buf, view]);
  const completeLen = all.length - (all.length % 3);
  if (completeLen === 0) {
    if (all.length > 0) {
      this.__wjs2Base64Buf = Buffer.from(all);
      this.__wjs2Stored = all.length;
    }
    return '';
  }
  const out = __wjs2_bufEncode(all.subarray(0, completeLen),
    this.encoding === 'base64url' ? 'base64url' : 'base64');
  const rest = all.subarray(completeLen);
  if (rest.length > 0) {
    this.__wjs2Base64Buf = Buffer.from(rest);
    this.__wjs2Stored = rest.length;
  } else {
    this.__wjs2Base64Buf = undefined;
    this.__wjs2Stored = 0;
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
      // 10f：真机回 4 字节零填充 Buffer（`.equals` 可用；旧实现变长裸片，套件点名）。
      const out = Buffer.alloc(4);
      out.set(this.__wjs2Store.subarray(0, this.__wjs2Stored));
      return out;
    },
  },
  lastNeed: {
    __proto__: null,
    configurable: true,
    enumerable: true,
    get() {
      return this.__wjs2Missing;
    },
  },
  lastTotal: {
    __proto__: null,
    configurable: true,
    enumerable: true,
    get() {
      return this.__wjs2Stored + this.__wjs2Missing;
    },
  },
});

export { StringDecoder };
export default { StringDecoder };
"#;
