//! `node:dns`：lookup（std `ToSocketAddrs` 底座）+ 深件经 hickory（10d/10f）。
//!
//! 10d（用户拍板全套）：Cname/Mx/Txt/Srv/Ns/Ptr + resolveAny +
//! getServers/setServers/setDefaultResultOrder，读系统 DNS 配置；
//! `lookup` 维持 std（真机 getaddrinfo 口径）。
//! 10f：`Resolver` 独立实例（自有 servers/timeout/tries）经自建 UDP 单问路径
//! （`hickory-proto` 编解码 + tokio UdpSocket 传输，零新依赖）：hickory-resolver
//! 的 `NameServerConfig` 无端口面（non_exhaustive 外部不可构造），系统路径沿用
//! hickory（含 CNAME 跟随/hosts），定制路径单问直取（无跟随，stub 口径）。
//! 偏差记档：同步阻塞解析（helper 线程回结果）；无 DNSSEC；定制路径无 CNAME
//! 跟随/轮询细节（attempts×servers 顺序重试）；`lookupService` 见 JS 服务表记档。

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
        return "ETIMEOUT";
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
        // 规范形取 IP（端口仅定制路径可用；系统路径记档忽略端口）
        let ip = parse_server_addr(s)
            .map(|d| d.ip)
            .or_else(|| s.parse::<IpAddr>().ok());
        if let Some(ip) = ip {
            cfg.add_name_server(NameServerConfig::udp_and_tcp(ip));
        }
    }
    cfg
}

// ── 10f 定制路径：自建 UDP 单问（hickory-proto 编解码 + tokio 传输）─────────
// hickory-resolver 的 NameServerConfig 无端口面（non_exhaustive），
// `Resolver` 实例的高端口 servers（stub DNS）走此路；系统路径沿用 hickory
// （`resolver_config_for`，见上）。

/// 规范形 server 串（JS 已按 Node 口径校验 canonicalize，此处为 backstop）：
/// `ip` / `ip:port` / `[v6]` / `[v6]:port`（port 0 视 53）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct DnsServer {
    ip: IpAddr,
    port: u16,
}

fn parse_server_addr(s: &str) -> Option<DnsServer> {
    const DEFAULT_PORT: u16 = 53;
    let norm_port = |p: u16| if p == 0 { DEFAULT_PORT } else { p };
    if let Some(rest) = s.strip_prefix('[') {
        let end = rest.find(']')?;
        let ip: IpAddr = rest[..end].parse().ok()?;
        let tail = &rest[end + 1..];
        if tail.is_empty() {
            return Some(DnsServer { ip, port: DEFAULT_PORT });
        }
        let digits = tail.strip_prefix(':')?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let port: u16 = digits.parse().ok()?;
        return Some(DnsServer { ip, port: norm_port(port) });
    }
    match s.rsplit_once(':') {
        Some((host, digits))
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            // 裸 v6 含多个 ':'，只有恰一个 ':' 才视 host:port
            if host.contains(':') {
                let ip: IpAddr = s.parse().ok()?;
                Some(DnsServer { ip, port: DEFAULT_PORT })
            } else {
                let ip: IpAddr = host.parse().ok()?;
                let port: u16 = digits.parse().ok()?;
                Some(DnsServer { ip, port: norm_port(port) })
            }
        }
        _ => {
            let ip: IpAddr = s.parse().ok()?;
            Some(DnsServer { ip, port: DEFAULT_PORT })
        }
    }
}

#[derive(Debug)]
enum UdpFail {
    Timeout,
    BadResponse(String),
    Io(String),
}

fn udp_fail_code(f: &UdpFail) -> (&'static str, String) {
    match f {
        UdpFail::Timeout => ("ETIMEOUT", "query timed out".to_string()),
        UdpFail::BadResponse(m) => ("EBADRESP", m.clone()),
        UdpFail::Io(m) => {
            // ECONNREFUSED 与 EAI_AGAIN 按字面先行，其余归 EAI_AGAIN（hermetic 形状）
            let l = m.to_ascii_lowercase();
            if l.contains("refused") {
                ("ECONNREFUSED", m.clone())
            } else {
                ("EAI_AGAIN", m.clone())
            }
        }
    }
}

/// 单次 UDP 问答（调用方已备好 runtime；id 由调用方给，便于应答核对）。
async fn udp_ask(
    server: &DnsServer,
    name: &str,
    qtype: hickory_resolver::proto::rr::RecordType,
    id: u16,
    timeout: std::time::Duration,
) -> Result<hickory_resolver::proto::op::Message, UdpFail> {
    use hickory_resolver::proto::op::{Message, MessageType, OpCode, Query};
    use hickory_resolver::proto::rr::{Name, RecordType};
    use std::str::FromStr as _;
    let qname = if qtype == RecordType::PTR && name.parse::<IpAddr>().is_ok() {
        Name::from(name.parse::<IpAddr>().expect("checked"))
    } else {
        Name::from_str(name).map_err(|e| UdpFail::BadResponse(format!("bad name '{name}': {e}")))?
    };
    let mut msg = Message::new(id, MessageType::Query, OpCode::Query);
    msg.metadata.recursion_desired = true;
    msg.add_query(Query::query(qname, qtype));
    let bytes = msg
        .to_vec()
        .map_err(|e| UdpFail::BadResponse(format!("encode: {e}")))?;
    let bind = if server.ip.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let sock = tokio::net::UdpSocket::bind(bind)
        .await
        .map_err(|e| UdpFail::Io(e.to_string()))?;
    sock.send_to(&bytes, std::net::SocketAddr::new(server.ip, server.port))
        .await
        .map_err(|e| UdpFail::Io(e.to_string()))?;
    // 10f：64KB 收包（多应答 ANY 如 257 条 A，约 5KB+；loopback 上无分片之忧）
    let mut buf = vec![0u8; 65535];
    let (n, _) = tokio::time::timeout(timeout, sock.recv_from(&mut buf))
        .await
        .map_err(|_| UdpFail::Timeout)?
        .map_err(|e| UdpFail::Io(e.to_string()))?;
    let resp = Message::from_vec(&buf[..n])
        .map_err(|e| UdpFail::BadResponse(format!("decode: {e}")))?;
    if resp.metadata.id != id {
        return Err(UdpFail::BadResponse(format!(
            "id mismatch (want {id}, got {})",
            resp.metadata.id
        )));
    }
    Ok(resp)
}

