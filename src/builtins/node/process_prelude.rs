//! 启动期全局 `process` JS（对齐 process_.rs；求值见 node/mod.rs `node_prelude`）。

/// 启动期全局 `process`（`NODE_PRELUDE` 经 `runtime` 在主 PRELUDE 后求值）。
pub const PROCESS_PRELUDE: &str = r#"
// stdout/stderr 造形（10f stream 对拍）：node Socket 形——写直通 fd + EE 全表面
//（on/once/off/addListener/prependListener/removeAllListeners/listenerCount/
// listeners/emit/end/destroy）。写完成回调 microtask 异步回（§4.74）。
function __wjs_stdio_stream(fd) {
  return {
    __wjs_fd: fd,
    write(s, ...rest) {
      const r = fd === 1 ? __wjs_stdout_write(String(s)) : __wjs_stderr_write(String(s));
      const cb = rest.find((a) => typeof a === "function");
      if (cb) queueMicrotask(() => cb());
      return r;
    },
    get isTTY() { return __wjs_stdio_istty(fd); },
    clearLine() { return __wjs_stdio_istty(fd); },
    cursorTo() { return __wjs_stdio_istty(fd); },
    getColorDepth() { return __wjs_stdio_istty(fd) ? 8 : 1; },
    __wjs_listeners: {},
    on(type, cb) {
      if (typeof cb !== "function") throw new TypeError("stdio.on: listener must be a function");
      (this.__wjs_listeners[String(type)] ??= []).push(cb);
      return this;
    },
    addListener(type, cb) { return this.on(type, cb); },
    once(type, cb) {
      const self = this;
      const wrapped = (...a) => { self.off(type, wrapped); cb(...a); };
      wrapped.__wjs_orig = cb;
      return self.on(type, wrapped);
    },
    prependListener(type, cb) {
      if (typeof cb !== "function") throw new TypeError("stdio.prependListener: listener must be a function");
      (this.__wjs_listeners[String(type)] ??= []).unshift(cb);
      return this;
    },
    off(type, cb) {
      const list = this.__wjs_listeners[String(type)];
      if (list) {
        let i = list.findIndex((l) => l === cb || l.__wjs_orig === cb);
        while (i >= 0) { list.splice(i, 1); i = list.findIndex((l) => l === cb || l.__wjs_orig === cb); }
      }
      return this;
    },
    removeListener(type, cb) { return this.off(type, cb); },
    removeAllListeners(type) {
      if (type === undefined) this.__wjs_listeners = {};
      else delete this.__wjs_listeners[String(type)];
      return this;
    },
    listenerCount(type) { return (this.__wjs_listeners[String(type)] || []).length; },
    listeners(type) { return (this.__wjs_listeners[String(type)] || []).slice(); },
    emit(type, ...args) {
      const list = (this.__wjs_listeners[String(type)] || []).slice();
      for (const l of list) l(...args);
      return list.length > 0;
    },
    end(...rest) {
      const cb = rest.find((a) => typeof a === "function");
      if (cb) queueMicrotask(() => cb());
      return this;
    },
    destroy() { return this; },
    __wjs_maxListeners: 10,
    getMaxListeners() { return this.__wjs_maxListeners; },
    setMaxListeners(n) { this.__wjs_maxListeners = Number(n); return this; },
  };
}

