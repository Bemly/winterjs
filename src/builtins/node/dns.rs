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

/// `__wjs2_dns_lookup(host)` → JSON 数组 `[{address, family}]`（v4+v6 全量；
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
        .name("winterjs2-dns".into())
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

/// `__wjs2_dns_query(kind, name)` → 结果 JSON；失败抛 `CODE: queryKind 'name': msg`。
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

/// `__wjs2_dns_job_start(kind, name, servers_json, timeout_ms, tries, max_timeout_ms)`
/// 定制查询的异步投递：helper 线程跑 `query_custom_sync`，结果进 job 表；
/// JS 侧 `setInterval` 轮询 `__wjs2_dns_job_poll` 收割（5ms 粒度，refed 保活，
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
        .name("winterjs2-dns-job".into())
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

/// `__wjs2_dns_job_poll(id)` → `{"status":"pending"}` /
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

/// `__wjs2_dns_job_forget(id)` → 丢弃 job（cancel 后线程迟归不堆积）。
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

/// `__wjs2_dns_servers_get()` → JSON 数组。
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

/// `__wjs2_dns_servers_set(json)`：存规范形覆盖层（JS 已按 Node 口径校验 +
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

/// `__wjs2_dns_order_get()` → `"verbatim"` / `"ipv4first"`。
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

/// `__wjs2_dns_order_set(s)`：只收 verbatim/ipv4first。
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

/// 内嵌 ESM 源（`node:dns`；§0.9 按域分块：`dns.js` 全量 + 单测挂 `dns_tests.rs`
///（`#[path]` 子模块，`super` 即 dns 本体，测试代码逐字节不动），concat 字节恒等）。
pub const SOURCE: &str = include_str!("dns.js");

#[cfg(test)]
#[path = "dns_tests.rs"]
mod tests;
