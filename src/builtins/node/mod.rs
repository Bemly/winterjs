//! `node:` 内建表（plan Phase 4；Phase 9a 起含互引 + `node:internal/*`）：
//! 注册名 → 内嵌 ESM 源。resolve 规范名为 `node:X` URL（`fs` 与 `node:fs` 同一
//! 模块，见 `normalize_spec`）；`prepare()` 取源走 `source()`，不经过 fetch。
//!
//! Phase 9a 起：内嵌源可经**绝对 `node:` URL** 互引（loader resolve 不依赖 base）；
//! `node:internal/*` 共享小件走 `internal::INTERNALS`，只供内建互引——
//! `normalize_spec` 接受（宽松），但**不进 `available()` 报错列表**。

pub mod assert;
pub mod assert_strict;
pub mod async_hooks;
pub mod buffer;
pub mod child;
pub mod cluster;
pub mod console;
pub mod crypto;
pub mod diagnostics_channel;
pub mod domain;
pub mod dgram;
pub mod dns;
pub mod dns_promises;
pub mod events;
pub mod fs;
pub mod http;
pub mod http2;
pub mod https;
pub mod internal;
pub mod inspector;
pub mod net;
pub mod nodemodule;
pub mod os;
pub mod path;
pub mod path_posix;
pub mod path_win32;
pub mod perf_hooks;
pub mod process_;
pub mod punycode;
pub mod querystring;
pub mod quic;
pub mod readline;
/// 10c-3: node:repl（REPLServer/start/Recoverable，骑 readline Interface）。
pub mod repl;
pub mod require;
pub mod sqlite;
pub mod stream;
pub mod stream_consumers;
pub mod stream_promises;
pub mod stream_web;
pub mod string_decoder;
pub mod testmod;
pub mod timers;
pub mod timers_promises;
pub mod tls;
pub mod trace_events;
pub mod tty;
pub mod util;
pub mod util_types;
pub mod url;
pub mod v8;
pub mod vm;
pub mod worker;
pub mod zlib;

/// 全局 `process` 等启动期求值的 JS（`runtime` 在主 PRELUDE 后求值）。
/// 版本占位 `26.9.13` 在求值前替换为 `CARGO_PKG_VERSION`（发版时两处同步改，不漂移）。
pub fn node_prelude() -> String {
    let base = process_::PROCESS_PRELUDE.replace("26.9.13", env!("CARGO_PKG_VERSION"));
    format!("{base}\n{}", require::REQUIRE_PRELUDE)
}

