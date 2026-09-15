//! `node:dns`：lookup（std `ToSocketAddrs` 底座）+ 深件经 hickory-resolver（10d）。
//!
//! 10d（用户拍板全套）：Cname/Mx/Txt/Srv/Ns/Ptr + resolveAny +
//! getServers/setServers/setDefaultResultOrder，读系统 DNS 配置；
//! `lookup` 维持 std（真机 getaddrinfo 口径）。
//! 偏差记档：同步阻塞解析（JS 线程阻塞等 helper 线程回结果，与既有 lookup 同哲学）；
//! 无 DNSSEC/轮询细节；`lookupService` 未做（端口→服务名映射另议）。

use std::net::IpAddr;
use std::sync::OnceLock;

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

// ── 10d 深件：hickory 底座 ──────────────────────────────────────────────

fn servers_slot() -> &'static parking_lot::Mutex<Option<Vec<String>>> {
    static SLOT: OnceLock<parking_lot::Mutex<Option<Vec<String>>>> = OnceLock::new();
    SLOT.get_or_init(|| parking_lot::Mutex::new(None))
}

fn order_slot() -> &'static parking_lot::Mutex<String> {
    static SLOT: OnceLock<parking_lot::Mutex<String>> = OnceLock::new();
    SLOT.get_or_init(|| parking_lot::Mutex::new("verbatim".to_string()))
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    *servers_slot().lock() = None;
    *order_slot().lock() = "verbatim".to_string();
}

/// 真实系统服务器：优先解析 `/etc/resolv.conf` 的 `nameserver` 行（hermetic 可测，
/// 不依赖 hickory 内部字段）；读不到回落 `127.0.0.1`。
pub(crate) fn effective_servers() -> Vec<String> {
    if let Some(v) = servers_slot().lock().clone() {
        return v;
    }
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in text.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("nameserver") else {
                continue;
            };
            let ip = rest.trim().split_whitespace().next().unwrap_or("");
            if ip.parse::<IpAddr>().is_ok() {
                out.push(ip.to_string());
            }
        }
    }
    if out.is_empty() {
        out.push("127.0.0.1".to_string());
    }
    out
}

pub(crate) fn trim_dot(s: &str) -> String {
    s.strip_suffix('.').unwrap_or(s).to_string()
}

pub(crate) fn node_code_for(err_text: &str) -> &'static str {
    let lower = err_text.to_ascii_lowercase();
    if lower.contains("no records found")
        || lower.contains("no record found")
        || lower.contains("norecordsfound")
        || lower.contains("nodata")
        || lower.contains("no data")
    {
        return "ENODATA";
    }
    if lower.contains("nxdomain") || lower.contains("nx domain") || lower.contains("no such host") {
        return "ENOTFOUND";
    }
    if lower.contains("timed out") || lower.contains("timeout") {
        return "TIMEOUT";
    }
    if lower.contains("refused") {
        return "EREFUSED";
    }
    if lower.contains("servfail") || lower.contains("server failure") {
        return "SERVFAIL";
    }
    if lower.contains("bad name") || lower.contains("invalid name") || lower.contains("badname") {
        return "EBADNAME";
    }
    if lower.contains("cancelled") || lower.contains("canceled") {
        return "ECANCELLED";
    }
    "ENOTFOUND"
}

fn resolver_config_for(servers: &[String]) -> hickory_resolver::config::ResolverConfig {
    use hickory_resolver::config::{NameServerConfig, ResolverConfig};
    let mut cfg = ResolverConfig::default();
    for s in servers {
        if let Ok(ip) = s.parse::<IpAddr>() {
            cfg.add_name_server(NameServerConfig::udp_and_tcp(ip));
        }
    }
    cfg
}

fn record_to_json(
    rtype: hickory_resolver::proto::rr::RecordType,
    rdata: &hickory_resolver::proto::rr::RData,
) -> Option<serde_json::Value> {
    use hickory_resolver::proto::rr::RData;
    match rdata {
        RData::A(a) => Some(serde_json::json!({"type": "A", "address": a.to_string()})),
        RData::AAAA(a) => Some(serde_json::json!({"type": "AAAA", "address": a.to_string()})),
        RData::CNAME(n) => Some(serde_json::json!({"type": "CNAME", "value": trim_dot(&n.to_string())})),
        RData::NS(n) => Some(serde_json::json!({"type": "NS", "value": trim_dot(&n.to_string())})),
        RData::PTR(n) => Some(serde_json::json!({"type": "PTR", "value": trim_dot(&n.to_string())})),
        RData::MX(mx) => Some(
            serde_json::json!({"type": "MX", "exchange": trim_dot(&mx.exchange.to_string()), "priority": mx.preference}),
        ),
        RData::SRV(s) => Some(serde_json::json!({
            "type": "SRV", "name": trim_dot(&s.target.to_string()),
            "port": s.port, "priority": s.priority, "weight": s.weight,
        })),
        RData::TXT(t) => {
            let parts: Vec<String> = t.txt_data.iter().map(|b| String::from_utf8_lossy(b).into_owned()).collect();
            Some(serde_json::json!({"type": "TXT", "entries": parts}))
        }
        _ => {
            let _ = rtype;
            None
        }
    }
}

