// NODE_OPTIONS 白名单（node 口径 per_thread.js buildAllowedFlags：
// 数据表取自真机 node 26.10.0 `process.allowedNodeEnvironmentFlags` 全量 dump（310 项）；
// has() 按原文归一化（下划线转横线、前导横线去 `=值`、无横线比去横线表），add/delete/clear no-op。
// 惰性首读构造并替换本属性（真机同）；可整体覆写。
Object.defineProperty(globalThis.process, "allowedNodeEnvironmentFlags", {
  get() {
    const __flags = [
    "--node-memory-debug", "--bench-warmup", "--perf-basic-prof-only-functions", "--cpu-prof", "--no-cpu-prof", "--max-heap-size",
    "--watch-path", "--experimental-eventsource", "--no-experimental-eventsource", "--perf-prof-unwinding-info", "--experimental-shadow-realm", "--no-experimental-shadow-realm",
    "--perf-basic-prof", "--heap-prof-interval", "--allow-ffi", "--no-allow-ffi", "--warnings", "--no-warnings",
    "--experimental-modules", "--experimental-package-map", "--perf-prof", "--test-shard", "--report-exclude-env", "--no-report-exclude-env",
    "--test-global-setup", "--redirect-warnings", "--tls-min-v1.2", "--no-tls-min-v1.2", "--tls-min-v1.1", "--no-tls-min-v1.1",
    "--disallow-code-generation-from-strings", "--tls-max-v1.2", "--no-tls-max-v1.2", "--preserve-symlinks-main", "--no-preserve-symlinks-main", "--enable-etw-stack-walking",
    "--watch-kill-signal", "--unhandled-rejections", "--use-system-ca", "--no-use-system-ca", "--stack-trace-limit", "--trace-require-module",
    "--inspect-wait", "--no-inspect-wait", "--entry-url", "--no-entry-url", "--test-coverage-include-all", "--no-test-coverage-include-all",
    "--trace-exit", "--no-trace-exit", "--throw-deprecation", "--no-throw-deprecation", "--report-on-signal", "--no-report-on-signal",
    "--test-coverage-exclude", "--abort-on-uncaught-exception", "--inspect-publish-uid", "--test-reporter-destination", "--test-reporter", "--trace-env-native-stack",
    "--no-trace-env-native-stack", "--require", "--experimental-report", "--bench-reporter", "--verify-base-objects", "--no-verify-base-objects",
    "--interpreted-frames-native-stack", "--test-randomize", "--no-test-randomize", "--test-only", "--no-test-only", "--test-skip-pattern",
    "--tls-keylog", "--bench-isolation", "--bench-reporter-destination", "--max-http-header-size", "--trace-env-js-stack", "--no-trace-env-js-stack",
    "--preserve-symlinks", "--no-preserve-symlinks", "--permission-audit", "--no-permission-audit", "--use-env-proxy", "--no-use-env-proxy",
    "--test-coverage-lines", "--experimental-websocket", "--no-experimental-websocket", "--force-node-api-uncaught-exceptions-policy", "--no-force-node-api-uncaught-exceptions-policy", "--insecure-http-parser",
    "--no-insecure-http-parser", "--tls-min-v1.3", "--no-tls-min-v1.3", "--trace-tls", "--no-trace-tls", "--expose-gc",
    "--experimental-loader", "--http-parser", "--allow-openssl-store", "--no-allow-openssl-store", "--test-isolation", "--inspect-port",
    "--disable-wasm-trap-handler", "--no-disable-wasm-trap-handler", "--experimental-top-level-await", "--heapsnapshot-near-heap-limit", "--report-exclude-network", "--no-report-exclude-network",
    "--tls-max-v1.3", "--no-tls-max-v1.3", "--async-context-frame", "--no-async-context-frame", "--watch", "--no-watch",
    "--experimental-wasi-unstable-preview1", "--cpu-prof-name", "--experimental-vm-modules", "--no-experimental-vm-modules", "--experimental-print-required-tla", "--no-experimental-print-required-tla",
    "--experimental-repl-await", "--no-experimental-repl-await", "--trace-uncaught", "--no-trace-uncaught", "--allow-worker", "--no-allow-worker",
    "--trace-sigint", "--no-trace-sigint", "--test-coverage-include", "--allow-child-process", "--no-allow-child-process", "--test-coverage-functions",
    "--heap-prof", "--no-heap-prof", "--heap-prof-name", "--report-compact", "--no-report-compact", "--cpu-prof-dir",
    "--track-heap-objects", "--no-track-heap-objects", "--disable-proto", "--trace-env", "--no-trace-env", "--frozen-intrinsics",
    "--no-frozen-intrinsics", "--allow-wasi", "--no-allow-wasi", "--experimental-dtls", "--experimental-abortcontroller", "--allow-fs-read",
    "--experimental-import-text", "--no-experimental-import-text", "--allow-net", "--no-allow-net", "--permission", "--no-permission",
    "--disable-sigusr1", "--no-disable-sigusr1", "--deprecation", "--no-deprecation", "--experimental-wasm-modules", "--cpu-prof-interval",
    "--bench-name-pattern", "--addons", "--no-addons", "--trace-sync-io", "--no-trace-sync-io", "--experimental-json-modules",
    "--allow-inspector", "--no-allow-inspector", "--trace-promises", "--no-trace-promises", "--global-search-paths", "--no-global-search-paths",
    "--require-module", "--no-require-module", "--experimental-webstorage", "--no-experimental-webstorage", "--experimental-web-worker", "--no-experimental-web-worker",
    "--experimental-bench", "--no-experimental-bench", "--disable-warning", "--experimental-vfs", "--no-experimental-vfs", "--dns-result-order",
    "--jitless", "--experimental-sqlite", "--no-experimental-sqlite", "--inspect", "--no-inspect", "--heapsnapshot-signal",
    "--experimental-import-meta-resolve", "--no-experimental-import-meta-resolve", "--test-coverage-branches", "--localstorage-file", "--experimental-ffi", "--no-experimental-ffi",
    "--report-signal", "--test-random-seed", "--experimental-fetch", "--bench-samples", "--experimental-global-customevent", "--network-family-autoselection",
    "--no-network-family-autoselection", "--max-old-space-size", "--experimental-quic", "--inspect-brk", "--no-inspect-brk", "--test-name-pattern",
    "--experimental-addon-modules", "--no-experimental-addon-modules", "--strip-types", "--no-strip-types", "--openssl-legacy-provider", "--no-openssl-legacy-provider",
    "--use-largepages", "--experimental-detect-module", "--no-experimental-detect-module", "--max-semi-space-size", "--vfs-mount", "--network-family-autoselection-attempt-timeout",
    "--allow-fs-write", "--extra-info-on-fatal-exception", "--no-extra-info-on-fatal-exception", "--enable-fips-indicator-events", "--no-enable-fips-indicator-events", "--snapshot-blob",
    "--experimental-require-module", "--no-experimental-require-module", "--secure-heap-min", "--diagnostic-dir", "--title", "--experimental-global-navigator",
    "--no-experimental-global-navigator", "--napi-modules", "--import", "--force-context-aware", "--no-force-context-aware", "--enable-fips",
    "--no-enable-fips", "--watch-preserve-output", "--no-watch-preserve-output", "--enable-source-maps", "--no-enable-source-maps", "--use-openssl-ca",
    "--no-use-openssl-ca", "--openssl-config", "--icu-data-dir", "--experimental-specifier-resolution", "--v8-pool-size", "--report-on-fatalerror",
    "--no-report-on-fatalerror", "--secure-heap", "--test-rerun-failures", "--experimental-stream-iter", "--no-experimental-stream-iter", "--trace-deprecation",
    "--no-trace-deprecation", "--trace-warnings", "--no-trace-warnings", "--force-async-hooks-checks", "--no-force-async-hooks-checks", "--tls-min-v1.0",
    "--no-tls-min-v1.0", "--zero-fill-buffers", "--no-zero-fill-buffers", "--report-dir", "--use-bundled-ca", "--no-use-bundled-ca",
    "--pending-deprecation", "--no-pending-deprecation", "--allow-fs-vfs", "--no-allow-fs-vfs", "--max-old-space-size-percentage", "--experimental-global-webcrypto",
    "--force-fips", "--no-force-fips", "--report-filename", "--report-uncaught-exception", "--no-report-uncaught-exception", "--tls-cipher-list",
    "--node-snapshot", "--no-node-snapshot", "--debug-arraybuffer-allocations", "--no-debug-arraybuffer-allocations", "--trace-event-file-pattern", "--conditions",
    "--allow-addons", "--no-allow-addons", "--worker-snapshot", "--no-worker-snapshot", "--openssl-shared-config", "--no-openssl-shared-config",
    "--input-type", "--heap-prof-dir", "--experimental-worker", "--trace-event-categories", "--debug-port", "-r",
    "--es-module-specifier-resolution", "--prof-process", "-C", "--loader", "--webstorage", "--experimental-strip-types",
    "--enable-network-family-autoselection", "--experimental-test-isolation", "--report-directory", "--trace-events-enabled",
    ];
    const __noDash = __flags.map((f) => f.replace(/^--?/, ""));
    // 真机结构（per_thread.js）：Set 本体恒空，迭代/数量全走内部数组
    // （`Set.prototype.add.call(set,"foo")` 写进的内部槽永不被访问）；
    // forEach 逐数组项回（v, v, set）；size 取数组长。嵌入表为真机 dump 去重后
    // 310 项（真机数组 314 含 4 重项，size/forEach 计数差 4，无套件可观察）。
    class NodeEnvironmentFlagsSet extends Set {
      constructor() {
        super();
        // 冻结后仍可写内部盒（真机 kInternal 同构；实例冻结，盒不冻结）。
        this.__wjs2_box = { arr: __flags, noDash: __noDash, set: null };
      }
      add() { return this; }
      delete() { return false; }
      clear() {}
      has(key) {
        if (typeof key !== "string") return false;
        key = key.replace(/_/g, "-");
        if (/^--?/.test(key)) {
          key = key.replace(/=.*$/, "");
          return this.__wjs2_box.arr.includes(key);
        }
        return this.__wjs2_box.noDash.includes(key);
      }
      __wjs2_iter() {
        const b = this.__wjs2_box;
        if (b.set === null) b.set = new Set(b.arr);
        return b.set;
      }
      entries() { return this.__wjs2_iter().entries(); }
      values() { return this.__wjs2_iter().values(); }
      keys() { return this.values(); }
      [Symbol.iterator]() { return this.values(); }
      forEach(cb, thisArg) {
        for (const v of this.__wjs2_box.arr) cb.call(thisArg, v, v, this);
      }
      get size() { return this.__wjs2_box.arr.length; }
    }
    const set = new NodeEnvironmentFlagsSet();
    // 真机同：Set 本体冻结（add/delete/clear 已是 no-op，freeze 防覆写）。
    Object.freeze(set);
    Object.defineProperty(globalThis.process, "allowedNodeEnvironmentFlags", {
      value: set, writable: true, enumerable: true, configurable: true,
    });
    return set;
  },
  set(v) {
    Object.defineProperty(globalThis.process, "allowedNodeEnvironmentFlags", {
      value: v, writable: true, enumerable: true, configurable: true,
    });
  },
  enumerable: true,
  configurable: true,
});
