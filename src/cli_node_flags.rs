//! node 运行时旗名单（D1 复盘，2026-09-25）：取自 node 26.8.2 `node --help` 并剔除
//! 改变执行模式/加载语义的旗（`--run/--test*/--watch*/--eval/--print/--require/--import/
//! --env-file*/--inspect*/--prof*` 等——静默剥除会吞语义，交 clap 报错更诚实），另补常见
//! V8 旗。**只收精确名单，不收前缀族**：node 对未知旗报 `bad option` 退出，前缀放行会让
//! 非法旗被剥除后重跑同一文件——自 spawn 套件即无限递归（2026-09-25 fork 链致系统 panic，
//! pitfalls 4.209）。

/// 精确名单（按 `--k` 基名匹配；`--k=v` 形取 `=` 前）。
pub const NODE_RUNTIME_FLAGS: &[&str] = &[
    "--abort-on-uncaught-exception",
    "--allow-addons",
    "--allow-child-process",
    "--allow-ffi",
    "--allow-fs-read",
    "--allow-fs-write",
    "--allow-inspector",
    "--allow-net",
    "--allow-openssl-store",
    "--allow-wasi",
    "--allow-worker",
    "--build-sea",
    "--debug-port",
    "--disable-proto",
    "--disable-sigusr1",
    "--disable-warning",
    "--disable-wasm-trap-handler",
    "--disallow-code-generation-from-strings",
    "--dns-result-order",
    "--enable-etw-stack-walking",
    "--enable-fips",
    "--enable-network-family-autoselection",
    "--enable-source-maps",
    "--experimental-addon-modules",
    "--experimental-default-config-file",
    "--experimental-eventsource",
    "--experimental-ffi",
    "--experimental-import-meta-resolve",
    "--experimental-import-text",
    "--experimental-inspector-network-resource",
    "--experimental-network-inspection",
    "--experimental-package-map",
    "--experimental-print-required-tla",
    "--experimental-storage-inspection",
    "--experimental-stream-iter",
    "--experimental-strip-types",
    "--experimental-test-coverage",
    "--experimental-test-isolation",
    "--experimental-test-module-mocks",
    "--experimental-test-tag-filter",
    "--experimental-vfs",
    "--experimental-vm-modules",
    "--experimental-worker-inspection",
    "--expose-gc",
    "--force-context-aware",
    "--force-fips",
    "--force-node-api-uncaught-exceptions-policy",
    "--frozen-intrinsics",
    "--harmony",
    "--heapsnapshot-near-heap-limit",
    "--heapsnapshot-signal",
    "--interpreted-frames-native-stack",
    "--jitless",
    "--max-old-space-size",
    "--max-old-space-size-percentage",
    "--max-semi-space-size",
    "--network-family-autoselection-attempt-timeout",
    "--no-addons",
    "--no-async-context-frame",
    "--no-deprecation",
    "--no-experimental-detect-module",
    "--no-experimental-global-navigator",
    "--no-experimental-repl-await",
    "--no-experimental-require-module",
    "--no-experimental-sqlite",
    "--no-experimental-websocket",
    "--no-extra-info-on-fatal-exception",
    "--no-force-async-hooks-checks",
    "--no-global-search-paths",
    "--no-require-module",
    "--no-warnings",
    "--node-memory-debug",
    "--openssl-legacy-provider",
    "--openssl-shared-config",
    "--pending-deprecation",
    "--permission",
    "--permission-audit",
    "--preserve-symlinks",
    "--preserve-symlinks-main",
    "--report-compact",
    "--report-exclude-env",
    "--report-exclude-network",
    "--report-on-fatalerror",
    "--report-on-signal",
    "--report-signal",
    "--report-uncaught-exception",
    "--stack-size",
    "--stack-trace-limit",
    "--throw-deprecation",
    "--tls-cipher-list",
    "--tls-max-v1",
    "--tls-min-v1",
    "--trace-deprecation",
    "--trace-env",
    "--trace-env-js-stack",
    "--trace-env-native-stack",
    "--trace-event-categories",
    "--trace-event-file-pattern",
    "--trace-exit",
    "--trace-promises",
    "--trace-require-module",
    "--trace-sigint",
    "--trace-sync-io",
    "--trace-tls",
    "--trace-uncaught",
    "--trace-warnings",
    "--track-heap-objects",
    "--unhandled-rejections",
    "--use-env-proxy",
    "--use-largepages",
    "--use-system-ca",
    "--v8-pool-size",
    "--webstorage",
    "--zero-fill-buffers",
];

/// 必须带值的旗（只认 `--k=v` 整项）。
pub const VALUE_REQUIRED: &[&str] = &[
    "--unhandled-rejections", "--stack-trace-limit", "--stack-size", "--max-old-space-size",
    "--max-semi-space-size", "--disable-warning", "--dns-result-order", "--tls-cipher-list",
    "--report-signal", "--heapsnapshot-signal", "--trace-event-categories",
    "--trace-event-file-pattern", "--v8-pool-size",
];

/// 取值校验（node 同款拒收即 exit 9）：返回 Err(node 文案)。
pub fn validate_node_flag(arg: &str) -> Result<(), String> {
    let (base, val) = match arg.split_once('=') {
        Some((b, v)) => (b, Some(v)),
        None => (arg, None),
    };
    match base {
        "--unhandled-rejections" => match val {
            Some("strict" | "warn" | "none" | "throw" | "warn-with-error-code") => Ok(()),
            _ => Err("invalid value for --unhandled-rejections".into()),
        },
        "--stack-trace-limit" | "--stack-size" | "--max-old-space-size" | "--max-semi-space-size" => {
            match val.map(str::parse::<i64>) {
                Some(Ok(_)) => Ok(()),
                _ => Err(format!("bad option: {arg}")),
            }
        }
        "--disable-warning" | "--dns-result-order" | "--tls-cipher-list" | "--report-signal"
        | "--heapsnapshot-signal" | "--trace-event-categories" | "--trace-event-file-pattern"
        | "--v8-pool-size" => match val {
            Some(v) if !v.is_empty() => Ok(()),
            _ => Err(format!("{base} requires an argument")),
        },
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_values() {
        assert!(validate_node_flag("--unhandled-rejections=strict").is_ok());
        assert_eq!(validate_node_flag("--unhandled-rejections=foobar").unwrap_err(), "invalid value for --unhandled-rejections");
        assert!(validate_node_flag("--stack-trace-limit=3").is_ok());
        assert!(validate_node_flag("--stack-trace-limit=abc").is_err());
        assert!(validate_node_flag("--disable-warning=").is_err());
        assert!(validate_node_flag("--pending-deprecation").is_ok());
    }

    #[test]
    fn list_excludes_mode_flags() {
        for f in ["--run", "--test", "--watch", "--eval", "--require", "--import", "--inspect"] {
            assert!(!NODE_RUNTIME_FLAGS.contains(&f), "{f}");
        }
        assert!(NODE_RUNTIME_FLAGS.contains(&"--experimental-stream-iter"));
    }
}