fn query_blocking(kind: &str, name: &str) -> Result<serde_json::Value, (String, String)> {
    let kind = kind.to_string();
    let name = name.to_string();
    let servers = effective_servers();
    let (tx, rx) = crossbeam_channel::bounded::<Result<serde_json::Value, (String, String)>>(1);
    let spawned = std::thread::Builder::new()
        .name("winterjs-dns".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(("EAI_AGAIN".to_string(), format!("dns runtime: {e}"))));
                    return;
                }
            };
            let out: Result<serde_json::Value, (String, String)> = rt.block_on(async move {
                use hickory_resolver::net::runtime::TokioRuntimeProvider;
                use hickory_resolver::proto::rr::RecordType;
                use hickory_resolver::Resolver;
                let cfg = resolver_config_for(&servers);
                if cfg.name_servers().is_empty() {
                    return Err(("ENOTFOUND".to_string(), "no DNS servers".to_string()));
                }
                let resolver = match Resolver::builder_with_config(
                    cfg,
                    TokioRuntimeProvider::default(),
                )
                .build()
                {
                    Ok(r) => r,
                    Err(e) => return Err(("EAI_AGAIN".to_string(), e.to_string())),
                };
                let rtype = match kind.as_str() {
                    "cname" => RecordType::CNAME,
                    "mx" => RecordType::MX,
                    "ns" => RecordType::NS,
                    "txt" => RecordType::TXT,
                    "srv" => RecordType::SRV,
                    "ptr" => RecordType::PTR,
                    "a" => RecordType::A,
                    "aaaa" => RecordType::AAAA,
                    _ => RecordType::A,
                };
                if kind == "reverse" {
                    let ip: IpAddr = name
                        .parse()
                        .map_err(|_| ("EBADNAME".to_string(), format!("bad IP '{name}'")))?;
                    match resolver.reverse_lookup(ip).await {
                        Ok(lookup) => {
                            use hickory_resolver::proto::rr::RData;
                            let mut vals: Vec<serde_json::Value> = Vec::new();
                            for rec in lookup.answers() {
                                if let RData::PTR(n) = &rec.data {
                                    vals.push(serde_json::Value::String(trim_dot(&n.to_string())));
                                }
                            }
                            return Ok(serde_json::Value::Array(vals));
                        }
                        Err(e) => {
                            let text = e.to_string();
                            return Err((node_code_for(&text).to_string(), text));
                        }
                    }
                }
                if kind == "any" {
                    let mut acc: Vec<serde_json::Value> = Vec::new();
                    for rt in [
                        RecordType::A,
                        RecordType::AAAA,
                        RecordType::MX,
                        RecordType::TXT,
                        RecordType::NS,
                        RecordType::CNAME,
                        RecordType::SRV,
                    ] {
                        if let Ok(lookup) = resolver.lookup(name.clone(), rt).await {
                            for rec in lookup.answers() {
                                if let Some(v) = record_to_json(rec.record_type(), &rec.data) {
                                    acc.push(v);
                                }
                            }
                        }
                    }
                    return Ok(serde_json::Value::Array(acc));
                }
                if kind == "ptr" && name.parse::<IpAddr>().is_ok() {
                    let ip: IpAddr = name.parse().expect("checked");
                    match resolver.reverse_lookup(ip).await {
                        Ok(lookup) => {
                            use hickory_resolver::proto::rr::RData;
                            let mut vals: Vec<serde_json::Value> = Vec::new();
                            for rec in lookup.answers() {
                                if let RData::PTR(n) = &rec.data {
                                    vals.push(serde_json::Value::String(trim_dot(&n.to_string())));
                                }
                            }
                            return Ok(serde_json::Value::Array(vals));
                        }
                        Err(e) => {
                            let text = e.to_string();
                            return Err((node_code_for(&text).to_string(), text));
                        }
                    }
                }
                match resolver.lookup(name.clone(), rtype).await {
                    Ok(lookup) => {
                        let mut vals: Vec<serde_json::Value> = Vec::new();
                        for rec in lookup.answers() {
                            use hickory_resolver::proto::rr::RData;
                            match (kind.as_str(), &rec.data) {
                                ("cname", RData::CNAME(n)) => {
                                    vals.push(serde_json::Value::String(trim_dot(&n.to_string())));
                                }
                                ("ns", RData::NS(n)) => {
                                    vals.push(serde_json::Value::String(trim_dot(&n.to_string())));
                                }
                                ("mx", RData::MX(mx)) => vals.push(serde_json::json!({
                                    "exchange": trim_dot(&mx.exchange.to_string()),
                                    "priority": mx.preference,
                                })),
                                ("txt", RData::TXT(t)) => {
                                    let parts: Vec<serde_json::Value> = t
                                        .txt_data
                                        .iter()
                                        .map(|b| serde_json::Value::String(
                                            String::from_utf8_lossy(b).into_owned(),
                                        ))
                                        .collect();
                                    vals.push(serde_json::Value::Array(parts));
                                }
                                ("srv", RData::SRV(s)) => vals.push(serde_json::json!({
                                    "name": trim_dot(&s.target.to_string()), "port": s.port,
                                    "priority": s.priority, "weight": s.weight,
                                })),
                                ("ptr", RData::PTR(n)) => {
                                    vals.push(serde_json::Value::String(trim_dot(&n.to_string())));
                                }
                                ("a", RData::A(a)) => {
                                    vals.push(serde_json::Value::String(a.to_string()));
                                }
                                ("aaaa", RData::AAAA(a)) => {
                                    vals.push(serde_json::Value::String(a.to_string()));
                                }
                                _ => {}
                            }
                        }
                        Ok(serde_json::Value::Array(vals))
                    }
                    Err(e) => {
                        let text = e.to_string();
                        Err((node_code_for(&text).to_string(), text))
                    }
                }
            });
            let _ = tx.send(out);
        });
    if spawned.is_err() {
        return Err(("EAI_AGAIN".to_string(), "failed to spawn dns thread".to_string()));
    }
    rx.recv()
        .unwrap_or_else(|_| Err(("EAI_AGAIN".to_string(), "dns thread died".to_string())))
}

