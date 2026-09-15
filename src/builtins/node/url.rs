//! `node:url`（Node `lib/url.js` 部分面，MIT；plan 9j：vite 顶层 import）。
//!
//! 忠实面（真机 node 26.8.2 逐项对过码与文案）：`URL`/`URLSearchParams`
//! （全局重导出）、`fileURLToPath`（string｜URL → posix 路径；类型错
//! `ERR_INVALID_ARG_TYPE`、串解不出 URL 即 `ERR_INVALID_URL`、非 file scheme
//! 即 `ERR_INVALID_URL_SCHEME`、host 非空非 localhost 即
//! `ERR_INVALID_FILE_URL_HOST`）、`pathToFileURL`（相对按 `process.cwd()` 解）。
//! 10a：legacy 面（`parse/format/resolve/resolveObject` + `Url` 类 +
//! `domainToASCII/domainToUnicode` + `urlToHttpOptions`，lib/url.js 口径移植）。
//!
//! 偏差（记档）：win32 盘符只做 `/X:/` 前导剥离；legacy `resolve` 为
//! `resolveObject` 完整移植（RFC 3986 合并，非 WHATWG 近似）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/url.js (partial face; see module docs for deviations).
import { resolve as resolvePath } from 'node:path';
import { parse as qsParse, stringify as qsStringify } from 'node:querystring';
// 10f：punycode 懒加载（顶层 import 会在 require('node:url') 时连带求值
// node:punycode，提前触发 DEP0040；真机同款懒加载，test-punycode.js 点名）。
import { createRequire } from 'node:module';
const __urlRequire = createRequire('node:url');
let __puny = null;
function __punyToASCII(s) {
  if (__puny === null) __puny = __urlRequire('node:punycode');
  return __puny.toASCII(s);
}
function __punyToUnicode(s) {
  if (__puny === null) __puny = __urlRequire('node:punycode');
  return __puny.toUnicode(s);
}
import errors from 'node:internal/errors';

const {
  codes: {
    ERR_INVALID_ARG_TYPE,
    ERR_INVALID_FILE_URL_HOST,
    ERR_INVALID_URL,
    ERR_INVALID_URL_SCHEME,
  },
} = errors;

const URL_ = globalThis.URL;
const URLSearchParams_ = globalThis.URLSearchParams;

function fileURLToPath(path) {
  if (typeof path === 'string') {
    try {
      path = new URL_(path);
    } catch {
      throw new ERR_INVALID_URL(path);
    }
  } else if (!(path instanceof URL_)) {
    throw new ERR_INVALID_ARG_TYPE('path', ['string', 'URL'], path);
  }
  if (path.protocol !== 'file:') {
    throw new ERR_INVALID_URL_SCHEME('file');
  }
  if (path.host !== '' && path.host !== 'localhost') {
    throw new ERR_INVALID_FILE_URL_HOST(process.platform);
  }
  let pathname;
  try {
    pathname = decodeURIComponent(path.pathname);
  } catch {
    throw new ERR_INVALID_URL(path.href);
  }
  // win32 盘符前导剥离（/C:/x → C:/x；posix 原样，偏差记档）。
  if (/^\/[A-Za-z]:\//.test(pathname)) {
    pathname = pathname.slice(1);
  }
  return pathname;
}

function pathToFileURL(path) {
  if (typeof path !== 'string') {
    throw new ERR_INVALID_ARG_TYPE('path', ['string'], path);
  }
  const abs = resolvePath(process.cwd(), path);
  const encoded = abs.split('/').map((seg) => encodeURIComponent(seg)).join('/');
  return new URL_('file://' + encoded);
}

// ---- legacy 面（10a，Node lib/url.js 口径移植；导出见文件尾）----

const hostlessProtocol = { __proto__: null, javascript: true, 'javascript:': true };
const slashedProtocol = {
  __proto__: null,
  http: true, https: true, ftp: true, gopher: true, file: true,
  'http:': true, 'https:': true, 'ftp:': true, 'gopher:': true, 'file:': true,
};
const protocolPattern = /^([a-z0-9.+-]+:)/i;
const portPattern = /:[0-9]*$/;
const urlKeys = ['protocol', 'slashes', 'auth', 'host', 'port', 'hostname',
  'hash', 'search', 'query', 'pathname', 'path', 'href'];

class Url {
  constructor() {
    this.protocol = null;
    this.slashes = null;
    this.auth = null;
    this.host = null;
    this.port = null;
    this.hostname = null;
    this.hash = null;
    this.search = null;
    this.query = null;
    this.pathname = null;
    this.path = null;
    this.href = null;
  }

