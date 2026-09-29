//! WinterCG 存储 JS 面：全局 `storage` async KV + `localStorage` 同步垫片（prelude 分域；拼接顺序见 mod.rs）。
//!
//! 薄壳规则：校验在调 native 之前（§4.51 同族）；错误一律 plain `TypeError`，
//! 不包 code（Web 面无 node 错误码口径）；值编解码与 sqlite 同桥
//!（`Uint8Array` ↔ `{"$blob": b64}`，分片拼串防大数组爆栈）。
pub const STORAGE_JS: &str = r#"
// WinterCG 存储（S1）：turso 单文件 KV，默认库见 __wjs2_storage_default_path。
// localStorage 键走 '__localStorage__:' 保留前缀（storage.keys 默认滤掉它；
// storage.set 用该前缀直接 TypeError，防串域）。
{
  const LS_PREFIX = '__localStorage__:';
  let __wjs2_storage_id = null;
  const __openStorage = () => {
    if (__wjs2_storage_id === null) {
      __wjs2_storage_id = __wjs2_storage_open(__wjs2_storage_default_path());
    }
    return __wjs2_storage_id;
  };
  const __checkKey = (k) => {
    if (typeof k !== 'string' || k.length === 0) {
      throw new TypeError('storage key must be a non-empty string');
    }
    if ([...k].length > 1024) throw new TypeError('storage key is too long (max 1024 chars)');
    if (k.startsWith(LS_PREFIX)) throw new TypeError('storage key uses reserved prefix');
  };
  const __u8ToB64 = (u8) => {
    let s = '';
    for (let i = 0; i < u8.length; i += 8192) {
      s += String.fromCharCode.apply(null, u8.subarray(i, i + 8192));
    }
    return btoa(s);
  };
  const __b64ToU8 = (b64) => {
    const s = atob(b64);
    const u8 = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) u8[i] = s.charCodeAt(i);
    return u8;
  };
  const __encode = (v) => {
    if (v instanceof Blob) throw new TypeError('storage value does not support Blob/File (use Uint8Array)');
    if (v instanceof Uint8Array) return { $blob: __u8ToB64(v) };
    if (v !== null && typeof v === 'object') {
      if (Array.isArray(v)) return v.map(__encode);
      // 只递归纯对象；Date 等类实例交回 JSON.stringify 默认行为（Date→ISO 串），
      // 与本仓 structuredClone“纯数据”偏差口径一致，不在此自造序列化。
      const proto = Object.getPrototypeOf(v);
      if (proto !== Object.prototype && proto !== null) return v;
      const o = {};
      for (const k of Object.keys(v)) o[k] = __encode(v[k]);
      return o;
    }
    return v;
  };
  const __revive = (v) => {
    if (v !== null && typeof v === 'object') {
      if (v.$blob !== undefined && typeof v.$blob === 'string' && Object.keys(v).length === 1) {
        return __b64ToU8(v.$blob);
      }
      if (Array.isArray(v)) return v.map(__revive);
      const o = {};
      for (const k of Object.keys(v)) o[k] = __revive(v[k]);
      return o;
    }
    return v;
  };
  const __toJson = (v) => {
    const s = JSON.stringify(__encode(v));
    if (s === undefined) throw new TypeError('storage value is not serializable');
    return s;
  };
  globalThis.storage = {
    async get(k) {
      __checkKey(k);
      return __revive(JSON.parse(__wjs2_storage_get(__openStorage(), k)));
    },
    async set(k, v) {
      __checkKey(k);
      __wjs2_storage_set(__openStorage(), k, __toJson(v));
    },
    async delete(k) {
      __checkKey(k);
      return __wjs2_storage_delete(__openStorage(), k);
    },
    async has(k) {
      __checkKey(k);
      return JSON.parse(__wjs2_storage_get(__openStorage(), k)) !== null;
    },
    async keys(prefix) {
      if (prefix !== undefined && typeof prefix !== 'string') {
        throw new TypeError('storage keys prefix must be a string');
      }
      const all = JSON.parse(__wjs2_storage_keys(__openStorage(), prefix === undefined ? '' : prefix));
      return all.filter((k) => !k.startsWith(LS_PREFIX));
    },
    async clear() {
      // 只清 storage 域：逐键删（保留 localStorage 域；全清另调 localStorage.clear）。
      const all = await globalThis.storage.keys('');
      for (const k of all) __wjs2_storage_delete(__openStorage(), k);
      return all.length;
    },
    async size() {
      return (await globalThis.storage.keys('')).length;
    },
  };
  const __lsCheck = (k) => {
    if (typeof k !== 'string') throw new TypeError('localStorage key must be a string');
  };
  globalThis.localStorage = {
    getItem(k) {
      __lsCheck(k);
      const hit = JSON.parse(__wjs2_storage_get(__openStorage(), LS_PREFIX + k));
      if (hit === null) return null;
      return hit;
    },
    setItem(k, v) {
      __lsCheck(k);
      __wjs2_storage_set(__openStorage(), LS_PREFIX + k, JSON.stringify(String(v)));
    },
    removeItem(k) {
      __lsCheck(k);
      __wjs2_storage_delete(__openStorage(), LS_PREFIX + k);
    },
    clear() {
      const all = JSON.parse(__wjs2_storage_keys(__openStorage(), LS_PREFIX));
      for (const k of all) __wjs2_storage_delete(__openStorage(), k);
    },
    key(n) {
      const all = JSON.parse(__wjs2_storage_keys(__openStorage(), LS_PREFIX));
      if (typeof n !== 'number' || n < 0 || n >= all.length) return null;
      return all[n].slice(LS_PREFIX.length);
    },
    get length() {
      return JSON.parse(__wjs2_storage_keys(__openStorage(), LS_PREFIX)).length;
    },
  };
}
"#;