/// `__wjs_dns_query(kind, name)` → 结果 JSON；失败抛 `CODE: queryKind 'name': msg`。
pub unsafe extern "C" fn dns_query(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: dns query needs kind and name");
        return false;
    }
    let kind = value_to_string(&mut cx, frame.arg(0));
    let name = value_to_string(&mut cx, frame.arg(1));
    if name.is_empty() {
        report_error(&mut cx, &format!("EBADNAME: query{kind} '': empty hostname"));
        return false;
    }
    match query_blocking(&kind, &name) {
        Ok(v) => {
            let text = v.to_string();
            rooted!(&in(cx) let mut r = UndefinedValue());
            text.as_str().to_jsval(&mut cx, r.handle_mut());
            frame.set_rval(r.get());
            true
        }
        Err((code, msg)) => {
            report_error(&mut cx, &format!("{code}: query{kind} '{name}': {msg}"));
            false
        }
    }
}

/// `__wjs_dns_servers_get()` → JSON 数组。
pub unsafe extern "C" fn dns_servers_get(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let text = serde_json::Value::Array(
        effective_servers()
            .into_iter()
            .map(serde_json::Value::String)
            .collect(),
    )
    .to_string();
    rooted!(&in(cx) let mut r = UndefinedValue());
    text.as_str().to_jsval(&mut cx, r.handle_mut());
    frame.set_rval(r.get());
    true
}

