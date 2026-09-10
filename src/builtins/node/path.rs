//! `node:path`：纯 JS 实现（posix + win32 双命名空间，无 natives）。
//! 与 Node 对齐常用语义；`resolve` 用 `globalThis.process.cwd()`（调用时取）。

/// 内嵌 ESM 源（`node:` 表注册；默认导出取平台：win32 系走 win32 实现）。
pub const SOURCE: &str = r#"
function posixSplit(p) { return p.split("/"); }
function posixNormalizeParts(parts, allowAboveRoot) {
  const out = [];
  for (const part of parts) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (out.length && out[out.length - 1] !== "..") out.pop();
      else if (allowAboveRoot) out.push("..");
    } else out.push(part);
  }
  return out;
}
function makePosix() {
  const sep = "/";
  function normalize(p) {
    p = String(p);
    if (p === "") return ".";
    const absolute = p[0] === "/";
    const trailing = p.length > 1 && p[p.length - 1] === "/";
    const parts = posixNormalizeParts(posixSplit(p), !absolute);
    let out = (absolute ? "/" : "") + parts.join("/");
    if (out === "") out = absolute ? "/" : ".";
    else if (trailing && out !== "/") out += "/";
    return out;
  }
  function join(...parts) {
    if (parts.length === 0) return ".";
    return normalize(parts.map(String).join("/"));
  }
  function resolve(...parts) {
    let resolved = "", absolute = false;
    for (let i = parts.length - 1; i >= 0; i--) {
      const p = String(parts[i]);
      if (p === "") continue;
      resolved = resolved ? p + "/" + resolved : p;
      if (p[0] === "/") { absolute = true; break; }
    }
    if (!absolute) {
      const cwd = globalThis.process ? globalThis.process.cwd() : "/";
      resolved = resolved ? cwd + "/" + resolved : cwd;
      absolute = true;
    }
    const trailing = resolved.length > 1 && resolved[resolved.length - 1] === "/";
    const out = "/" + posixNormalizeParts(posixSplit(resolved), false).join("/");
    return trailing && out !== "/" ? out + "/" : out;
  }
  function dirname(p) {
    p = String(p);
    if (p === "") return ".";
    const hasRoot = p[0] === "/";
    const noTrail = p.length > 1 && p[p.length - 1] === "/" ? p.slice(0, -1) : p;
    const idx = noTrail.lastIndexOf("/");
    if (idx === -1) return ".";
    if (idx === 0) return "/";
    const dir = noTrail.slice(0, idx);
    return dir === "" && hasRoot ? "/" : dir || ".";
  }
  function basename(p, suffix) {
    p = String(p);
    const noTrail = p.length > 1 && p[p.length - 1] === "/" ? p.slice(0, -1) : p;
    const idx = noTrail.lastIndexOf("/");
    let base = idx === -1 ? noTrail : noTrail.slice(idx + 1);
    if (suffix !== undefined && base.endsWith(suffix) && suffix !== "") {
      base = base.slice(0, base.length - suffix.length);
    }
    return base;
  }
  function extname(p) {
    p = basename(String(p));
    const idx = p.lastIndexOf(".");
    if (idx <= 0) return "";
    return p.slice(idx);
  }
  function isAbsolute(p) { return String(p).length > 0 && String(p)[0] === "/"; }
  function relative(from, to) {
    from = resolve(from); to = resolve(to);
    if (from === to) return "";
    const f = from.split("/").filter((s) => s !== "");
    const t = to.split("/").filter((s) => s !== "");
    let i = 0;
    while (i < f.length && i < t.length && f[i] === t[i]) i++;
    const up = f.slice(i).map(() => "..");
    return up.concat(t.slice(i)).join("/") || ".";
  }
  function parse(p) {
    p = String(p);
    const root = p[0] === "/" ? "/" : "";
    const dir = dirname(p);
    const base = basename(p);
    const ext = extname(p);
    return { root, dir, base, ext, name: ext ? base.slice(0, -ext.length) : base };
  }
  function format(o) {
    if (typeof o === "string") return o;
    const dir = o.dir || "", base = o.base || ((o.name || "") + (o.ext || ""));
    if (dir) return dir === "/" ? "/" + base : dir + "/" + base;
    return (o.root || "") + base;
  }
  return { sep, delimiter: ":", normalize, join, resolve, dirname, basename, extname, isAbsolute, relative, parse, format };
}
// ---- win32 ----
function winSplit(p) { return p.split(/[\\/]/); }
function winDevice(p) {
  // 返回 { device, rest }：盘符 `C:` / UNC `\\s\s` / 无
  if (/^[a-zA-Z]:/.test(p)) return { device: p.slice(0, 2), rest: p.slice(2) };
  if (/^[\\/]{2}[^\\/]+[\\/]+[^\\/]+/.test(p)) {
    const m = p.match(/^([\\/]{2}[^\\/]+[\\/]+[^\\/]+)([\\/]?[\s\S]*)$/);
    return { device: m[1].replace(/[\\/]+/g, "\\"), rest: m[2] || "" };
  }
  return { device: "", rest: p };
}
function winNormalizeParts(parts, allowAboveRoot) {
  const out = [];
  for (const part of parts) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (out.length && out[out.length - 1] !== "..") out.pop();
      else if (allowAboveRoot) out.push("..");
    } else out.push(part);
  }
  return out;
}
function makeWin32() {
  const sep = "\\";
  function splitRoot(p) {
    p = String(p);
    const { device, rest } = winDevice(p);
    const absolute = rest.length > 0 && (rest[0] === "\\" || rest[0] === "/");
    return { device, absolute, rest };
  }
  function normalize(p) {
    p = String(p);
    if (p === "") return ".";
    const { device, absolute, rest } = splitRoot(p);
    const trailing = /[\\/]$/.test(rest) && rest.length > 1;
    const parts = winNormalizeParts(winSplit(rest), !absolute && !device);
    let out = parts.join("\\");
    if (absolute) out = "\\" + out;
    if (device) out = device + (absolute || out ? "\\" + out.replace(/^\\/, "") : out || (absolute ? "" : "."));
    if (out === "" || out === device) out = device || ".";
    else if (trailing && !/[\\/]$/.test(out)) out += "\\";
    if (device && !absolute && out === device) return out;
    return out;
  }
  function join(...parts) {
    if (parts.length === 0) return ".";
    return normalize(parts.map(String).join("\\"));
  }
  function resolve(...parts) {
    let device = "", resolved = "", absolute = false;
    for (let i = parts.length - 1; i >= 0; i--) {
      let p = String(parts[i]);
      if (p === "") continue;
      const r = splitRoot(p);
      if (r.device && device && r.device.toLowerCase() !== device.toLowerCase()) continue;
      if (r.device && !device) device = r.device;
      resolved = resolved ? p.slice(r.device.length) + "\\" + resolved : p.slice(r.device.length);
      if (r.absolute) { absolute = true; break; }
    }
    if (!absolute) {
      const cwd = globalThis.process ? globalThis.process.cwd() : "C:\\";
      const r = splitRoot(cwd);
      if (!device) device = r.device;
      resolved = r.rest.replace(/^[\\/]/, "") + "\\" + resolved;
      absolute = true;
    }
    const trailing = /[\\/]$/.test(resolved);
    const parts2 = winNormalizeParts(winSplit(resolved), false);
    let out = (device ? device + "\\" : "\\") + parts2.join("\\");
    if (trailing && !/[\\/]$/.test(out)) out += "\\";
    return out;
  }
  function dirname(p) {
    p = String(p);
    if (p === "") return ".";
    const { device, rest } = winDevice(p);
    const noTrail = /[\\/]$/.test(rest) && rest.length > 1 ? rest.slice(0, -1) : rest;
    const idx = Math.max(noTrail.lastIndexOf("\\"), noTrail.lastIndexOf("/"));
    if (idx === -1) return device || ".";
    if (idx === 0) return device + "\\";
    return device + noTrail.slice(0, idx);
  }
  function basename(p, suffix) {
    p = String(p);
    const { rest } = winDevice(p);
    const noTrail = /[\\/]$/.test(rest) && rest.length > 1 ? rest.slice(0, -1) : rest;
    const idx = Math.max(noTrail.lastIndexOf("\\"), noTrail.lastIndexOf("/"));
    let base = idx === -1 ? noTrail : noTrail.slice(idx + 1);
    if (suffix !== undefined && base.endsWith(suffix) && suffix !== "") {
      base = base.slice(0, base.length - suffix.length);
    }
    return base;
  }
  function extname(p) {
    p = basename(String(p));
    const idx = p.lastIndexOf(".");
    if (idx <= 0) return "";
    return p.slice(idx);
  }
  function isAbsolute(p) {
    p = String(p);
    if (p.length === 0) return false;
    const { device, rest } = winDevice(p);
    if (/^[\\/]{2}/.test(p)) return true;
    if (device && rest.length > 0 && (rest[0] === "\\" || rest[0] === "/")) return true;
    return /^[\\/]/.test(p);
  }
  function relative(from, to) {
    const rf = resolve(from), rt = resolve(to);
    const df = splitRoot(rf), dt = splitRoot(rt);
    if (df.device.toLowerCase() !== dt.device.toLowerCase()) return rt;
    if (rf === rt) return "";
    const f = df.rest.split(/[\\/]/).filter((s) => s !== "");
    const t = dt.rest.split(/[\\/]/).filter((s) => s !== "");
    let i = 0;
    while (i < f.length && i < t.length && f[i].toLowerCase() === t[i].toLowerCase()) i++;
    return f.slice(i).map(() => "..").concat(t.slice(i)).join("\\") || ".";
  }
  function parse(p) {
    p = String(p);
    const { device, rest } = winDevice(p);
    const root = rest.length > 0 && (rest[0] === "\\" || rest[0] === "/") ? device + "\\" : device;
    const dir = dirname(p);
    const base = basename(p);
    const ext = extname(p);
    return { root, dir, base, ext, name: ext ? base.slice(0, -ext.length) : base };
  }
  function format(o) {
    if (typeof o === "string") return o;
    const dir = o.dir || "", base = o.base || ((o.name || "") + (o.ext || ""));
    if (dir) return /[\\/]$/.test(dir) ? dir + base : dir + "\\" + base;
    return (o.root || "") + base;
  }
  return { sep, delimiter: ";", normalize, join, resolve, dirname, basename, extname, isAbsolute, relative, parse, format };
}
const posix = makePosix();
const win32 = makeWin32();
const isWin = typeof globalThis.process !== "undefined" && globalThis.process.platform === "win32";
const path = isWin ? { ...win32, posix, win32 } : { ...posix, posix, win32 };
export default path;
export const { sep, delimiter, normalize, join, resolve, dirname, basename, extname, isAbsolute, relative, parse, format } = path;
export { posix, win32 };
"#;
