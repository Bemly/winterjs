// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node lib/url.js (partial face; see module docs for deviations).
import { resolve as resolvePath } from 'node:path';
import { resolve as win32Resolve } from 'node:path/win32';
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
    ERR_INVALID_ARG_VALUE,
    ERR_INVALID_FILE_URL_HOST,
    ERR_INVALID_FILE_URL_PATH,
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
      throw Object.assign(new ERR_INVALID_URL(path), { input: path });
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
  // node getPathFromURLPosix 口径：pathname 含编码的 /（%2F）即抛
  // ERR_INVALID_FILE_URL_PATH，input 挂 URL 对象（套件逐项断言）。
  // %5C 检查属 win32 专查——posix 下 file:///foo%5Cbar 是合法路径
  //（套件 posixTestCases 的 '/foo\\bar' 用例现形）。
  const pn = path.pathname;
  for (let n = 0; n < pn.length; n++) {
    if (pn[n] === '%') {
      const third = String.fromCharCode(pn.charCodeAt(n + 2) | 0x20);
      if (pn[n + 1] === '2' && third === 'f') {
        throw Object.assign(new ERR_INVALID_FILE_URL_PATH(), { input: path });
      }
    }
  }
  let pathname;
  try {
    pathname = decodeURIComponent(path.pathname);
  } catch {
    throw Object.assign(new ERR_INVALID_URL(path.href), { input: path.href });
  }
  // win32 盘符前导剥离（/C:/x → C:/x；posix 原样，偏差记档）。
  if (/^\/[A-Za-z]:\//.test(pathname)) {
    pathname = pathname.slice(1);
  }
  return pathname;
}

// node pathToFileURL（internal/url.js 口径，windows 选项驱动）：
// 扩展前缀（\\?\UNC\、\\?\盘符）/ UNC（hostname 状态机：terminator 截断、
// ignored 码点删除、forbidden 抛 ERR_INVALID_URL）/ 盘符 / posix（尾分隔符
// 保留 + 逐段百分号编码）。
function pathToFileURL(path, options) {
  if (typeof path !== 'string') {
    throw new ERR_INVALID_ARG_TYPE('path', ['string'], path);
  }
  const windows = options?.windows ?? false;
  if (windows) {
    // 扩展 UNC：\\?\UNC\server\share\file → file://server/share/file
    if (path.startsWith('\\\\?\\UNC\\')) {
      const rest = path.slice(8);
      const hostEnd = rest.indexOf('\\');
      if (hostEnd === -1) {
        throw new ERR_INVALID_ARG_VALUE('path', path, 'Missing UNC resource path');
      }
      return __makeFileURL(rest.slice(0, hostEnd), rest.slice(hostEnd));
    }
    // 扩展路径：\\?\C:\dir\file → file:///C:/dir/file（剥前缀按盘符走）
    if (path.startsWith('\\\\?\\')) {
      return __makeFileURL('', __winDriveOrThrow(path.slice(4)));
    }
    const isUNC = path.startsWith('\\\\');
    if (isUNC) {
      const hostEnd = path.indexOf('\\', 2);
      if (hostEnd === -1) {
        throw new ERR_INVALID_ARG_VALUE('path', path, 'Missing UNC resource path');
      }
      if (hostEnd === 2) {
        throw new ERR_INVALID_ARG_VALUE('path', path, 'Empty UNC servername');
      }
      // hostname 状态机：terminator（# ? /）截断（其后到下一个 \ 的段丢弃）；
      // ignored（\n \r \t）删除；forbidden（空格 @ : [ ]）抛。
      const rawHost = path.slice(2, hostEnd);
      let host = '';
      let skipped = false;
      for (let i = 0; i < rawHost.length; i++) {
        const ch = rawHost[i];
        if (ch === '#' || ch === '?' || ch === '/') { skipped = true; break; }
        host += ch;
      }
      let tail;
      if (skipped) {
        const afterTerm = path.slice(2 + rawHost.length);
        const stop = afterTerm.indexOf('\\');
        if (stop === -1) {
          throw new ERR_INVALID_ARG_VALUE('path', path, 'Missing UNC resource path');
        }
        tail = afterTerm.slice(stop);
      } else {
        tail = path.slice(hostEnd);
      }
      const cleanHost = host.replaceAll(/[\n\r\t]/g, '');
      if (/[ @:\[\]]/.test(cleanHost)) {
        throw Object.assign(new ERR_INVALID_URL(path), { input: path });
      }
      return __makeFileURL(cleanHost, tail);
    }
    // 盘符/相对路径：win32 resolve（相对按 cwd），尾分隔符保留
    let out = __winDriveOrThrow(path);
    const lastChar = path.charCodeAt(path.length - 1);
    if ((lastChar === 92 /* \ */) && out[out.length - 1] !== '/') out += '/';
    return __makeFileURL('', out);
  }
  // posix：resolve 剥尾分隔符须补回（node 原文口径）
  let resolved = resolvePath(process.cwd(), path);
  const lastChar = path.charCodeAt(path.length - 1);
  if (lastChar === 47 /* / */ && resolved[resolved.length - 1] !== '/') {
    resolved += '/';
  }
  const encoded = resolved.split('/').map(__encodePathSegment).join('/');
  return new URL_('file://' + encoded);
}

// 盘符/相对 → /C:/dir 形（win32 resolve；\ → /；扩展前缀已在调用方剥离）
function __winDriveOrThrow(p) {
  const resolved = win32Resolve(p);
  if (resolved.startsWith('\\\\?\\')) {
    throw new ERR_INVALID_ARG_VALUE('path', p, 'The extended path prefix must be removed');
  }
  return resolved.replaceAll('\\', '/');
}

function __encodePathSegment(seg) {
  // WHATWG path percent-encode set（query set + ? ` { }）+ '\\' + '%' +
  // '^' '|[' ']' '~'（ada file-path 模式口径——posix/win 两分支同表，
  // 套件 '/foo\\bar'→%5C、'C:\\foo^bar'→%5E、'~'→%7E 逐字现形）。
  // 其余原样保留（: ; & = 等不编，与 node fileURL 引擎 path 态一致——
  // encodeURIComponent 会把 : & = 多编）。代理对整体编码。
  let out = '';
  for (let i = 0; i < seg.length; i++) {
    const code = seg.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff && i + 1 < seg.length) {
      const next = seg.charCodeAt(i + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        out += encodeURIComponent(seg.slice(i, i + 2));
        i++;
        continue;
      }
    }
    const ch = seg[i];
    if (code <= 0x20 || code >= 0x7f || ch === '"' || ch === '#' ||
        ch === '<' || ch === '>' || ch === '?' || ch === '`' ||
        ch === '{' || ch === '}' || ch === '\\' || ch === '%' ||
        ch === '^' || ch === '|' || ch === '[' || ch === ']' ||
        ch === '~') {
      // encodeURIComponent 对 unreserved 的 '~' 原样放行（同理不收 !'()*-._），
      // 本表要求 %7E——手动给出；其余被收字符 encodeURIComponent 恒编码。
      out += ch === '~' ? '%7E' : encodeURIComponent(ch);
    } else {
      out += ch;
    }
  }
  return out;
}

function __makeFileURL(host, unixPath) {
  // UNC tail 仍带 \ 分隔（\\host\share\file → /share/file），统一转 /。
  const norm = unixPath.replaceAll('\\', '/');
  const p = norm.startsWith('/') ? norm : '/' + norm;
  const encoded = p.split('/').map(__encodePathSegment).join('/');
  if (host) return new URL_('file://' + encodeURIComponent(host) + encoded);
  return new URL_('file://' + encoded);
}

