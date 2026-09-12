//! `node:dns`：lookup/resolve4/resolve6（std `ToSocketAddrs` 底座，dependencies2
//! §9d 口径内的 tokio/hickory 组合中，本切片仅用 std——resolve* 深件（CNAME/MX/
//! TXT）需 hickory-resolver 行级增补，动工前按 §0.5 问用户拍板，记档）。
/// 偏差记档：同步阻塞解析（跑在 JS 线程；hermetic 用例全走 localhost/回环）；
/// 无 DNS 后缀搜索/hosts 缓存语义（直用系统解析）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};

/// `__wjs_dns_lookup(host)` → JSON 数组 `[{address, family}]`（v4+v6 全量；
/// 空数组由 JS 侧翻 ENOTFOUND）。
pub unsafe extern "C" fn dns_lookup(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: lookup needs a hostname");
        return false;
    }
    let host = value_to_string(&mut cx, frame.arg(0));
    use std::net::ToSocketAddrs as _;
    match (host.as_str(), 0u16).to_socket_addrs() {
        Ok(addrs) => {
            let entries: Vec<serde_json::Value> = addrs
                .map(|a| {
                    serde_json::json!({
                        "address": a.ip().to_string(),
                        "family": if a.is_ipv4() { 4 } else { 6 },
                    })
                })
                .collect();
            let text = serde_json::Value::Array(entries).to_string();
            rooted!(&in(cx) let mut v = UndefinedValue());
            text.as_str().to_jsval(&mut cx, v.handle_mut());
            frame.set_rval(v.get());
            true
        }
        Err(e) => {
            let code = crate::builtins::node::fs::io_code(&e);
            report_error(&mut cx, &format!("{code}: getaddrinfo '{host}': {e}"));
            false
        }
    }
}

/// 内嵌 ESM 源（`node:dns`）。
pub const SOURCE: &str = r#"
function __entries(hostname) {
  let raw;
  try {
    raw = JSON.parse(__wjs_dns_lookup(String(hostname)));
  } catch (e) {
    const err = new Error(`EAI_AGAIN: getaddrinfo '${hostname}': ${e.message}`);
    err.code = "EAI_AGAIN";
    err.hostname = String(hostname);
    err.syscall = "getaddrinfo";
    throw err;
  }
  if (!Array.isArray(raw) || raw.length === 0) {
    const err = new Error(`ENOTFOUND: getaddrinfo '${hostname}': no such host`);
    err.code = "ENOTFOUND";
    err.hostname = String(hostname);
    err.syscall = "getaddrinfo";
    throw err;
  }
  return raw;
}
function __filter(entries, family) {
  if (family === 4) return entries.filter((e) => e.family === 4);
  if (family === 6) return entries.filter((e) => e.family === 6);
  return entries;
}
function __lookupCore(hostname, options) {
  const all = !!(options && options.all);
  const family = options && (options.family === 4 || options.family === 6) ? options.family : 0;
  const picked = __filter(__entries(hostname), family);
  if (picked.length === 0) {
    const err = new Error(`ENOTFOUND: getaddrinfo '${hostname}': no such host`);
    err.code = "ENOTFOUND";
    err.hostname = String(hostname);
    err.syscall = "getaddrinfo";
    throw err;
  }
  return all ? picked.map((e) => ({ address: e.address, family: e.family })) : picked[0];
}
export function lookup(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  if (typeof cb !== "function") throw new TypeError("dns.lookup: callback must be a function");
  queueMicrotask(() => {
    try {
      const r = __lookupCore(hostname, options);
      if (Array.isArray(r)) cb(null, r);
      else cb(null, r.address, r.family);
    } catch (e) {
      cb(e);
    }
  });
}
export function resolve4(hostname, cb) {
  if (typeof cb !== "function") throw new TypeError("dns.resolve4: callback must be a function");
  queueMicrotask(() => {
    try { cb(null, __filter(__entries(hostname), 4).map((e) => e.address)); }
    catch (e) { cb(e); }
  });
}
export function resolve6(hostname, cb) {
  if (typeof cb !== "function") throw new TypeError("dns.resolve6: callback must be a function");
  queueMicrotask(() => {
    try { cb(null, __filter(__entries(hostname), 6).map((e) => e.address)); }
    catch (e) { cb(e); }
  });
}
const __as = (fn) => function (...args) { return Promise.resolve().then(() => fn(...args)); };
export const promises = {
  lookup: (hostname, options) => Promise.resolve().then(() => __lookupCore(hostname, options)),
  resolve4: __as(resolve4b),
  resolve6: __as(resolve6b),
};
function resolve4b(hostname) { return __filter(__entries(hostname), 4).map((e) => e.address); }
function resolve6b(hostname) { return __filter(__entries(hostname), 6).map((e) => e.address); }
const __api = { lookup, resolve4, resolve6, promises };
export default __api;
export { resolve4b as __resolve4, resolve6b as __resolve6 };
"#;
