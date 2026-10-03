//! 启动期全局 `process` JS（对齐 process_.rs；求值见 node/mod.rs `node_prelude`）。

/// 启动期全局 `process`（`NODE_PRELUDE` 经 `runtime` 在主 PRELUDE 后求值）。
pub const PROCESS_PRELUDE: &str = r#"
// stdout/stderr 造形（10f stream 对拍）：node Socket 形——写直通 fd + EE 全表面
//（on/once/off/addListener/prependListener/removeAllListeners/listenerCount/
// listeners/emit/end/destroy）。写完成回调 microtask 异步回（§4.74）。
function __wjs2_stdio_stream(fd) {
  return {
    __wjs2_fd: fd,
    // 可写恒真（execve-throws 套件点名 stdout/stderr.writable；真机流面）。
    writable: true,
    write(s, ...rest) {
      const r = fd === 1 ? __wjs2_stdout_write(String(s)) : __wjs2_stderr_write(String(s));
      const cb = rest.find((a) => typeof a === "function");
      if (cb) queueMicrotask(() => cb());
      return r;
    },
    get isTTY() { return __wjs2_stdio_istty(fd); },
    clearLine() { return __wjs2_stdio_istty(fd); },
    cursorTo() { return __wjs2_stdio_istty(fd); },
    getColorDepth() { return __wjs2_stdio_istty(fd) ? 8 : 1; },
    __wjs2_listeners: {},
    on(type, cb) {
      if (typeof cb !== "function") throw new TypeError("stdio.on: listener must be a function");
      (this.__wjs2_listeners[String(type)] ??= []).push(cb);
      return this;
    },
    addListener(type, cb) { return this.on(type, cb); },
    once(type, cb) {
      const self = this;
      const wrapped = (...a) => { self.off(type, wrapped); cb(...a); };
      wrapped.__wjs2_orig = cb;
      return self.on(type, wrapped);
    },
    prependListener(type, cb) {
      if (typeof cb !== "function") throw new TypeError("stdio.prependListener: listener must be a function");
      (this.__wjs2_listeners[String(type)] ??= []).unshift(cb);
      return this;
    },
    off(type, cb) {
      const list = this.__wjs2_listeners[String(type)];
      if (list) {
        let i = list.findIndex((l) => l === cb || l.__wjs2_orig === cb);
        while (i >= 0) { list.splice(i, 1); i = list.findIndex((l) => l === cb || l.__wjs2_orig === cb); }
      }
      return this;
    },
    removeListener(type, cb) { return this.off(type, cb); },
    removeAllListeners(type) {
      if (type === undefined) this.__wjs2_listeners = {};
      else delete this.__wjs2_listeners[String(type)];
      return this;
    },
    listenerCount(type) { return (this.__wjs2_listeners[String(type)] || []).length; },
    listeners(type) { return (this.__wjs2_listeners[String(type)] || []).slice(); },
    emit(type, ...args) {
      const list = (this.__wjs2_listeners[String(type)] || []).slice();
      for (const l of list) l(...args);
      return list.length > 0;
    },
    end(...rest) {
      const cb = rest.find((a) => typeof a === "function");
      if (cb) queueMicrotask(() => cb());
      return this;
    },
    destroy() { return this; },
    __wjs2_maxListeners: 10,
    getMaxListeners() { return this.__wjs2_maxListeners; },
    setMaxListeners(n) { this.__wjs2_maxListeners = Number(n); return this; },
  };
}

// cpu 面 prevValue 校验（node 口径 per_thread.js previousValueIsValid +
// validateObject/validateNumber；范围错为 RangeError 名 + ERR_INVALID_ARG_VALUE 码，
// 本仓 errors 端口无 RangeError 子构造，此处按文案逐字手拼）。
function __wjs2_checkUsagePrev(prevValue) {
  if (!prevValue) return;
  const E = require("internal/errors").codes;
  const valid = (n) => typeof n === "number" && n <= Number.MAX_SAFE_INTEGER && n >= 0;
  if (!valid(prevValue.user)) {
    const v = prevValue;
    if (v === null || Array.isArray(v) || typeof v !== "object") throw new E.ERR_INVALID_ARG_TYPE("prevValue", "Object", v);
    if (typeof v.user !== "number") throw new E.ERR_INVALID_ARG_TYPE("prevValue.user", "number", v.user);
    const e = new RangeError(`The property 'prevValue.user' is invalid. Received ${String(v.user)}`);
    e.code = "ERR_INVALID_ARG_VALUE";
    throw e;
  }
  if (!valid(prevValue.system)) {
    if (typeof prevValue.system !== "number") throw new E.ERR_INVALID_ARG_TYPE("prevValue.system", "number", prevValue.system);
    const e = new RangeError(`The property 'prevValue.system' is invalid. Received ${String(prevValue.system)}`);
    e.code = "ERR_INVALID_ARG_VALUE";
    throw e;
  }
}

// POSIX 身份参数门（node 口径 does_own_process_state.js validateId）。
function __wjs2_validateId(id, name) {
  const E = require("internal/errors").codes;
  if (typeof id === "number") {
    if (!Number.isInteger(id)) throw new E.ERR_OUT_OF_RANGE(name, "an integer", id);
    if (id < 0 || id > 4294967295) throw new E.ERR_OUT_OF_RANGE(name, ">= 0 && <= 4294967295", id);
  } else if (typeof id !== "string") {
    throw new E.ERR_INVALID_ARG_TYPE(name, ["number", "string"], id);
  }
}

// 未知身份错（node 口径 `X identifier does not exist: id`）。
function __wjs2_unknownCredential(type, id) {
  const e = new Error(`${type} identifier does not exist: ${id}`);
  e.code = "ERR_UNKNOWN_CREDENTIAL";
  throw e;
}

