//! WHATWG URL（URL/URLSearchParams/TextEncoder-Decoder/btoa-atob）（prelude 分域；拼接顺序见 mod.rs）。
pub const URL_JS: &str = r#"
// ---- Phase 3a: URL / URLSearchParams / TextEncoder/Decoder / base64 / crypto ----
globalThis.btoa = (s) => __wjs2_btoa(String(s));
globalThis.atob = (s) => __wjs2_atob(String(s));
const __wjs2_urlState = new WeakMap();
const __wjs2_uspState = new WeakMap();
function __wjs2_setHref(urlObj, newHref) {
  const st = __wjs2_urlState.get(urlObj);
  st.href = newHref;
  if (st.usp) {
    const search = __wjs2_url_get(newHref, "search");
    const q = search.startsWith("?") ? search.slice(1) : search;
    __wjs2_uspState.get(st.usp).pairs = q === "" ? [] : JSON.parse(__wjs2_usp_parse(q));
  }
}
function __wjs2_pushSearch(urlObj) {
  const st = __wjs2_urlState.get(urlObj);
  if (!st.usp) return;
  const q = __wjs2_usp_serialize(JSON.stringify(__wjs2_uspState.get(st.usp).pairs));
  st.href = __wjs2_url_set(st.href, "search", q === "" ? "" : "?" + q);
}
function __wjs2_uspFromUrl(urlObj) {
  const usp = new URLSearchParams("");
  __wjs2_uspState.get(usp).parent = urlObj;
  const st = __wjs2_urlState.get(urlObj);
  const search = __wjs2_url_get(st.href, "search");
  const q = search.startsWith("?") ? search.slice(1) : search;
  __wjs2_uspState.get(usp).pairs = q === "" ? [] : JSON.parse(__wjs2_usp_parse(q));
  st.usp = usp;
  return usp;
}
function __wjs2_uspTouch(usp) {
  const s = __wjs2_uspState.get(usp);
  if (s.parent) __wjs2_pushSearch(s.parent);
}
// node 口径：解析失败抛 TypeError "Invalid URL"，带 code=ERR_INVALID_URL、input（有 base 时另带 base）
// ——修前是原生层文案的普通 Error（"TypeError: Invalid URL: …"），按 code 断言的套件全挂。
function __wjs2_urlParseOrThrow(url, base) {
  try {
    return base === undefined ? __wjs2_url_parse(url) : __wjs2_url_parse(url, base);
  } catch {
    const e = new TypeError("Invalid URL");
    e.code = "ERR_INVALID_URL";
    e.input = url;
    if (base !== undefined) e.base = base;
    throw e;
  }
}
globalThis.URL = class URL {
  constructor(url, base) {
    const href = __wjs2_urlParseOrThrow(String(url), base === undefined ? undefined : String(base));
    __wjs2_urlState.set(this, { href, usp: null });
  }
  static canParse(url, base) {
    try {
      if (base === undefined) __wjs2_url_parse(String(url));
      else __wjs2_url_parse(String(url), String(base));
      return true;
    } catch { return false; }
  }
  get href() { return __wjs2_urlState.get(this).href; }
  set href(v) { __wjs2_setHref(this, __wjs2_urlParseOrThrow(String(v))); }
  get protocol() { return __wjs2_url_get(this.href, "protocol"); }
  set protocol(v) { __wjs2_setHref(this, __wjs2_url_set(this.href, "protocol", String(v))); }
  get username() { return __wjs2_url_get(this.href, "username"); }
  set username(v) { __wjs2_setHref(this, __wjs2_url_set(this.href, "username", String(v))); }
  get password() { return __wjs2_url_get(this.href, "password"); }
  set password(v) { __wjs2_setHref(this, __wjs2_url_set(this.href, "password", String(v))); }
  get host() { return __wjs2_url_get(this.href, "host"); }
  set host(v) { __wjs2_setHref(this, __wjs2_url_set(this.href, "host", String(v))); }
  get hostname() { return __wjs2_url_get(this.href, "hostname"); }
  set hostname(v) { __wjs2_setHref(this, __wjs2_url_set(this.href, "hostname", String(v))); }
  get port() { return __wjs2_url_get(this.href, "port"); }
  set port(v) { __wjs2_setHref(this, __wjs2_url_set(this.href, "port", String(v))); }
  get pathname() { return __wjs2_url_get(this.href, "pathname"); }
  set pathname(v) { __wjs2_setHref(this, __wjs2_url_set(this.href, "pathname", String(v))); }
  get search() { return __wjs2_url_get(this.href, "search"); }
  set search(v) { __wjs2_setHref(this, __wjs2_url_set(this.href, "search", String(v))); }
  get hash() { return __wjs2_url_get(this.href, "hash"); }
  set hash(v) { __wjs2_setHref(this, __wjs2_url_set(this.href, "hash", String(v))); }
  get origin() { return __wjs2_url_get(this.href, "origin"); }
  get searchParams() {
    const st = __wjs2_urlState.get(this);
    if (!st.usp) return __wjs2_uspFromUrl(this);
    return st.usp;
  }
  toString() { return this.href; }
  toJSON() { return this.href; }
};
globalThis.URLSearchParams = class URLSearchParams {
  constructor(init) {
    let pairs;
    if (init === undefined) pairs = [];
    else if (typeof init === "string") {
      const q = init.startsWith("?") ? init.slice(1) : init;
      pairs = q === "" ? [] : JSON.parse(__wjs2_usp_parse(q));
    } else if (Array.isArray(init)) pairs = init.map((p) => [String(p[0]), String(p[1])]);
    else if (typeof init === "object" && init !== null) {
      pairs = Object.entries(init).map(([k, v]) => [String(k), String(v)]);
    } else throw new TypeError("URLSearchParams: unsupported init");
    __wjs2_uspState.set(this, { pairs, parent: null });
  }
  get size() { return __wjs2_uspState.get(this).pairs.length; }
  append(n, v) { __wjs2_uspState.get(this).pairs.push([String(n), String(v)]); __wjs2_uspTouch(this); }
  delete(n, v) {
    n = String(n);
    const s = __wjs2_uspState.get(this);
    s.pairs = (v === undefined)
      ? s.pairs.filter((p) => p[0] !== n)
      : s.pairs.filter((p) => !(p[0] === n && p[1] === String(v)));
    __wjs2_uspTouch(this);
  }
  get(n) { const p = __wjs2_uspState.get(this).pairs.find((p) => p[0] === String(n)); return p ? p[1] : null; }
  getAll(n) { n = String(n); return __wjs2_uspState.get(this).pairs.filter((p) => p[0] === n).map((p) => p[1]); }
  has(n, v) {
    n = String(n);
    const ps = __wjs2_uspState.get(this).pairs;
    return (v === undefined)
      ? ps.some((p) => p[0] === n)
      : ps.some((p) => p[0] === n && p[1] === String(v));
  }
  set(n, v) {
    n = String(n); v = String(v);
    const s = __wjs2_uspState.get(this);
    let found = false;
    s.pairs = s.pairs.filter((p) => {
      if (p[0] !== n) return true;
      if (!found) { p[1] = v; found = true; return true; }
      return false;
    });
    if (!found) s.pairs.push([n, v]);
    __wjs2_uspTouch(this);
  }
  sort() {
    __wjs2_uspState.get(this).pairs.sort((a, b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0);
    __wjs2_uspTouch(this);
  }
  toString() { return __wjs2_usp_serialize(JSON.stringify(__wjs2_uspState.get(this).pairs)); }
  *keys() { for (const [k] of __wjs2_uspState.get(this).pairs) yield k; }
  *values() { for (const [, v] of __wjs2_uspState.get(this).pairs) yield v; }
  *entries() { for (const p of __wjs2_uspState.get(this).pairs) yield p; }
  [Symbol.iterator]() { return this.entries(); }
  forEach(cb, thisArg) { for (const [k, v] of __wjs2_uspState.get(this).pairs) cb.call(thisArg, v, k, this); }
};
globalThis.URLPattern = (function () {
  // URLPattern（plan3 §5 专项）：WHATWG 匹配语义由 `urlpattern` 轮子承载，
  // 此处只做 WebIDL 重载分流 + 取值 + 结果组装。错误自含（prelude 禁 import
  // errors 面，文案逐字对真机 node 26.8.2）。
  const state = new WeakMap();
  function brand(self) {
    const st = state.get(self);
    if (st === undefined) throw new TypeError("Illegal invocation");
    return st;
  }
  function coded(code, message) {
    const err = new TypeError(message);
    err.code = code;
    return err;
  }
  const COMPONENTS = ["protocol", "username", "password", "hostname", "port", "pathname", "search", "hash"];
  // init 字典读取：仅收字符串（数字/null 等一律视为缺席，真机口径）；
  // 属性读取走正常取值（用户 getter 抛错天然透传）。
  function readInit(obj) {
    const out = {};
    for (const k of COMPONENTS) {
      if (k in obj) {
        const v = obj[k];
        if (typeof v === "string") out[k] = v;
      }
    }
    return out;
  }
  // 构造输入归一：undefined/null → {}；string/对象直通；其余 ARG_TYPE。
  function normInput(input) {
    if (input === undefined || input === null) return {};
    if (typeof input === "string" || typeof input === "object") return input;
    throw coded("ERR_INVALID_ARG_TYPE", "Input must be an object or a string");
  }
  // exec/test 输入归一（文案不同，见 test-urlpattern-types）。
  function normMatchInput(input) {
    if (input === undefined || input === null) return {};
    if (typeof input === "string" || typeof input === "object") return input;
    throw coded("ERR_INVALID_ARG_TYPE", "URLPattern input needs to be a string or an object");
  }
  // base 归一：undefined → 缺席（null）；null → "null"（WebIDL 字符串化）；
  // string 直通；其余 ARG_TYPE。
  function normBase(base) {
    if (base === undefined) return null;
    if (base === null) return "null";
    if (typeof base === "string") return base;
    throw coded("ERR_INVALID_ARG_TYPE", "baseURL must be a string");
  }
  function matchPayload(input) {
    return (typeof input === "string") ? JSON.stringify(input) : JSON.stringify(readInit(input));
  }
  function buildResult(comps, inputEcho) {
    function comp(c) {
      const groups = {};
      for (const k of Object.keys(c.groups)) {
        const v = c.groups[k];
        groups[k] = (v === null) ? undefined : v;
      }
      return { groups, input: c.input };
    }
    // 键序钉死（test-urlpattern.js deepStrictEqual）：hash,hostname,inputs,
    // password,pathname,port,protocol,search,username。
    return {
      hash: comp(comps.hash),
      hostname: comp(comps.hostname),
      inputs: [inputEcho],
      password: comp(comps.password),
      pathname: comp(comps.pathname),
      port: comp(comps.port),
      protocol: comp(comps.protocol),
      search: comp(comps.search),
      username: comp(comps.username),
    };
  }
  function URLPattern(input, baseOrOptions, maybeOptions) {
    if (!(this instanceof URLPattern)) {
      throw coded("ERR_CONSTRUCT_CALL_REQUIRED", "Cannot call constructor without `new`");
    }
    let pattern, base = null, options;
    if (arguments.length >= 3 || typeof baseOrOptions === "string") {
      // 三参形或 (input, baseURL) 形（`undefined` base 亦 present，字符串化后解析）。
      pattern = normInput(input);
      base = (typeof baseOrOptions === "string") ? baseOrOptions : String(baseOrOptions);
      options = maybeOptions;
      if (options !== undefined && options !== null
        && (typeof options !== "object" && typeof options !== "function")) {
        throw coded("ERR_INVALID_ARG_TYPE", "options must be an object");
      }
      if (typeof pattern !== "string") {
        // 字典 + base → 构造失败（真机口径，非 OPERATION_FAILED）。
        throw coded("ERR_INVALID_URL_PATTERN", "Failed to construct URLPattern");
      }
    } else if (baseOrOptions !== undefined && baseOrOptions !== null
      && typeof baseOrOptions !== "object" && typeof baseOrOptions !== "function") {
      throw coded("ERR_INVALID_ARG_TYPE", "second argument must be a string or object");
    } else {
      pattern = normInput(input);
      options = baseOrOptions;
      if (options !== undefined && options !== null
        && (typeof options !== "object" && typeof options !== "function")) {
        throw coded("ERR_INVALID_ARG_TYPE", "options must be an object");
      }
    }
    const ignoreCase = !!(options && options.ignoreCase);
    const payload = (typeof pattern === "string") ? JSON.stringify(pattern) : JSON.stringify(readInit(pattern));
    let parsed;
    try {
      parsed = JSON.parse(__wjs2_urlpattern_parse(payload, base, ignoreCase));
    } catch {
      throw coded("ERR_INVALID_URL_PATTERN", "Failed to construct URLPattern");
    }
    state.set(this, { id: parsed.id, comps: parsed });
  }
  for (const k of COMPONENTS) {
    Object.defineProperty(URLPattern.prototype, k, {
      get() { return brand(this).comps[k]; },
      enumerable: true, configurable: true,
    });
  }
  Object.defineProperty(URLPattern.prototype, "hasRegExpGroups", {
    get() { return brand(this).comps.hasRegExpGroups; },
    enumerable: true, configurable: true,
  });
  URLPattern.prototype.test = function (input, base) {
    const st = brand(this);
    const norm = normMatchInput(input);
    const b = normBase(base);
    if (b !== null && typeof norm !== "string") {
      throw coded("ERR_OPERATION_FAILED", "Failed to test URLPattern");
    }
    return __wjs2_urlpattern_test(st.id, matchPayload(norm), b);
  };
  URLPattern.prototype.exec = function (input, base) {
    const st = brand(this);
    const norm = normMatchInput(input);
    const b = normBase(base);
    if (b !== null && typeof norm !== "string") {
      throw coded("ERR_OPERATION_FAILED", "Failed to exec URLPattern");
    }
    const raw = __wjs2_urlpattern_exec(st.id, matchPayload(norm), b);
    if (raw === null || raw === "null") return null;
    return buildResult(JSON.parse(raw), norm);
  };
  return URLPattern;
})();
globalThis.TextEncoder = class TextEncoder {
  get encoding() { return "utf-8"; }
  encode(s) { return __wjs2_te_encode(String(s === undefined ? "" : s)); }
  encodeInto(s, dest) { return JSON.parse(__wjs2_te_encode_into(String(s), dest)); }
};
globalThis.TextDecoder = class TextDecoder {
  #label; #fatal; #ignoreBOM; #streamId;
  constructor(label = "utf-8", options) {
    this.#label = __wjs2_td_canonical(String(label));
    this.#fatal = !!(options && options.fatal);
    this.#ignoreBOM = !!(options && options.ignoreBOM);
    this.#streamId = undefined;
  }
  get encoding() { return this.#label; }
  get fatal() { return this.#fatal; }
  get ignoreBOM() { return this.#ignoreBOM; }
  decode(input, options) {
    let view = input;
    if (view === undefined) view = undefined;
    else if (view instanceof ArrayBuffer) view = new Uint8Array(view);
    else if (typeof SharedArrayBuffer !== "undefined" && view instanceof SharedArrayBuffer) {
      throw new TypeError("TextDecoder.decode does not accept SharedArrayBuffer views yet");
    } else if (ArrayBuffer.isView(view) && !(view instanceof Uint8Array)) {
      view = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
    } else if (view !== undefined && view !== null && !(view instanceof Uint8Array)) {
      // R3b：非 BufferSource 即 ERR_INVALID_ARG_TYPE（真机文案逐字；consumers 套件点名码）。
      const e = new TypeError(`The "list" argument must be an instance of SharedArrayBuffer, ArrayBuffer or ArrayBufferView. Received ${view === null ? "null" : typeof view}`);
      e.code = "ERR_INVALID_ARG_TYPE";
      throw e;
    }
    if (options && options.stream) {
      // 流式：有状态解码器攒截断序列（跨片多字节/stateful 编码正确）；
      // 中途 view 缺省视为空片（只推进状态，不收尾）。
      if (this.#streamId === undefined) {
        this.#streamId = __wjs2_td_stream_open(this.#label, this.#fatal ? 1 : 0, this.#ignoreBOM ? 1 : 0);
      }
      return __wjs2_td_stream_feed(this.#streamId, view, 0);
    }
    if (this.#streamId !== undefined) {
      // 非流式调用即收尾（含攒下的截断序列），id 自动回收
      const out = __wjs2_td_stream_feed(this.#streamId, view, 1);
      this.#streamId = undefined;
      return out;
    }
    if (view === undefined) return __wjs2_td_decode(this.#label, 0, 0, undefined);
    return __wjs2_td_decode(this.#label, this.#fatal ? 1 : 0, this.#ignoreBOM ? 1 : 0, view);
  }
};
"#;
