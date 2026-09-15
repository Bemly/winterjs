//! `node:path`：纯 JS 实现（posix + win32 双命名空间，无 natives）。
//! 与 Node 对齐常用语义；`resolve` 用 `globalThis.process.cwd()`（调用时取）。

/// 内嵌 ESM 源（`node:` 表注册；默认导出取平台：win32 系走 win32 实现）。
pub const SOURCE: &str = r#"
import { validateObject, validateString } from 'node:internal/validators';
function isPosixSep(code) { return code === 47; }
function isSep(code) { return code === 47 || code === 92; }
function isWinDeviceRoot(code) { return (code >= 65 && code <= 90) || (code >= 97 && code <= 122); }
const WINDOWS_RESERVED_NAMES = [
  'CON', 'PRN', 'AUX', 'NUL',
  'COM1', 'COM2', 'COM3', 'COM4', 'COM5', 'COM6', 'COM7', 'COM8', 'COM9',
  'LPT1', 'LPT2', 'LPT3', 'LPT4', 'LPT5', 'LPT6', 'LPT7', 'LPT8', 'LPT9',
];
function isWindowsReservedName(path, colonIndex) {
  const devicePart = path.slice(0, colonIndex).toUpperCase();
  return WINDOWS_RESERVED_NAMES.includes(devicePart);
}
// Node `lib/path.js` normalizeString 直译（`.`/`..` 归一核心，posix/win32 共用）。
function normalizeString(path, allowAboveRoot, separator, isSepFn) {
  let res = "", lastSegmentLength = 0, lastSlash = -1, dots = 0, code = 0;
  for (let i = 0; i <= path.length; ++i) {
    if (i < path.length) code = path.charCodeAt(i);
    else if (isSepFn(code)) break;
    else code = 47;
    if (isSepFn(code)) {
      if (lastSlash === i - 1 || dots === 1) {
      } else if (dots === 2) {
        if (res.length < 2 || lastSegmentLength !== 2 ||
            res.charCodeAt(res.length - 1) !== 46 ||
            res.charCodeAt(res.length - 2) !== 46) {
          if (res.length > 2) {
            const lastSlashIndex = res.length - lastSegmentLength - 1;
            if (lastSlashIndex === -1) { res = ""; lastSegmentLength = 0; }
            else {
              res = res.slice(0, lastSlashIndex);
              lastSegmentLength = res.length - 1 - res.lastIndexOf(separator);
            }
            lastSlash = i; dots = 0; continue;
          } else if (res.length !== 0) {
            res = ""; lastSegmentLength = 0; lastSlash = i; dots = 0; continue;
          }
        }
        if (allowAboveRoot) {
          res += res.length > 0 ? `${separator}..` : "..";
          lastSegmentLength = 2;
        }
      } else {
        if (res.length > 0) res += `${separator}${path.slice(lastSlash + 1, i)}`;
        else res = path.slice(lastSlash + 1, i);
        lastSegmentLength = i - lastSlash - 1;
      }
      lastSlash = i; dots = 0;
    } else if (code === 46 && dots !== -1) ++dots;
    else dots = -1;
  }
  return res;
}
function formatExt(ext) {
  return ext ? `${ext[0] === "." ? "" : "."}${ext}` : "";
}
// Node `lib/path.js` _format 直译（含 validateObject 精确报错）。
function fmtPath(sep, pathObject) {
  validateObject(pathObject, "pathObject");
  const dir = pathObject.dir || pathObject.root;
  const base = pathObject.base || `${pathObject.name || ""}${formatExt(pathObject.ext)}`;
  if (!dir) return base;
  return dir === pathObject.root ? `${dir}${base}` : `${dir}${sep}${base}`;
}
function makePosix() {
  const sep = "/";
  function normalize(p) {
    validateString(p, "path");
    if (p.length === 0) return ".";
    const isAbsolute = p.charCodeAt(0) === 47;
    const trailingSeparator = p.charCodeAt(p.length - 1) === 47;
    p = normalizeString(p, !isAbsolute, "/", isPosixSep);
    if (p.length === 0) {
      if (isAbsolute) return "/";
      return trailingSeparator ? "./" : ".";
    }
    if (trailingSeparator) p += "/";
    return isAbsolute ? `/${p}` : p;
  }
  function join(...parts) {
    if (parts.length === 0) return ".";
    const path = [];
    for (let i = 0; i < parts.length; ++i) {
      const arg = parts[i];
      validateString(arg, "path");
      if (arg.length > 0) path.push(arg);
    }
    if (path.length === 0) return ".";
    return normalize(path.join("/"));
  }
  function resolve(...args) {
    if (args.length === 0 || (args.length === 1 && (args[0] === "" || args[0] === "."))) {
      const cwd = globalThis.process ? globalThis.process.cwd() : "/";
      if (cwd.charCodeAt(0) === 47) return cwd;
    }
    let resolvedPath = "", resolvedAbsolute = false;
    for (let i = args.length - 1; i >= 0 && !resolvedAbsolute; i--) {
      const path = args[i];
      validateString(path, `paths[${i}]`);
      if (path.length === 0) continue;
      resolvedPath = `${path}/${resolvedPath}`;
      resolvedAbsolute = path.charCodeAt(0) === 47;
    }
    if (!resolvedAbsolute) {
      const cwd = globalThis.process ? globalThis.process.cwd() : "/";
      resolvedPath = `${cwd}/${resolvedPath}`;
      resolvedAbsolute = cwd.charCodeAt(0) === 47;
    }
    resolvedPath = normalizeString(resolvedPath, !resolvedAbsolute, "/", isPosixSep);
    if (resolvedAbsolute) return `/${resolvedPath}`;
    return resolvedPath.length > 0 ? resolvedPath : ".";
  }
  function dirname(p) {
    validateString(p, "path");
    if (p.length === 0) return ".";
    const hasRoot = p.charCodeAt(0) === 47;
    let end = -1, matchedSlash = true;
    for (let i = p.length - 1; i >= 1; --i) {
      if (p.charCodeAt(i) === 47) {
        if (!matchedSlash) { end = i; break; }
      } else matchedSlash = false;
    }
    if (end === -1) return hasRoot ? "/" : ".";
    if (hasRoot && end === 1) return "//";
    return p.slice(0, end);
  }
  function basename(p, suffix) {
    if (suffix !== undefined) validateString(suffix, "suffix");
    validateString(p, "path");
    let start = 0, end = -1, matchedSlash = true;
    if (suffix !== undefined && suffix.length > 0 && suffix.length <= p.length) {
      if (suffix === p) return "";
      let extIdx = suffix.length - 1, firstNonSlashEnd = -1;
      for (let i = p.length - 1; i >= 0; --i) {
        const code = p.charCodeAt(i);
        if (code === 47) {
          if (!matchedSlash) { start = i + 1; break; }
        } else {
          if (firstNonSlashEnd === -1) { matchedSlash = false; firstNonSlashEnd = i + 1; }
          if (extIdx >= 0) {
            if (code === suffix.charCodeAt(extIdx)) {
              if (--extIdx === -1) end = i;
            } else { extIdx = -1; end = firstNonSlashEnd; }
          }
        }
      }
      if (start === end) end = firstNonSlashEnd;
      else if (end === -1) end = p.length;
      return p.slice(start, end);
    }
    for (let i = p.length - 1; i >= 0; --i) {
      if (p.charCodeAt(i) === 47) {
        if (!matchedSlash) { start = i + 1; break; }
      } else if (end === -1) { matchedSlash = false; end = i + 1; }
    }
    if (end === -1) return "";
    return p.slice(start, end);
  }
  function extname(p) {
    validateString(p, "path");
    p = basename(p);
    if (p === "..") return "";
    const idx = p.lastIndexOf(".");
    if (idx <= 0) return "";
    return p.slice(idx);
  }
  function isAbsolute(p) {
    validateString(p, "path");
    return p.length > 0 && p.charCodeAt(0) === 47;
  }
  function relative(from, to) {
    validateString(from, "from");
    validateString(to, "to");
    if (from === to) return "";
    from = resolve(from);
    to = resolve(to);
    if (from === to) return "";
    const fromStart = 1, fromEnd = from.length, fromLen = fromEnd - fromStart;
    const toStart = 1, toLen = to.length - toStart;
    const length = fromLen < toLen ? fromLen : toLen;
    let lastCommonSep = -1, i = 0;
    for (; i < length; i++) {
      const fromCode = from.charCodeAt(fromStart + i);
      if (fromCode !== to.charCodeAt(toStart + i)) break;
      else if (fromCode === 47) lastCommonSep = i;
    }
    if (i === length) {
      if (toLen > length) {
        if (to.charCodeAt(toStart + i) === 47) return to.slice(toStart + i + 1);
        if (i === 0) return to.slice(toStart + i);
      } else if (fromLen > length) {
        if (from.charCodeAt(fromStart + i) === 47) lastCommonSep = i;
        else if (i === 0) lastCommonSep = 0;
      }
    }
    let out = "";
    for (i = fromStart + lastCommonSep + 1; i <= fromEnd; ++i) {
      if (i === fromEnd || from.charCodeAt(i) === 47) {
        out += out.length === 0 ? ".." : "/..";
      }
    }
    return `${out}${to.slice(toStart + lastCommonSep)}`;
  }
  function parse(p) {
    validateString(p, "path");
    const ret = { root: "", dir: "", base: "", ext: "", name: "" };
    if (p.length === 0) return ret;
    const isAbsolute = p.charCodeAt(0) === 47;
    let start;
    if (isAbsolute) { ret.root = "/"; start = 1; }
    else start = 0;
    let startDot = -1, startPart = 0, end = -1, matchedSlash = true, preDotState = 0;
    const i0 = p.length - 1;
    for (let i = i0; i >= start; --i) {
      const code = p.charCodeAt(i);
      if (code === 47) {
        if (!matchedSlash) { startPart = i + 1; break; }
        continue;
      }
      if (end === -1) { matchedSlash = false; end = i + 1; }
      if (code === 46) {
        if (startDot === -1) startDot = i;
        else if (preDotState !== 1) preDotState = 1;
      } else if (startDot !== -1) preDotState = -1;
    }
    if (end !== -1) {
      const s = startPart === 0 && isAbsolute ? 1 : startPart;
      if (startDot === -1 || preDotState === 0 ||
          (preDotState === 1 && startDot === end - 1 && startDot === startPart + 1)) {
        ret.base = ret.name = p.slice(s, end);
      } else {
        ret.name = p.slice(s, startDot);
        ret.base = p.slice(s, end);
        ret.ext = p.slice(startDot, end);
      }
    }
    if (startPart > 0) ret.dir = p.slice(0, startPart - 1);
    else if (isAbsolute) ret.dir = "/";
    return ret;
  }
  function format(o) { return fmtPath("/", o); }
  function toNamespacedPath(p) { return p; }
  function _makeLong(p) { return toNamespacedPath(p); }
  return { sep, delimiter: ":", normalize, join, resolve, dirname, basename, extname, isAbsolute, relative, parse, format, toNamespacedPath, _makeLong };
}
// ---- win32 ----
function winDevice(p) {
  // 返回 { device, rest }：盘符 `C:` / UNC `\\s\s` / 无
  if (/^[a-zA-Z]:/.test(p)) return { device: p.slice(0, 2), rest: p.slice(2) };
  if (/^[\\/]{2}[^\\/]+[\\/]+[^\\/]+/.test(p)) {
    const m = p.match(/^([\\/]{2}[^\\/]+[\\/]+[^\\/]+)([\\/]?[\s\S]*)$/);
    // UNC 设备保留前导双分隔符（`\\server\share`；内部分隔符归一为 `\`）。
    return { device: "\\" + m[1].replace(/[\\/]+/g, "\\"), rest: m[2] || "" };
  }
  return { device: "", rest: p };
}
function makeWin32() {
  const sep = "\\";
  function normalize(p) {
    validateString(p, "path");
    const len = p.length;
    if (len === 0) return ".";
    let rootEnd = 0, device, isAbsolute = false;
    const code = p.charCodeAt(0);
    if (len === 1) return code === 47 ? "\\" : p;
    if (isSep(code)) {
      isAbsolute = true;
      if (isSep(p.charCodeAt(1))) {
        let j = 2, last = j;
        while (j < len && !isSep(p.charCodeAt(j))) j++;
        if (j < len && j !== last) {
          const firstPart = p.slice(last, j);
          last = j;
          while (j < len && isSep(p.charCodeAt(j))) j++;
          if (j < len && j !== last) {
            last = j;
            while (j < len && !isSep(p.charCodeAt(j))) j++;
            if (j === len || j !== last) {
              if (firstPart === "." || firstPart === "?") {
                device = `\\\\${firstPart}`;
                rootEnd = 4;
                const colonIndex = p.indexOf(":");
                const possibleDevice = p.slice(4, colonIndex + 1);
                if (isWindowsReservedName(possibleDevice, possibleDevice.length - 1)) {
                  device = `\\\\?\\${possibleDevice}`;
                  rootEnd = 4 + possibleDevice.length;
                }
              } else if (j === len) {
                return `\\\\${firstPart}\\${p.slice(last)}\\`;
              } else {
                device = `\\\\${firstPart}\\${p.slice(last, j)}`;
                rootEnd = j;
              }
            }
          }
        }
      } else rootEnd = 1;
    } else {
      const colonIndex = p.indexOf(":");
      if (colonIndex > 0) {
        if (isWinDeviceRoot(code) && colonIndex === 1) {
          device = p.slice(0, 2);
          rootEnd = 2;
          if (len > 2 && isSep(p.charCodeAt(2))) { isAbsolute = true; rootEnd = 3; }
        } else if (isWindowsReservedName(p, colonIndex)) {
          device = p.slice(0, colonIndex + 1);
          rootEnd = colonIndex + 1;
        }
      }
    }
    let tail = rootEnd < len ? normalizeString(p.slice(rootEnd), !isAbsolute, "\\", isSep) : "";
    if (tail.length === 0 && !isAbsolute) tail = ".";
    if (tail.length > 0 && isSep(p.charCodeAt(len - 1))) tail += "\\";
    if (!isAbsolute && device === undefined && p.includes(":")) {
      // CVE-2024-36139：相对路径不得被整形成盘符绝对形。
      if (tail.length >= 2 && isWinDeviceRoot(tail.charCodeAt(0)) && tail.charCodeAt(1) === 58) {
        return `.\\${tail}`;
      }
      let index = p.indexOf(":");
      do {
        if (index === len - 1 || isSep(p.charCodeAt(index + 1))) return `.\\${tail}`;
      } while ((index = p.indexOf(":", index + 1)) !== -1);
    }
    const colonIndex = p.indexOf(":");
    if (isWindowsReservedName(p, colonIndex)) return `.\\${device ?? ""}${tail}`;
    if (device === undefined) return isAbsolute ? `\\${tail}` : tail;
    return isAbsolute ? `${device}\\${tail}` : `${device}${tail}`;
  }
  function join(...args) {
    if (args.length === 0) return ".";
    const path = [];
    for (let i = 0; i < args.length; ++i) {
      const arg = args[i];
      validateString(arg, "path");
      if (arg.length > 0) path.push(arg);
    }
    if (path.length === 0) return ".";
    const firstPart = path[0];
    let joined = path.join("\\");
    // 首部多余斜杠会误导 normalize 判 UNC；真 UNC 首部（恰两条+非斜杠）保留。
    let needsReplace = true, slashCount = 0;
    if (isSep(firstPart.charCodeAt(0))) {
      ++slashCount;
      const firstLen = firstPart.length;
      if (firstLen > 1 && isSep(firstPart.charCodeAt(1))) {
        ++slashCount;
        if (firstLen > 2) {
          if (isSep(firstPart.charCodeAt(2))) ++slashCount;
          else needsReplace = false;
        }
      }
    }
    if (needsReplace) {
      while (slashCount < joined.length && isSep(joined.charCodeAt(slashCount))) slashCount++;
      if (slashCount >= 2) joined = `\\${joined.slice(slashCount)}`;
    }
    // 保留字设备名在场时跳过归一（Node 口径：原样回，仅统一 `/`→`\`）。
    const parts = [];
    let part = "";
    for (let i = 0; i < joined.length; i++) {
      if (joined[i] === "\\") {
        if (part) parts.push(part);
        part = "";
        while (i + 1 < joined.length && joined[i + 1] === "\\") i++;
      } else part += joined[i];
    }
    if (part) parts.push(part);
    if (parts.some((q) => {
      const ci = q.indexOf(":");
      return ci !== -1 && isWindowsReservedName(q, ci);
    })) {
      let result = "";
      for (let i = 0; i < joined.length; i++) result += joined[i] === "/" ? "\\" : joined[i];
      return result;
    }
    return normalize(joined);
  }
  function resolve(...args) {
    const isWinPlat = typeof globalThis.process !== "undefined" && globalThis.process.platform === "win32";
    let resolvedDevice = "", resolvedTail = "", resolvedAbsolute = false;
    for (let i = args.length - 1; i >= -1; i--) {
      let path;
      if (i >= 0) {
        path = args[i];
        validateString(path, `paths[${i}]`);
        if (path.length === 0) continue;
      } else if (resolvedDevice.length === 0) {
        path = globalThis.process ? globalThis.process.cwd() : "C:\\";
        if (args.length === 0 || (args.length === 1 && (args[0] === "" || args[0] === ".")) &&
            isSep(path.charCodeAt(0))) {
          if (!isWinPlat) path = path.replace(/\//g, "\\");
          return path;
        }
      } else {
        // 盘符相对 cwd（`D:foo` 形）；取不到回盘符根。
        path = (globalThis.process && globalThis.process.env[`=${resolvedDevice}`]) ||
          (globalThis.process ? globalThis.process.cwd() : `${resolvedDevice}\\`);
        if (path === undefined ||
            (path.slice(0, 2).toLowerCase() !== resolvedDevice.toLowerCase() &&
             path.charCodeAt(2) === 92)) {
          path = `${resolvedDevice}\\`;
        }
      }
      const len = path.length;
      let rootEnd = 0, device = "", isAbsolute = false;
      const code = path.charCodeAt(0);
      if (len === 1) {
        if (isSep(code)) { rootEnd = 1; isAbsolute = true; }
      } else if (isSep(code)) {
        isAbsolute = true;
        if (isSep(path.charCodeAt(1))) {
          let j = 2, last = j;
          while (j < len && !isSep(path.charCodeAt(j))) j++;
          if (j < len && j !== last) {
            const firstPart = path.slice(last, j);
            last = j;
            while (j < len && isSep(path.charCodeAt(j))) j++;
            if (j < len && j !== last) {
              last = j;
              while (j < len && !isSep(path.charCodeAt(j))) j++;
              if (j === len || j !== last) {
                if (firstPart !== "." && firstPart !== "?") {
                  device = `\\\\${firstPart}\\${path.slice(last, j)}`;
                  rootEnd = j;
                } else {
                  device = `\\\\${firstPart}`;
                  rootEnd = 4;
                }
              }
            }
          }
        } else rootEnd = 1;
      } else if (isWinDeviceRoot(code) && path.charCodeAt(1) === 58) {
        device = path.slice(0, 2);
        rootEnd = 2;
        if (len > 2 && isSep(path.charCodeAt(2))) { isAbsolute = true; rootEnd = 3; }
      }
      if (device.length > 0) {
        if (resolvedDevice.length > 0) {
          if (device.toLowerCase() !== resolvedDevice.toLowerCase()) continue;
        } else resolvedDevice = device;
      }
      if (resolvedAbsolute) {
        if (resolvedDevice.length > 0) break;
      } else {
        resolvedTail = `${path.slice(rootEnd)}\\${resolvedTail}`;
        resolvedAbsolute = isAbsolute;
        if (isAbsolute && resolvedDevice.length > 0) break;
      }
    }
    resolvedTail = normalizeString(resolvedTail, !resolvedAbsolute, "\\", isSep);
    return resolvedAbsolute ? `${resolvedDevice}\\${resolvedTail}` : `${resolvedDevice}${resolvedTail}` || ".";
  }
  function dirname(p) {
    // Node `lib/path.js` win32.dirname 直译（分隔符原样保留；UNC 根直回）。
    validateString(p, "path");
    const len = p.length;
    if (len === 0) return ".";
    const isSep = (c) => c === "\\" || c === "/";
    if (len === 1) return isSep(p[0]) ? p : ".";
    let rootEnd = -1, offset = 0;
    if (isSep(p[0])) {
      rootEnd = offset = 1;
      if (len > 1 && isSep(p[1])) {
        let j = 2, last = j;
        while (j < len && !isSep(p[j])) j++;
        if (j < len && j !== last) {
          last = j;
          while (j < len && isSep(p[j])) j++;
          if (j < len && j !== last) {
            last = j;
            while (j < len && !isSep(p[j])) j++;
            if (j === len) return p;
            if (j !== last) rootEnd = offset = j + 1;
          }
        }
      }
    } else if (/^[a-zA-Z]/.test(p[0]) && p[1] === ":") {
      rootEnd = (len > 2 && isSep(p[2])) ? 3 : 2;
      offset = rootEnd;
    }
    let end = -1, matchedSlash = true;
    for (let i = len - 1; i >= offset; --i) {
      if (isSep(p[i])) {
        if (!matchedSlash) { end = i; break; }
      } else matchedSlash = false;
    }
    if (end === -1) {
      if (rootEnd === -1) return ".";
      end = rootEnd;
    }
    return p.slice(0, end);
  }
  function basename(p, suffix) {
    if (suffix !== undefined) validateString(suffix, "suffix");
    validateString(p, "path");
    const { rest } = winDevice(p);
    let noTrail = rest;
    while (noTrail.length > 1 && /[\\/]$/.test(noTrail)) noTrail = noTrail.slice(0, -1);
    const idx = Math.max(noTrail.lastIndexOf("\\"), noTrail.lastIndexOf("/"));
    let base = idx === -1 ? noTrail : noTrail.slice(idx + 1);
    if (suffix !== undefined && suffix !== "") {
      if (suffix === p) return "";
      if (base.endsWith(suffix) && base.length !== suffix.length) {
        base = base.slice(0, base.length - suffix.length);
      }
    }
    return base;
  }
  function extname(p) {
    validateString(p, "path");
    p = basename(p);
    if (p === "..") return "";
    const idx = p.lastIndexOf(".");
    if (idx <= 0) return "";
    return p.slice(idx);
  }
  function isAbsolute(p) {
    validateString(p, "path");
    if (p.length === 0) return false;
    const { device, rest } = winDevice(p);
    if (/^[\\/]{2}/.test(p)) return true;
    if (device && rest.length > 0 && (rest[0] === "\\" || rest[0] === "/")) return true;
    return /^[\\/]/.test(p);
  }
  function relative(from, to) {
    validateString(from, "from");
    validateString(to, "to");
    if (from === to) return "";
    const fromOrig = resolve(from), toOrig = resolve(to);
    if (fromOrig === toOrig) return "";
    from = fromOrig.toLowerCase();
    to = toOrig.toLowerCase();
    if (from === to) return "";
    if (fromOrig.length !== from.length || toOrig.length !== to.length) {
      const fromSplit = fromOrig.split("\\"), toSplit = toOrig.split("\\");
      if (fromSplit[fromSplit.length - 1] === "") fromSplit.pop();
      if (toSplit[toSplit.length - 1] === "") toSplit.pop();
      const fromLen = fromSplit.length, toLen = toSplit.length;
      const length = fromLen < toLen ? fromLen : toLen;
      let k;
      for (k = 0; k < length; k++) {
        if (fromSplit[k].toLowerCase() !== toSplit[k].toLowerCase()) break;
      }
      if (k === 0) return toOrig;
      else if (k === length) {
        if (toLen > length) return toSplit.slice(k).join("\\");
        if (fromLen > length) return "..\\".repeat(fromLen - 1 - k) + "..";
        return "";
      }
      return "..\\".repeat(fromLen - k) + toSplit.slice(k).join("\\");
    }
    let fromStart = 0;
    while (fromStart < from.length && from.charCodeAt(fromStart) === 92) fromStart++;
    let fromEnd = from.length;
    while (fromEnd - 1 > fromStart && from.charCodeAt(fromEnd - 1) === 92) fromEnd--;
    const fromLen = fromEnd - fromStart;
    let toStart = 0;
    while (toStart < to.length && to.charCodeAt(toStart) === 92) toStart++;
    let toEnd = to.length;
    while (toEnd - 1 > toStart && to.charCodeAt(toEnd - 1) === 92) toEnd--;
    const toLen = toEnd - toStart;
    const length = fromLen < toLen ? fromLen : toLen;
    let lastCommonSep = -1, i = 0;
    for (; i < length; i++) {
      const fromCode = from.charCodeAt(fromStart + i);
      if (fromCode !== to.charCodeAt(toStart + i)) break;
      else if (fromCode === 92) lastCommonSep = i;
    }
    if (i !== length) {
      if (lastCommonSep === -1) return toOrig;
    } else {
      if (toLen > length) {
        if (to.charCodeAt(toStart + i) === 92) return toOrig.slice(toStart + i + 1);
        if (i === 2) return toOrig.slice(toStart + i);
      }
      if (fromLen > length) {
        if (from.charCodeAt(fromStart + i) === 92) lastCommonSep = i;
        else if (i === 2) lastCommonSep = 3;
      }
      if (lastCommonSep === -1) lastCommonSep = 0;
    }
    let out = "";
    for (i = fromStart + lastCommonSep + 1; i <= fromEnd; ++i) {
      if (i === fromEnd || from.charCodeAt(i) === 92) {
        out += out.length === 0 ? ".." : "\\..";
      }
    }
    toStart += lastCommonSep;
    if (out.length > 0) return `${out}${toOrig.slice(toStart, toEnd)}`;
    if (toOrig.charCodeAt(toStart) === 92) ++toStart;
    return toOrig.slice(toStart, toEnd);
  }
  function parse(p) {
    validateString(p, "path");
    const ret = { root: "", dir: "", base: "", ext: "", name: "" };
    if (p.length === 0) return ret;
    const len = p.length;
    let rootEnd = 0, code = p.charCodeAt(0);
    if (len === 1) {
      if (isSep(code)) { ret.root = ret.dir = p; return ret; }
      ret.base = ret.name = p;
      return ret;
    }
    if (isSep(code)) {
      rootEnd = 1;
      if (isSep(p.charCodeAt(1))) {
        let j = 2, last = j;
        while (j < len && !isSep(p.charCodeAt(j))) j++;
        if (j < len && j !== last) {
          last = j;
          while (j < len && isSep(p.charCodeAt(j))) j++;
          if (j < len && j !== last) {
            last = j;
            while (j < len && !isSep(p.charCodeAt(j))) j++;
            if (j === len) rootEnd = j;
            else if (j !== last) rootEnd = j + 1;
          }
        }
      }
    } else if (isWinDeviceRoot(code) && p.charCodeAt(1) === 58) {
      if (len <= 2) { ret.root = ret.dir = p; return ret; }
      rootEnd = 2;
      if (isSep(p.charCodeAt(2))) {
        if (len === 3) { ret.root = ret.dir = p; return ret; }
        rootEnd = 3;
      }
    }
    if (rootEnd > 0) ret.root = p.slice(0, rootEnd);
    let startDot = -1, startPart = rootEnd, end = -1, matchedSlash = true, preDotState = 0;
    for (let i = p.length - 1; i >= rootEnd; --i) {
      code = p.charCodeAt(i);
      if (isSep(code)) {
        if (!matchedSlash) { startPart = i + 1; break; }
        continue;
      }
      if (end === -1) { matchedSlash = false; end = i + 1; }
      if (code === 46) {
        if (startDot === -1) startDot = i;
        else if (preDotState !== 1) preDotState = 1;
      } else if (startDot !== -1) preDotState = -1;
    }
    if (end !== -1) {
      if (startDot === -1 || preDotState === 0 ||
          (preDotState === 1 && startDot === end - 1 && startDot === startPart + 1)) {
        ret.base = ret.name = p.slice(startPart, end);
      } else {
        ret.name = p.slice(startPart, startDot);
        ret.base = p.slice(startPart, end);
        ret.ext = p.slice(startDot, end);
      }
    }
    if (startPart > 0 && startPart !== rootEnd) ret.dir = p.slice(0, startPart - 1);
    else ret.dir = ret.root;
    return ret;
  }
  function format(o) { return fmtPath("\\", o); }
  function toNamespacedPath(p) {
    if (typeof p !== "string" || p.length === 0) return p;
    const resolvedPath = resolve(p);
    if (resolvedPath.length <= 2) return p;
    if (resolvedPath.charCodeAt(0) === 92) {
      if (resolvedPath.charCodeAt(1) === 92) {
        const code = resolvedPath.charCodeAt(2);
        if (code !== 63 && code !== 46) {
          return `\\\\?\\UNC\\${resolvedPath.slice(2)}`;
        }
      }
    } else if (isWinDeviceRoot(resolvedPath.charCodeAt(0)) &&
               resolvedPath.charCodeAt(1) === 58 &&
               resolvedPath.charCodeAt(2) === 92) {
      return `\\\\?\\${resolvedPath}`;
    }
    return resolvedPath;
  }
  function _makeLong(p) { return toNamespacedPath(p); }
  return { sep, delimiter: ";", normalize, join, resolve, dirname, basename, extname, isAbsolute, relative, parse, format, toNamespacedPath, _makeLong };
}
const posix = makePosix();
const win32 = makeWin32();
const isWin = typeof globalThis.process !== "undefined" && globalThis.process.platform === "win32";
const path = isWin ? { ...win32, posix, win32 } : { ...posix, posix, win32 };
export default path;
export const { sep, delimiter, normalize, join, resolve, dirname, basename, extname, isAbsolute, relative, parse, format, toNamespacedPath } = path;
export { posix, win32 };
"#;