/// `__wjs_dns_servers_set(json)`：校验 IP 数组后存入覆盖层。
pub unsafe extern "C" fn dns_servers_set(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: setServers needs an array");
        return false;
    }
    let text = value_to_string(&mut cx, frame.arg(0));
    let parsed: serde_json::Value =
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let serde_json::Value::Array(items) = parsed else {
        report_error(&mut cx, "TypeError: setServers needs an array of IP addresses");
        return false;
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let serde_json::Value::String(s) = item else {
            report_error(&mut cx, "TypeError: setServers needs an array of IP addresses");
            return false;
        };
        if s.parse::<IpAddr>().is_err() {
            report_error(&mut cx, &format!("TypeError: setServers got invalid IP '{s}'"));
            return false;
        }
        out.push(s);
    }
    *servers_slot().lock() = Some(out);
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs_dns_order_get()` → `"verbatim"` / `"ipv4first"`。
pub unsafe extern "C" fn dns_order_get(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let text = order_slot().lock().clone();
    rooted!(&in(cx) let mut r = UndefinedValue());
    text.as_str().to_jsval(&mut cx, r.handle_mut());
    frame.set_rval(r.get());
    true
}

/// `__wjs_dns_order_set(s)`：只收 verbatim/ipv4first。
pub unsafe extern "C" fn dns_order_set(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: setDefaultResultOrder needs a value");
        return false;
    }
    let text = value_to_string(&mut cx, frame.arg(0));
    if text != "verbatim" && text != "ipv4first" {
        report_error(
            &mut cx,
            &format!("TypeError: setDefaultResultOrder got '{text}' (verbatim/ipv4first)"),
        );
        return false;
    }
    *order_slot().lock() = text;
    frame.set_rval(UndefinedValue());
    true
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
// ── 10d 深件：hickory 查询（callback + promise 双形态，Node 错误形状）──
function __qkind(kind, hostname) {
  const text = __wjs_dns_query(kind, String(hostname));
  return JSON.parse(text);
}
function __qerr(kind, hostname, e) {
  const msg = String((e && e.message) || e);
  const m = msg.match(/^([A-Z_]+):\s*/);
  const code = m ? m[1] : "ENOTFOUND";
  const err = new Error(`${code}: query${kind} '${hostname}': ${msg}`);
  err.code = code;
  err.syscall = `query${kind[0].toUpperCase()}${kind.slice(1)}`;
  err.hostname = String(hostname);
  throw err;
}
function __mkQuery(kind, syscall) {
  const fn_ = function (hostname, cb) {
    if (typeof hostname !== "string") throw new TypeError(`dns.${syscall}: hostname must be a string`);
    if (typeof cb !== "function") throw new TypeError(`dns.${syscall}: callback must be a function`);
    queueMicrotask(() => {
      try { cb(null, __qkind(kind, hostname)); }
      catch (e) {
        try { __qerr(kind, hostname, e); } catch (err) { cb(err); }
      }
    });
  };
  return fn_;
}
export const resolveCname = __mkQuery("cname", "resolveCname");
export const resolveMx = __mkQuery("mx", "resolveMx");
export const resolveNs = __mkQuery("ns", "resolveNs");
export const resolveTxt = __mkQuery("txt", "resolveTxt");
export const resolveSrv = __mkQuery("srv", "resolveSrv");
export const resolvePtr = __mkQuery("ptr", "resolvePtr");
export const resolveAny = __mkQuery("any", "resolveAny");
export function resolve(hostname, rrtype, cb) {
  if (typeof rrtype === "function") { cb = rrtype; rrtype = "A"; }
  if (typeof cb !== "function") throw new TypeError("dns.resolve: callback must be a function");
  const t = String(rrtype || "A").toUpperCase();
  const map = { CNAME: "cname", MX: "mx", NS: "ns", TXT: "txt", SRV: "srv", PTR: "ptr", ANY: "any", A: "a", AAAA: "aaaa" };
  const kind = map[t];
  if (!kind) {
    const err = new TypeError(`dns.resolve: unknown rrtype '${rrtype}'`);
    err.code = "EBADNAME";
    throw err;
  }
  queueMicrotask(() => {
    try { cb(null, __qkind(kind, hostname)); }
    catch (e) {
      try { __qerr(kind, hostname, e); } catch (err) { cb(err); }
    }
  });
}
export function reverse(ip, cb) {
  if (typeof cb !== "function") throw new TypeError("dns.reverse: callback must be a function");
  queueMicrotask(() => {
    try { cb(null, __qkind("reverse", ip)); }
    catch (e) {
      try { __qerr("reverse", ip, e); } catch (err) { cb(err); }
    }
  });
}
export function getServers() { return JSON.parse(__wjs_dns_servers_get()); }
export function setServers(servers) {
  if (!Array.isArray(servers)) throw new TypeError("dns.setServers: servers must be an array");
  __wjs_dns_servers_set(JSON.stringify(servers.map(String)));
}
export function getDefaultResultOrder() { return __wjs_dns_order_get(); }
export function setDefaultResultOrder(order) { __wjs_dns_order_set(String(order)); }
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
// 回调转 promise（resolve 系统一包一层，保持 callback 语义单源）
function __promisifyQuery(kind, syscall) {
  return (hostname) => new Promise((resolveP, reject) => {
    if (typeof hostname !== "string") throw new TypeError(`dns.${syscall}: hostname must be a string`);
    queueMicrotask(() => {
      try { resolveP(__qkind(kind, hostname)); }
      catch (e) {
        try { __qerr(kind, hostname, e); } catch (err) { reject(err); }
      }
    });
  });
}
export const promises = {
  lookup: (hostname, options) => Promise.resolve().then(() => __lookupCore(hostname, options)),
  resolve4: __as(resolve4b),
  resolve6: __as(resolve6b),
  resolveCname: __promisifyQuery("cname", "resolveCname"),
  resolveMx: __promisifyQuery("mx", "resolveMx"),
  resolveNs: __promisifyQuery("ns", "resolveNs"),
  resolveTxt: __promisifyQuery("txt", "resolveTxt"),
  resolveSrv: __promisifyQuery("srv", "resolveSrv"),
  resolvePtr: __promisifyQuery("ptr", "resolvePtr"),
  resolveAny: __promisifyQuery("any", "resolveAny"),
  resolve: (hostname, rrtype) => {
    const t = String(rrtype || "A").toUpperCase();
    const map = { CNAME: "cname", MX: "mx", NS: "ns", TXT: "txt", SRV: "srv", PTR: "ptr", ANY: "any", A: "a", AAAA: "aaaa" };
    const kind = map[t];
    if (!kind) {
      const err = new TypeError(`dns.resolve: unknown rrtype '${rrtype}'`);
      err.code = "EBADNAME";
      throw err;
    }
    return __promisifyQuery(kind, "resolve")(hostname);
  },
  reverse: (ip) => new Promise((resolveP, reject) => {
    queueMicrotask(() => {
      try { resolveP(__qkind("reverse", ip)); }
      catch (e) {
        try { __qerr("reverse", ip, e); } catch (err) { reject(err); }
      }
    });
  }),
  getServers: () => Promise.resolve().then(() => getServers()),
  setServers: (s) => Promise.resolve().then(() => setServers(s)),
  getDefaultResultOrder: () => Promise.resolve().then(() => getDefaultResultOrder()),
  setDefaultResultOrder: (o) => Promise.resolve().then(() => setDefaultResultOrder(o)),
};
function resolve4b(hostname) { return __filter(__entries(hostname), 4).map((e) => e.address); }
function resolve6b(hostname) { return __filter(__entries(hostname), 6).map((e) => e.address); }
const __api = { lookup, resolve4, resolve6, resolve, resolveCname, resolveMx, resolveNs, resolveTxt, resolveSrv, resolvePtr, resolveAny, reverse, getServers, setServers, getDefaultResultOrder, setDefaultResultOrder, promises };
export default __api;
export { resolve4b as __resolve4, resolve6b as __resolve6 };
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_node_codes() {
        reset_for_tests();
        assert_eq!(node_code_for("DNS error: NoRecordsFound(..)"), "ENODATA");
        assert_eq!(node_code_for("nxdomain: no such host"), "ENOTFOUND");
        assert_eq!(node_code_for("request timed out"), "TIMEOUT");
        assert_eq!(node_code_for("refused by server"), "EREFUSED");
        assert_eq!(node_code_for("servfail"), "SERVFAIL");
        assert_eq!(node_code_for("totally unknown blah"), "ENOTFOUND");
        reset_for_tests();
    }

    #[test]
    fn dns_servers_roundtrip() {
        reset_for_tests();
        let first = effective_servers();
        assert!(!first.is_empty());
        *servers_slot().lock() = Some(vec!["1.1.1.1".to_string()]);
        assert_eq!(effective_servers(), vec!["1.1.1.1".to_string()]);
        *order_slot().lock() = "ipv4first".to_string();
        assert_eq!(order_slot().lock().as_str(), "ipv4first");
        reset_for_tests();
    }

    #[test]
    fn dns_trim_dot() {
        assert_eq!(trim_dot("localhost."), "localhost");
        assert_eq!(trim_dot("a.b."), "a.b");
        assert_eq!(trim_dot("127.0.0.1"), "127.0.0.1");
    }

    #[test]
    fn dns_resolver_config_builds() {
        use hickory_resolver::config::ResolverConfig;
        let cfg = resolver_config_for(&["127.0.0.1".to_string(), "nope".to_string()]);
        assert_eq!(cfg.name_servers().len(), 1);
        let empty = ResolverConfig::default();
        assert!(empty.name_servers().is_empty());
    }
}