// set*id syscall 失败错（EPERM 等；String(err) 形如 `Error: EPERM, …`，套件正则口径）。
function __wjs2_credSysErr(errno, syscall, id) {
  const name = { 1: "EPERM", 13: "EACCES", 22: "EINVAL" }[errno] ?? `errno-${errno}`;
  const e = new Error(`${name}, ${syscall} '${id}'`);
  e.code = name;
  e.errno = errno;
  e.syscall = syscall;
  throw e;
}

// wrapIdSetter 口径：校验 → 数形归一 → 未知身份错 → syscall 错。
function __wjs2_credIdSetter(type, syscall, native, id) {
  __wjs2_validateId(id, "id");
  if (typeof id === "number") id >>>= 0;
  const r = native(JSON.stringify(id));
  if (r === 1) __wjs2_unknownCredential(type, id);
  if (r < 0) __wjs2_credSysErr(-r, syscall, id);
}

// env 非串/数/布尔赋值即 DEP0104（env-deprecation 套件；文案逐字）。
function __wjs2_emitEnvDeprecation(v) {
  if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") return;
  try {
    globalThis.process.emitWarning(
      "Assigning any value other than a string, number, or boolean to a process.env property is deprecated. " +
      "Please make sure to convert the value to a string before setting process.env with it.",
      "DeprecationWarning", "DEP0104");
  } catch {}
}