/// 定制 servers 查询：attempts 轮 × servers 顺序，首个成功即收；
/// 全败回末错。`types` 为单问类型；`want_any` 时按 ANY 单问取全应答。
/// 定制 servers 查询本体（无线程，调用方给线程；attempts 轮 × servers 顺序，
/// 首个成功即收；全败回末错。`types` 为单问类型；ANY 单问取全应答）。
fn query_custom_sync(
    servers: &[DnsServer],
    name: &str,
    qtype: hickory_resolver::proto::rr::RecordType,
    timeout: std::time::Duration,
    tries: u32,
    max_timeout: std::time::Duration,
) -> Result<Vec<serde_json::Value>, (String, String)> {
    if servers.is_empty() {
        return Err(("ENOTFOUND".to_string(), "no DNS servers".to_string()));
    }
    let name = name.to_string();
    let servers = servers.to_vec();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ("EAI_AGAIN".to_string(), format!("dns runtime: {e}")))?;
    rt.block_on(async move {
        let mut last: (String, String) =
            ("ETIMEOUT".to_string(), "query timed out".to_string());
        // c-ares 退避：无 maxTimeout 时每轮超时翻倍，有则截顶（max-timeout 套件门）
        let mut attempt_timeout = timeout;
        for _ in 0..tries.max(1) {
            if !max_timeout.is_zero() && attempt_timeout > max_timeout {
                attempt_timeout = max_timeout;
            }
            for s in &servers {
                // 10f：rand 0.10 在树内（Uuidv4 同源），id 随机防串扰
                let id: u16 = rand::random();
                match udp_ask(s, &name, qtype, id, attempt_timeout).await {
                    Ok(resp) => {
                        let mut vals = Vec::new();
                        for rec in &resp.answers {
                            if let Some(v) = record_to_json(
                                rec.record_type(),
                                &rec.data,
                                rec.ttl,
                            ) {
                                vals.push(v);
                            }
                        }
                        // ANY 取全应答；SOA 带 type 键但不过滤亦可（单问只取首）；
                        // 他型只收同型（stub 回包可能带杂项）
                        if qtype
                            != hickory_resolver::proto::rr::RecordType::ANY
                            && qtype
                                != hickory_resolver::proto::rr::RecordType::SOA
                        {
                            vals.retain(|v| {
                                v.get("type").and_then(|t| t.as_str())
                                    == Some(match qtype {
                                        hickory_resolver::proto::rr::RecordType::A => "A",
                                        hickory_resolver::proto::rr::RecordType::AAAA => "AAAA",
                                        hickory_resolver::proto::rr::RecordType::CNAME => "CNAME",
                                        hickory_resolver::proto::rr::RecordType::MX => "MX",
                                        hickory_resolver::proto::rr::RecordType::NS => "NS",
                                        hickory_resolver::proto::rr::RecordType::TXT => "TXT",
                                        hickory_resolver::proto::rr::RecordType::SRV => "SRV",
                                        hickory_resolver::proto::rr::RecordType::SOA => "SOA",
                                        hickory_resolver::proto::rr::RecordType::PTR => "PTR",
                                        _ => "",
                                    })
                            });
                        }
                        if vals.is_empty() {
                            last = (
                                "ENODATA".to_string(),
                                "no records found".to_string(),
                            );
                            continue;
                        }
                        return Ok(vals);
                    }
                    Err(f) => {
                        last = {
                            let (c, m) = udp_fail_code(&f);
                            (c.to_string(), m)
                        };
                    }
                }
            }
            attempt_timeout = attempt_timeout.saturating_mul(2);
        }
        Err(last)
    })
}

