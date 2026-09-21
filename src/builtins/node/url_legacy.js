// ---- legacy 面（10a→10f：Node lib/url.js 原文直译；套件 parse/format 深表对拍）----

const hostlessProtocol = { __proto__: null, javascript: true, 'javascript:': true };
const unsafeProtocol = { __proto__: null, javascript: true, 'javascript:': true };
const slashedProtocol = { __proto__: null,
  ftp: true, 'ftp:': true, file: true, 'file:': true, gopher: true, 'gopher:': true,
  http: true, 'http:': true, https: true, 'https:': true, ws: true, 'ws:': true,
  wss: true, 'wss:': true,
};
const protocolPattern = /^[a-z0-9.+-]+:/i;
const portPattern = /:[0-9]*$/;
const urlKeys = ['protocol', 'slashes', 'auth', 'host', 'port', 'hostname',
  'hash', 'search', 'query', 'pathname', 'path', 'href'];
const hostPattern = /^\/\/[^@/]+@[^@/]+/;
// Special case for a simple path URL
const simplePathPattern = /^(\/\/?(?!\/)[^?\s]*)(\?[^\s]*)?$/;
const hostnameMaxLen = 255;

// This prevents some common spoofing bugs due to our use of IDNA toASCII.
const forbiddenHostChars = /[\0\t\n\r #%/:<>?@[\\\]^|]/;
// For IPv6, permit '[', ']', and ':'.
const forbiddenHostCharsIpv6 = /[\0\t\n\r #%/<>?@\\^|]/;

function isIpv6Hostname(hostname) {
  return hostname.charCodeAt(0) === 91 /* [ */ &&
         hostname.charCodeAt(hostname.length - 1) === 93 /* ] */;
}

// Automatically escape all delimiters and unwise characters from RFC 2396.
// Also escape single quotes in case of an XSS attack.
const escapedCodes = [
  /* 0 - 9 */ '', '', '', '', '', '', '', '', '', '%09',
  /* 10 - 19 */ '%0A', '', '', '%0D', '', '', '', '', '', '',
  /* 20 - 29 */ '', '', '', '', '', '', '', '', '', '',
  /* 30 - 39 */ '', '', '%20', '', '%22', '', '', '', '', '%27',
  /* 40 - 49 */ '', '', '', '', '', '', '', '', '', '',
  /* 50 - 59 */ '', '', '', '', '', '', '', '', '', '',
  /* 60 - 69 */ '%3C', '', '%3E', '', '', '', '', '', '', '',
  /* 70 - 79 */ '', '', '', '', '', '', '', '', '', '',
  /* 80 - 89 */ '', '', '', '', '', '', '', '', '', '',
  /* 90 - 99 */ '', '', '%5C', '', '%5E', '', '%60', '', '', '',
  /* 100 - 109 */ '', '', '', '', '', '', '', '', '', '',
  /* 110 - 119 */ '', '', '', '', '', '', '', '', '', '',
  /* 120 - 125 */ '', '', '', '%7B', '%7C', '%7D',
];
function autoEscapeStr(rest) {
  let escaped = '';
  let lastEscapedPos = 0;
  for (let i = 0; i < rest.length; ++i) {
    const escapedChar = escapedCodes[rest.charCodeAt(i)];
    if (escapedChar) {
      if (i > lastEscapedPos) escaped += rest.slice(lastEscapedPos, i);
      escaped += escapedChar;
      lastEscapedPos = i + 1;
    }
  }
  if (lastEscapedPos !== rest.length) escaped += rest.slice(lastEscapedPos);
  return escaped;
}

// These characters do not need escaping (node noEscapeAuth 表原文)：
// ! - . _ ~ ' ( ) * : digits alpha
const noEscapeAuth = new Int8Array([
  0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x00 - 0x0F
  0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0x10 - 0x1F
  0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, // 0x20 - 0x2F
  1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, // 0x30 - 0x3F
  0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0x40 - 0x4F
  1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, // 0x50 - 0x5F
  0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 0x60 - 0x6F
  1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, // 0x70 - 0x7F
]);
function encodeStrAuth(auth) {
  // node encodeStr 口径：代理对整体按 4 字节 UTF-8 编码（逐码元
  // encodeURIComponent 会对孤立代理抛 URIError，套件 surrogate-in-auth 现形）。
  let out = '';
  let last = 0;
  for (let i = 0; i < auth.length;) {
    const code = auth.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff && i + 1 < auth.length) {
      const next = auth.charCodeAt(i + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        if (i > last) out += auth.slice(last, i);
        out += encodeURIComponent(auth.slice(i, i + 2));
        i += 2;
        last = i;
        continue;
      }
    }
    if (code < 0x80) {
      if (!noEscapeAuth[code]) {
        if (i > last) out += auth.slice(last, i);
        out += '%' + code.toString(16).toUpperCase().padStart(2, '0');
        last = i + 1;
      }
      i++;
    } else {
      if (i > last) out += auth.slice(last, i);
      out += encodeURIComponent(auth[i]);
      last = i + 1;
      i++;
    }
  }
  if (last < auth.length) out += auth.slice(last);
  return out;
}

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

    // Copy chrome, IE, opera backslash-handling behavior.
    // Back slashes before the query string get converted to forward slashes
    let hasHash = false;
    let hasAt = false;
    let start = -1;
    let end = -1;
    let rest = '';
    let lastPos = 0;
    for (let i = 0, inWs = false, split = false; i < url.length; ++i) {
      const code = url.charCodeAt(i);

      // Find first and last non-whitespace characters for trimming
      const isWs = code < 33 ||
                   code === 0xA0 /* NO_BREAK_SPACE */ ||
                   code === 0xFEFF /* ZERO_WIDTH_NOBREAK_SPACE */;
      if (start === -1) {
        if (isWs) continue;
        lastPos = start = i;
      } else if (inWs) {
        if (!isWs) {
          end = -1;
          inWs = false;
        }
      } else if (isWs) {
        end = i;
        inWs = true;
      }

      // Only convert backslashes while we haven't seen a split character
      if (!split) {
        switch (code) {
          case 64 /* @ */:
            hasAt = true;
            break;
          case 35 /* # */:
            hasHash = true;
            // Fall through
          case 63 /* ? */:
            split = true;
            break;
          case 92 /* \ */:
            if (i - lastPos > 0) rest += url.slice(lastPos, i);
            rest += '/';
            lastPos = i + 1;
            break;
        }
      } else if (!hasHash && code === 35 /* # */) {
        hasHash = true;
      }
    }

    // Check if string was non-empty (including strings with only whitespace)
    if (start !== -1) {
      if (lastPos === start) {
        // We didn't convert any backslashes
        if (end === -1) {
          if (start === 0) rest = url;
          else rest = url.slice(start);
        } else {
          rest = url.slice(start, end);
        }
      } else if (end === -1 && lastPos < url.length) {
        rest += url.slice(lastPos);
      } else if (end !== -1 && lastPos < end) {
        rest += url.slice(lastPos, end);
      }
    }

    if (!slashesDenoteHost && !hasHash && !hasAt) {
      // Try fast path regexp
      const simplePath = simplePathPattern.exec(rest);
      if (simplePath) {
        this.path = rest;
        this.href = rest;
        this.pathname = simplePath[1];
        if (simplePath[2]) {
          this.search = simplePath[2];
          if (parseQueryString) {
            this.query = qsParse(this.search.slice(1));
          } else {
            this.query = this.search.slice(1);
          }
        } else if (parseQueryString) {
          this.search = null;
          this.query = { __proto__: null };
        }
        return this;
      }
    }

    let proto = protocolPattern.exec(rest);
    let lowerProto;
    if (proto) {
      proto = proto[0];
      lowerProto = proto.toLowerCase();
      this.protocol = lowerProto;
      rest = rest.slice(proto.length);
    }

    // Figure out if it's got a host
    let slashes;
    if (slashesDenoteHost || proto || hostPattern.test(rest)) {
      slashes = rest.charCodeAt(0) === 47 /* / */ &&
                rest.charCodeAt(1) === 47 /* / */;
      if (slashes && !(proto && hostlessProtocol[lowerProto])) {
        rest = rest.slice(2);
        this.slashes = true;
      }
    }

    if (!hostlessProtocol[lowerProto] &&
        (slashes || (proto && !slashedProtocol[proto]))) {

      // there's a hostname.
      // the first instance of /, ?, ;, or # ends the host.
      // If there is an @ in the hostname, then non-host chars *are* allowed
      // to the left of the last @ sign, unless some host-ending character
      // comes *before* the @-sign.
      let hostEnd = -1;
      let atSign = -1;
      let nonHost = -1;
      for (let i = 0; i < rest.length; ++i) {
        switch (rest.charCodeAt(i)) {
          case 9: case 10: case 13:
            // WHATWG URL removes tabs, newlines, and carriage returns.
            rest = rest.slice(0, i) + rest.slice(i + 1);
            i -= 1;
            break;
          case 32: /* space */
          case 34: /* " */
          case 37: /* % */
          case 39: /* ' */
          case 59: /* ; */
          case 60: /* < */
          case 62: /* > */
          case 92: /* \ */
          case 94: /* ^ */
          case 96: /* ` */
          case 123: /* { */
          case 124: /* | */
          case 125: /* } */
            // Characters that are never ever allowed in a hostname from RFC 2396
            if (nonHost === -1) nonHost = i;
            break;
          case 35: /* # */
          case 47: /* / */
          case 63: /* ? */
            // Find the first instance of any host-ending characters
            if (nonHost === -1) nonHost = i;
            hostEnd = i;
            break;
          case 64: /* @ */
            atSign = i;
            nonHost = -1;
            break;
        }
        if (hostEnd !== -1) break;
      }
      start = 0;
      if (atSign !== -1) {
        this.auth = decodeURIComponent(rest.slice(0, atSign));
        start = atSign + 1;
      }
      if (nonHost === -1) {
        this.host = rest.slice(start);
        rest = '';
      } else {
        this.host = rest.slice(start, nonHost);
        rest = rest.slice(nonHost);
      }

      // pull out port.
      this.parseHost();

      // We've indicated that there is a hostname,
      // so even if it's empty, it has to be present.
      if (typeof this.hostname !== 'string') this.hostname = '';

      const hostname = this.hostname;

      // If hostname begins with [ and ends with ] assume it's an IPv6 address.
      const ipv6Hostname = isIpv6Hostname(hostname);

      // validate a little.
      if (!ipv6Hostname) {
        rest = getHostname(this, rest, hostname, url);
      }

      if (this.hostname.length > hostnameMaxLen) {
        this.hostname = '';
      } else {
        // Hostnames are always lower case.
        this.hostname = this.hostname.toLowerCase();
      }

      if (this.hostname !== '') {
        if (ipv6Hostname) {
          if (forbiddenHostCharsIpv6.test(this.hostname)) {
            throw Object.assign(new ERR_INVALID_URL(url), { input: url });
          }
        } else {
          // IDNA: NFKC（NFKD 分解 + 规范组合——'bücher' 须回到预组合 ü，
          // punycode 才得 'xn--bcher-kva'，套件 parse-format 现形）+ ignored
          // 码点（软连字符 U+00AD）删除 + 违禁校验 + punycode（node
          // toASCII/UTS46 口径——映射后为空或含违禁字符即抛 ERR_INVALID_URL，
          // 套件 badIDNA 29 码点 + 软连字符逐项）。
          this.hostname = this.hostname.split('.').map((label) => {
            if (!/[^\x00-\x7f]/.test(label)) return label;
            const mapped = label.normalize('NFKC').replaceAll('\u00AD', '');
            if (mapped === '' || forbiddenHostChars.test(mapped)) {
              throw Object.assign(new ERR_INVALID_URL(url), { input: url });
            }
            return __punyToASCII(mapped);
          }).join('.');

          // Prevent two potential routes of hostname spoofing.
          if (this.hostname === '' || forbiddenHostChars.test(this.hostname)) {
            throw Object.assign(new ERR_INVALID_URL(url), { input: url });
          }
        }
      }

      const p = this.port ? ':' + this.port : '';
      const h = this.hostname || '';
      this.host = h + p;

      // strip [ and ] from the hostname; the host field still retains them
      if (ipv6Hostname) {
        this.hostname = this.hostname.slice(1, -1);
        if (rest.charCodeAt(0) !== 47 /* / */) {
          rest = '/' + rest;
        }
      }
    }

    // Now rest is set to the post-host stuff.
    // Chop off any delim chars.
    if (!unsafeProtocol[lowerProto]) {
      // First, make 100% sure that any "autoEscape" chars get escaped.
      rest = autoEscapeStr(rest);
    }

    let questionIdx = -1;
    let hashIdx = -1;
    for (let i = 0; i < rest.length; ++i) {
      const code = rest.charCodeAt(i);
      if (code === 35 /* # */) {
        this.hash = rest.slice(i);
        hashIdx = i;
        break;
      } else if (code === 63 /* ? */ && questionIdx === -1) {
        questionIdx = i;
      }
    }

    if (questionIdx !== -1) {
      if (hashIdx === -1) {
        this.search = rest.slice(questionIdx);
        this.query = rest.slice(questionIdx + 1);
      } else {
        this.search = rest.slice(questionIdx, hashIdx);
        this.query = rest.slice(questionIdx + 1, hashIdx);
      }
      if (parseQueryString) {
        this.query = qsParse(this.query);
      }
    } else if (parseQueryString) {
      // No query string, but parseQueryString still requested
      this.search = null;
      this.query = { __proto__: null };
    }

    const useQuestionIdx =
      questionIdx !== -1 && (hashIdx === -1 || questionIdx < hashIdx);
    const firstIdx = useQuestionIdx ? questionIdx : hashIdx;
    if (firstIdx === -1) {
      if (rest.length > 0) this.pathname = rest;
    } else if (firstIdx > 0) {
      this.pathname = rest.slice(0, firstIdx);
    }
    if (slashedProtocol[lowerProto] && this.hostname && !this.pathname) {
      this.pathname = '/';
    }

    // To support http.request
    if (this.pathname || this.search) {
      const p = this.pathname || '';
      const s = this.search || '';
      this.path = p + s;
    }

    // Finally, reconstruct the href based on what has been validated.
    this.href = this.format();
    return this;
  }

  parseHost() {
    let host = this.host;
    const port = portPattern.exec(host);
    if (port) {
      const portStr = port[0];
      if (portStr !== ':') {
        this.port = portStr.slice(1);
      }
      host = host.slice(0, host.length - portStr.length);
    }
    if (host) this.hostname = host;
  }

  format() {
    let auth = this.auth || '';
    if (auth) {
      auth = encodeStrAuth(auth);
      auth += '@';
    }

    let protocol = this.protocol || '';
    if (protocol && protocol.charCodeAt(protocol.length - 1) !== 58 /* : */) {
      protocol += ':';
    }

    let pathname = this.pathname || '';
    let hash = this.hash || '';
    let host = '';
    let query = '';

    if (this.host) {
      host = auth + this.host;
    } else if (this.hostname) {
      host = auth + (
        this.hostname.indexOf(':') !== -1 && !isIpv6Hostname(this.hostname) ?
          '[' + this.hostname + ']' :
          this.hostname
      );
      if (this.port) {
        host += ':' + this.port;
      }
    }

    if (this.query !== null && typeof this.query === 'object') {
      query = qsStringify(this.query);
    }
    let search = this.search || (query && ('?' + query)) || '';

    if (pathname.indexOf('#') !== -1 || pathname.indexOf('?') !== -1) {
      let newPathname = '';
      let lastPos = 0;
      const len = pathname.length;
      for (let i = 0; i < len; i++) {
        const code = pathname.charCodeAt(i);
        if (code === 35 /* # */ || code === 63 /* ? */) {
          if (i > lastPos) newPathname += pathname.slice(lastPos, i);
          newPathname += (code === 35 ? '%23' : '%3F');
          lastPos = i + 1;
        }
      }
      if (lastPos < len) newPathname += pathname.slice(lastPos);
      pathname = newPathname;
    }

    // Only the slashedProtocols get the //.  Not mailto:, xmpp:, etc.
    // unless they had them to begin with.
    if (this.slashes || slashedProtocol[protocol]) {
      if (this.slashes || host) {
        if (pathname && pathname.charCodeAt(0) !== 47 /* / */) {
          pathname = '/' + pathname;
        }
        host = '//' + host;
      } else if (protocol.length >= 4 &&
                 protocol.charCodeAt(0) === 102 /* f */ &&
                 protocol.charCodeAt(1) === 105 /* i */ &&
                 protocol.charCodeAt(2) === 108 /* l */ &&
                 protocol.charCodeAt(3) === 101 /* e */) {
        host = '//';
      }
    }

    // Escape '#' in search.
    if (search.indexOf('#') !== -1) {
      search = search.replaceAll('#', '%23');
    }

    if (hash && hash.charCodeAt(0) !== 35 /* # */) {
      hash = '#' + hash;
    }
    if (search && search.charCodeAt(0) !== 63 /* ? */) {
      search = '?' + search;
    }

    return protocol + host + pathname + search + hash;
  }

  resolve(relative) {
    return this.resolveObject(urlParse(relative, false, true)).format();
  }

  resolveObject(relative) {
    if (typeof relative === 'string') {
      const rel = new Url();
      rel.parse(relative, false, true);
      relative = rel;
    }
    const result = new Url();
    for (const key of urlKeys) result[key] = this[key];
    // Hash is always overridden, no matter what.
    result.hash = relative.hash;
    if (relative.href === '') {
      result.href = result.format();
      return result;
    }
    // Hrefs like //foo/bar always cut to the protocol.
    if (relative.slashes && !relative.protocol) {
      for (const key of urlKeys) {
        if (key !== 'protocol') result[key] = relative[key];
      }
      if (slashedProtocol[result.protocol] &&
          result.hostname && !result.pathname) {
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
      // node 原文条件：file: 虽在 slashedProtocol 但走 else（host 缺省路径）。
      if (!relative.host && !/^file:?$/.test(relative.protocol) &&
          !hostlessProtocol[relative.protocol]) {
        const relPath = (relative.pathname || '').split('/');
        // node 原文：首段抬进 relative.host（随后 result.host 从它取；
        // 若 shift 进 result.host 会被下方 result.host = relative.host 覆盖）。
        while (relPath.length && !(relative.host = relPath.shift()));
        relative.host ||= '';
        relative.hostname ||= '';
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
    let mustEndAbs = isRelAbs || isSourceAbs || (result.host && relative.pathname);
    const removeAllDots = mustEndAbs;
    let srcPath = (result.pathname && result.pathname.split('/')) || [];
    const relPath = (relative.pathname && relative.pathname.split('/')) || [];
    // node 原文：非斜杠协议（foo:/mailto: 等）——相对路径可爬升进 host，
    // 尾部再把首段放回 host（resolve('../c', 'foo:a/b') == 'foo:c' 现形）。
    const noLeadingSlashes = result.protocol &&
        !slashedProtocol[result.protocol];
    if (noLeadingSlashes) {
      result.hostname = '';
      result.port = null;
      if (result.host) {
        if (srcPath[0] === '') srcPath[0] = result.host;
        else srcPath.unshift(result.host);
      }
      result.host = '';
      if (relative.protocol) {
        relative.hostname = null;
        relative.port = null;
        result.auth = null;
        if (relative.host) {
          if (relPath[0] === '') relPath[0] = relative.host;
          else relPath.unshift(relative.host);
        }
        relative.host = null;
      }
      mustEndAbs &&= (relPath[0] === '' || srcPath[0] === '');
    }
    if (isRelAbs) {
      // node 原文：相对带 host/hostname（含空串）才覆盖 host/port/hostname，
      // host 变更即清 auth；port 无条件随 relative（null 也覆盖）。
      if (relative.host || relative.host === '') {
        if (result.host !== relative.host) result.auth = null;
        result.host = relative.host;
        result.port = relative.port;
      }
      if (relative.hostname || relative.hostname === '') {
        if (result.hostname !== relative.hostname) result.auth = null;
        result.hostname = relative.hostname;
      }
      result.search = relative.search;
      result.query = relative.query;
      srcPath = relPath;
      // Fall through to the dot-handling below.
    } else if (relPath.length) {
      // it's relative — throw away the existing file, take the new path
      srcPath ||= [];
      srcPath.pop();
      srcPath = srcPath.concat(relPath);
      result.search = relative.search;
      result.query = relative.query;
    } else if (relative.search !== null && relative.search !== undefined) {
      // 纯 query（href='?foo'）——放在另两分支后因简化布尔；非斜杠协议
      // 需先把 host 从 srcPath 抬回（node 原文）。
      if (noLeadingSlashes) {
        result.hostname = result.host = srcPath.shift();
        const authInHost =
          result.host && result.host.indexOf('@') > 0 && result.host.split('@');
        if (authInHost) {
          result.auth = authInHost.shift();
          result.host = result.hostname = authInHost.shift();
        }
      }
      result.search = relative.search;
      result.query = relative.query;
      if (result.pathname !== null || result.search !== null) {
        result.path = (result.pathname ? result.pathname : '') +
                      (result.search ? result.search : '');
      }
      result.href = result.format();
      return result;
    }
    if (!srcPath.length) {
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
    let last = srcPath[srcPath.length - 1];
    const hasTrailingSlash = ((result.host || relative.host || srcPath.length > 1) &&
      (last === '.' || last === '..')) || last === '';
    // 去单点、双点回父级（越根的 .. 由 up 计数，非 mustEndAbs 时恢复）
    let up = 0;
    for (let i = srcPath.length - 1; i >= 0; i--) {
      last = srcPath[i];
      if (last === '.') {
        srcPath.splice(i, 1);
      } else if (last === '..') {
        srcPath.splice(i, 1);
        up++;
      } else if (up) {
        srcPath.splice(i, 1);
        up--;
      }
    }
    if (!mustEndAbs && !removeAllDots) {
      while (up--) srcPath.unshift('..');
    }
    if (mustEndAbs && srcPath[0] !== '' &&
        (!srcPath[0] || srcPath[0].charCodeAt(0) !== 47)) {
      srcPath.unshift('');
    }
    if (hasTrailingSlash && srcPath.join('/').slice(-1) !== '/') {
      srcPath.push('');
    }
    const isAbsolute = srcPath[0] === '' ||
      (srcPath[0] && srcPath[0].charCodeAt(0) === 47);
    // node 原文：非斜杠协议把首段放回 host（auth 卡在 host 一并拆）。
    if (noLeadingSlashes) {
      result.hostname =
        result.host = isAbsolute ? '' : srcPath.length ? srcPath.shift() : '';
      const authInHost = result.host && result.host.indexOf('@') > 0 ?
        result.host.split('@') : false;
      if (authInHost) {
        result.auth = authInHost.shift();
        result.host = result.hostname = authInHost.shift();
      }
    }
    mustEndAbs ||= (result.host && srcPath.length);
    if (mustEndAbs && !isAbsolute) {
      srcPath.unshift('');
    }
    if (!srcPath.length) {
      result.pathname = null;
      result.path = null;
    } else {
      result.pathname = srcPath.join('/');
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

function getHostname(self, rest, hostname, url) {
  for (let i = 0; i < hostname.length; ++i) {
    const code = hostname.charCodeAt(i);
    const isValid = (code !== 47 /* / */ &&
                     code !== 92 /* \ */ &&
                     code !== 35 /* # */ &&
                     code !== 63 /* ? */ &&
                     code !== 58 /* : */);
    if (!isValid) {
      // If leftover starts with :, then it represents an invalid port.
      if (code === 58 /* : */) {
        throw new ERR_INVALID_ARG_VALUE('url', 'Invalid port in url', url);
      }
      self.hostname = hostname.slice(0, i);
      return `/${hostname.slice(i)}${rest}`;
    }
  }
  return rest;
}

let __urlParseWarned = false;
// node isInsideNodeModules 口径：调用链上有 node_modules 帧即不警告
//（套件 fixtures/node_modules 的抑制语义依赖此）。本引擎栈行形如
// `fn@file:line:col`，[0]=本函数、[1]=urlParse，[2] 起为调用方。
function __isInsideNodeModules(depth) {
  try {
    const frames = new Error().stack.split('\n').filter((l) => l.includes('@'));
    for (let i = 2; i <= depth + 1 && i < frames.length; i++) {
      if (frames[i].includes('node_modules')) return true;
    }
  } catch { /* 栈不可用按"不在 node_modules"处理 */ }
  return false;
}

function urlParse(url, parseQueryString, slashesDenoteHost) {
  // node 原文口径：DEP0169 每进程一次，先于 instanceof 短路；node_modules
  // 内的调用点不警告（test-url-parse-deprecation 套件逐项）。
  if (!__urlParseWarned && !__isInsideNodeModules(4)) {
    __urlParseWarned = true;
    process.emitWarning(
      '`url.parse()` behavior is not standardized and prone to ' +
      'errors that have security implications. Use the WHATWG URL API ' +
      'instead. CVEs are not issued for `url.parse()` vulnerabilities.',
      'DeprecationWarning',
      'DEP0169',
    );
  }
  if (url instanceof Url) return url;
  const u = new Url();
  u.parse(url, parseQueryString, slashesDenoteHost);
  return u;
}

function urlResolve(source, relative) {
  return urlParse(source, false, true).resolve(relative);
}

function urlResolveObject(source, relative) {
  // node 原文：空源短路——resolveObject('', 'foo') 返回原始串 'foo'
  //（非 Url 对象，套件首断言）。
  if (!source) return relative;
  return urlParse(source, false, true).resolveObject(relative);
}

// WHATWG format options（node bindingUrl.format(href, fragment, unicode,
// search, auth) 的 JS 口径：Boolean 化逐件剥离；unicode 反解 host）。
function formatWhatwg(urlObj, options) {
  let fragment = true;
  let unicode = false;
  let search = true;
  let auth = true;
  if (options) {
    if (typeof options !== 'object') {
      throw new ERR_INVALID_ARG_TYPE('options', 'Object', options);
    }
    if (options.fragment != null) fragment = Boolean(options.fragment);
    if (options.unicode != null) unicode = Boolean(options.unicode);
    if (options.search != null) search = Boolean(options.search);
    if (options.auth != null) auth = Boolean(options.auth);
  }
  let href = urlObj.href;
  let hashStr = '';
  const hashIdx = href.indexOf('#');
  if (hashIdx !== -1) { hashStr = href.slice(hashIdx); href = href.slice(0, hashIdx); }
  let searchStr = '';
  const qIdx = href.indexOf('?');
  if (qIdx !== -1) { searchStr = href.slice(qIdx); href = href.slice(0, qIdx); }
  const schemeEnd = href.indexOf('://');
  const authorityStart = schemeEnd === -1 ? 0 : schemeEnd + 3;
  const pathStart = href.indexOf('/', authorityStart);
  let authority = pathStart === -1 ? href.slice(authorityStart) : href.slice(authorityStart, pathStart);
  const rest = pathStart === -1 ? '' : href.slice(pathStart);
  const atIdx = authority.lastIndexOf('@');
  let userinfo = '';
  if (atIdx !== -1) {
    userinfo = authority.slice(0, atIdx) + '@';
    authority = authority.slice(atIdx + 1);
  }
  if (unicode && authority) {
    // host → toUnicode（host 可能带 :port；[v6] 的冒号在 ] 后）
    let hostPart = authority;
    let portPart = '';
    if (authority.startsWith('[')) {
      const closeIdx = authority.indexOf(']');
      if (closeIdx !== -1 && authority[closeIdx + 1] === ':') {
        hostPart = authority.slice(0, closeIdx + 1);
        portPart = authority.slice(closeIdx + 1);
      }
    } else {
      const colonIdx = authority.lastIndexOf(':');
      if (colonIdx !== -1) {
        hostPart = authority.slice(0, colonIdx);
        portPart = authority.slice(colonIdx);
      }
    }
    hostPart = __punyToUnicode(hostPart);
    authority = hostPart + portPart;
  }
  if (!auth) userinfo = '';
  if (!search) searchStr = '';
  if (!fragment) hashStr = '';
  return href.slice(0, authorityStart) + userinfo + authority + rest + searchStr + hashStr;
}

function urlFormat(urlObject, options) {
  // Ensure it's an object, and not a string url.
  if (typeof urlObject === 'string') {
    urlObject = urlParse(urlObject);
  } else if (typeof urlObject !== 'object' || urlObject === null) {
    throw new ERR_INVALID_ARG_TYPE('urlObject', ['Object', 'string'], urlObject);
  } else if (urlObject instanceof URL_) {
    return formatWhatwg(urlObject, options);
  }
  return Url.prototype.format.call(urlObject);
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
  // node 原文口径：validateObject（对象形）+ 解构取值（拷贝对象无数据属性 →
  // port Number(undefined)=NaN、path=''，套件 copied-url 逐项）。
  if (typeof url !== 'object' || url === null) {
    throw new ERR_INVALID_ARG_TYPE('url', 'object', url);
  }
  const { hostname, pathname, port, username, password, search } = url;
  const options = {
    ...url, // In case the url object was extended by the user.
    protocol: url.protocol,
    hostname: hostname && hostname[0] === '[' ?
      hostname.slice(1, -1) :
      hostname,
    hash: url.hash,
    search: search,
    pathname: pathname,
    path: `${pathname || ''}${search || ''}`,
    href: url.href,
  };
  if (port !== '') {
    options.port = Number(port);
  }
  if (username || password) {
    options.auth = `${decodeURIComponent(username)}:${decodeURIComponent(password)}`;
  }
  return options;
}

// node 口径：URL.createObjectURL/revokeObjectURL 缺参 → ERR_MISSING_ARGS
//（引擎原生的通用 TypeError 无 code，套件按 code 匹配）。
{
  const __revoke = URL_.revokeObjectURL?.bind(URL_);
  const __create = URL_.createObjectURL?.bind(URL_);
  URL_.revokeObjectURL = function revokeObjectURL(objURL) {
    if (objURL === undefined) throw new errors.codes.ERR_MISSING_ARGS('objURL');
    return __revoke ? __revoke(objURL) : undefined;
  };
  URL_.createObjectURL = function createObjectURL(obj, options) {
    if (obj === undefined) throw new errors.codes.ERR_MISSING_ARGS('obj');
    return __create ? __create(obj, options) : undefined;
  };
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