  parse(url, parseQueryString, slashesDenoteHost) {
    if (typeof url !== 'string') {
      throw new ERR_INVALID_ARG_TYPE('url', 'string', url);
    }
    let rest = url;
    // protocol
    const proto = protocolPattern.exec(rest);
    let protoStr = null;
    if (proto) {
      protoStr = proto[0];
      this.protocol = protoStr.toLowerCase();
      rest = rest.slice(protoStr.length);
    }
    // slashes
    if (slashesDenoteHost || protoStr || /^\/\/[^@/]+@[^@/]+/.test(rest)) {
      const slashes = rest.slice(0, 2) === '//';
      if (slashes && !(protoStr && hostlessProtocol[protoStr])) {
        rest = rest.slice(2);
        this.slashes = true;
      }
    }
    // host (+ auth)
    if (!hostlessProtocol[protoStr] && (this.slashes || (protoStr && !slashedProtocol[protoStr]))) {
      let hostEnd = -1;
      for (let i = 0; i < rest.length; i++) {
        const c = rest[i];
        if (c === '/' || c === '?' || c === '#') { hostEnd = i; break; }
      }
      let hostPart;
      if (hostEnd === -1) {
        hostPart = rest;
        rest = '';
      } else {
        hostPart = rest.slice(0, hostEnd);
        rest = rest.slice(hostEnd);
      }
      const atSign = hostPart.lastIndexOf('@');
      if (atSign !== -1) {
        this.auth = decodeURIComponent(hostPart.slice(0, atSign));
        hostPart = hostPart.slice(atSign + 1);
      }
      this.host = hostPart;
      if (!hostPart) {
        this.hostname = '';
      } else {
        let port = portPattern.exec(hostPart);
        if (port) {
          port = port[0];
          if (port !== ':') this.port = port.slice(1);
          hostPart = hostPart.slice(0, hostPart.length - port.length);
        }
        if (hostPart) {
          let hostname = hostPart.toLowerCase();
          // ipv6 去括号（format 侧补回，真机口径）；括号形跳过 IDNA。
          if (hostname[0] === '[' && hostname[hostname.length - 1] === ']') {
            hostname = hostname.slice(1, -1);
          } else if (/[^\x00-\x7f]/.test(hostname)) {
            // IDNA：非 ASCII 标号逐个 punycode。
            hostname = hostname.split('.').map((label) =>
              /[^\x00-\x7f]/.test(label) ? __punyToASCII(label) : label).join('.');
          }
          this.hostname = hostname;
        }
      }
    }
    // hash
    const hashIndex = rest.indexOf('#');
    if (hashIndex !== -1) {
      this.hash = rest.slice(hashIndex);
      rest = rest.slice(0, hashIndex);
    } else {
      this.hash = null;
    }
    // query
    const qmIndex = rest.indexOf('?');
    if (qmIndex !== -1) {
      this.search = rest.slice(qmIndex);
      const q = rest.slice(qmIndex + 1);
      this.query = parseQueryString ? qsParse(q) : q;
      rest = rest.slice(0, qmIndex);
    } else {
      this.search = null;
      this.query = null;
    }
    // pathname（空串保持 null；slash+host 下补 '/'，真机口径）
    if (rest) this.pathname = rest;
    if (this.slashes && this.hostname && !this.pathname) {
      this.pathname = '/';
    }
    if (this.pathname || this.search) {
      this.path = (this.pathname || '') + (this.search || '');
    }
    this.href = this.format();
  }

  format() {
    return urlFormat(this);
  }

  resolve(relative) {
    return this.resolveObject(urlParse(relative, false, true)).format();
  }

