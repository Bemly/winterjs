//! 本体 FS JS 面：`WinterJS.fs` + `globalThis.fs` 别名（prelude 分域；拼接顺序见 mod.rs）。
//!
//! 薄壳规则：校验在调 native 之前；错误 plain `TypeError`/`WfsError`，不带 node code；
//! 二进制经 base64 桥（分片拼串防大数组爆栈）；async 形（Promise 包同步 native）。
pub const WFS_JS: &str = r#"
{
  const __wfs_b64ToU8 = (b64) => {
    const s = atob(b64);
    const u8 = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) u8[i] = s.charCodeAt(i);
    return u8;
  };
  const __wfs_u8ToB64 = (u8) => {
    let s = "";
    for (let i = 0; i < u8.length; i += 8192) {
      s += String.fromCharCode.apply(null, u8.subarray(i, i + 8192));
    }
    return btoa(s);
  };
  const __wfs_toU8 = (v) => {
    if (v instanceof Uint8Array) return v;
    if (typeof v === "string") return new TextEncoder().encode(v);
    if (v instanceof ArrayBuffer) return new Uint8Array(v);
    if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
    throw new TypeError("fs data must be string, Uint8Array, or ArrayBuffer");
  };
  const __wfs_checkPath = (p) => {
    if (typeof p !== "string" || p.length === 0) throw new TypeError("fs path must be a non-empty string");
  };
  const __wfs_fs = {
    readFile(p) {
      __wfs_checkPath(p);
      return Promise.resolve().then(() => __wfs_b64ToU8(__wjs_wfs_read(String(p))));
    },
    readTextFile(p) {
      __wfs_checkPath(p);
      return Promise.resolve().then(() => {
        const u8 = __wfs_b64ToU8(__wjs_wfs_read(String(p)));
        return new TextDecoder().decode(u8);
      });
    },
    writeFile(p, d) {
      __wfs_checkPath(p);
      const u8 = __wfs_toU8(d);
      return Promise.resolve().then(() => __wjs_wfs_write(String(p), __wfs_u8ToB64(u8)));
    },
    writeTextFile(p, t) {
      __wfs_checkPath(p);
      return globalThis.fs.writeFile(String(p), String(t));
    },
    stat(p) {
      __wfs_checkPath(p);
      return Promise.resolve().then(() => JSON.parse(__wjs_wfs_stat(String(p))));
    },
    mkdir(p, o) {
      __wfs_checkPath(p);
      const rec = !!(o && o.recursive);
      return Promise.resolve().then(() => __wjs_wfs_mkdir(String(p), rec));
    },
    readdir(p) {
      __wfs_checkPath(p);
      return Promise.resolve().then(() => JSON.parse(__wjs_wfs_readdir(String(p))));
    },
    remove(p, o) {
      __wfs_checkPath(p);
      const rec = !!(o && o.recursive);
      return Promise.resolve().then(() => __wjs_wfs_remove(String(p), rec));
    },
    rename(a, b) {
      __wfs_checkPath(a); __wfs_checkPath(b);
      return Promise.resolve().then(() => __wjs_wfs_rename(String(a), String(b)));
    },
    copyFile(a, b) {
      __wfs_checkPath(a); __wfs_checkPath(b);
      return Promise.resolve().then(() => __wjs_wfs_copy(String(a), String(b)));
    },
    exists(p) {
      __wfs_checkPath(p);
      return Promise.resolve().then(() => __wjs_wfs_exists(String(p)));
    },
  };
  if (globalThis.fs === undefined) globalThis.fs = __wfs_fs;
  try {
    if (globalThis.WinterJS && globalThis.WinterJS.fs === undefined) globalThis.WinterJS.fs = __wfs_fs;
  } catch {}
}
"#;