/// 内建源表（规范名 → ESM 源；互引走绝对 `node:` URL，natives 走全局 `__wjs_*`）。
/// `node:internal/*` 不在本表——`internal::INTERNALS` 为其唯一源（source() 先查它），
/// 保证 `available()` 天然不含 internal。
const BUILTINS: &[(&str, &str)] = &[
    ("node:path", path::SOURCE),
    ("node:path/posix", path_posix::SOURCE),
    ("node:path/win32", path_win32::SOURCE),
    ("node:os", os::SOURCE),
    ("node:process", process_::SOURCE),
    ("node:fs", fs::SOURCE),
    ("node:fs/promises", fs::PROMISES_SOURCE),
    ("node:child_process", child::SOURCE),
    // 10e：node:cluster（worker 线程底座；fork/message/exit 对等面）
    ("node:cluster", cluster::SOURCE),
    // M5 vitest 牵引：node:console 模块面（纯 JS，全局 console + Console 类）
    ("node:console", console::SOURCE),
    ("node:net", net::SOURCE),
    ("node:dns", dns::SOURCE),
    ("node:dns/promises", dns_promises::SOURCE),
    ("node:dgram", dgram::SOURCE),
    ("node:http", http::SOURCE),
    // Phase 9d-6
    ("node:https", https::SOURCE),
    ("node:tls", tls::SOURCE),
    // Phase 9d-7
    ("node:http2", http2::SOURCE),
    // Phase 9e-1a
    ("node:crypto", crypto::SOURCE),
    ("node:assert", assert::SOURCE),
    ("node:assert/strict", assert_strict::SOURCE),
    ("node:test", testmod::SOURCE),
    // Phase 9a
    ("node:async_hooks", async_hooks::SOURCE),
    ("node:events", events::SOURCE),
    ("node:util", util::SOURCE),
    ("node:util/types", util_types::SOURCE),
    ("node:querystring", querystring::SOURCE),
    ("node:punycode", punycode::SOURCE),
    // Phase 9g-1
    ("node:quic", quic::SOURCE),
    ("node:string_decoder", string_decoder::SOURCE),
    // Phase 9b
    ("node:buffer", buffer::SOURCE),
    ("node:stream", stream::SOURCE),
    ("node:stream/promises", stream_promises::SOURCE),
    ("node:stream/consumers", stream_consumers::SOURCE),
    ("node:stream/web", stream_web::SOURCE),
    ("node:timers/promises", timers_promises::SOURCE),
    // M5 vitest 牵引：回调形态
    ("node:timers", timers::SOURCE),
    ("node:diagnostics_channel", diagnostics_channel::SOURCE),
    // 10e：node:domain（遗留薄面；同步路由）
    ("node:domain", domain::SOURCE),
    ("node:trace_events", trace_events::SOURCE),
    ("node:tty", tty::SOURCE),
    // Phase 9d-5
    ("node:zlib", zlib::SOURCE),
    // Phase 9e-3
    ("node:perf_hooks", perf_hooks::SOURCE),
    // Phase 9e-4
    ("node:inspector", inspector::SOURCE),
    ("node:inspector/promises", inspector::SOURCE),
    // Phase 9f-1
    ("node:vm", vm::SOURCE),
    // Phase 9f-2
    ("node:worker_threads", worker::SOURCE),
    // Phase 9j
    ("node:module", nodemodule::SOURCE),
    ("node:v8", v8::SOURCE),
    ("node:readline", readline::SOURCE),
    ("node:repl", repl::SOURCE),
    ("node:url", url::SOURCE),
    // 10d：node:sqlite（turso 底座；DatabaseSync/StatementSync）
    ("node:sqlite", sqlite::SOURCE),
];

/// spec 规范化（`node:` 前缀可选；internal 走 `internal::normalize_internal`；
/// 未知返回 None，调用方报可用列表）。
/// 纯函数，单元测试覆盖（`node_spec_table`）。
pub fn normalize_spec(spec: &str) -> Option<&'static str> {
    if let Some(canonical) = internal::normalize_internal(spec) {
        return Some(canonical);
    }
    // node 内部模块遗留别名（套件 require('_http_agent') 直引）
    if spec == "_http_agent" {
        return Some("node:_http_agent");
    }
    if spec == "_http_common" {
        return Some("node:_http_common");
    }
    if spec == "_http_server" {
        return Some("node:_http_server");
    }
    if spec == "_http_outgoing" {
        return Some("node:_http_outgoing");
    }
    match spec.strip_prefix("node:").unwrap_or(spec) {
        "path" => Some("node:path"),
        // M5 vitest 牵引：子路径
        "path/posix" => Some("node:path/posix"),
        "path/win32" => Some("node:path/win32"),
        "os" => Some("node:os"),
        "process" => Some("node:process"),
        "fs" => Some("node:fs"),
        "fs/promises" => Some("node:fs/promises"),
        "child_process" => Some("node:child_process"),
        // 10e
        "cluster" => Some("node:cluster"),
        // M5 vitest 牵引
        "console" => Some("node:console"),
        "net" => Some("node:net"),
        "dns" => Some("node:dns"),
        "dns/promises" => Some("node:dns/promises"),
        "dgram" => Some("node:dgram"),
        "http" => Some("node:http"),
        // Phase 9d-6
        "https" => Some("node:https"),
        "tls" => Some("node:tls"),
        // Phase 9d-7
        "http2" => Some("node:http2"),
        // Phase 9e-1a
        "crypto" => Some("node:crypto"),
        "assert" => Some("node:assert"),
        "assert/strict" => Some("node:assert/strict"),
        "test" => Some("node:test"),
        "async_hooks" => Some("node:async_hooks"),
        "events" => Some("node:events"),
        "util" => Some("node:util"),
        "util/types" => Some("node:util/types"),
        // 10a：`node:sys` 是 util 的废弃别名（真机同一模块实例；无运行时警告）。
        // 规范到同一 canonical，走同一源 + 同一注册表单例。
        "sys" => Some("node:util"),
        "querystring" => Some("node:querystring"),
        "punycode" => Some("node:punycode"),
        // Phase 9g-1
        "quic" => Some("node:quic"),
        "string_decoder" => Some("node:string_decoder"),
        "diagnostics_channel" => Some("node:diagnostics_channel"),
        // 10e
        "domain" => Some("node:domain"),
        "trace_events" => Some("node:trace_events"),
        "tty" => Some("node:tty"),
        // Phase 9b
        "buffer" => Some("node:buffer"),
        "stream" => Some("node:stream"),
        "stream/promises" => Some("node:stream/promises"),
        "stream/consumers" => Some("node:stream/consumers"),
        "stream/web" => Some("node:stream/web"),
        "timers/promises" => Some("node:timers/promises"),
        // M5 vitest 牵引：回调形态
        "timers" => Some("node:timers"),
        // Phase 9d-5
        "zlib" => Some("node:zlib"),
        // Phase 9e-3
        "perf_hooks" => Some("node:perf_hooks"),
        // Phase 9e-4
        "inspector" => Some("node:inspector"),
        "inspector/promises" => Some("node:inspector/promises"),
        // Phase 9f-1
        "vm" => Some("node:vm"),
        // Phase 9f-2
        "worker_threads" => Some("node:worker_threads"),
        // Phase 9j
        "module" => Some("node:module"),
        "v8" => Some("node:v8"),
        "readline" => Some("node:readline"),
        "repl" => Some("node:repl"),
        "url" => Some("node:url"),
        // 10d
        "sqlite" => Some("node:sqlite"),
        _ => None,
    }
}