  resolveObject(relative) {
    if (typeof relative === 'string') {
      relative = urlParse(relative, false, true);
    }
    const result = new Url();
    for (const key of urlKeys) result[key] = this[key];
    // hash 恒被覆盖
    result.hash = relative.hash;
    if (relative.href === '') {
      result.href = result.format();
      return result;
    }
    // //foo/bar 形：协议沿用 source
    if (relative.slashes && !relative.protocol) {
      for (const key of urlKeys) {
        if (key !== 'protocol') result[key] = relative[key];
      }
      if (slashedProtocol[result.protocol] && result.hostname && !result.pathname) {
        result.path = result.pathname = '/';
      }
      result.href = result.format();
      return result;
    }
    if (relative.protocol && relative.protocol !== result.protocol) {
      if (!slashedProtocol[relative.protocol]) {
        for (const key of urlKeys) result[key] = relative[key];
        result.href = result.format();
        return result;
      }
      result.protocol = relative.protocol;
      if (!relative.host && !hostlessProtocol[relative.protocol]) {
        const relPath = (relative.pathname || '').split('/');
        while (relPath.length && !(result.host = relPath.shift()));
        if (!result.host) result.host = '';
        if (!result.hostname) result.hostname = '';
        if (relPath[0] !== '') relPath.unshift('');
        if (relPath.length < 2) relPath.unshift('');
        result.pathname = relPath.join('/');
      } else {
        result.pathname = relative.pathname;
      }
      result.search = relative.search;
      result.query = relative.query;
      result.host = relative.host || '';
      result.auth = relative.auth;
      result.hostname = relative.hostname || relative.host;
      result.port = relative.port;
      if (result.pathname || result.search) {
        result.path = (result.pathname || '') + (result.search || '');
      }
      result.slashes = result.slashes || relative.slashes;
      result.href = result.format();
      return result;
    }
    const isSourceAbs = result.pathname && result.pathname.charCodeAt(0) === 47;
    const isRelAbs = relative.host ||
      (relative.pathname && relative.pathname.charCodeAt(0) === 47);
    const mustEndAbs = isRelAbs || isSourceAbs || (result.host && relative.pathname);
    let srcPath = (result.pathname && result.pathname.split('/')) || [];
    const relPath = (relative.pathname && relative.pathname.split('/')) || [];
    if (isRelAbs) {
      // 绝对路径：host/port 沿用 source（真机：/g 保 host+port+auth，只换 path；
      // search/query 照置空），只有相对带 host 才覆盖。
      result.host = relative.host || result.host;
      result.hostname = relative.hostname || result.hostname;
      result.search = relative.search;
      result.query = relative.query;
      result.port = relative.port || result.port;
      srcPath = relPath;
    } else if (relPath.length !== 0) {
      if (!srcPath.length) srcPath = [];
      srcPath.pop();
      for (const seg of relPath) srcPath.push(seg);
      result.search = relative.search;
      result.query = relative.query;
    } else if (relative.search !== null && relative.search !== undefined) {
      // 纯 query
      srcPath = result.pathname ? result.pathname.split('/') : null;
      result.search = relative.search;
      result.query = relative.query;
      if (result.pathname !== null) {
        result.path = result.pathname + result.search;
      } else {
        result.path = result.search;
      }
      result.href = result.format();
      return result;
    }
    if (!srcPath || !srcPath.length) {
      result.pathname = null;
      if (result.search) {
        result.path = '/' + result.search;
      } else {
        result.path = null;
      }
      result.href = result.format();
      return result;
    }
    // 末段 . / .. / 空 → 尾斜杠
    const last = srcPath[srcPath.length - 1];
    const hasTrailingSlash = ((result.host || relative.host || srcPath.length > 1) &&
      (last === '.' || last === '..')) || last === '';
    // 去单点、双点回父级
    let up = 0;
    for (let i = srcPath.length - 1; i >= 0; i--) {
      const seg = srcPath[i];
      if (seg === '.') {
        srcPath.splice(i, 1);
      } else if (seg === '..') {
        srcPath.splice(i, 1);
        up++;
      } else if (up) {
        srcPath.splice(i, 1);
        up--;
      }
    }
    // 非绝对形保留前导 ..（mustEndAbs 时 removeAllDots 同真，不恢复）
    if (!mustEndAbs) {
      for (; up > 0; up--) srcPath.unshift('..');
    }
    if (mustEndAbs && srcPath[0] !== '' &&
        (!srcPath[0] || srcPath[0].charCodeAt(0) !== 47)) {
      srcPath.unshift('');
    }
    result.pathname = srcPath.join('/');
    if (hasTrailingSlash && result.pathname.slice(-1) !== '/') {
      result.pathname += '/';
    }
    if (result.pathname !== null || result.search !== null) {
      result.path =
        (result.pathname ? result.pathname : '') +
        (result.search ? result.search : '');
    }
    result.auth = relative.auth || result.auth;
    result.slashes = result.slashes || relative.slashes;
    result.href = result.format();
    return result;
  }
}

function urlParse(url, parseQueryString, slashesDenoteHost) {
  if (url instanceof Url) return url;
  const u = new Url();
  u.parse(url, parseQueryString, slashesDenoteHost);
  return u;
}