globalThis.process = {
  argv: JSON.parse(__wjs_argv_json()),
  // 真机口径：argv0 缺省即 argv[0]（spawn-argv0 套件点名自举回显）。
  argv0: JSON.parse(__wjs_argv_json())[0] ?? __wjs_exec_path(),
  env: (() => {
    // 10f 对拍：worker 会话带 env 快照（创建时复制或自定义对象）——读写全落
    // 本地 store，不碰进程级 env（process-env 套件隔离/快照断言）；主会话与
    // SHARE_ENV 会话走真 env native（原语义不变）。
    const snap = __wjs_worker_env_snapshot();
    if (snap === undefined) {
      return new Proxy({}, {
        get(_, k) {
          if (typeof k !== "string") return undefined;
          const v = __wjs_env_get(k);
          return v === undefined ? undefined : v;
        },
        set(_, k, v) { __wjs_env_set(String(k), String(v)); return true; },
        deleteProperty(_, k) { __wjs_env_del(String(k)); return true; },
        has(_, k) { return __wjs_env_get(String(k)) !== undefined; },
        ownKeys() { return JSON.parse(__wjs_env_keys()); },
        getOwnPropertyDescriptor(_, k) {
          const v = __wjs_env_get(String(k));
          if (v === undefined) return undefined;
          return { value: v, writable: true, enumerable: true, configurable: true };
        },
      });
    }
    const store = JSON.parse(snap);
    return new Proxy({}, {
      get(_, k) { return typeof k === "string" ? store[k] : undefined; },
      set(_, k, v) { store[k] = String(v); return true; },
      deleteProperty(_, k) { delete store[k]; return true; },
      has(_, k) { return typeof k === "string" && k in store; },
      ownKeys() { return Object.keys(store); },
      getOwnPropertyDescriptor(_, k) {
        if (typeof k !== "string" || !(k in store)) return undefined;
        return { value: store[k], writable: true, enumerable: true, configurable: true };
      },
      // node 口径：env 只收 configurable+writable+enumerable 齐备的数据描述符
      //（process-env 套件 defineProperty {value:42} 即抛，message 逐字）。
      defineProperty(_, k, desc) {
        if (desc !== null && typeof desc === "object" &&
            desc.writable === true && desc.enumerable === true && desc.configurable === true &&
            !("get" in desc) && !("set" in desc)) {
          store[k] = String(desc.value);
          return true;
        }
        const e = new TypeError("'process.env' only accepts a configurable, writable, and enumerable data descriptor");
        e.code = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";
        throw e;
      },
    });
  })(),
  cwd() { return __wjs_cwd(); },
  chdir(d) { __wjs_chdir(String(d)); },
  exit(code) {
    // node 口径：'exit' 监听同步派发后再 unwind（mustCall 计数在监听内结算；
    // _exiting 置位，监听内再 mustCall 即抛，真机同）。
    this._exiting = true;
    try { this.__wjs_emit("exit", code === undefined ? (this.exitCode || 0) : Number(code)); } catch {}
    __wjs_process_exit(code === undefined ? undefined : Number(code));
  },
  // node 口径：循环排空即派发 'beforeExit'（exitCode 为参；监听可再排任务续命，
  // 排空后再发）。经 nextTick 投递——监听抛错走 uncaughtException/fatal 同一路由。
  // 返回是否有监听（无则事件循环直接收尾，不多转一轮）。
  __wjs_queueBeforeExit() {
    if (this.listenerCount("beforeExit") === 0) return false;
    this.nextTick(() => this.emit("beforeExit", this.exitCode ?? 0));
    return true;
  },
  // node 口径：退出中标志（common.mustCall 在 exit 处理器内禁调；真机 process._exiting）。
  // 本仓 exit 经哨兵错 unwind：设旗后抛，'exit' 监听在 unwind 前同步派发（见下）。
  _exiting: false,
  // 存活句柄表（assert-leaks 套件：`process._getActiveHandles()` 数组；
  // 本仓收录 watch 句柄（fs 侧登记/摘除），其余底座另案记档）。
  _getActiveHandles() { return [...(globalThis.__wjsFsHandles ?? [])]; },
  // 存活资源类型表（unref-in-cluster 套件：unref 的 UDP 不在表内；
  // 本仓现收录 UDPWrap（dgram 侧登记/摘除），其余底座另案记档）。
  getActiveResourcesInfo() { return [...(globalThis.__wjsActiveResources?.values() ?? [])]; },
  get exitCode() { return __wjs_exit_code_get(); },
  set exitCode(v) {
    const n = Number(v);
    if (!Number.isInteger(n)) throw new TypeError("process.exitCode must be an integer");
    __wjs_exit_code_set(n);
  },
  get platform() { return __wjs_os_platform(); },
  get arch() { return __wjs_os_arch(); },
  version: "v26.9.13",
  // versions.node = Node API 兼容水位（Bun 同哲学：process.version 是自家版本，
  // versions.node 报兼容等级）。22.12 = vite 8 的最低地板（22 && minor>=12），
  // 22.x 大版本保 `^22` caret 区间可用；22.0.0 过不了 vite checkNodeVersion。
  // openssl/sqlite 为兼容水位（套件门控 `hasCrypto/hasSQLite` 用；TLS 底座实为
  // rustls/ring、DB 实为 turso，引擎差异见模块头注；10f 跑 test/common 前置）。
  versions: { node: "22.12.0", winterjs: "26.9.13", mozjs: "153", openssl: "3.6.4", sqlite: "3.53.4" },
  // 构建配置（10f 跑 test/common 前置；键集按套件读取面收敛，非全量 115 键）。
  config: {
    target_defaults: { default_configuration: "Release" },
    variables: {
      asan: 0,
      node_shared: false,
      node_use_ffi: false,
      v8_enable_i18n_support: 1,
      v8_enable_temporal_support: 1,
      v8_use_perfetto: false,
    },
  },
  // 特性门控（套件 hasInspector/hasQuic 等用；inspector 本仓为薄层故 false，
  // quic 真机 26 亦 false；10f 前置）。
  features: {
    inspector: false, debug: false, uv: true, ipv6: true,
    tls: true, tls_alpn: true, tls_sni: true, tls_ocsp: true,
    cached_builtins: true, require_module: true, quic: false,
  },
  execPath: __wjs_exec_path(),
  // node 选项透传（M5 vitest 牵引：无旗恒 []；CLI 起点剥下的 node 运行时旗
  // 回填——common.js 自举 respawn 的 flags 可见性，真机口径）。
  execArgv: JSON.parse(__wjs_node_compat_json()),
  pid: __wjs_pid(),
  // 文件创建掩码（10f：读无参回当前，置数回旧值；真机口径）。
  umask(mask) {
    if (mask === undefined) return __wjs_umask();
    return __wjs_umask(Number(mask));
  },
  uptime() { return __wjs_uptime(); },
  hrtime: Object.assign(
    (t) => {
      const now = BigInt(__wjs_hrtime_ns());
      if (t === undefined) {
        const s = now / 1000000000n;
        return [Number(s), Number(now - s * 1000000000n)];
      }
      const base = BigInt(t[0]) * 1000000000n + BigInt(t[1]);
      const d = now - base;
      return [Number(d / 1000000000n), Number(d % 1000000000n)];
    },
    { bigint: () => BigInt(__wjs_hrtime_ns()) },
  ),
  memoryUsage() { return JSON.parse(__wjs_memory_usage()); },
  // Node 22.3+（vite 用 getBuiltinModule('node:module').Module 做互操作）；
  // 裸名（'module'）与 'node:module' 双形均收（Node 口径），非内置走 require
  // 的可读报错；require 的 ESM-default 口径（node:module default 导出带 Module 类）。
  getBuiltinModule(id) {
    const spec = String(id);
    return globalThis.require(spec.startsWith("node:") ? spec : `node:${spec}`);
  },
  // stdout/stderr 富流（真 node 是 Socket；10f 起 helper 造形：直写 fd +
  // EE 全表面——pipe 的 dest.on/emit('pipe')/close/finish 登记接得住；
  // 事件面空转（无 data/end 发射）偏差记档）。clearLine/cursorTo/getColorDepth
  // 非 TTY no-op（vite dev；TTY 下调用方自写 ANSI）。stdin：监听登记 +
  // isTTY + EOF read()（偏差记档：stdin EOF/data 不投递、信号不投递——
  // 注册表只收不发，SIGTERM 默认行为不变（OS 默认终止））。
  stdout: __wjs_stdio_stream(1),
  stderr: __wjs_stdio_stream(2),
  stdin: {
    get isTTY() { return __wjs_stdio_istty(0); },
    __wjs_listeners: {},
    __wjs_enc: null,
    __wjs_polling: false,
    __wjs_ended: false,
    on(type, cb) {
      if (typeof cb !== "function") throw new TypeError("stdin.on: listener must be a function");
      (this.__wjs_listeners[String(type)] ??= []).push(cb);
      if (type === "data" || type === "readable" || type === "end") this.__wjs_startPoll();
      return this;
    },
    once(type, cb) { return this.on(type, cb); },
    off(type, cb) {
      const list = this.__wjs_listeners[String(type)];
      if (list) {
        const i = list.indexOf(cb);
        if (i >= 0) list.splice(i, 1);
      }
      return this;
    },
    removeListener(type, cb) { return this.off(type, cb); },
    setEncoding(e) { this.__wjs_enc = (e === null || e === undefined) ? null : String(e); return this; },
    __wjs_emitStdin(type, arg) {
      const list = (this.__wjs_listeners[String(type)] || []).slice();
      for (const l of list) { try { l(arg); } catch {} }
      return list.length;
    },
    // stdin 轮询投递（kill 套件：子进程读父写 stdin；echo x | winterjs 真机口径）：
    // 首个 data/readable/end 监听即起 10ms refed 轮询（续命到 EOF），EOF 清环
    // 发 end；TTY 归 REPL，不管；Buffer 块（setEncoding 即转串）。
    __wjs_startPoll() {
      if (this.__wjs_polling || this.__wjs_ended) return;
      if (__wjs_stdio_istty(0)) return;
      this.__wjs_polling = true;
      const self = this;
      const timer = setInterval(() => {        let r;
        try { r = __wjs_stdin_poll(); } catch { r = "E"; }
        if (r === "E") {
          clearInterval(timer);
          self.__wjs_polling = false;
          self.__wjs_ended = true;
          self.__wjs_emitStdin("end");
          // node 口径：stdin EOF 后发 'close'（chunk-problem 的 shasum 形靠它；
          // 异步一轮——end 监听内挂 close 仍可达）。
          queueMicrotask(() => self.__wjs_emitStdin("close"));
          return;
        }
        if (r !== "") {
          const bin = atob(r.slice(1));
          const u8 = new Uint8Array(bin.length);
          for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
          const chunk = self.__wjs_enc !== null ? Buffer.from(u8).toString(self.__wjs_enc) : Buffer.from(u8);
          self.__wjs_emitStdin("data", chunk);
        }
      }, 10);
      this.__wjs_timer = timer;
    },
    read() { return null; },
    pause() { return this; },
    resume() { return this; },
    setRawMode() { return this; },
    unref() { return this; },
    ref() { return this; },
    // destroy 即关（listen-after-destroying-stdin 套件）：停轮询、标终结、
    // 发 close（真机语义；读端已决议的不重发 end）。
    destroy() {
      this.__wjs_ended = true;
      try { if (this.__wjs_timer) clearInterval(this.__wjs_timer); } catch {}
      this.__wjs_polling = false;
      this.__wjs_emitStdin("close");
      return this;
    },
  },
  getuid() { return __wjs_process_getuid(); },
  getgid() { return __wjs_process_getgid(); },
  geteuid() { return __wjs_process_geteuid(); },
  getegid() { return __wjs_process_getegid(); },
  getgroups() { return __wjs_process_getgroups(); },
  nextTick(cb, ...args) {
    if (typeof cb !== "function") throw new TypeError("nextTick: callback must be a function");
    // 原生队列（node 口径）：tick 由 pump 在 RunJobs 前后收割——同步期入队的
    // tick 先于微任务、微任务期入队的等整轮微任务排空（V8 checkpoint 原子性）。
    // 回调抛错经 drain 侧 uncaughtException 路由（destroy/emitErrorNT 等内建
    // 全走 nextTick，throw 落成 rejection 即全族套件反红）。
    __wjs_next_tick(cb, args);
  },
  // Phase 9a（node:events MaxListenersExceededWarning 路径）：warning 监听 + emitWarning。
  // Node 语义收敛：string → 包 Error（name=type||'Warning'，code/detail 挂载）；
  // Error 原样；第二参可 string（type）或 { type, code, detail }；有监听走监听，
  // 否则 stderr 默认打印 `(node:<pid>) [code] Name: message`。
  __wjs_warningListeners: [],
  // 通用监听表（warning 沿旧径；signal/stdin 等只登记不投递——偏差记档，
  // SIGTERM 默认行为不变）。emit 供未来事件循环接信号投递。
  // 方法一律走 `this`（套件 process-tampering：node common 载入期捕获
  // `const process = globalThis.process`，之后全局被换也不经它读表）。
  __wjs_listeners: {},
  on(type, cb) {
    if (type === "warning" && typeof cb === "function") this.__wjs_warningListeners.push(cb);
    if (typeof cb !== "function") throw new TypeError("process.on: listener must be a function");
    (this.__wjs_listeners[String(type)] ??= []).push(cb);
    return this;
  },
  once(type, cb) {
    if (typeof cb !== "function") throw new TypeError("process.once: listener must be a function");
    const self = this;
    const wrapped = (...args) => { self.off(type, wrapped); cb(...args); };
    wrapped.__wjs_orig = cb;
    return self.on(type, wrapped);
  },
  off(type, cb) {
    const list = this.__wjs_listeners[String(type)];
    if (list) {
      let i = list.findIndex((l) => l === cb || l.__wjs_orig === cb);
      while (i >= 0) { list.splice(i, 1); i = list.findIndex((l) => l === cb || l.__wjs_orig === cb); }
    }
    return this;
  },
  removeListener(type, cb) { return this.off(type, cb); },
  // node process 即 EventEmitter（套件 promises-scheduler：process.addListener/
  // process.emit 直用）；emit 返回是否命中监听（node 口径）。
  addListener(type, cb) { return this.on(type, cb); },
  // node 口径（EventEmitter.emit）：监听抛错原样上抛；无监听的 'error' 即抛。
  // 宿主内部派发走 `__wjs_emit`（吞错，结算点不被用户监听打断）。
  emit(type, ...args) {
    const list = [...(this.__wjs_listeners[String(type)] ?? [])];
    if (list.length === 0 && type === "error") {
      const er = args[0];
      if (er instanceof Error) throw er;
      const e = new Error(`Unhandled error. (${require("node:util").inspect(er)})`);
      e.code = "ERR_UNHANDLED_ERROR";
      e.context = er;
      throw e;
    }
    for (const l of list) Reflect.apply(l, this, args);
    return list.length > 0;
  },
  removeAllListeners(type) {
    if (type === undefined) this.__wjs_listeners = {};
    else delete this.__wjs_listeners[String(type)];
    return this;
  },
  listenerCount(type) { return (this.__wjs_listeners[String(type)] ?? []).length; },
  // EventEmitter 读表（M5 vitest 牵引：init 链 `process.listeners(..).bind(..)`）。
  listeners(type) { return [...(this.__wjs_listeners[String(type)] ?? [])]; },
  rawListeners(type) { return this.listeners(type); },
  eventNames() { return Object.keys(this.__wjs_listeners); },
  __wjs_emit(type, ...args) {
    const list = [...(this.__wjs_listeners[String(type)] ?? [])];
    for (const l of list) {
      try { l.call(this, ...args); } catch {}
    }
    return list.length;
  },
  emitWarning(warning, typeOrOptions, code, _ctor) {
    let type, detail;
    if (typeof typeOrOptions === "object" && typeOrOptions !== null) {
      type = typeOrOptions.type; code = typeOrOptions.code; detail = typeOrOptions.detail;
    } else {
      type = typeOrOptions;
    }
    if (typeof warning === "string") {
      warning = new Error(warning);
      warning.name = String(type || "Warning");
      if (code) warning.code = String(code);
      if (detail) warning.detail = String(detail);
    } else if (warning !== null && typeof warning === "object") {
      if (type && !warning.name) warning.name = String(type);
      if (code && !warning.code) warning.code = String(code);
    } else {
      throw new TypeError("warning must be a string or an Error");
    }
    // node 口径：warning 异步派发（nextTick）——emitWarning 同步返回后
    // 调用方才挂 'warning' 监听（套件"先 parse 后 expectWarning"的时序
    // 依赖此，10f url DEP0169 现形）；§4.74 同源教训。
    queueMicrotask(() => {
      const listeners = this.__wjs_warningListeners;
      if (listeners.length > 0) {
        for (const l of listeners) {
          try { l.call(this, warning); } catch {}
        }
      } else {
        const codePart = warning.code ? `[${warning.code}] ` : "";
        const line = `(node:${__wjs_pid()}) ${codePart}${warning.name}: ${warning.message}`;
        __wjs_stderr_write(line + "\n");
        if (warning.detail) __wjs_stderr_write(warning.detail + "\n");
      }
    });
  },
};
// 真机口径：process[Symbol.toStringTag] = "process"（不可枚举，实测 getter 面），
// String(process) → '[object process]'（vm basic 套件 / util.inspect 点名）。
Object.defineProperty(globalThis.process, Symbol.toStringTag, { value: "process" });
// Node 兼容旗语义（CLI 起点剥下，见 cli::strip_node_compat_args）：
// --expose-gc 即暴露 globalThis.gc（async no-op——真收集另案，调用形状先行；
// 无旗不暴露，真机口径）；名单挂内部位供 http 默认宽松等消费（不进 process.env）。
try {
  const __compat = JSON.parse(__wjs_node_compat_json());
  globalThis.__wjs_nodeCompat = Array.isArray(__compat) ? __compat : [];
  // gc 门控（--expose-gc/--expose_gc 双拼写， deterioration 套件用下划线形）。
  const __hasGc = globalThis.__wjs_nodeCompat.includes("--expose-gc") ||
    globalThis.__wjs_nodeCompat.includes("--expose_gc");
  if (__hasGc && typeof globalThis.gc !== "function") {
    globalThis.gc = async function gc() { return undefined; };
  }
} catch { globalThis.__wjs_nodeCompat = []; }
// Node 口径：NODE_DEBUG 置位即启动期警告一次（首 section 名；debug.js 套件
// 逐字断言。stderr 直写，不走 warning 通道）。
try {
  const __nd = __wjs_env_get("NODE_DEBUG");
  if (__nd !== undefined && __nd !== null && String(__nd).trim() !== "") {
    const __sec = String(__nd).split(",")[0].trim();
    __wjs_stderr_write(`Setting the NODE_DEBUG environment variable to '${__sec}' can expose sensitive data (such as passwords, tokens and authentication headers) in the resulting log.\n`);
  }
} catch { /* 环境不可读即跳过 */ }
"#;