/// 规范名 → 内嵌源（internal 表一并可查）。
pub fn source(canonical: &str) -> Option<&'static str> {
    if let Some(src) = internal::source(canonical) {
        return Some(src);
    }
    BUILTINS.iter().find(|(name, _)| *name == canonical).map(|(_, src)| *src)
}

/// 可用内建列表（报错信息用；`node:internal/*` 只供互引，不列出）。
pub fn available() -> Vec<&'static str> {
    BUILTINS.iter().map(|(name, _)| *name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_spec_table() {
        assert_eq!(normalize_spec("node:path"), Some("node:path"));
        assert_eq!(normalize_spec("path"), Some("node:path"));
        assert_eq!(normalize_spec("node:os"), Some("node:os"));
        assert_eq!(normalize_spec("os"), Some("node:os"));
        assert_eq!(normalize_spec("node:process"), Some("node:process"));
        assert_eq!(normalize_spec("process"), Some("node:process"));
        assert_eq!(normalize_spec("node:fs"), Some("node:fs"));
        assert_eq!(normalize_spec("fs"), Some("node:fs"));
        assert_eq!(normalize_spec("node:fs/promises"), Some("node:fs/promises"));
        assert_eq!(normalize_spec("fs/promises"), Some("node:fs/promises"));
        assert_eq!(normalize_spec("node:child_process"), Some("node:child_process"));
        // 10e：node:cluster
        assert_eq!(normalize_spec("cluster"), Some("node:cluster"));
        assert_eq!(normalize_spec("node:cluster"), Some("node:cluster"));
        assert!(source("node:cluster").is_some());
        assert_eq!(normalize_spec("child_process"), Some("node:child_process"));
        assert_eq!(normalize_spec("node:assert"), Some("node:assert"));
        assert_eq!(normalize_spec("node:test"), Some("node:test"));
        assert_eq!(normalize_spec("node:fs/watch"), None);
        assert_eq!(normalize_spec("node:"), None);
        assert_eq!(normalize_spec(""), None);
        // Phase 9a
        assert_eq!(normalize_spec("node:events"), Some("node:events"));
        assert_eq!(normalize_spec("events"), Some("node:events"));
        assert_eq!(normalize_spec("node:async_hooks"), Some("node:async_hooks"));
        assert_eq!(normalize_spec("async_hooks"), Some("node:async_hooks"));
        assert_eq!(normalize_spec("node:util"), Some("node:util"));
        assert_eq!(normalize_spec("util"), Some("node:util"));
        // 10a：sys 别名（import/require 双形态同实例，见黑盒 phase10a_sys_alias）。
        assert_eq!(normalize_spec("sys"), Some("node:util"));
        assert_eq!(normalize_spec("node:sys"), Some("node:util"));
        assert_eq!(normalize_spec("node:util/types"), Some("node:util/types"));
        assert_eq!(normalize_spec("util/types"), Some("node:util/types"));
        assert!(source("node:util").is_some());
        assert!(source("node:util/types").is_some());
        assert_eq!(normalize_spec("querystring"), Some("node:querystring"));
        assert_eq!(normalize_spec("node:punycode"), Some("node:punycode"));
        assert_eq!(normalize_spec("node:string_decoder"), Some("node:string_decoder"));
        assert!(source("node:querystring").is_some());
        assert!(source("node:punycode").is_some());
        // Phase 9g-1
        assert_eq!(normalize_spec("quic"), Some("node:quic"));
        assert_eq!(normalize_spec("node:quic"), Some("node:quic"));
        assert!(source("node:quic").is_some());
        assert!(source("node:string_decoder").is_some());
        assert_eq!(normalize_spec("diagnostics_channel"), Some("node:diagnostics_channel"));
        // 10e：node:domain
        assert_eq!(normalize_spec("domain"), Some("node:domain"));
        assert_eq!(normalize_spec("node:domain"), Some("node:domain"));
        assert!(source("node:domain").is_some());
        assert_eq!(normalize_spec("node:trace_events"), Some("node:trace_events"));
        assert_eq!(normalize_spec("tty"), Some("node:tty"));
        assert!(source("node:diagnostics_channel").is_some());
        assert!(source("node:trace_events").is_some());
        assert!(source("node:tty").is_some());
        // M5 vitest 牵引
        assert_eq!(normalize_spec("console"), Some("node:console"));
        assert_eq!(normalize_spec("node:console"), Some("node:console"));
        assert!(source("node:console").is_some());
        // Phase 9b
        assert_eq!(normalize_spec("buffer"), Some("node:buffer"));
        assert_eq!(normalize_spec("node:stream"), Some("node:stream"));
        assert_eq!(normalize_spec("stream/promises"), Some("node:stream/promises"));
        assert_eq!(normalize_spec("node:stream/web"), Some("node:stream/web"));
        assert_eq!(normalize_spec("timers/promises"), Some("node:timers/promises"));
        // Phase 9d-5
        assert_eq!(normalize_spec("zlib"), Some("node:zlib"));
        assert_eq!(normalize_spec("node:zlib"), Some("node:zlib"));
        assert!(source("node:zlib").is_some());
        // Phase 9e-3
        assert_eq!(normalize_spec("perf_hooks"), Some("node:perf_hooks"));
        assert_eq!(normalize_spec("node:perf_hooks"), Some("node:perf_hooks"));
        assert!(source("node:perf_hooks").is_some());
        // Phase 9e-4
        assert_eq!(normalize_spec("inspector"), Some("node:inspector"));
        assert_eq!(normalize_spec("node:inspector/promises"), Some("node:inspector/promises"));
        assert!(source("node:inspector").is_some());
        assert!(source("node:inspector/promises").is_some());
        // Phase 9f-1
        assert_eq!(normalize_spec("vm"), Some("node:vm"));
        assert_eq!(normalize_spec("node:vm"), Some("node:vm"));
        assert!(source("node:vm").is_some());
        // Phase 9f-2
        assert_eq!(normalize_spec("worker_threads"), Some("node:worker_threads"));
        assert_eq!(normalize_spec("node:worker_threads"), Some("node:worker_threads"));
        assert!(source("node:worker_threads").is_some());
        // Phase 9j
        assert_eq!(normalize_spec("module"), Some("node:module"));
        assert_eq!(normalize_spec("node:module"), Some("node:module"));
        assert!(source("node:module").is_some());
        assert_eq!(normalize_spec("v8"), Some("node:v8"));
        assert_eq!(normalize_spec("node:v8"), Some("node:v8"));
        assert!(source("node:v8").is_some());
        assert_eq!(normalize_spec("readline"), Some("node:readline"));
        assert_eq!(normalize_spec("repl"), Some("node:repl"));
        assert_eq!(normalize_spec("node:repl"), Some("node:repl"));
        assert!(source("node:repl").is_some());
        assert_eq!(normalize_spec("node:readline"), Some("node:readline"));
        assert!(source("node:readline").is_some());
        assert_eq!(normalize_spec("url"), Some("node:url"));
        assert_eq!(normalize_spec("node:url"), Some("node:url"));
        assert!(source("node:url").is_some());
        // 10d：node:sqlite
        assert_eq!(normalize_spec("sqlite"), Some("node:sqlite"));
        assert_eq!(normalize_spec("node:sqlite"), Some("node:sqlite"));
        assert!(source("node:sqlite").is_some());
        // M5 vitest 牵引：子路径 + 回调 timers + console 模块面
        assert_eq!(normalize_spec("path/posix"), Some("node:path/posix"));
        assert_eq!(normalize_spec("node:path/win32"), Some("node:path/win32"));
        assert!(source("node:path/posix").is_some());
        assert!(source("node:path/win32").is_some());
        assert_eq!(normalize_spec("timers"), Some("node:timers"));
        assert_eq!(normalize_spec("node:timers"), Some("node:timers"));
        assert!(source("node:timers").is_some());
        assert_eq!(normalize_spec("assert/strict"), Some("node:assert/strict"));
        assert!(source("node:assert/strict").is_some());
        assert_eq!(normalize_spec("dns/promises"), Some("node:dns/promises"));
        assert!(source("node:dns/promises").is_some());
        // Phase 9d-6
        assert_eq!(normalize_spec("https"), Some("node:https"));
        assert_eq!(normalize_spec("node:tls"), Some("node:tls"));
        assert!(source("node:https").is_some());
        assert!(source("node:tls").is_some());
        // Phase 9d-7
        assert_eq!(normalize_spec("http2"), Some("node:http2"));
        assert_eq!(normalize_spec("node:http2"), Some("node:http2"));
        assert!(source("node:http2").is_some());
        // Phase 9e-1a
        assert_eq!(normalize_spec("crypto"), Some("node:crypto"));
        assert_eq!(normalize_spec("node:crypto"), Some("node:crypto"));
        assert!(source("node:crypto").is_some());
        assert!(source("node:internal/http_framing").is_some());
        assert_eq!(normalize_spec("node:internal/streams/readable"), Some("node:internal/streams/readable"));
        assert!(source("node:buffer").is_some());
        assert!(source("node:stream").is_some());
        assert!(source("node:stream/promises").is_some());
        assert!(source("node:stream/consumers").is_some());
        assert!(source("node:stream/web").is_some());
        assert!(source("node:timers/promises").is_some());
        assert!(source("node:internal/streams/readable").is_some());
        assert!(source("node:internal/querystring").is_some());
        // internal：可解析、可取源，但不在 available()
        assert_eq!(normalize_spec("node:internal/errors"), Some("node:internal/errors"));
        assert_eq!(normalize_spec("internal/errors"), Some("node:internal/errors"));
        assert_eq!(
            normalize_spec("internal/events/abort_listener"),
            Some("node:internal/events/abort_listener")
        );
        assert!(source("node:internal/errors").is_some());
        assert!(source("node:events").is_some());
        assert!(source("node:async_hooks").is_some());
        assert_eq!(normalize_spec("node:internal/nope"), None);
        assert_eq!(normalize_spec("node:nope"), None);
        for name in available() {
            assert!(!name.starts_with("node:internal/"), "{name} leaked into available()");
            assert!(source(name).is_some(), "{name} missing source");
        }
        assert_eq!(available().len(), BUILTINS.len());
    }
}