function urlFormat(urlObject) {
  if (typeof urlObject === 'string') {
    urlObject = urlParse(urlObject);
  } else if (typeof urlObject !== 'object' || urlObject === null) {
    throw new ERR_INVALID_ARG_TYPE('urlObject', ['Object', 'string'], urlObject);
  }
  let auth = urlObject.auth || '';
  if (auth) {
    auth = encodeURIComponent(auth);
    auth = auth.replace(/%3A/i, ':');
    auth += '@';
  }
  let protocol = urlObject.protocol || '';
  let pathname = urlObject.pathname || '';
  let hash = urlObject.hash || '';
  let host = false;
  let query = '';
  if (urlObject.host) {
    host = auth + urlObject.host;
  } else if (urlObject.hostname !== undefined && urlObject.hostname !== null) {
    let hostname = urlObject.hostname;
    // ipv6 裸地址补括号（已带括号的不重复，真机三探针全同）。
    if (hostname.indexOf(':') !== -1 && hostname.charCodeAt(0) !== 91) {
      hostname = '[' + hostname + ']';
    }
    host = auth + hostname;
    if (urlObject.port) host += ':' + urlObject.port;
  }
  if (urlObject.query !== null && typeof urlObject.query === 'object') {
    query = qsStringify(urlObject.query);
  }
  if (urlObject.search) {
    query = urlObject.search;
  } else if (query) {
    query = '?' + query;
  }
  if (protocol && protocol.slice(-1) !== ':') protocol += ':';
  if (urlObject.slashes ||
      ((!protocol || slashedProtocol[protocol]) && host !== false)) {
    host = '//' + (host || '');
    if (pathname && pathname.charCodeAt(0) !== 47) pathname = '/' + pathname;
  } else if (!host) {
    host = '';
  }
  if (hash && hash.charCodeAt(0) !== 35) hash = '#' + hash;
  if (query && query.charCodeAt(0) !== 63) query = '?' + query;
  pathname = pathname.replace(/[?#]/g, (m) => encodeURIComponent(m));
  query = query.replace('#', '%23');
  return protocol + host + pathname + query + hash;
}

function urlResolve(source, relative) {
  return urlParse(source, false, true).resolve(relative);
}

function urlResolveObject(source, relative) {
  return urlParse(source, false, true).resolveObject(relative);
}

function domainToASCII(domain) {
  if (typeof domain !== 'string') {
    throw new ERR_INVALID_ARG_TYPE('domain', 'string', domain);
  }
  return __punyToASCII(domain);
}

function domainToUnicode(domain) {
  if (typeof domain !== 'string') {
    throw new ERR_INVALID_ARG_TYPE('domain', 'string', domain);
  }
  return __punyToUnicode(domain);
}

function urlToHttpOptions(url) {
  // 真机口径（26.8.2）：string 直接拒（ERR_INVALID_ARG_TYPE 'object'），
  // WHATWG URL 走映射，legacy Url 对象走透传（port 串转数）。
  if (url instanceof URL_) {
    const options = {
      protocol: url.protocol,
      hostname: (typeof url.hostname === 'string' && url.hostname.startsWith('[')) ?
        url.hostname.slice(1, -1) :
        url.hostname,
      hash: url.hash,
      search: url.search,
      pathname: url.pathname,
      path: (url.pathname || '') + (url.search || ''),
      href: url.href,
    };
    if (url.port !== '') {
      options.port = Number(url.port);
    }
    if (url.username || url.password) {
      options.auth =
        decodeURIComponent(url.username) + ':' + decodeURIComponent(url.password);
    }
    return options;
  }
  if (typeof url !== 'object' || url === null) {
    throw new ERR_INVALID_ARG_TYPE('url', 'object', url);
  }
  const options = { ...url };
  if (typeof options.port === 'string' && options.port !== '') {
    options.port = Number(options.port);
  }
  return options;
}

export {
  URL_ as URL, URLSearchParams_ as URLSearchParams, fileURLToPath, pathToFileURL,
  urlParse as parse, urlFormat as format, urlResolve as resolve,
  urlResolveObject as resolveObject, Url,
  domainToASCII, domainToUnicode, urlToHttpOptions,
};
export default {
  URL: URL_, URLSearchParams: URLSearchParams_, fileURLToPath, pathToFileURL,
  parse: urlParse, format: urlFormat, resolve: urlResolve,
  resolveObject: urlResolveObject, Url,
  domainToASCII, domainToUnicode, urlToHttpOptions,
};
"#;