globalThis.process = {
  argv: JSON.parse(__wjs2_argv_json()),
  // 真机口径：argv0 缺省即 argv[0]（spawn-argv0 套件点名自举回显）。
  argv0: JSON.parse(__wjs2_argv_json())[0] ?? __wjs2_exec_path(),
  env: (() => {
    // 10f 对拍：worker 会话带 env 快照（创建时复制或自定义对象）——读写全落
    // 本地 store，不碰进程级 env（process-env 套件隔离/快照断言）；主会话与
    // SHARE_ENV 会话走真 env native（原语义不变）。
    const snap = __wjs2_worker_env_snapshot();
    if (snap === undefined) {
      return new Proxy({}, {
        get(t, k) {
          if (typeof k !== "string") return undefined;
          const v = __wjs2_env_get(k);
          // 非变量键回原型（hasOwnProperty 等；env.js 套件点名）。
          return v === undefined ? t[k] : v;
        },
        set(_, k, v) {
          // 符号键/值不收（严格模式即 TypeError；env-symbols 套件点名）。
          if (typeof k !== "string" || typeof v === "symbol") return false;
          __wjs2_emitEnvDeprecation(v);
          __wjs2_env_set(String(k), String(v));
          return true;
        },
        deleteProperty(_, k) { __wjs2_env_del(String(k)); return true; },
        has(t, k) { return (typeof k === "string" && __wjs2_env_get(k) !== undefined) || k in t; },
        ownKeys() { return JSON.parse(__wjs2_env_keys()); },
        getOwnPropertyDescriptor(_, k) {
          const v = __wjs2_env_get(String(k));
          if (v === undefined) return undefined;
          return { value: v, writable: true, enumerable: true, configurable: true };
        },
        // node 口径：env 只收 configurable+writable+enumerable 齐备的数据描述符
        //（process-env 套件 defineProperty {value:42} 即抛，message 逐字）。
        defineProperty(_, k, desc) {
          if (desc !== null && typeof desc === "object" &&
            desc.writable === true && desc.enumerable === true && desc.configurable === true &&
            !("get" in desc) && !("set" in desc)) {
            __wjs2_emitEnvDeprecation(desc.value);
            __wjs2_env_set(String(k), String(desc.value));
            return true;
          }
          const isAccessor = desc !== null && typeof desc === "object" &&
            ("get" in desc || "set" in desc);
          const e = new TypeError(isAccessor
            ? "'process.env' does not accept an accessor(getter/setter) descriptor"
            : "'process.env' only accepts a configurable, writable, and enumerable data descriptor");
          e.code = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";
          throw e;
        },
      });
    }
    const store = JSON.parse(snap);
    return new Proxy({}, {
      get(_, k) { return typeof k === "string" ? store[k] : undefined; },
      set(_, k, v) {
        if (typeof k !== "string" || typeof v === "symbol") return false;
        __wjs2_emitEnvDeprecation(v);
        store[k] = String(v);
        return true;
      },
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
  cwd() { return __wjs2_cwd(); },
  chdir(d) {
    if (typeof d !== "string") throw new (require("internal/errors").codes.ERR_INVALID_ARG_TYPE)("directory", "string", d);
    try { __wjs2_chdir(d); } catch (e) {
      if (e && typeof e.code === "string" && e.code !== "" && e.code !== "UNKNOWN") throw e;
      const cwd = __wjs2_cwd();
      const err = new Error(`ENOENT: no such file or directory, chdir '${cwd}' -> '${d}'`);
      err.code = "ENOENT";
      err.errno = -2;
      err.syscall = "chdir";
      err.path = cwd;
      err.dest = d;
      throw err;
    }
  },
  exit(code) {
    // node 口径：显式传参即经 exitCode setter（非法同步抛）；'exit' 同步派发后
    // 走可 mock 的 reallyExit（really-exit 套件点名；默认即哨兵退出）。
    if (code !== undefined) this.exitCode = code;
    this._exiting = true;
    try { this.__wjs2_emit("exit", this.exitCode || 0); } catch {}
    this.reallyExit(this.exitCode || 0);
  },
  reallyExit(code) {
    __wjs2_process_exit(code === undefined ? undefined : Number(code));
  },
  // node 口径：循环排空即派发 'beforeExit'（exitCode 为参；监听可再排任务续命，
  // 排空后再发）。经 nextTick 投递——监听抛错走 uncaughtException/fatal 同一路由。
  // 返回是否有监听（无则事件循环直接收尾，不多转一轮）。
  __wjs2_queueBeforeExit() {
    if (this.listenerCount("beforeExit") === 0) return false;
    this.nextTick(() => this.emit("beforeExit", this.exitCode ?? 0));
    return true;
  },
  // node 口径：退出中标志（common.mustCall 在 exit 处理器内禁调；真机 process._exiting）。
  // 本仓 exit 经哨兵错 unwind：设旗后抛，'exit' 监听在 unwind 前同步派发（见下）。
  _exiting: false,
  // 存活句柄表（assert-leaks 套件：`process._getActiveHandles()` 数组；
  // 本仓收录 watch 句柄（fs 侧登记/摘除），其余底座另案记档）。
  _getActiveHandles() { return [...(globalThis.__wjs2FsHandles ?? [])]; },
  // 存活资源类型表（unref-in-cluster 套件：unref 的 UDP 不在表内；
  // 本仓现收录 UDPWrap（dgram 侧登记/摘除），其余底座另案记档）。
  getActiveResourcesInfo() { return [...(globalThis.__wjs2ActiveResources?.values() ?? [])]; },
  get exitCode() { return __wjs2_exit_code_get(); },
  set exitCode(code) {
    // node 口径 internal/bootstrap/node.js：null/undefined 清除；非空串先
    // Number() 试转（NaN 则保留原串进校验）；validateInteger 分两错
    //（非 number 即 ERR_INVALID_ARG_TYPE，散件.pattern 点名；非整数即
    // ERR_OUT_OF_RANGE，2.1/Infinity/NaN 点名）。
    if (code === null || code === undefined) { __wjs2_exit_code_unset(); return; }
    const E = require("internal/errors").codes;
    let value = code;
    if (typeof code === "string" && code !== "") {
      const n = Number(code);
      if (!Number.isNaN(n)) value = n;
    }
    if (typeof value !== "number") throw new E.ERR_INVALID_ARG_TYPE("code", "number", value);
    if (!Number.isInteger(value)) {
      const e = new RangeError(`The value of "code" is out of range. It must be an integer. Received ${String(value)}`);
      e.code = "ERR_OUT_OF_RANGE";
      throw e;
    }
    __wjs2_exit_code_set(value);
  },
  get platform() { return __wjs2_os_platform(); },
  get arch() { return __wjs2_os_arch(); },
  version: "v26.10.3",
  // versions.node = Node API 兼容水位（Bun 同哲学：process.version 是自家版本，
  // versions.node 报兼容等级）。22.12 = vite 8 的最低地板（22 && minor>=12），
  // 22.x 大版本保 `^22` caret 区间可用；22.0.0 过不了 vite checkNodeVersion。
  // openssl/sqlite 为兼容水位（套件门控 `hasCrypto/hasSQLite` 用；TLS 底座实为
  // rustls/ring、DB 实为 turso，引擎差异见模块头注；10f 跑 test/common 前置）。
  versions: { node: "22.12.0", winterjs2: "26.10.3", mozjs: "153", openssl: "3.6.4", sqlite: "3.53.4" },
  release: { name: "node", lts: "Jod", sourceUrl: "https://nodejs.org/download/release/v22.12.0/node-v22.12.0.tar.gz", headersUrl: "https://nodejs.org/download/release/v22.12.0/node-v22.12.0-headers.tar.gz" },
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
  execPath: __wjs2_exec_path(),
  // node 选项透传（M5 vitest 牵引：无旗恒 []；CLI 起点剥下的 node 运行时旗
  // 回填——common.js 自举 respawn 的 flags 可见性，真机口径）。
  execArgv: JSON.parse(__wjs2_node_compat_json()),
  pid: __wjs2_pid(),
  ppid: __wjs2_ppid(),
  // 标题（get 缺省回 execPath 基名；set 透写 store，真机读写口径）。
  get title() {
    const t = globalThis.__wjs2_processTitle;
    if (typeof t === "string") return t;
    try {
      const exe = String(__wjs2_exec_path());
      const base = exe.split(/[\\/]/).pop().replace(/\.exe$/, "");
      return base || exe;
    } catch { return ""; }
  },
  set title(v) { globalThis.__wjs2_processTitle = String(v); },
  // 信号投递（node 口径 per_thread.js kill 逐字：pid 松散门 + 信号名/数双形 +
  // _kill 可 mock 点 + errno 成错；sig 数形直通，名形查 os 表）。
  kill(pid, sig) {
    const E = require("internal/errors").codes;
    if (pid != (pid | 0)) throw new E.ERR_INVALID_ARG_TYPE("pid", "number", pid);
    let err;
    if (sig === (sig | 0)) {
      err = this._kill(pid, sig);
    } else {
      sig ||= "SIGTERM";
      const SIG = require("node:os").constants.signals;
      if (SIG[sig]) {
        err = this._kill(pid, SIG[sig]);
      } else {
        throw new E.ERR_UNKNOWN_SIGNAL(sig);
      }
    }
    if (err) {
      const name = { 1: "EPERM", 3: "ESRCH", 22: "EINVAL" }[err] ?? `errno-${err}`;
      const e = new Error(`kill ${name}`);
      e.code = name;
      e.errno = err;
      e.syscall = "kill";
      throw e;
    }
    return true;
  },
  // 默认投递器（可被用户 mock，见 kill-pid 套件；返回 errno 数，0 即成）。
  _kill(pid, sig) { return __wjs2_kill(JSON.stringify(pid), JSON.stringify(sig)); },
  // 镜像替换（node 口径 per_thread.js execve 逐字：worker/平台门 + 校验 +
  // 成功不返回；自身软链/直链补 --run 自举，__selfArgv 同口径）。
  execve(execPath, args = [], env = process.env) {
    const E = require("internal/errors").codes;
    const { isMainThread } = require("node:worker_threads");
    if (!isMainThread) {
      throw new E.ERR_WORKER_UNSUPPORTED_OPERATION("Calling process.execve");
    }
    if (process.platform === "win32" || process.platform === "os400") {
      throw new E.ERR_FEATURE_UNAVAILABLE_ON_PLATFORM("process.execve");
    }
    if (typeof execPath !== "string") throw new E.ERR_INVALID_ARG_TYPE("execPath", "string", execPath);
    if (!Array.isArray(args)) throw new E.ERR_INVALID_ARG_TYPE("args", "Array", args);
    for (let i = 0; i < args.length; i++) {
      const arg = args[i];
      if (typeof arg !== "string" || arg.includes("\0")) {
        throw new E.ERR_INVALID_ARG_VALUE(`args[${i}]`, arg, "must be a string without null bytes");
      }
    }
    if (env === null || Array.isArray(env) || typeof env !== "object") {
      throw new E.ERR_INVALID_ARG_TYPE("env", "Object", env);
    }
    const envArray = [];
    for (const [key, value] of Object.entries(env)) {
      if (typeof key !== "string" || typeof value !== "string" ||
          key.includes("\0") || value.includes("\0")) {
        throw new E.ERR_INVALID_ARG_VALUE("env", env, "must be an object with string keys and values without null bytes");
      }
      envArray.push(`${key}=${value}`);
    }
    // 自身：裸文件形补 --run（子进程 argv 保持 node 形，execve 套件点名）。
    let argv = args;
    try {
      const fs = require("node:fs");
      if (fs.realpathSync(execPath) === fs.realpathSync(process.execPath)) {
        const a = [...args];
        if (a[1] === "-e" || a[1] === "-p") a.splice(1, 1, "--eval");
        else if (a[1] !== undefined && !String(a[1]).startsWith("-")) a.splice(1, 0, "--run");
        argv = a;
      }
    } catch {}
    const r = JSON.parse(__wjs2_execve(execPath, JSON.stringify(argv), JSON.stringify(envArray)));
    // 到此即失败（成功不返回）：成系统错（ENOENT 口径 `ENOENT, text 'path'`）。
    const code = { 1: "EPERM", 2: "ENOENT", 8: "ENOEXEC", 13: "EACCES", 20: "ENOTDIR", 22: "EINVAL", 40: "ELOOP", 63: "ENAMETOOLONG" }[r.errno] ?? `ERRNO_${r.errno}`;
    const e = new Error(`${code}, ${r.text} '${execPath}'`);
    e.code = code;
    e.errno = r.errno;
    e.syscall = "execve";
    e.path = execPath;
    throw e;
  },
  // 文件创建掩码（node 口径 lib/internal/bootstrap/switches/does_own_process_state.js：
  // 串形按八进制解析（非法即 ERR_INVALID_ARG_VALUE），数形走 uint32 门）。
  umask(mask) {
    if (mask === undefined) return __wjs2_umask();
    const E = require("internal/errors").codes;
    if (typeof mask === "string") {
      if (!/^[0-7]+$/.test(mask)) throw new E.ERR_INVALID_ARG_VALUE("mask", mask, "must be a 32-bit unsigned integer or an octal string");
      mask = parseInt(mask, 8);
    } else if (typeof mask !== "number") {
      throw new E.ERR_INVALID_ARG_TYPE("mask", "number", mask);
    } else {
      if (!Number.isInteger(mask)) throw new E.ERR_OUT_OF_RANGE("mask", "an integer", mask);
      if (mask < 0 || mask > 4294967295) throw new E.ERR_OUT_OF_RANGE("mask", ">= 0 && <= 4294967295", mask);
    }
    return __wjs2_umask(mask);
  },
  uptime() { return __wjs2_uptime(); },
  hrtime: Object.assign(
    (t) => {
      if (t !== undefined) {
        if (!Array.isArray(t)) throw new (require("internal/errors").codes.ERR_INVALID_ARG_TYPE)("time", "Array", t);
        if (t.length !== 2) throw new (require("internal/errors").codes.ERR_OUT_OF_RANGE)("time", 2, t.length);
      }
      const now = BigInt(__wjs2_hrtime_ns());
      if (t === undefined) {
        const s = now / 1000000000n;
        return [Number(s), Number(now - s * 1000000000n)];
      }
      // node 口径（lib/internal/process/per_thread.js）：秒/纳秒分开减，
      // 纳秒借位（nsec<0 即 sec-1、nsec+1e9）——diff[1] 恒 ∈ [0,1e9），
      // 未来时刻 diff[0] 可为负（nodejs/node#4751；单 BigInt 取余会带负号）。
      let sec = now / 1000000000n - BigInt(t[0]);
      let nsec = now % 1000000000n - BigInt(t[1]);
      if (nsec < 0n) { sec -= 1n; nsec += 1000000000n; }
      return [Number(sec), Number(nsec)];
    },
    { bigint: () => BigInt(__wjs2_hrtime_ns()) },
  ),
  memoryUsage() { return JSON.parse(__wjs2_memory_usage()); },
  // abort 为箭头函数：无 prototype（套件点名），new 即 TypeError；调用即 SIGABRT。
  abort: () => { __wjs2_process_abort(); },
  availableMemory() { return __wjs2_available_memory(); },
  constrainedMemory() { return __wjs2_constrained_memory(); },
  // cpu 面校验（node 口径 lib/internal/process/per_thread.js wrapProcessMethods）：
  // prevValue 非法形逐级抛（对象门 → user 数门 → user 范围门 → system 同序）。
  cpuUsage(prevValue) {
    __wjs2_checkUsagePrev(prevValue);
    const cur = JSON.parse(__wjs2_cpu_usage());
    if (prevValue) return { user: cur.user - prevValue.user, system: cur.system - prevValue.system };
    return { user: cur.user, system: cur.system };
  },
  threadCpuUsage(prevValue) {
    if (globalThis.process && globalThis.process.platform === "sunos") {
      throw new (require("internal/errors").codes.ERR_OPERATION_FAILED)("threadCpuUsage is not available on SunOS");
    }
    __wjs2_checkUsagePrev(prevValue);
    const cur = JSON.parse(__wjs2_thread_cpu_usage());
    if (prevValue) return { user: cur.user - prevValue.user, system: cur.system - prevValue.system };
    return { user: cur.user, system: cur.system };
  },
  // Node 22.3+（vite 用 getBuiltinModule('node:module').Module 做互操作）；
  // 真机 internal/modules/helpers.js：非串即 ERR_INVALID_ARG_TYPE；
  // 归一化失败（'test'/'sea'/internal/* 等）回 undefined 不抛（R9）。
  // 注意：不做裸名→node: 前缀拼接——'test' 必须 undefined 而 'fs' 本就
  // 经 require 别名可达（builtinModules 混合表，真机同）。
  getBuiltinModule(id) {
    const E = require("internal/errors").codes;
    if (typeof id !== "string") throw new E.ERR_INVALID_ARG_TYPE("id", "string", id);
    // R9：裸 'test' 在 node builtinModules 无此项（仅 'node:test'），必须
    // undefined（本仓注册表含裸 'test' 别名，require 可达，真机不可）；
    // 'internal/*' 同理（本仓内部件 require 可达，真机归一化回 undefined）。
    // 新风格裸名（sqlite/quic/sea/ffi/vfs）同理（真机仅前缀形）。
    if (id === "test" || id === "sqlite" || id === "quic" || id === "sea"
      || id === "ffi" || id === "vfs" || id.startsWith("internal/")) return undefined;
    try {
      return globalThis.require(id);
    } catch {
      return undefined;
    }
  },
  // 内部绑定（realm.js 口径最小集：'util' 回 16 键与 util.types 恒等对象，
  // 他名即 `No such module`；String() 归一，真机同）。
  binding(mod) {
    mod = String(mod);
    if (mod === "util") {
      const t = require("node:util").types;
      const out = {};
      for (const k of ["isAnyArrayBuffer","isArrayBuffer","isArrayBufferView",
        "isAsyncFunction","isDataView","isDate","isExternal","isMap",
        "isMapIterator","isNativeError","isPromise","isRegExp","isSet",
        "isSetIterator","isTypedArray","isUint8Array"]) out[k] = t[k];
      return out;
    }
    throw new Error(`No such module: ${mod}`);
  },
  // 裸调试输出（per_thread.js 口径：util.format 后直写 fd，不走 stderr.write；
  // hijack 套件点名绕过）。
  _rawDebug(...args) {
    try {
      const { format } = require("node:util");
      __wjs2_stderr_write(`${format(...args)}\n`);
    } catch {
      try { __wjs2_stderr_write(`${args.join(" ")}\n`); } catch {}
    }
  },
  // sourcemaps 开关（bootstrap/node.js 口径：布尔门；存根只收不兑现）。
  setSourceMapsEnabled(val) {
    if (typeof val !== "boolean") {
      throw new (require("internal/errors").codes.ERR_INVALID_ARG_TYPE)("enabled", "boolean", val);
    }
  },
  // 存活引用（per_thread.js 口径：Symbol.for('nodejs.ref/unref') 优先，
  // 回落 .ref/.unref；null/undefined 即返）。
  ref(m) {
    if (m === null || m === undefined) return;
    const fn = m[Symbol.for("nodejs.ref")] || m.ref;
    if (typeof fn === "function") Reflect.apply(fn, m, []);
  },
  unref(m) {
    if (m === null || m === undefined) return;
    const fn = m[Symbol.for("nodejs.unref")] || m.unref;
    if (typeof fn === "function") Reflect.apply(fn, m, []);
  },
  // stdout/stderr 富流（真 node 是 Socket；10f 起 helper 造形：直写 fd +
  // EE 全表面——pipe 的 dest.on/emit('pipe')/close/finish 登记接得住；
  // 事件面空转（无 data/end 发射）偏差记档）。clearLine/cursorTo/getColorDepth
  // 非 TTY no-op（vite dev；TTY 下调用方自写 ANSI）。stdin：监听登记 +
  // isTTY + EOF read()（偏差记档：stdin EOF/data 不投递、信号不投递——
  // 注册表只收不发，SIGTERM 默认行为不变（OS 默认终止））。
  stdout: __wjs2_stdio_stream(1),
  stderr: __wjs2_stdio_stream(2),
  stdin: {
    get isTTY() { return __wjs2_stdio_istty(0); },
    __wjs2_listeners: {},
    __wjs2_enc: null,
    __wjs2_polling: false,
    __wjs2_ended: false,
    on(type, cb) {
      if (typeof cb !== "function") throw new TypeError("stdin.on: listener must be a function");
      (this.__wjs2_listeners[String(type)] ??= []).push(cb);
      if (type === "data" || type === "readable" || type === "end") this.__wjs2_startPoll();
      return this;
    },
    once(type, cb) { return this.on(type, cb); },
    off(type, cb) {
      const list = this.__wjs2_listeners[String(type)];
      if (list) {
        const i = list.indexOf(cb);
        if (i >= 0) list.splice(i, 1);
      }
      return this;
    },
    removeListener(type, cb) { return this.off(type, cb); },
    setEncoding(e) { this.__wjs2_enc = (e === null || e === undefined) ? null : String(e); return this; },
    __wjs2_emitStdin(type, arg) {
      const list = (this.__wjs2_listeners[String(type)] || []).slice();
      for (const l of list) { try { l(arg); } catch {} }
      return list.length;
    },
    // stdin 轮询投递（kill 套件：子进程读父写 stdin；echo x | winterjs2 真机口径）：
    // 首个 data/readable/end 监听即起 10ms refed 轮询（续命到 EOF），EOF 清环
    // 发 end；TTY 归 REPL，不管；Buffer 块（setEncoding 即转串）。
    __wjs2_startPoll() {
      if (this.__wjs2_polling || this.__wjs2_ended) return;
      if (__wjs2_stdio_istty(0)) return;
      this.__wjs2_polling = true;
      const self = this;
      const timer = setInterval(() => {        let r;
        try { r = __wjs2_stdin_poll(); } catch { r = "E"; }
        if (r === "E") {
          clearInterval(timer);
          self.__wjs2_polling = false;
          self.__wjs2_ended = true;
          self.__wjs2_emitStdin("end");
          // node 口径：stdin EOF 后发 'close'（chunk-problem 的 shasum 形靠它；
          // 异步一轮——end 监听内挂 close 仍可达）。
          queueMicrotask(() => self.__wjs2_emitStdin("close"));
          return;
        }
        if (r !== "") {
          const bin = atob(r.slice(1));
          const u8 = new Uint8Array(bin.length);
          for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
          const chunk = self.__wjs2_enc !== null ? Buffer.from(u8).toString(self.__wjs2_enc) : Buffer.from(u8);
          self.__wjs2_emitStdin("data", chunk);
        }
      }, 10);
      this.__wjs2_timer = timer;
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
      this.__wjs2_ended = true;
      try { if (this.__wjs2_timer) clearInterval(this.__wjs2_timer); } catch {}
      this.__wjs2_polling = false;
      this.__wjs2_emitStdin("close");
      return this;
    },
  },
  getuid() { return __wjs2_process_getuid(); },
  getgid() { return __wjs2_process_getgid(); },
  geteuid() { return __wjs2_process_geteuid(); },
  getegid() { return __wjs2_process_getegid(); },
  getgroups() { return __wjs2_process_getgroups(); },
  // POSIX 身份设置（node 口径 wrapPosixCredentialSetters；unix-only native，
  // Windows 面记档——本仓 Windows 构建本就不含 process_.rs 身份系）。
  setuid(id) { __wjs2_credIdSetter("User", "setuid", (v) => __wjs2_setuid(v), id); },
  setgid(id) { __wjs2_credIdSetter("Group", "setgid", (v) => __wjs2_setgid(v), id); },
  seteuid(id) { __wjs2_credIdSetter("User", "seteuid", (v) => __wjs2_seteuid(v), id); },
  setegid(id) { __wjs2_credIdSetter("Group", "setegid", (v) => __wjs2_setegid(v), id); },
  setgroups(groups) {
    const E = require("internal/errors").codes;
    if (!Array.isArray(groups)) throw new E.ERR_INVALID_ARG_TYPE("groups", "Array", groups);
    for (let i = 0; i < groups.length; i++) __wjs2_validateId(groups[i], `groups[${i}]`);
    const r = __wjs2_setgroups(JSON.stringify(groups));
    if (r > 0) __wjs2_unknownCredential("Group", groups[r - 1]);
    if (r !== 0) __wjs2_credSysErr(-r, "setgroups", groups.join(","));
  },
  initgroups(user, extraGroup) {
    __wjs2_validateId(user, "user");
    __wjs2_validateId(extraGroup, "extraGroup");
    const r = __wjs2_initgroups(JSON.stringify(user), JSON.stringify(extraGroup));
    if (r === 1) __wjs2_unknownCredential("User", user);
    if (r === 2) __wjs2_unknownCredential("Group", extraGroup);
    if (r !== 0) __wjs2_credSysErr(-r, "initgroups", user);
  },
  nextTick(cb, ...args) {
    if (typeof cb !== "function") throw new (require("internal/errors").codes.ERR_INVALID_ARG_TYPE)("callback", "Function", cb);
    // 原生队列（node 口径）：tick 由 pump 在 RunJobs 前后收割——同步期入队的
    // tick 先于微任务、微任务期入队的等整轮微任务排空（V8 checkpoint 原子性）。
    // 回调抛错经 drain 侧 uncaughtException 路由（destroy/emitErrorNT 等内建
    // 全走 nextTick，throw 落成 rejection 即全族套件反红）。
    __wjs2_next_tick(cb, args);
  },
  // 通用监听表（warning 沿旧径；signal/stdin 等只登记不投递——偏差记档，
  // SIGTERM 默认行为不变）。emit 供未来事件循环接信号投递。
  // 方法一律走 `this`（套件 process-tampering：node common 载入期捕获
  // `const process = globalThis.process`，之后全局被换也不经它读表）。
  __wjs2_listeners: {},
  on(type, cb) {
    if (typeof cb !== "function") throw new TypeError("process.on: listener must be a function");
    (this.__wjs2_listeners[String(type)] ??= []).push(cb);
    return this;
  },
  once(type, cb) {
    if (typeof cb !== "function") throw new TypeError("process.once: listener must be a function");
    const self = this;
    const wrapped = (...args) => { self.off(type, wrapped); cb(...args); };
    wrapped.__wjs2_orig = cb;
    return self.on(type, wrapped);
  },
  off(type, cb) {
    const list = this.__wjs2_listeners[String(type)];
    if (list) {
      let i = list.findIndex((l) => l === cb || l.__wjs2_orig === cb);
      while (i >= 0) { list.splice(i, 1); i = list.findIndex((l) => l === cb || l.__wjs2_orig === cb); }
    }
    return this;
  },
  removeListener(type, cb) { return this.off(type, cb); },
  // node process 即 EventEmitter（套件 promises-scheduler：process.addListener/
  // process.emit 直用）；emit 返回是否命中监听（node 口径）。
  addListener(type, cb) { return this.on(type, cb); },
  // node 口径（EventEmitter.emit）：监听抛错原样上抛；无监听的 'error' 即抛。
  // 宿主内部派发走 `__wjs2_emit`（吞错，结算点不被用户监听打断）。
  emit(type, ...args) {
    const list = [...(this.__wjs2_listeners[String(type)] ?? [])];
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
    if (type === undefined) this.__wjs2_listeners = {};
    else delete this.__wjs2_listeners[String(type)];
    return this;
  },
  listenerCount(type) { return (this.__wjs2_listeners[String(type)] ?? []).length; },
  // 未捕获异常捕获回调（node 口径 execution.js：null 清除；重复设置即抛；
  // 接住后 uncaughtException 不发、进程不 fatal，见 bootstrap __wjs2_uncaught）。
  __wjs2_captureCb: null,
  hasUncaughtExceptionCaptureCallback() { return typeof this.__wjs2_captureCb === "function"; },
  setUncaughtExceptionCaptureCallback(fn) {
    const E = require("internal/errors").codes;
    if (fn === null) {
      this.__wjs2_captureCb = null;
      return;
    }
    if (typeof fn !== "function") throw new E.ERR_INVALID_ARG_TYPE("fn", ["Function", "null"], fn);
    if (typeof this.__wjs2_captureCb === "function") {
      throw new E.ERR_UNCAUGHT_EXCEPTION_CAPTURE_ALREADY_SET();
    }
    this.__wjs2_captureCb = fn;
  },
  // EventEmitter 读表（M5 vitest 牵引：init 链 `process.listeners(..).bind(..)`）。
  listeners(type) { return [...(this.__wjs2_listeners[String(type)] ?? [])]; },
  rawListeners(type) { return this.listeners(type); },
  eventNames() { return Object.keys(this.__wjs2_listeners); },
  __wjs2_emit(type, ...args) {
    const list = [...(this.__wjs2_listeners[String(type)] ?? [])];
    for (const l of list) {
      try { l.call(this, ...args); } catch {}
    }
    return list.length;
  },
  // node lib/internal/process/warning.js 逐段移植：参数归一 → string 包 Error（栈截到 ctor）
  // → Deprecation 受 noDeprecation/throwDeprecation 门控 → nextTick 派发 'warning'。
  // 缺省打印是登记在表内的普通监听（`__wjs2_onWarning`，--no-warnings 不登记），可被 off 摘除。
  emitWarning(warning, type, code, ctor) {
    let detail;
    if (type !== null && typeof type === "object" && !Array.isArray(type)) {
      ctor = type.ctor;
      code = type.code;
      if (typeof type.detail === "string") detail = type.detail;
      type = type.type || "Warning";
    } else if (typeof type === "function") {
      ctor = type;
      code = undefined;
      type = "Warning";
    }
    const invalid = (name, exp, v) => new (require("internal/errors").codes.ERR_INVALID_ARG_TYPE)(name, exp, v);
    if (type !== undefined && typeof type !== "string") throw invalid("type", "string", type);
    if (typeof code === "function") {
      ctor = code;
      code = undefined;
    } else if (code !== undefined && typeof code !== "string") {
      throw invalid("code", "string", code);
    }
    if (typeof warning === "string") {
      warning = new Error(warning);
      warning.name = String(type || "Warning");
      if (code !== undefined) warning.code = code;
      if (detail !== undefined) warning.detail = detail;
      if (typeof Error.captureStackTrace === "function") Error.captureStackTrace(warning, ctor || this.emitWarning);
    } else if (!(warning instanceof Error)) {
      throw invalid("warning", ["Error", "string"], warning);
    }
    if (warning.name === "DeprecationWarning") {
      if (this.noDeprecation) return;
      if (this.throwDeprecation) throw warning;
    }
    this.nextTick(() => this.emit("warning", warning));
  },
  __wjs2_onWarning(warning) {
    if (!(warning instanceof Error)) return;
    const p = globalThis.process;
    const isDeprecation = warning.name === "DeprecationWarning";
    if (isDeprecation && p.noDeprecation) return;
    const trace = p.traceProcessWarnings || (isDeprecation && p.traceDeprecation);
    let msg = `(node:${__wjs2_pid()}) `;
    if (warning.code) msg += `[${warning.code}] `;
    if (trace && warning.stack) msg += `${warning.stack}`;
    else msg += typeof warning.toString === "function" ? `${warning.toString()}` : Error.prototype.toString.call(warning);
    if (typeof warning.detail === "string") msg += `\n${warning.detail}`;
    if (!trace && !p.__wjs2_traceHelperShown) {
      const flag = isDeprecation ? "--trace-deprecation" : "--trace-warnings";
      const argv0 = String(p.argv0 || "node").split(/[\\/]/).pop().replace(/\.exe$/, "");
      msg += `\n(Use \`${argv0} ${flag} ...\` to show where the warning was created)`;
      p.__wjs2_traceHelperShown = true;
    }
    const file = p.__wjs2_warningFile;
    if (file) {
      try { require("node:fs").appendFileSync(file, `${msg}\n`); return; } catch {}
    }
    __wjs2_stderr_write(`${msg}\n`);
  },
};
// 真机口径：process[Symbol.toStringTag] = "process"（不可枚举，实测 getter 面），
// String(process) → '[object process]'（vm basic 套件 / util.inspect 点名）。
Object.defineProperty(globalThis.process, Symbol.toStringTag, { value: "process" });
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
// Node 兼容旗语义（CLI 起点剥下，见 cli::strip_node_compat_args）：
// --expose-gc 即暴露 globalThis.gc（async no-op——真收集另案，调用形状先行；
// 无旗不暴露，真机口径）；名单挂内部位供 http 默认宽松等消费（不进 process.env）。
try {
  const __compat = JSON.parse(__wjs2_node_compat_json());
  globalThis.__wjs2_nodeCompat = Array.isArray(__compat) ? __compat : [];
  // gc 门控（--expose-gc/--expose_gc 双拼写， deterioration 套件用下划线形）。
  const __hasGc = globalThis.__wjs2_nodeCompat.includes("--expose-gc") ||
    globalThis.__wjs2_nodeCompat.includes("--expose_gc");
  if (__hasGc && typeof globalThis.gc !== "function") {
    globalThis.gc = async function gc() { return undefined; };
  }
} catch { globalThis.__wjs2_nodeCompat = []; }
// 标题（node 口径：--title=v 末个赢；缺省回 execPath 基名；set 透写同 store）。
try {
  const __tf = globalThis.__wjs2_nodeCompat.filter((a) => a.startsWith("--title="));
  globalThis.__wjs2_processTitle = __tf.length ? __tf[__tf.length - 1].slice("--title=".length) : null;
} catch { globalThis.__wjs2_processTitle = null; }
// 告警旗（node 口径：旗在才定义属性）+ 缺省打印监听（--no-warnings / NODE_NO_WARNINGS=1 不登记）。
{
  const __f = globalThis.__wjs2_nodeCompat;
  const __p = globalThis.process;
  if (__f.includes("--no-deprecation")) __p.noDeprecation = true;
  if (__f.includes("--throw-deprecation")) __p.throwDeprecation = true;
  if (__f.includes("--trace-deprecation")) __p.traceDeprecation = true;
  if (__f.includes("--trace-warnings")) __p.traceProcessWarnings = true;
  const __rw = __f.find((a) => a.startsWith("--redirect-warnings="));
  if (__rw) __p.__wjs2_warningFile = __rw.slice("--redirect-warnings=".length);
  let __nw = false;
  try { __nw = __wjs2_env_get("NODE_NO_WARNINGS") === "1"; } catch {}
  if (!__f.includes("--no-warnings") && !__nw) __p.on("warning", __p.__wjs2_onWarning);
}
// Node 口径：NODE_DEBUG 置位即启动期警告一次（首 section 名；debug.js 套件
// 逐字断言。stderr 直写，不走 warning 通道）。
try {
  const __nd = __wjs2_env_get("NODE_DEBUG");
  if (__nd !== undefined && __nd !== null && String(__nd).trim() !== "") {
    const __sec = String(__nd).split(",")[0].trim();
    __wjs2_stderr_write(`Setting the NODE_DEBUG environment variable to '${__sec}' can expose sensitive data (such as passwords, tokens and authentication headers) in the resulting log.\n`);
  }
} catch { /* 环境不可读即跳过 */ }
// R9：config 冻结 + exitCode 不可删除（真机 bootstrap 口径：config 经 reviver
// 深冻；exitCode configurable:false，严格模式 delete 即抛）。
try {
  const __pc = globalThis.process.config;
  if (__pc && typeof __pc === "object") {
    try { if (__pc.target_defaults) Object.freeze(__pc.target_defaults); } catch {}
    try { if (__pc.variables) Object.freeze(__pc.variables); } catch {}
    Object.freeze(__pc);
  }
} catch {}
try {
  const __d = Object.getOwnPropertyDescriptor(globalThis.process, "exitCode");
  if (__d) Object.defineProperty(globalThis.process, "exitCode", { ...__d, configurable: false });
} catch {}
// R9：SM 删除不可配置属性的文案与 V8 不同（`property "exitCode" is ...` vs
// 真机 `Cannot delete property 'exitCode' of #<process>`，validation 套件点名
// 正则）。Proxy 只拦 deleteProperty，其余默认透传（get/set 经 target，
// this 为 proxy 时读写 __wjs2_* 表经转发一致；PROTO_FIXUP 的 setPrototypeOf
// 亦默认透传）。
try {
  const __target = globalThis.process;
  globalThis.process = new Proxy(__target, {
    deleteProperty(t, p) {
      if (p === "exitCode") {
        throw new TypeError("Cannot delete property 'exitCode' of #<process>");
      }
      return Reflect.deleteProperty(t, p);
    },
  });
} catch {}
"#;

/// EventEmitter 原型链修正（test-process-prototype 口径；须在 `require` 可用后执行，
/// 由 `node_prelude()` 拼在 REQUIRE_PRELUDE 之后——prelude 主体求值时 require 尚无。
/// 真机形态：proto ≠ EE.prototype 本身但链上含之；constructor 为 proto 自有不可枚举槽）。
pub const PROCESS_PROTO_FIXUP: &str = r#"
{
  const EE = require("node:events").EventEmitter;
  const processProto = Object.create(EE.prototype);
  const ProcessCtor = function Process() {};
  ProcessCtor.prototype = processProto;
  Object.defineProperty(processProto, "constructor", { value: ProcessCtor, writable: true, enumerable: false, configurable: true });
  Object.setPrototypeOf(globalThis.process, processProto);
}
"#;