fn record_to_json(
    rtype: hickory_resolver::proto::rr::RecordType,
    rdata: &hickory_resolver::proto::rr::RData,
    ttl: u32,
) -> Option<serde_json::Value> {
    use hickory_resolver::proto::rr::RData;
    match rdata {
        // 10f：A/AAAA 带 ttl（resolveAny/resolve4-ttl 口径；地址形态由 JS 侧归一）
        RData::A(a) => Some(serde_json::json!({"type": "A", "address": a.to_string(), "ttl": ttl})),
        RData::AAAA(a) => Some(serde_json::json!({"type": "AAAA", "address": a.to_string(), "ttl": ttl})),
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
        // 10f：SOA 全键（含 type；resolveSoa/resolveAny 共用，套件逐字钉死）
        RData::SOA(s) => Some(serde_json::json!({
            "type": "SOA",
            "nsname": trim_dot(&s.mname.to_string()),
            "hostmaster": trim_dot(&s.rname.to_string()),
            "serial": s.serial, "refresh": s.refresh, "retry": s.retry,
            "expire": s.expire, "minttl": s.minimum,
        })),
        // 10f：CAA issue 形（{type, critical, issue}；他 tag 仅 critical+tag 记档）
        RData::CAA(c) => {
            let critical = (if c.issuer_critical { 128 } else { 0 }) | c.reserved_flags;
            if c.tag == "issue" {
                Some(serde_json::json!({
                    "type": "CAA", "critical": critical,
                    "issue": String::from_utf8_lossy(&c.value).into_owned(),
                }))
            } else {
                Some(serde_json::json!({"type": "CAA", "critical": critical, "tag": c.tag}))
            }
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
                    "soa" => RecordType::SOA,
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
                                if let Some(v) = record_to_json(
                                    rec.record_type(),
                                    &rec.data,
                                    rec.ttl,
                                ) {
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
                                    vals.push(serde_json::json!({
                                        "type": "A", "address": a.to_string(), "ttl": rec.ttl,
                                    }));
                                }
                                ("aaaa", RData::AAAA(a)) => {
                                    vals.push(serde_json::json!({
                                        "type": "AAAA", "address": a.to_string(), "ttl": rec.ttl,
                                    }));
                                }
                                ("soa", RData::SOA(s)) => {
                                    vals.push(serde_json::json!({
                                        "nsname": trim_dot(&s.mname.to_string()),
                                        "hostmaster": trim_dot(&s.rname.to_string()),
                                        "serial": s.serial, "refresh": s.refresh,
                                        "retry": s.retry, "expire": s.expire,
                                        "minttl": s.minimum,
                                    }));
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
    dns_query_inner(&mut cx, &frame, &kind, &name)
}

/// `__wjs_dns_job_start(kind, name, servers_json, timeout_ms, tries, max_timeout_ms)`
/// 定制查询的异步投递：helper 线程跑 `query_custom_sync`，结果进 job 表；
/// JS 侧 `setInterval` 轮询 `__wjs_dns_job_poll` 收割（5ms 粒度，refed 保活，
/// 结算即清）。阻塞 native 会停转事件循环（stub 回包无人分发，见 §4.x），
/// 故定制路径永不阻塞（系统 hickory 路径沿旧同步语义，不动）。
/// `servers_json` 为 JSON 数组（规范形）；`timeout_ms < 0` 取 5000；
/// `tries <= 0` 取 2。id 失败（参数坏）回空串（JS 侧已校验，此为 backstop）。
pub unsafe extern "C" fn dns_job_start(
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
    let servers_json = if frame.argc() >= 3 {
        value_to_string(&mut cx, frame.arg(2))
    } else {
        String::new()
    };
    let timeout_ms: i64 = if frame.argc() >= 4 {
        value_to_string(&mut cx, frame.arg(3)).parse().unwrap_or(-1)
    } else {
        -1
    };
    let tries: i64 = if frame.argc() >= 5 {
        value_to_string(&mut cx, frame.arg(4)).parse().unwrap_or(0)
    } else {
        0
    };
    let timeout = std::time::Duration::from_millis(if timeout_ms < 0 {
        5000
    } else {
        timeout_ms as u64
    });
    let tries = if tries <= 0 { 2 } else { tries as u32 };
    let max_timeout_ms: i64 = if frame.argc() >= 6 {
        value_to_string(&mut cx, frame.arg(5)).parse().unwrap_or(-1)
    } else {
        -1
    };
    let max_timeout = if max_timeout_ms <= 0 {
        std::time::Duration::ZERO
    } else {
        std::time::Duration::from_millis(max_timeout_ms as u64)
    };
    use hickory_resolver::proto::rr::RecordType;
    let qtype = match kind.as_str() {
        "a" => RecordType::A,
        "aaaa" => RecordType::AAAA,
        "cname" => RecordType::CNAME,
        "mx" => RecordType::MX,
        "ns" => RecordType::NS,
        "txt" => RecordType::TXT,
        "srv" => RecordType::SRV,
        "soa" => RecordType::SOA,
        "ptr" | "reverse" => RecordType::PTR,
        "any" => RecordType::ANY,
        _ => RecordType::A,
    };
    let servers: Vec<DnsServer> = serde_json::from_str::<serde_json::Value>(&servers_json)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .map(|items| {
            items
                .into_iter()
                .filter_map(|i| i.as_str().and_then(parse_server_addr))
                .collect()
        })
        .unwrap_or_default();
    let id = DNS_JOB_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dns_jobs().lock().insert(id, None);
    let spawned = std::thread::Builder::new()
        .name("winterjs-dns-job".into())
        .spawn(move || {
            let res = std::panic::catch_unwind(|| {
                query_custom_sync(&servers, &name, qtype, timeout, tries, max_timeout)
            })
            .unwrap_or_else(|_| {
                Err(("EAI_AGAIN".to_string(), "dns worker panicked".to_string()))
            });
            if let Some(slot) = dns_jobs().lock().get_mut(&id) {
                *slot = Some(res);
            }
        });
    if spawned.is_err() {
        dns_jobs().lock().remove(&id);
        report_error(&mut cx, "EAI_AGAIN: failed to spawn dns thread");
        return false;
    }
    let text = id.to_string();
    rooted!(&in(cx) let mut r = UndefinedValue());
    text.as_str().to_jsval(&mut cx, r.handle_mut());
    frame.set_rval(r.get());
    true
}

/// `__wjs_dns_job_poll(id)` → `{"status":"pending"}` /
/// `{"status":"ok","value":[...]}` / `{"status":"err","code","msg"}`。
/// 取走即摘除（收割语义）；未知 id 回 pending（已收割/已遗忘不报错）。
pub unsafe extern "C" fn dns_job_poll(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let text = if frame.argc() < 1 {
        "{\"status\":\"pending\"}".to_string()
    } else {
        let id: u64 = value_to_string(&mut cx, frame.arg(0)).parse().unwrap_or(u64::MAX);
        let done = matches!(dns_jobs().lock().get(&id), Some(Some(_)));
        if !done {
            "{\"status\":\"pending\"}".to_string()
        } else {
            match dns_jobs().lock().remove(&id) {
                Some(Some(Ok(v))) => {
                    serde_json::json!({"status": "ok", "value": v}).to_string()
                }
                Some(Some(Err((code, msg)))) => {
                    serde_json::json!({"status": "err", "code": code, "msg": msg}).to_string()
                }
                _ => "{\"status\":\"pending\"}".to_string(),
            }
        }
    };
    rooted!(&in(cx) let mut r = UndefinedValue());
    text.as_str().to_jsval(&mut cx, r.handle_mut());
    frame.set_rval(r.get());
    true
}

/// `__wjs_dns_job_forget(id)` → 丢弃 job（cancel 后线程迟归不堆积）。
pub unsafe extern "C" fn dns_job_forget(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() >= 1 {
        if let Ok(id) = value_to_string(&mut cx, frame.arg(0)).parse::<u64>() {
            dns_jobs().lock().remove(&id);
        }
    }
    frame.set_rval(UndefinedValue());
    true
}

static DNS_JOB_NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// job 表：id → None（在跑）/ Some（已落定）。cancel 不杀线程（结果自然沉底，
/// JS 侧 forget 摘除）；线程必落定（panic 亦收敛为 EAI_AGAIN），无悬垂。
fn dns_jobs() -> &'static parking_lot::Mutex<std::collections::HashMap<u64, Option<Result<Vec<serde_json::Value>, (String, String)>>>> {
    static SLOT: OnceLock<
        parking_lot::Mutex<std::collections::HashMap<u64, Option<Result<Vec<serde_json::Value>, (String, String)>>>>,
    > = OnceLock::new();
    SLOT.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

/// `dns_query` 的共享收尾（供新老两入口复用）。
fn dns_query_inner(
    cx: &mut mozjs::context::JSContext,
    frame: &crate::jsapi_glue::Frame,
    kind: &str,
    name: &str,
) -> bool {
    match query_blocking(kind, name) {
        Ok(v) => {
            let text = v.to_string();
            rooted!(&in(cx) let mut r = UndefinedValue());
            text.as_str().to_jsval(&mut *cx, r.handle_mut());
            frame.set_rval(r.get());
            true
        }
        Err((code, msg)) => {
            report_error(&mut *cx, &format!("{code}: query{kind} '{name}': {msg}"));
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

/// `__wjs_dns_servers_set(json)`：存规范形覆盖层（JS 已按 Node 口径校验 +
/// canonicalize；此处只做 backstop 解析，非法项跳过——空即清空）。
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
        // 规范形（含端口）经 parse_server_addr 回 canonical；裸 IP 直存
        if parse_server_addr(&s).is_none() && s.parse::<IpAddr>().is_err() {
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
// ── 10f：Node 精确校验（internal/validators 口径，套件逐字断言）─────────────
function __dnsReceived(v) {
  if (v === null) return 'null';
  if (v === undefined) return 'undefined';
  const t = typeof v;
  // invalidArgTypeHelper 口径（字符串带类型前缀；ARG_VALUE 另用 inspect 形）
  if (t === 'string') return `type string ('${v}')`;
  if (t === 'number' || t === 'boolean' || t === 'bigint') return `type ${t} (${String(v)})`;
  if (t === 'symbol') return `type symbol (${String(v)})`;
  if (t === 'function') return `function ${v.name || '(anonymous)'}`;
  const name = v.constructor?.name;
  if (typeof name === 'string' && name !== '') return `an instance of ${name}`;
  return 'an instance of Object';
}
function __dnsInspectValue(v) {
  // kInspect 口径（ERR_INVALID_ARG_VALUE 的 Received 部分）：字符串单引裸形
  if (typeof v === 'string') return `'${v}'`;
  if (v === null || v === undefined) return String(v);
  const t = typeof v;
  if (t === 'number' || t === 'boolean' || t === 'bigint') return String(v);
  return __dnsReceived(v);
}
// dns 标志位（c-ares AI_* 口径；hints 掩码校验用）
const V4MAPPED = 2048;
const ADDRCONFIG = 1024;
const ALL = 256;
function __dnsValidateLookupOptions(options) {
  // 真机口径：falsy options 整体跳过（`lookup(h, -0, cb)` 绿）；数字 family
  // 由 lookup 入口先转 { family }（custom-lookup 套件 `lookup(h, 4, cb)` 真机绿）。
  if (!options) return undefined;
  if (typeof options !== "object" || options === null) {
    throw __dnsErrInvalidArgType("options", "of type object", options);
  }
  const { family, hints, order } = options;
  let fam = 0;
  if (family !== undefined) {
    if (family === 0 || family === 4 || family === 6 || family === "IPv4" || family === "IPv6") {
      fam = family === "IPv4" ? 4 : family === "IPv6" ? 6 : family;
    } else {
      throw __dnsErrInvalidArgType("family", "of type number", family);
    }
  }
  if (hints !== undefined) {
    if (typeof hints !== "number" || !Number.isInteger(hints) || (hints & ~(V4MAPPED | ADDRCONFIG | ALL)) !== 0) {
      const e = new TypeError(`The argument 'hints' is invalid. Received ${String(hints)}`);
      e.code = "ERR_INVALID_ARG_VALUE";
      throw e;
    }
  }
  if (order !== undefined && order !== "verbatim" && order !== "ipv4first" && order !== "ipv6first") {
    throw __dnsErrInvalidArgType("order", "of type string", order);
  }
  return { ...options, family: fam };
}
function __dnsErrInvalidArgType(name, expected, actual) {
  const e = new TypeError(`The "${name}" argument must be ${expected}. Received ${__dnsReceived(actual)}`);
  e.code = 'ERR_INVALID_ARG_TYPE';
  return e;
}
function __dnsValidateString(v, name) {
  if (typeof v !== 'string') throw __dnsErrInvalidArgType(name, 'of type string', v);
}
function __dnsValidateFunction(v, name) {
  if (typeof v !== 'function') throw __dnsErrInvalidArgType(name, 'of type function', v);
}
// isIP（Node internal/net 口径子集：v4 严格四段 / v6 压缩形 + v4 尾；
// 纯 JS，线性扫描无回溯——REDOS 探针（10 万冒号）安全）
function __dnsIsV4(s) {
  const parts = s.split('.');
  if (parts.length !== 4) return false;
  for (const p of parts) {
    if (p.length === 0 || p.length > 3) return false;
    let n = 0;
    for (let i = 0; i < p.length; i++) {
      const c = p.charCodeAt(i);
      if (c < 48 || c > 57) return false;
      n = n * 10 + (c - 48);
    }
    if (n > 255) return false;
  }
  return true;
}
function __dnsIsV6(s) {
  const dc = s.indexOf('::');
  if (dc !== -1 && s.indexOf('::', dc + 2) !== -1) return false;
  const head = dc === -1 ? s : s.slice(0, dc);
  const tail = dc === -1 ? null : s.slice(dc + 2);
  return __dnsIsV6Inner(head, tail);
}
function __dnsIsV6Inner(head, tail) {
  // 段收集（含 v4 尾：末段 dotted-quad 占两组）
  const collect = (part) => {
    if (part === '') return [];
    const segs = part.split(':');
    const out = [];
    for (let i = 0; i < segs.length; i++) {
      const g = segs[i];
      if (i === segs.length - 1 && g.indexOf('.') !== -1) {
        if (!__dnsIsV4(g)) return null;
        out.push('v4', 'v4');
      } else {
        if (g.length === 0 || g.length > 4) return null;
        for (let j = 0; j < g.length; j++) {
          const c = g.charCodeAt(j);
          if (!((c >= 48 && c <= 57) || (c >= 65 && c <= 70) || (c >= 97 && c <= 102))) return null;
        }
        out.push(g);
      }
    }
    return out;
  };
  const h = collect(head);
  if (h === null) return false;
  if (tail === null) return h.length === 8;
  const t = collect(tail);
  if (t === null) return false;
  // '::' 填充剩余组（含 '::' 本身 total 0；满 8 组时不得再有 '::'）
  return h.length + t.length <= 7;
}
function __dnsIsIP(s) {
  if (typeof s !== 'string') return 0;
  if (__dnsIsV4(s)) return 4;
  if (__dnsIsV6(s)) return 6;
  return 0;
}
function __entries(hostname) {
  let raw;
  try {
    raw = JSON.parse(__wjs_dns_lookup(String(hostname)));
  } catch (e) {
    throw __dnsMakeError("getaddrinfo", "EAI_AGAIN", hostname);
  }
  if (!Array.isArray(raw) || raw.length === 0) {
    throw __dnsMakeError("getaddrinfo", "ENOTFOUND", hostname);
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
    throw __dnsMakeError("getaddrinfo", "ENOTFOUND", hostname);
  }
  return all ? picked.map((e) => ({ address: e.address, family: e.family })) : picked[0];
}
export function lookup(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  // 数字 family 重载（custom-lookup 套件：lookup(host, 4, cb) 直通真机；
  // 旧"数字即抛"系伪语义，真机 26.8.2 实测接受）。
  if (typeof options === "number") options = { family: options };
  const norm = __dnsValidateLookupOptions(options);
  // 真机口径：falsy 先行（ARG_VALUE），再类型（ARG_TYPE）——`lookup('')`
  // 系同步抛，非回调错（test-dns.js；旧黑盒按回调错编码，已翻转见 §4.x）
  if (!hostname) {
    const e = new TypeError(`The argument 'hostname' is invalid. Received ${__dnsReceived(hostname)}`);
    e.code = "ERR_INVALID_ARG_VALUE";
    throw e;
  }
  __dnsValidateString(hostname, "hostname");
  __dnsValidateFunction(cb, "callback");
  queueMicrotask(() => {
    try {
      const r = __lookupCore(hostname, norm ?? options);
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
// Node DNSException 全文口径：`${syscall} ${code} ${hostname}`（无 detail；
// `foo.onion` 用例逐字钉死，见 test-dns.js）
function __dnsMakeError(syscall, code, hostname) {
  const err = new Error(`${syscall} ${code} ${hostname}`);
  err.code = code;
  err.syscall = syscall;
  err.hostname = String(hostname);
  return err;
}
function __dnsSyscallOf(kind) {
  return `query${kind[0].toUpperCase()}${kind.slice(1)}`;
}
function __qerr(kind, hostname, e) {
  const msg = String((e && e.message) || e);
  const m = msg.match(/^([A-Z_]+):\s*/);
  const code = m ? m[1] : "ENOTFOUND";
  throw __dnsMakeError(__dnsSyscallOf(kind), code, hostname);
}
function __mkQuery(kind, syscall) {
  const fn_ = function (hostname, cb) {
    __dnsValidateString(hostname, "name");
    __dnsValidateFunction(cb, "callback");
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
export function resolveAny(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  if (__dnsOverrideSnapshot()) { __moduleQueryCustom("any", "queryAny", hostname, cb, false); return; }
  __mkQuery("any", "resolveAny")(hostname, cb);
}
export function resolve(hostname, rrtype, cb) {
  if (typeof rrtype === "function") { cb = rrtype; rrtype = "A"; }
  __dnsValidateString(hostname, "name");
  if (typeof rrtype !== "string") throw __dnsErrInvalidArgType("rrtype", "of type string", rrtype);
  __dnsValidateFunction(cb, "callback");
  const t = String(rrtype || "A").toUpperCase();
  const map = { CNAME: "cname", MX: "mx", NS: "ns", TXT: "txt", SRV: "srv", PTR: "ptr", ANY: "any", A: "a", AAAA: "aaaa", SOA: "soa" };
  const kind = map[t];
  if (!kind) {
    const err = new TypeError(`dns.resolve: unknown rrtype '${rrtype}'`);
    err.code = "EBADNAME";
    throw err;
  }
  queueMicrotask(() => {
    let raw;
    try { raw = JSON.parse(__wjs_dns_query(kind, hostname)); }
    catch (e) {
      try { __qerr(kind, hostname, e); } catch (err) { cb(err); }
      return;
    }
    const norm = __dnsNormResult(kind, raw);
    if (!Array.isArray(norm) || norm.length === 0) {
      cb(__dnsMakeError(__dnsSyscallOf(kind), "ENODATA", hostname));
      return;
    }
    cb(null, kind === "soa" ? norm[0] : norm);
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
// setServers 全套校验（Node internal/dns/utils 口径：forEach 语义跳 holes；
// getter 副作用（length 截断）天然收敛——forEach 只读一次 length；
// 端口 0 视默认；非法 IP → ERR_INVALID_IP_ADDRESS；越界端口 → ERR_SOCKET_BAD_PORT）
function __dnsValidatePort(p) {
  if (!Number.isInteger(p) || p < 0 || p > 65535) {
    const e = new RangeError(`Port should be >= 0 and < 65536. Received ${String(p)}`);
    e.code = "ERR_SOCKET_BAD_PORT";
    throw e;
  }
  return p;
}
function __dnsParseServers(servers) {
  if (!Array.isArray(servers)) {
    throw __dnsErrInvalidArgType("servers", "an instance of Array", servers);
  }
  const out = [];
  servers.forEach((serv, index) => {
    if (typeof serv !== "string") {
      throw __dnsErrInvalidArgType(`servers[${index}]`, "of type string", serv);
    }
    if (__dnsIsIP(serv) !== 0) {
      out.push([__dnsIsIP(serv), serv, 53]);
      return;
    }
    const br = /^\[([^[\]]*)\]/.exec(serv);
    if (br) {
      const v = __dnsIsIP(br[1]);
      if (v !== 0) {
        const rest = serv.slice(br[0].length);
        let port = 53;
        if (rest !== "") {
          const pm = /^:(\d+)$/.exec(rest);
          // 尾巴非 :digits 即 Node 同款宽容（replace 取 $2 为空 → 53）
          port = pm ? __dnsValidatePort(Number(pm[1])) : 53;
          if (port === 0) port = 53;
        }
        out.push([v, br[1], port]);
        return;
      }
    }
    const sp = /(^.+?)(?::(\d+))?$/.exec(serv);
    if (sp) {
      const host = sp[1];
      const v = __dnsIsIP(host);
      if (v !== 0) {
        let port = 53;
        if (sp[2] !== undefined) {
          port = __dnsValidatePort(Number(sp[2]));
          if (port === 0) port = 53;
        }
        out.push([v, host, port]);
        return;
      }
    }
    const e = new TypeError(`Invalid IP address: ${serv}`);
    e.code = "ERR_INVALID_IP_ADDRESS";
    throw e;
  });
  return out;
}
function __dnsPublicForm([ver, ip, port]) {
  if (!port || port === 53) return ip;
  return ver === 6 ? `[${ip}]:${port}` : `${ip}:${port}`;
}
function __dnsWireForm([ver, ip, port]) {
  if (!port || port === 53) return ip;
  return ver === 6 ? `[${ip}]:${port}` : `${ip}:${port}`;
}
export function setServers(servers) {
  const triples = __dnsParseServers(servers);
  __wjs_dns_servers_set(JSON.stringify(triples.map(__dnsPublicForm)));
  // 全局 override 快照（模块级 resolveAny/4/6/Soa 遇 override 走定制单问；
  // 调用时判定，microtask 排空前已 set 即命中——phase10d 行 89/102 序安全）
  __dnsUserServers = triples;
}
let __dnsUserServers = null;
function __dnsOverrideSnapshot() {
  return __dnsUserServers ? __dnsUserServers.map((t) => t.slice()) : null;
}
// 模块级定制查询（全局 override 快照；pending 计数无——模块 setServers 永不抛）
function __moduleQueryCustom(kind, syscall, hostname, cb, wantTtl) {
  const st = { servers: __dnsOverrideSnapshot(), timeout: 5000, tries: 2, pending: 0, jobs: new Map() };
  const wire = JSON.stringify(st.servers.map(__dnsWireForm));
  const id = Number(__wjs_dns_job_start(kind, hostname, wire, "5000", "2"));
  const rec = { poll: 0 };
  st.jobs.set(id, rec);
  const settle = (err, val) => {
    if (!st.jobs.has(id)) return;
    st.jobs.delete(id);
    clearInterval(rec.poll);
    cb(err, val);
  };
  rec.poll = setInterval(() => {
    let r;
    try { r = JSON.parse(__wjs_dns_job_poll(String(id))); }
    catch { return; }
    if (r.status === "pending") return;
    if (r.status === "err") {
      settle(__dnsMakeError(syscall, r.code, hostname), undefined);
      return;
    }
    const norm = __dnsNormResult(kind, r.value);
    if (!Array.isArray(norm) || norm.length === 0) {
      settle(__dnsMakeError(syscall, "ENODATA", hostname), undefined);
      return;
    }
    if (wantTtl && (kind === "a" || kind === "aaaa")) {
      settle(null, norm.map((e) => ({ address: typeof e === "string" ? e : e.address, ttl: (e && e.ttl) || 0 })));
      return;
    }
    settle(null, kind === "soa" ? norm[0] : norm);
  }, 5);
}
export function getDefaultResultOrder() { return __wjs_dns_order_get(); }
export function setDefaultResultOrder(order) { __wjs_dns_order_set(String(order)); }
// 10f：resolve4/6 走 DNS-A/AAAA 查询（hickory 系统路径；`{ttl:true}` 回
// [{address, ttl}]，否则 [address]）。localhost 经 hosts/系统 DNS（phase9d  pin）。
function __resolveAddr(kind, syscall, hostname, options, cb) {
  if (options !== undefined && (typeof options !== "object" || options === null)) {
    throw __dnsErrInvalidArgType("options", "of type object", options);
  }
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  const wantTtl = !!(options && options.ttl);
  __resolverQueryMod(kind, syscall, hostname, cb, wantTtl);
}
// 模块级查询（全局 servers；hickory 同步阻塞语义沿 10d，不动）
function __resolverQueryMod(kind, syscall, hostname, cb, wantTtl) {
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  queueMicrotask(() => {
    let raw;
    try { raw = JSON.parse(__wjs_dns_query(kind, hostname)); }
    catch (e) {
      try { __qerr(kind, hostname, e); } catch (err) { cb(err); }
      return;
    }
    const norm = __dnsNormResult(kind, raw);
    if (!Array.isArray(norm) || norm.length === 0) {
      const cap = syscall.slice(0, 1).toUpperCase() + syscall.slice(1);
      const err = new Error(`ENODATA: query${cap} '${hostname}': no records found`);
      err.code = "ENODATA";
      err.syscall = syscall;
      err.hostname = hostname;
      cb(err);
      return;
    }
    if (wantTtl && (kind === "a" || kind === "aaaa")) {
      cb(null, norm.map((e) => ({ address: typeof e === "string" ? e : e.address, ttl: (e && e.ttl) || 0 })));
      return;
    }
    cb(null, norm);
  });
}
export function resolve4(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  if (__dnsOverrideSnapshot()) {
    const wantTtl = !!(options && options.ttl);
    __moduleQueryCustom("a", "queryA", hostname, cb, wantTtl);
    return;
  }
  __resolveAddr("a", "queryA", hostname, options, cb);
}
export function resolve6(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  if (__dnsOverrideSnapshot()) {
    const wantTtl = !!(options && options.ttl);
    __moduleQueryCustom("aaaa", "queryAaaa", hostname, cb, wantTtl);
    return;
  }
  __resolveAddr("aaaa", "queryAaaa", hostname, options, cb);
}
export function resolveSoa(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  if (__dnsOverrideSnapshot()) { __moduleQueryCustom("soa", "querySoa", hostname, cb, false); return; }
  queueMicrotask(() => {
    let raw;
    try { raw = JSON.parse(__wjs_dns_query("soa", hostname)); }
    catch (e) {
      try { __qerr("soa", hostname, e); } catch (err) { cb(err); }
      return;
    }
    const list = Array.isArray(raw) ? raw : [];
    if (list.length === 0) {
      cb(__dnsMakeError("querySoa", "ENODATA", hostname));
      return;
    }
    cb(null, list[0]);
  });
}
const __as = (fn) => function (...args) { return Promise.resolve().then(() => fn(...args)); };
// 回调转 promise（resolve 系统一包一层，保持 callback 语义单源；
// hostname 同步校验先行——真机口径下 `resolveNs([])` 系同步抛，非 reject）
function __promisifyQuery(kind, syscall) {
  return (hostname) => {
    __dnsValidateString(hostname, "name");
    return new Promise((resolveP, reject) => {
      queueMicrotask(() => {
        try { resolveP(__qkind(kind, hostname)); }
        catch (e) {
          try { __qerr(kind, hostname, e); } catch (err) { reject(err); }
        }
      });
    });
  };
}
// getservbyport 最小静态表（真机查系统库；未知端口回 String(port)，套件接受双形）
const __dnsServices = {
  7: "echo", 9: "discard", 13: "daytime", 19: "chargen", 20: "ftp-data",
  21: "ftp", 22: "ssh", 23: "telnet", 25: "smtp", 37: "time", 42: "nameserver",
  43: "whois", 53: "domain", 67: "bootps", 68: "bootpc", 69: "tftp",
  70: "gopher", 79: "finger", 80: "http", 88: "kerberos", 101: "hostname",
  102: "iso-tsap", 107: "rtelnet", 109: "pop2", 110: "pop3", 111: "sunrpc",
  113: "auth", 115: "sftp", 119: "nntp", 123: "ntp", 143: "imap",
  161: "snmp", 162: "snmptrap", 179: "bgp", 194: "irc", 220: "imap3",
  389: "ldap", 443: "https", 445: "microsoft-ds", 465: "smtps",
  512: "exec", 513: "login", 514: "shell", 515: "printer", 547: "dhcpv6-server",
  548: "dhcpv6-client", 587: "submission", 636: "ldaps", 873: "rsync",
  990: "ftps", 993: "imaps", 995: "pop3s",
};
function __dnsServiceName(port) {
  return __dnsServices[port] ?? String(port);
}
function __dnsMissingArgs(...names) {
  const q = names.map((n) => `"${n}"`);
  const head = q.length > 2 ? `${q.slice(0, -1).join(", ")}, and ${q[q.length - 1]}`
    : q.length === 2 ? `${q[0]} and ${q[1]}` : q[0];
  const e = new TypeError(`The ${head} argument${q.length > 1 ? "s" : ""} must be specified`);
  e.code = "ERR_MISSING_ARGS";
  return e;
}
function __dnsInvalidArgValue(name, value, reason = "is invalid") {
  const e = new TypeError(`The argument '${name}' ${reason}. Received ${__dnsInspectValue(value)}`);
  e.code = "ERR_INVALID_ARG_VALUE";
  return e;
}
export function lookupService(address, port, callback) {
  if (arguments.length !== 3) throw __dnsMissingArgs("address", "port", "callback");
  __lookupServiceImpl(address, port, callback);
}
function __dnsValidatePortLoose(p) {
  // validatePort 宽松形（任意类型先判形，symbol 不求值即拒；-0 归 +0）
  let n;
  if (typeof p === "number") n = p;
  else if (typeof p === "string" && p.trim() !== "") n = Number(p);
  else n = NaN;
  if (!Number.isInteger(n) || n < 0 || n > 65535) {
    const e = new RangeError(`Port should be >= 0 and < 65536. Received ${String(p)}`);
    e.code = "ERR_SOCKET_BAD_PORT";
    throw e;
  }
  return n + 0;
}
function __lookupServiceImpl(address, port, callback) {
  __dnsValidateString(address, "address");
  if (__dnsIsIP(address) === 0) throw __dnsInvalidArgValue("address", address);
  const portCoerced = __dnsValidatePortLoose(port);
  __dnsValidateFunction(callback, "callback");
  queueMicrotask(() => {
    let hostnames;
    try { hostnames = __qkind("reverse", address); }
    catch (e) {
      try { __qerr("reverse", address, e); } catch (err) {
        // getnameinfo 口径：无名即 ENOTFOUND（PTR 查询层的 ENODATA 在此翻译；
        // `reverse()` 本体保持查询语义不动）
        if (err && err.code === "ENODATA") {
          err.code = "ENOTFOUND";
          err.syscall = "getnameinfo";
        }
        callback(err);
        return;
      }
    }
    if (!Array.isArray(hostnames) || hostnames.length === 0) {
      callback(__dnsMakeError("getnameinfo", "ENOTFOUND", address));
      return;
    }
    callback(null, hostnames[0], __dnsServiceName(portCoerced));
  });
}
// ── 10f：Resolver 独立实例（Node internal/dns/utils 口径）──────────────────
// 自有 servers/timeout/tries/maxTimeout；查询经 `__wjs_dns_query_cfg` 定制单问；
// pending 计数 + cancel 代际：cancel 后落定的 in-flight 一律合成 ECANCELLED
// （真机 c-ares 回调语义；线程侧不强杀，只改 JS 侧结算，见模块头注）。
const __resolverPriv = new WeakMap();
function __dnsErrOutOfRange(name, range, value) {
  const e = new RangeError(`The value of "${name}" is out of range. It must be ${range}. Received ${__dnsReceived(value)}`);
  e.code = "ERR_OUT_OF_RANGE";
  return e;
}
function __dnsValidateTimeout(options) {
  const o = options === undefined || options === null ? {} : Object(options);
  const raw = o.timeout === undefined ? -1 : o.timeout;
  if (typeof raw !== "number") throw __dnsErrInvalidArgType("options.timeout", "of type number", o.timeout);
  if (!Number.isInteger(raw) || raw < -1 || raw > 2147483647) {
    throw __dnsErrOutOfRange("options.timeout", ">= -1 && <= 2147483647", raw);
  }
  return raw + 0;
}
function __dnsValidateTries(options) {
  const o = options === undefined || options === null ? {} : Object(options);
  const raw = o.tries === undefined ? 4 : o.tries;
  if (typeof raw !== "number") throw __dnsErrInvalidArgType("options.tries", "of type number", o.tries);
  if (!Number.isInteger(raw) || raw < 1 || raw > 2147483647) {
    throw __dnsErrOutOfRange("options.tries", ">= 1 && <= 2147483647", raw);
  }
  return raw;
}
function __dnsValidateMaxTimeout(options) {
  const o = options === undefined || options === null ? {} : Object(options);
  const raw = o.maxTimeout === undefined ? 0 : o.maxTimeout;
  if (typeof raw !== "number") throw __dnsErrInvalidArgType("options.maxTimeout", "of type number", o.maxTimeout);
  if (!Number.isInteger(raw) || raw < 0 || raw > 4294967295) {
    throw __dnsErrOutOfRange("options.maxTimeout", ">= 0 && <= 4294967295", raw);
  }
  return raw + 0;
}
function __dnsSetServersFailed(servers) {
  const e = new Error(`c-ares failed to set servers: "There are pending queries." ${JSON.stringify(servers)}`);
  e.code = "ERR_DNS_SET_SERVERS_FAILED";
  return e;
}
function __dnsCancelled(syscall, hostname) {
  const cap = syscall.slice(0, 1).toUpperCase() + syscall.slice(1);
  const e = new Error(`ECANCELLED: query${cap} '${hostname}'`);
  e.code = "ECANCELLED";
  e.syscall = syscall;
  e.hostname = hostname;
  return e;
}
// 定制查询双形态归一（kind 见 Rust 单问表；map 归一 hickory/custom 双形）
function __dnsNormResult(kind, raw) {
  if (!Array.isArray(raw)) return raw;
  switch (kind) {
    case "a":
    case "aaaa":
      return raw.map((e) => (typeof e === "string" ? e : e.address));
    case "cname":
    case "ns":
    case "ptr":
    case "reverse":
      return raw.map((e) => (typeof e === "string" ? e : e.value));
    case "txt":
      return raw.map((e) => (Array.isArray(e) ? e : e.entries));
    case "soa":
      return raw;
    default:
      return raw;
  }
}
function __resolverQuery(inst, kind, syscall, hostname, cb) {
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  const st = __resolverPriv.get(inst);
  st.pending++;
  const wire = JSON.stringify(st.servers.map(__dnsWireForm));
  // 投递即返（helper 线程跑查询，事件循环永不停转——阻塞 native 会饿死
  // stub 回包分发；5ms refed 轮询保活，结算/取消即清）
  const id = Number(__wjs_dns_job_start(kind, hostname, wire, String(st.timeout), String(st.tries), String(st.maxTimeout)));
  const rec = { kind, syscall, hostname, cb, poll: 0 };
  st.jobs.set(id, rec);
  const settle = (err, val) => {
    if (!st.jobs.has(id)) return;
    st.jobs.delete(id);
    clearInterval(rec.poll);
    st.pending--;
    cb(err, val);
  };
  rec.cancel = () => settle(__dnsCancelled(syscall, hostname), undefined);
  rec.poll = setInterval(() => {
    let r;
    try { r = JSON.parse(__wjs_dns_job_poll(String(id))); }
    catch { return; }
    if (r.status === "pending") return;
    if (r.status === "err") {
      settle(__dnsMakeError(syscall, r.code, hostname), undefined);
      return;
    }
    const norm = __dnsNormResult(kind, r.value);
    if (!Array.isArray(norm) || norm.length === 0) {
      settle(__dnsMakeError(syscall, "ENODATA", hostname), undefined);
      return;
    }
    settle(null, norm);
  }, 5);
}
function __resolverQueryP(inst, kind, syscall, hostname) {
  __dnsValidateString(hostname, "name");
  return new Promise((resolveP, reject) => {
    __resolverQuery(inst, kind, syscall, hostname, (e, v) => (e ? reject(e) : resolveP(v)));
  });
}
export class Resolver {
  constructor(options) {
    const st = {
      servers: __dnsParseServers(getServers()),
      timeout: __dnsValidateTimeout(options),
      tries: __dnsValidateTries(options),
      maxTimeout: __dnsValidateMaxTimeout(options),
      pending: 0,
      jobs: new Map(),
      localAddress: null,
    };
    __resolverPriv.set(this, st);
    // _handle 兼容垫片（真机为 c-ares ChannelWrap；getServers гейт经此，
    // 套件覆写 `_handle.getServers` 探活——见 test-dns-get-server）
    this._handle = {
      getServers: () => st.servers.map((t) => t.slice()),
    };
  }
  getServers() {
    const st = __resolverPriv.get(this);
    const raw = this._handle.getServers() || [];
    return raw.map(([ver, ip, port]) => __dnsPublicForm([ver, ip, port]));
  }
  setServers(servers) {
    const st = __resolverPriv.get(this);
    const triples = __dnsParseServers(servers);
    if (st.pending > 0) throw __dnsSetServersFailed(servers);
    st.servers = triples;
  }
  setLocalAddress(ipv4, ipv6) {
    const st = __resolverPriv.get(this);
    __dnsValidateString(ipv4, "ipv4");
    if (ipv6 !== undefined) __dnsValidateString(ipv6, "ipv6");
    // 真机口径（实测 node 26）：坏 IP → ERR_INVALID_ARG_VALUE "Invalid IP address."；
    // 同族双指定 → "Cannot specify two IPvX addresses."；v4+v6 混搭（任意序）OK
    const v4 = __dnsIsIP(ipv4);
    if (v4 === 0) {
      const e = new TypeError("Invalid IP address.");
      e.code = "ERR_INVALID_ARG_VALUE";
      throw e;
    }
    if (ipv6 !== undefined) {
      const v6 = __dnsIsIP(ipv6);
      if (v6 === 0) {
        const e = new TypeError("Invalid IP address.");
        e.code = "ERR_INVALID_ARG_VALUE";
        throw e;
      }
      if (v4 === 4 && v6 === 4) {
        const e = new TypeError("Cannot specify two IPv4 addresses.");
        e.code = "ERR_INVALID_ARG_VALUE";
        throw e;
      }
      if (v4 === 6 && v6 === 6) {
        const e = new TypeError("Cannot specify two IPv6 addresses.");
        e.code = "ERR_INVALID_ARG_VALUE";
        throw e;
      }
    }
    st.localAddress = { ipv4, ipv6 };
  }
  cancel() {
    // 真机 c-ares 语义：未决查询即刻以 ECANCELLED 结算（线程迟归由 forget 摘除，
    // 事件循环不为取消的查询多等一轮超时）
    const st = __resolverPriv.get(this);
    for (const [id, rec] of [...st.jobs]) {
      try { __wjs_dns_job_forget(String(id)); } catch { /* 摘除尽力 */ }
      rec.cancel();
    }
  }
  resolve4(h, cb) { __resolverQuery(this, "a", "queryA", h, cb); }
  resolve6(h, cb) { __resolverQuery(this, "aaaa", "queryAaaa", h, cb); }
  resolveCname(h, cb) { __resolverQuery(this, "cname", "queryCname", h, cb); }
  resolveMx(h, cb) { __resolverQuery(this, "mx", "queryMx", h, cb); }
  resolveNs(h, cb) { __resolverQuery(this, "ns", "queryNs", h, cb); }
  resolveTxt(h, cb) { __resolverQuery(this, "txt", "queryTxt", h, cb); }
  resolveSrv(h, cb) { __resolverQuery(this, "srv", "querySrv", h, cb); }
  resolvePtr(h, cb) { __resolverQuery(this, "ptr", "queryPtr", h, cb); }
  resolveAny(h, cb) { __resolverQuery(this, "any", "queryAny", h, cb); }
  resolveSoa(h, cb) {
    __resolverQuery(this, "soa", "querySoa", h, (e, v) => cb(e, v && v[0]));
  }
  reverse(ip, cb) { __resolverQuery(this, "reverse", "getHostByAddr", ip, cb); }
  resolve(hostname, rrtype, cb) {
    if (typeof rrtype === "function") { cb = rrtype; rrtype = "A"; }
    __dnsValidateString(hostname, "name");
    if (typeof rrtype !== "string") throw __dnsErrInvalidArgType("rrtype", "of type string", rrtype);
    __dnsValidateFunction(cb, "callback");
    const t = String(rrtype).toUpperCase();
    const map = { CNAME: "cname", MX: "mx", NS: "ns", TXT: "txt", SRV: "srv", PTR: "ptr", ANY: "any", A: "a", AAAA: "aaaa", SOA: "soa" };
    const kind = map[t];
    if (!kind) {
      const err = new TypeError(`Unknown rrtype '${rrtype}'`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    const syscall = { cname: "queryCname", mx: "queryMx", ns: "queryNs", txt: "queryTxt", srv: "querySrv", ptr: "queryPtr", any: "queryAny", a: "queryA", aaaa: "queryAaaa", soa: "querySoa" }[kind];
    __resolverQuery(this, kind, syscall, hostname, cb);
  }
}
class PromisesResolver {
  constructor(options) {
    this._r = new Resolver(options);
    this._handle = this._r._handle;
  }
  getServers() { return this._r.getServers(); }
  setServers(s) { this._r.setServers(s); }
  setLocalAddress(a, b) { this._r.setLocalAddress(a, b); }
  cancel() { this._r.cancel(); }
  resolve4(h) { return __resolverQueryP(this._r, "a", "queryA", h); }
  resolve6(h) { return __resolverQueryP(this._r, "aaaa", "queryAaaa", h); }
  resolveCname(h) { return __resolverQueryP(this._r, "cname", "queryCname", h); }
  resolveMx(h) { return __resolverQueryP(this._r, "mx", "queryMx", h); }
  resolveNs(h) { return __resolverQueryP(this._r, "ns", "queryNs", h); }
  resolveTxt(h) { return __resolverQueryP(this._r, "txt", "queryTxt", h); }
  resolveSrv(h) { return __resolverQueryP(this._r, "srv", "querySrv", h); }
  resolvePtr(h) { return __resolverQueryP(this._r, "ptr", "queryPtr", h); }
  resolveAny(h) { return __resolverQueryP(this._r, "any", "queryAny", h); }
  resolveSoa(h) {
    return __resolverQueryP(this._r, "soa", "querySoa", h).then((v) => v[0]);
  }
  reverse(ip) { return __resolverQueryP(this._r, "reverse", "getHostByAddr", ip); }
  resolve(hostname, rrtype) {
    __dnsValidateString(hostname, "name");
    if (rrtype !== undefined && typeof rrtype !== "string") {
      throw __dnsErrInvalidArgType("rrtype", "of type string", rrtype);
    }
    const t = String(rrtype || "A").toUpperCase();
    const map = { CNAME: "cname", MX: "mx", NS: "ns", TXT: "txt", SRV: "srv", PTR: "ptr", ANY: "any", A: "a", AAAA: "aaaa", SOA: "soa" };
    const kind = map[t];
    if (!kind) {
      const err = new TypeError(`Unknown rrtype '${rrtype}'`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    const syscall = { cname: "queryCname", mx: "queryMx", ns: "queryNs", txt: "queryTxt", srv: "querySrv", ptr: "queryPtr", any: "queryAny", a: "queryA", aaaa: "queryAaaa", soa: "querySoa" }[kind];
    return __resolverQueryP(this._r, kind, syscall, hostname);
  }
}
function __promisesResolveAddr(kind, syscall, hostname, options) {
  __dnsValidateString(hostname, "name");
  const wantTtl = !!(options && options.ttl);
  return new Promise((resolveP, reject) => {
    __resolverQueryMod(kind, syscall, hostname, (e, v) => (e ? reject(e) : resolveP(v)), wantTtl);
  });
}
export const promises = {
  lookup: (hostname, options) => {
    if (!hostname) return Promise.reject(__dnsInvalidArgValue("hostname", hostname));
    __dnsValidateString(hostname, "hostname");
    __dnsValidateLookupOptions(options);
    return Promise.resolve().then(() => __lookupCore(hostname, options));
  },
  resolve4: (h, o) => {
    __dnsValidateString(h, "name");
    if (__dnsOverrideSnapshot()) {
      return new Promise((resolveP, reject) => {
        __moduleQueryCustom("a", "queryA", h, (e, v) => (e ? reject(e) : resolveP(v)), !!(o && o.ttl));
      });
    }
    return __promisesResolveAddr("a", "queryA", h, o);
  },
  resolve6: (h, o) => {
    __dnsValidateString(h, "name");
    if (__dnsOverrideSnapshot()) {
      return new Promise((resolveP, reject) => {
        __moduleQueryCustom("aaaa", "queryAaaa", h, (e, v) => (e ? reject(e) : resolveP(v)), !!(o && o.ttl));
      });
    }
    return __promisesResolveAddr("aaaa", "queryAaaa", h, o);
  },
  resolveSoa: (h) => {
    __dnsValidateString(h, "name");
    if (__dnsOverrideSnapshot()) {
      return new Promise((resolveP, reject) => {
        __moduleQueryCustom("soa", "querySoa", h, (e, v) => (e ? reject(e) : resolveP(v)), false);
      });
    }
    return new Promise((resolveP, reject) => {
    queueMicrotask(() => {
      let raw;
      try { raw = JSON.parse(__wjs_dns_query("soa", h)); }
      catch (e) {
        try { __qerr("soa", h, e); } catch (err) { reject(err); }
        return;
      }
      const list = Array.isArray(raw) ? raw : [];
      if (list.length === 0) { reject(__dnsMakeError("querySoa", "ENODATA", h)); return; }
      resolveP(list[0]);
    });
    });
  },
  resolveCname: __promisifyQuery("cname", "resolveCname"),
  resolveMx: __promisifyQuery("mx", "resolveMx"),
  resolveNs: __promisifyQuery("ns", "resolveNs"),
  resolveTxt: __promisifyQuery("txt", "resolveTxt"),
  resolveSrv: __promisifyQuery("srv", "resolveSrv"),
  resolvePtr: __promisifyQuery("ptr", "resolvePtr"),
  resolveAny: (h) => {
    __dnsValidateString(h, "name");
    if (__dnsOverrideSnapshot()) {
      return new Promise((resolveP, reject) => {
        __moduleQueryCustom("any", "queryAny", h, (e, v) => (e ? reject(e) : resolveP(v)), false);
      });
    }
    return __promisifyQuery("any", "resolveAny")(h);
  },
  resolve: (hostname, rrtype) => {
    __dnsValidateString(hostname, "name");
    if (rrtype !== undefined && typeof rrtype !== "string") {
      throw __dnsErrInvalidArgType("rrtype", "of type string", rrtype);
    }
    const t = String(rrtype || "A").toUpperCase();
    const map = { CNAME: "cname", MX: "mx", NS: "ns", TXT: "txt", SRV: "srv", PTR: "ptr", ANY: "any", A: "a", AAAA: "aaaa", SOA: "soa" };
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
  // setServers 同步校验先行（`assert.throws` 同步口径）
  setServers: (s) => { setServers(s); return Promise.resolve(); },
  lookupService: function (address, port) {
    if (arguments.length < 2) throw __dnsMissingArgs("address", "port");
    // 同步校验先行（`assert.throws` 同步口径；进 Promise 执行器即变 reject）
    __dnsValidateString(address, "address");
    if (__dnsIsIP(address) === 0) throw __dnsInvalidArgValue("address", address);
    __dnsValidatePortLoose(port);
    return new Promise((resolveP, reject) => {
      __lookupServiceImpl(address, port, (e, h, svc) => (e ? reject(e) : resolveP({ hostname: h, service: svc })));
    });
  },
  Resolver: PromisesResolver,
  getDefaultResultOrder: () => Promise.resolve().then(() => getDefaultResultOrder()),
  setDefaultResultOrder: (o) => Promise.resolve().then(() => setDefaultResultOrder(o)),
};
function resolve4b(hostname) { return __filter(__entries(hostname), 4).map((e) => e.address); }
function resolve6b(hostname) { return __filter(__entries(hostname), 6).map((e) => e.address); }
const __api = { lookup, lookupService, resolve4, resolve6, resolve, resolveCname, resolveMx, resolveNs, resolveTxt, resolveSrv, resolvePtr, resolveAny, resolveSoa, reverse, getServers, setServers, getDefaultResultOrder, setDefaultResultOrder, Resolver, promises, V4MAPPED, ADDRCONFIG, ALL };
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
        assert_eq!(node_code_for("request timed out"), "ETIMEOUT");
        assert_eq!(node_code_for("refused by server"), "EREFUSED");
        assert_eq!(node_code_for("servfail"), "SERVFAIL");
        assert_eq!(node_code_for("totally unknown blah"), "ENOTFOUND");
        reset_for_tests();
    }

    #[test]
    fn dns_server_addr_parse() {
        // 10f 定制路径：规范形解析（JS 已 canonicalize，此处为 backstop）
        let v4 = "127.0.0.1".parse::<IpAddr>().unwrap();
        let v6 = "::1".parse::<IpAddr>().unwrap();
        assert_eq!(
            parse_server_addr("127.0.0.1"),
            Some(DnsServer { ip: v4, port: 53 })
        );
        assert_eq!(
            parse_server_addr("127.0.0.1:5353"),
            Some(DnsServer { ip: v4, port: 5353 })
        );
        // port 0 即默认端口（Node 口径）
        assert_eq!(
            parse_server_addr("127.0.0.1:0"),
            Some(DnsServer { ip: v4, port: 53 })
        );
        assert_eq!(
            parse_server_addr("[::1]"),
            Some(DnsServer { ip: v6, port: 53 })
        );
        assert_eq!(
            parse_server_addr("[::1]:5353"),
            Some(DnsServer { ip: v6, port: 5353 })
        );
        assert_eq!(
            parse_server_addr("2001:4860:4860::8888"),
            Some(DnsServer {
                ip: "2001:4860:4860::8888".parse().unwrap(),
                port: 53
            })
        );
        assert_eq!(parse_server_addr("foobar"), None);
        assert_eq!(parse_server_addr("127.0.0.1:va"), None);
        assert_eq!(parse_server_addr("127.0.0.1:"), None);
        assert_eq!(parse_server_addr("[::1"), None);
        assert_eq!(parse_server_addr(""), None);
    }

    #[test]
    fn dns_udp_fail_codes() {
        // 10f：失败→Node 码映射（stub 坏包/超时/拒连三件）
        assert_eq!(udp_fail_code(&UdpFail::Timeout).0, "ETIMEOUT");
        assert_eq!(udp_fail_code(&UdpFail::BadResponse("decode: x".into())).0, "EBADRESP");
        assert_eq!(
            udp_fail_code(&UdpFail::Io("connection refused".into())).0,
            "ECONNREFUSED"
        );
        assert_eq!(udp_fail_code(&UdpFail::Io("boom".into())).0, "EAI_AGAIN");
    }

    #[test]
    fn dns_custom_no_servers() {
        // 空 servers 即失败（无线程/I-O，纯逻辑）
        use hickory_resolver::proto::rr::RecordType;
        let err = query_custom_sync(
            &[],
            "example.org",
            RecordType::A,
            std::time::Duration::from_millis(10),
            1,
            std::time::Duration::ZERO,
        )
        .unwrap_err();
        assert_eq!(err.0, "ENOTFOUND");
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
