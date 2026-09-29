// cwd 归一（真机 normalizeSpawnArguments/getPathFromURL 口径，实测 probe）：
// 字符串直通；file: URL 取 pathname（host 必须 localhost/空——darwin 平台文案）；
// 非 file: 抛 ERR_INVALID_URL_SCHEME "The URL must be of scheme file"；
// 非串非 URL 抛 ARG_TYPE。
// worker env 快照会话：子进程缺省继承 worker 的 env 视图（含 worker 内写入；
// 真机 spawnSync 的 env 缺省即 worker 的 process.env——process-env 套件口径）。
// 主会话回 null（Rust 侧继承真进程 env，原语义）。
function __childEnv(o) {
  // node 口径：env 缺省继承（worker 快照/真进程）；for-in 含原型键（env 套件
  // FOO 经原型）；undefined 值跳过；余下 String() 化（null → "null"，否则
  // Rust 侧 JSON 解析报 "must be JSON"）；键值 \0 校验（reject-null-bytes 套件）。
  if (o.env === undefined || o.env === null) {
    if (__wjs2_worker_env_snapshot() !== undefined) o.env = { ...process.env };
    else return o.env;
  }
  const out = {};
  for (const k in o.env) {
    const v = o.env[k];
    if (v === undefined) continue;
    __nullCheck(k, `options.env['${k}']`);
    __nullCheck(typeof v === "string" ? v : String(v), `options.env['${k}']`);
    out[k] = typeof v === "string" ? v : String(v);
  }
  o.env = out;
  return o.env;
}
function __cwdPath(v) {
  __nullCheck(v, "options.cwd", "must be a string, Uint8Array, or URL without null bytes");
  if (typeof v === "string") return v;
  if (v !== null && typeof v === "object" && typeof v.href === "string") {
    const u = (typeof URL === "function" && v instanceof URL) ? v : new URL(v.href);
    if (u.protocol !== "file:") {
      const e = new Error("The URL must be of scheme file");
      e.code = "ERR_INVALID_URL_SCHEME";
      throw e;
    }
    if (u.hostname !== "" && u.hostname !== "localhost") {
      const e = new Error(`File URL host must be "localhost" or empty on ${process.platform}`);
      e.code = "ERR_INVALID_FILE_URL_HOST";
      throw e;
    }
    return decodeURIComponent(u.pathname);
  }
  throw new ERR_INVALID_ARG_TYPE("options.cwd", ["string", "URL"], v);
}

// spawn 预检（execvp 语义：失败走异步 error + close(-errno)，不抛、pid undefined）。
// 返回 null = 可起；否则返回待发 error（code/errno/syscall/path）。
// 相对路径按新 cwd 解析（node chdir 先于 exec）；裸名沿 PATH（子 env 优先）。
function __spawnPreflight(file, o) {
  const mk = (code, errno) => {
    const e = new Error(`spawn ${file} ${code}`);
    e.code = code;
    e.errno = errno;
    e.syscall = `spawn ${file}`;
    e.path = file;
    return e;
  };
  let dir = o.cwd;
  if (dir !== null && dir !== undefined && dir !== "") {
    try {
      const st = fs.statSync(dir);
      if (!st.isDirectory()) return mk("ENOTDIR", __syncErrno("ENOTDIR"));
    } catch {
      return mk("ENOENT", __syncErrno("ENOENT"));
    }
  } else {
    dir = null;
  }
  const probe = (p) => {
    try { fs.accessSync(p, fs.constants.X_OK); return true; } catch (e2) {
      return e2 && (e2.code === "EACCES" || e2.code === "EPERM") ? "eacces" : false;
    }
  };
  if (file.includes("/")) {
    const p = (dir !== null && !file.startsWith("/")) ? dir + "/" + file : file;
    const hit = probe(p);
    if (hit === "eacces") return mk("EACCES", __syncErrno("EACCES"));
    if (!hit) return mk("ENOENT", __syncErrno("ENOENT"));
    return null;
  }
  const pathVar = String((o.env && o.env.PATH) || process.env.PATH || "").split(":");
  for (const d of pathVar) {
    if (!d) continue;
    if (probe(d + "/" + file) === true) return null;
  }
  return mk("ENOENT", __syncErrno("ENOENT"));
}

// shell 模式预检（只查 cwd——命令本体经 shell 解析，PATH/可执行位由 shell 管）。
function __spawnPreflightCwd(o) {
  const dir = o.cwd;
  if (dir !== null && dir !== undefined && dir !== "") {
    try {
      const st = fs.statSync(dir);
      if (!st.isDirectory()) return __preflightErr("ENOTDIR", dir);
    } catch {
      return __preflightErr("ENOENT", dir);
    }
  }
  return null;
}
function __preflightErr(code, file) {
  const e = new Error(`spawn ${file} ${code}`);
  e.code = code;
  e.errno = __syncErrno(code);
  e.syscall = `spawn ${file}`;
  e.path = file;
  return e;
}
function __normSpawnAsyncOpts(opts) {
  // node 口径：stdio 缺省（整体缺或数组缺项）一律 'pipe'——spawnPromisified
  // 系套件直接读 child.stdout/stderr（'child.stderr is null' 现形于
  // test-url-parse-deprecation）；旧默认 inherit 是偏差。
  const o = { cwd: null, env: null, detached: false, stdio: ["pipe", "pipe", "pipe"], timeoutMs: 0, signal: null, killSigname: "SIGTERM", killSigno: 15, shell: undefined };
  if (opts === undefined || opts === null) { o.env = __childEnv(o); return o; }
  if (opts.cwd !== undefined && opts.cwd !== null) {
    o.cwd = __cwdPath(opts.cwd);
    // node：'' 不 chdir（cwd 套件 'number' pid 断言），Rust 侧空串即失败——置 null。
    if (o.cwd === "") o.cwd = null;
  }
  if (opts.env !== undefined) o.env = { ...opts.env };
  if (opts.detached !== undefined) o.detached = !!opts.detached;
  // uid/gid：validateInt32 逐字口径（真机 26.8.2 实测：非 number 即 ARG_TYPE，
  // 非整数/超 int32 即 RANGE；范围内负数过校验，spawn 期 EPERM 见下）。
  for (const k of ["uid", "gid"]) {
    const v = opts[k];
    if (v === undefined || v === null) continue;
    if (typeof v !== "number") throw new ERR_INVALID_ARG_TYPE(`options.${k}`, "number", v);
    if (!Number.isInteger(v)) {
      const e = new RangeError(`The value of "options.${k}" is out of range. It must be an integer. Received ${v}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    if (v < -2147483648 || v > 2147483647) {
      const e = new RangeError(`The value of "options.${k}" is out of range. It must be >= -2147483648 && <= 2147483647. Received ${v}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
  }
  // uid/gid：真机 _handle.spawn 同步 EPERM 口径——非特权指定他 id 即抛
  //（uid-gid 套件非 root 形；message 正则匹配）。值本身记档忽略。
  if (opts.uid !== undefined && opts.uid !== null && typeof opts.uid === "number") {
    if (typeof process.getuid === "function" && opts.uid !== process.getuid()) {
      const e = new Error("spawn EPERM");
      e.code = "EPERM"; e.errno = -1; e.syscall = "spawn"; throw e;
    }
  }
  if (opts.gid !== undefined && opts.gid !== null && typeof opts.gid === "number") {
    if (typeof process.getgroups === "function" && !process.getgroups().includes(opts.gid)) {
      const e = new Error("spawn EPERM");
      e.code = "EPERM"; e.errno = -1; e.syscall = "spawn"; throw e;
    }
  }
  // shell：boolean/string（node normalizeSpawnArguments 口径；spawn-shell 套件）。
  if (opts.shell !== undefined && opts.shell !== null) {
    if (typeof opts.shell !== "boolean" && typeof opts.shell !== "string") {
      throw new ERR_INVALID_ARG_TYPE("options.shell", ["boolean", "string"], opts.shell);
    }
    __nullCheck(opts.shell, "options.shell");
    o.shell = opts.shell;
  }
  // argv0：undefined/null 过；余下须字符串（真机 validateString）+ \0 校验；
  // 值由 Rust 侧经 unix arg0 落地（spawn-argv0 套件），此处只验不存。
  if (opts.argv0 !== undefined && opts.argv0 !== null) {
    if (typeof opts.argv0 !== "string") {
      throw new ERR_INVALID_ARG_TYPE("options.argv0", "string", opts.argv0);
    }
    __nullCheck(opts.argv0, "options.argv0");
  }
  // timeout：validateTimeout 逐字口径——非 number ARG_TYPE，负数/非整数 RANGE
  //（spawn-timeout-kill-signal 套件 'badValue'/{} 点名）。
  if (opts.timeout !== undefined && opts.timeout !== null) {
    if (typeof opts.timeout !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.timeout", "number", opts.timeout);
    }
    if (!Number.isInteger(opts.timeout) || opts.timeout < 0) {
      const e = new RangeError(`The value of "options.timeout" is out of range. It must be an integer >= 0. Received ${opts.timeout}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    o.timeoutMs = opts.timeout;
  }
  // signal：undefined 过；余下必须 AbortSignal 实例（真机 validateAbortSignal）。
  if (opts.signal !== undefined) {
    if (!(opts.signal instanceof AbortSignal)) {
      throw new ERR_INVALID_ARG_TYPE("options.signal", "AbortSignal", opts.signal);
    }
    o.signal = opts.signal;
  }
  // killSignal：undefined/null/合法信号过；类型先行 ARG_TYPE，落空 UNKNOWN_SIGNAL。
  if (opts.killSignal !== undefined && opts.killSignal !== null) {
    const ks = opts.killSignal;
    if (typeof ks !== "string" && typeof ks !== "number") {
      throw new ERR_INVALID_ARG_TYPE("options.killSignal", ["string", "number"], ks);
    }
    const hit = __sigResolve(ks);
    if (!hit) {
      const e = new TypeError(`Unknown signal: ${String(ks)}`);
      e.code = "ERR_UNKNOWN_SIGNAL"; throw e;
    }
    o.killSigno = hit.signo; o.killSigname = hit.name;
  }
  if (opts.stdio !== undefined) {
    const one = (s) => {
      if (!["inherit", "ignore", "pipe"].includes(s)) {
        throw new Error(`NotSupportedError: spawn stdio '${s}' (inherit/ignore/pipe)`);
      }
      return s;
    };
    // node 口径：裸 'ipc' 非法（ARG_VALUE）；数组双 ipc 即 ERR_IPC_ONE_PIPE。
    if (typeof opts.stdio === "string") {
      if (opts.stdio === "ipc") throw new ERR_INVALID_ARG_VALUE("stdio", opts.stdio);
      o.stdio = [one(opts.stdio), one(opts.stdio), one(opts.stdio)];
    }
    else if (Array.isArray(opts.stdio)) {
      // 三元数组（缺省补 pipe；Node 的复杂组合如 fd 重定向不在此列，文档记录）。
      // 四元 + [3] 'ipc' 即通道占位忽略（parser-lazy-loaded 套件只等 exit，
      // 真 IPC 另案）；余下超长仍拒。
      if (opts.stdio.length > 3) {
        if (opts.stdio.filter((s) => s === "ipc").length > 1) throw new ERR_IPC_ONE_PIPE();
        if (opts.stdio.length > 4 || opts.stdio[3] !== "ipc") {
          throw new Error("NotSupportedError: spawn stdio array takes at most 3 entries");
        }
      }
      // 流对象元（pipe-dataflow/merge/reuse 套件）：可读/可写流即转交位
      // （Rust 侧仍按 pipe 建真管，转交纯 JS 搭桥，见 __stdioWire）。
      o.stdio = [0, 1, 2].map((i) => {
        const s = opts.stdio[i];
        if (s === undefined) return "pipe";
        if (typeof s === "string") return one(s);
        if (s !== null && typeof s === "object" &&
            (typeof s.on === "function" || typeof s.write === "function")) {
          (o.stdioStreams ??= [])[i] = s;
          return "pipe";
        }
        throw new Error(`NotSupportedError: spawn stdio '${s}' (inherit/ignore/pipe)`);
      });
    } else {
      throw new Error("NotSupportedError: spawn stdio must be a string or array");
    }
  }
  if (opts.timeout !== undefined) o.timeoutMs = Number(opts.timeout);
  o.env = __childEnv(o);
  return o;
}
export function spawn(file, args, opts) {
  // normalizeSpawnArguments 逐字口径（真机 26.8.2 实测）：
  // file 必填字符串（空即 ARG_VALUE）；args 数组/缺席，余下非对象即 ARG_TYPE，
  // 纯对象回落为 options；options 缺席即 {}，显式 null/非对象即 ARG_TYPE。
  if (typeof file !== "string") throw new ERR_INVALID_ARG_TYPE("file", "string", file);
  if (file.length === 0) throw new ERR_INVALID_ARG_VALUE("file", file, "cannot be empty");
  if (Array.isArray(args)) { args = [...args]; }
  else if (args === undefined || args === null) { args = []; }
  else if (typeof args !== "object") { throw new ERR_INVALID_ARG_TYPE("args", "object", args); }
  else { opts = args; args = []; }
  if (opts === undefined) opts = {};
  else if (typeof opts !== "object" || opts === null || Array.isArray(opts)) { throw new ERR_INVALID_ARG_TYPE("options", "object", opts); }
  const o = __normSpawnAsyncOpts(opts);
  return __spawnInto(new ChildProcess(), file, args, o);
}
// stdio 流转交（pipe-dataflow/merge/reuse 套件）：spawn 数组元为流对象时
// 按位搭桥——stdin 位可读流 data/end 转入子 stdin；stdout/stderr 位可写流
// 接子对应流 data（end 不转：共享写端由持有者关，merge 套件多写者语义）。
function __stdioWire(proc, streams) {
  if (!streams) return;
  try {
    const src = streams[0];
    if (src && typeof src.on === "function" && proc.stdin) {
      src.on("data", (d) => { try { proc.stdin.write(d); } catch {} });
      src.on("end", () => { try { proc.stdin.end(); } catch {} });
    }
    const out = streams[1];
    if (out && typeof out.write === "function" && proc.stdout) {
      proc.stdout.on("data", (d) => { try { out.write(d); } catch {} });
    }
    const err = streams[2];
    if (err && typeof err.write === "function" && proc.stderr) {
      proc.stderr.on("data", (d) => { try { err.write(d); } catch {} });
    }
  } catch {}
}
// spawn 落地（函数与 ChildProcess.prototype.spawn 方法共用；proc 既是
// native 事件 target 也是返回对象——事件接线必须挂最终对象，禁中转搬运）。
let __dep0190Emitted = false;
function __spawnInto(proc, file, args, o) {
  // \0 校验（reject-null-bytes 套件；函数与方法共道）。
  __nullCheck(String(file), "file");
  for (let i = 0; i < (args || []).length; i++) __nullCheck(String(args[i]), `args[${i}]`);
  proc.spawnfile = String(file);
  proc.spawnargs = [...(args || [])].map(String);
  // shell（node normalizeSpawnArguments 口径）：file+args 空格拼接成 sh -c 串，
  // spawnfile 换 shell 本体、spawnargs 换 shell 形（spawn-shell 套件断
  // spawnargs 末元 = 拼串）；args 非空即 DEP0190（node 逐字文案）。
  let f2 = String(file);
  let a2 = [...(args || [])].map(String);
  if (o.shell !== undefined) {
    // node 口径：全进程只告警一次（emittedDEP0190Already）。
    if (a2.length > 0 && o.shell && !__dep0190Emitted) {
      __dep0190Emitted = true;
      process.emitWarning("Passing args to a child process with shell option true can lead to security vulnerabilities, as the arguments are not escaped, only concatenated.", "DeprecationWarning", "DEP0190");
    }
    const command = [f2, ...a2].join(" ");
    f2 = typeof o.shell === "string" ? o.shell : "/bin/sh";
    a2 = ["-c", __selfCmd(command, o.env ?? process.env)];
    proc.spawnfile = f2;
    proc.spawnargs = a2;
  }
  // execvp 预检（shell 时跳过——命令经 shell 解析；cwd 照查）：失败走异步
  // error + close(-errno)（cwd 套件：pid 'undefined'、error.code='ENOENT'、
  // exit 不发、close code -2——真机 probe）。
  const pf = o.shell === undefined ? __spawnPreflight(String(file), o) : __spawnPreflightCwd(o);
  if (pf !== null) {
    // spawn-error 套件：err.spawnargs 为参数数组（与 spawnargs 同值）。
    pf.spawnargs = [...proc.spawnargs];
    proc.__initStreams(o.stdio, 0);
    queueMicrotask(() => {
      proc.__emitError(pf);
      proc.__emitDeadClose(pf.errno);
    });
    return proc;
  }
  const [f3, a3] = __selfArgv(f2, a2);
  const id = __wjs2_spawn_start(String(f3), JSON.stringify(a3), JSON.stringify({
    cwd: o.cwd, env: o.env, detached: o.detached, timeout_ms: o.timeoutMs,
    kill_signo: o.killSigno, kill_signame: o.killSigname,
  }), proc, JSON.stringify(o.stdio));
  // abort（node spawn 逐字口径：预中止经 nextTick 走同一 onAbortListener——
  // 子进程已起，kill(killSignal) 作用到才发 error(AbortError, cause=reason)；
  // exit(null, killSignal) 由真实进程死亡自带；exit 后注销监听——
  // spawn-timeout-kill-signal 套件 listenerCount 断言）。
  if (o.signal) {
    const onAbort = () => {
      if (proc.exitCode !== null || proc.signalCode !== null) return;
      if (__wjs2_child_kill(id, String(o.killSigno))) proc.__emitAbort(o.signal.reason);
    };
    const disposable = addAbortListener(o.signal, onAbort);
    proc.__onExited = () => { try { disposable[Symbol.dispose](); } catch {} };
  }
  proc.__init(id, o.stdio);
  // stdio 流转交（数组流对象元，转交位搭桥）。
  __stdioWire(proc, o.stdioStreams);
  // 'spawn' 事件 nextTick/microtask 发射（监听挂载在 spawn() 返回后同步发生，
  // 恒早于数据/退出分发）。
  queueMicrotask(() => { try { proc.__emitSpawn(); } catch {} });
  return proc;
}
function __asyncOneShot(kind, run) {
  // run(): 同步 core 调用（抛转回调 err）；无 live 句柄（记档偏差）
  return (...args) => {
    const cb = args.findLast((a) => typeof a === "function");
    const rest = args.filter((a) => typeof a !== "function");
    if (typeof cb !== "function") {
      const err = new TypeError(`${kind} requires a callback for async form`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    queueMicrotask(() => {
      try {
        const [outErr, stdout, stderr] = run(...rest);
        cb(outErr, stdout, stderr);
      } catch (e) {
        cb(e);
      }
    });
    return undefined;
  };
}
// exec 族（10f 重写，node 架构：execFile = spawn + 收集 + close 回调 + 返回
// live ChildProcess；exec = execFile('/bin/sh', ['-c', cmd])（normalizeExecArgs
// 口径）。旧 __asyncOneShot 一次性内核退役——"无 live 句柄"偏差消账。
// maxBuffer 超限 kill（真机 ERR_CHILD_PROCESS_STDIO_MAXBUFFER）；timeout 走
// spawn 既有 timeout_ms（killSignal 自定值偏差记档）；错误 cmd/code/killed/
// signal 照真机挂载。execFile 无回调即抛 ERR_INVALID_ARG_TYPE（真机口径）。
function __execCollect(child, o, cmdStr, args, cb) {
  // node execFile v26 逐字口径（exithandler/errorhandler/kill/data 三面）：
  // encoding 非 buffer 且 isEncoding 才串化（否则 Buffer concat，'invalid' 同）；
  // maxBuffer 超限截断入块再 kill（RangeError ERR_CHILD_PROCESS_STDIO_MAXBUFFER
  // 优先于通用失败错）；Infinity 即不限；killed = child.killed || 本地 killed。
  const encoding = (o.encoding !== "buffer" && Buffer.isEncoding(o.encoding)) ? o.encoding : null;
  const _stdout = [];
  const _stderr = [];
  let stdoutLen = 0;
  let stderrLen = 0;
  let killed = false;
  let exited = false;
  let ex = null;

  function exithandler(code, signal) {
    if (exited) return;
    exited = true;
    if (!cb) return;
    let stdout;
    let stderr;
    if (encoding || child.stdout?.readableEncoding) stdout = _stdout.join("");
    else stdout = Buffer.concat(_stdout);
    if (encoding || child.stderr?.readableEncoding) stderr = _stderr.join("");
    else stderr = Buffer.concat(_stderr);
    if (!ex && code === 0 && signal === null) {
      cb(null, stdout, stderr);
      return;
    }
    let cmd = cmdStr;
    if (args?.length) cmd += ` ${args.join(" ")}`;
    ex ||= __genericNodeError(`Command failed: ${cmd}\n${stderr}`, {
      code: (typeof code === "number" && code < 0) ? __uvName(code) : code,
      killed: child.killed || killed,
      signal: signal,
    });
    ex.cmd = cmdStr;
    cb(ex, stdout, stderr);
  }
  function errorhandler(e) {
    ex = e;
    if (child.stdout) child.stdout.destroy();
    if (child.stderr) child.stderr.destroy();
    exithandler();
  }
  function kill() {
    if (child.stdout) child.stdout.destroy();
    if (child.stderr) child.stderr.destroy();
    killed = true;
    try {
      child.kill(o.killSignal);
    } catch (e) {
      ex = e;
      exithandler();
    }
  }
  if (child.stdout) {
    if (encoding) child.stdout.setEncoding(encoding);
    child.stdout.on("data", (chunk) => {
      if (o.maxBuffer === Infinity) { _stdout.push(chunk); return; }
      const len = typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.length;
      stdoutLen += len;
      if (stdoutLen > o.maxBuffer) {
        const truncatedLen = o.maxBuffer - (stdoutLen - len);
        _stdout.push(typeof chunk === "string" ? chunk.slice(0, truncatedLen) : chunk.slice(0, truncatedLen));
        ex = __maxBufferErr("stdout");
        kill();
      } else {
        _stdout.push(chunk);
      }
    });
  }
  if (child.stderr) {
    if (encoding) child.stderr.setEncoding(encoding);
    child.stderr.on("data", (chunk) => {
      if (o.maxBuffer === Infinity) { _stderr.push(chunk); return; }
      const len = typeof chunk === "string" ? Buffer.byteLength(chunk, encoding) : chunk.length;
      stderrLen += len;
      if (stderrLen > o.maxBuffer) {
        const truncatedLen = o.maxBuffer - (stderrLen - len);
        _stderr.push(typeof chunk === "string" ? chunk.slice(0, truncatedLen) : chunk.slice(0, truncatedLen));
        ex = __maxBufferErr("stderr");
        kill();
      } else {
        _stderr.push(chunk);
      }
    });
  }
  child.once("error", errorhandler);
  child.once("close", exithandler);
}
// node genericNodeError（execFile 失败错；killed/signal/code 挂载）。
function __genericNodeError(message, opts) {
  const err = new Error(message);
  const { code, killed, signal } = opts;
  err.code = code ?? null;
  err.killed = !!killed;
  err.signal = signal ?? null;
  return err;
}
function __maxBufferErr(stream) {
  const err = new RangeError(`${stream} maxBuffer length exceeded`);
  err.code = "ERR_CHILD_PROCESS_STDIO_MAXBUFFER";
  return err;
}

// spawn 预检（execFile 专用）：绝对/相对路径直查；裸名沿 PATH 找（node
// spawn 的 PATH 解析在 fork 失败即 ENOENT，pid 不发号——本仓 id 先发，故补查）。
function __canSpawnFile(file) {
  if (file.includes("/")) {
    try { fs.accessSync(file); return true; } catch { return false; }
  }
  const path = String(globalThis.process.env.PATH || "").split(":");
  for (const d of path) {
    if (!d) continue;
    try { fs.accessSync(d + "/" + file); return true; } catch { /* next */ }
  }
  return false;
}

export function execFile(file, args, opts, cb) {
  // normalizeExecFileArgs 逐字口径（真机 26.8.2 实测）：args 数组拷贝/函数即
  // 回调/纯对象回落 options/余下（字符串等）原位留待 spawn 位校验；
  // options 函数即回调/显式 null 即 {}/数组与非对象即 ARG_TYPE；
  // callback 给了但非函数即 ARG_TYPE（含 Received 段）。
  if (Array.isArray(args)) { args = [...args]; }
  else if (args !== undefined && args !== null && typeof args === "object") { cb = opts; opts = args; args = null; }
  else if (typeof args === "function") { cb = args; opts = null; args = null; }
  if (args === undefined || args === null) args = [];
  else if (!Array.isArray(args)) { throw new ERR_INVALID_ARG_TYPE("args", "object", args); }
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  else if (opts !== undefined && opts !== null) {
    if (typeof opts !== "object" || Array.isArray(opts)) { throw new ERR_INVALID_ARG_TYPE("options", "object", opts); }
  }
  if (opts === undefined || opts === null) opts = {};
  if (cb !== undefined && cb !== null && typeof cb !== "function") {
    throw new ERR_INVALID_ARG_TYPE("callback", "function", cb);
  }
  const o = __normExecOpts(opts);
  __nullCheck(String(file), "file");
  for (let i = 0; i < (args ?? []).length; i++) __nullCheck(String(args[i]), `args[${i}]`);
  // shell 透传（execFile 缺省 false——__normExecOpts 的 true 缺省是 execSync/exec
  // 语义，此处按 opts 原样）。
  const shellOpt = (opts !== null && typeof opts === "object" && opts.shell !== undefined) ? opts.shell : undefined;
  const argv = (args ?? []).map(String);
  const cmdStr = String(file);
  // execvp 预检（shell 模式跳过——命令经 shell 解析）：失败走异步 error + 回调
  //（不抛；node ENOENT 时 pid 恒 undefined——死句柄 + 微任务回调）。
  if (shellOpt === undefined && !__canSpawnFile(String(file))) {
    const err = new Error(`spawn ${file} ENOENT`);
    err.code = "ENOENT";
    err.errno = -2;
    err.syscall = `spawn ${file}`;
    err.path = String(file);
    err.cmd = cmdStr;
    queueMicrotask(() => cb && cb(err, "", ""));
    return new ChildProcess();
  }
  const child = spawn(String(file), argv, {
    cwd: o.cwd,
    env: o.env,
    signal: o.signal,
    shell: shellOpt,
    stdio: ["pipe", "pipe", "pipe"],
  });
  child.__cmdStr = cmdStr;
  __execCollect(child, o, cmdStr, argv, cb);
  // timeout/killSignal 在 execFile 层（node 口径——spawn 调用不带 timeout）。
  if (o.timeoutMs > 0 && Number.isFinite(o.timeoutMs)) {
    const tid = setTimeout(() => {
      try { child.kill(o.killSignal); } catch { /* 已退即走下 */ }
    }, o.timeoutMs);
    if (typeof tid.unref === "function") tid.unref();
  }
  return child;
}

export function exec(command, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = undefined; }
  // node 口径：callback 可缺席（返回 live child）；给了但非函数才 ARG_TYPE。
  if (cb !== undefined && cb !== null && typeof cb !== "function") {
    const err = new TypeError("The \"callback\" argument must be of type function");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const o = __normExecOpts(opts);
  __nullCheck(String(command), "command");
  const cmdStr = String(command);
  // 自举翻译（exec-encoding/timeout 系：escapePOSIXShell 的 ${ESCAPED_n} env
  // 间接形 + 裸文件形；err.cmd 保持原文——改写只作用于 /bin/sh -c 的串）。
  const child = execFile(cmdStr, undefined, { ...o, timeout: o.timeoutMs, encoding: o.encoding }, cb && ((err, stdout, stderr) => {
    if (err) err.cmd = cmdStr;
    cb(err, stdout, stderr);
  }));
  return child;
}
// node customPromiseExecFunction 逐字口径：promise.child 挂原函数返回值；
// 原函数在 executor **外**调用——同步校验抛错不被 Promise 吞（abortcontroller
// 套件 assert.throws 直调 promisify(exec) 形）。
function __customPromiseExec(orig) {
  return function (...args) {
    let res;
    let rej;
    const promise = new Promise((resolve, reject) => { res = resolve; rej = reject; });
    promise.child = orig(...args, (err, stdout, stderr) => {
      if (err !== null && err !== undefined) {
        err.stdout = stdout;
        err.stderr = stderr;
        rej(err);
      } else {
        res({ stdout, stderr });
      }
    });
    return promise;
  };
}
Object.defineProperty(exec, Symbol.for("nodejs.util.promisify.custom"), {
  value: __customPromiseExec(exec),
  enumerable: false,
});
Object.defineProperty(execFile, Symbol.for("nodejs.util.promisify.custom"), {
  value: __customPromiseExec(execFile),
  enumerable: false,
});
export function execFileSync(file, args, opts) {
  if (args !== undefined && args !== null && !Array.isArray(args)) { opts = args; args = []; }
  const o = __normSpawnOpts(opts);
  __nullCheck(String(file), "file");
  for (let i = 0; i < (args || []).length; i++) __nullCheck(String(args[i]), `args[${i}]`);
  const f = String(file);
  const [f2, a2] = __selfArgv(f, [...(args || [])].map(String));
  const r = JSON.parse(__wjs2_cp_spawn(f2, JSON.stringify(a2), JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: false, kill_signo: o.killSigno, kill_signame: o.killSigname,
    argv0: o.argv0, detached: !!o.detached,
    stdin_inherit: !!o.stdioInherit[0], stdout_inherit: !!o.stdioInherit[1],
    stderr_inherit: !!o.stdioInherit[2],
    input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  if (r.spawnErr || r.timedOut || r.status !== 0) __spawnError(file, r, o.encoding);
  return __toOut(r.stdout_b64, o.encoding);
}
// fork 子会话入口（worker eval 串；占位 `__FORK_MOD__`/`__FORK_ARGV__` 由
// `fork()` 经 replacer 函数填 JSON——`format!` 拼 JS 禁花括号转义，见 §4.44）。
// 子端 IPC 面：process.send/disconnect/on('message')/connected/channel，
// 经 parentPort 与父端 ChildProcess 桥接；控制信封 `{__wjs2_fork_ctl:
// // "disconnect"}` 单键载荷不投递给用户（见父端 `disconnect()`）。
const __FORK_CHILD_SRC = `
import { parentPort } from "node:worker_threads";
const __mod = __FORK_MOD__;
const __forkArgs = __FORK_ARGV__;
process.argv = [process.execPath, __mod, ...__forkArgs];
process.connected = true;
process.channel = { ref() {}, unref() {}, hasRef() { return true; } };
process.send = (message, ...rest) => {
  // 校验内联（eval 会话无模块作用域；与 __validateSend* 同口径，code/name 逐字。
  // 注意：本块在外层模板字符串内，禁用模板字面量与插值写法，一律字符串拼接）。
  const __received = (v) => v === null ? "null" : (typeof v === "string" ? ("'" + v + "'") : String(v));
  let handle, options, cb = null;
  const a = [...rest];
  if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
  else {
    handle = a.shift();
    if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
    else {
      options = a.shift();
      if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
    }
  }
  if (options !== undefined && (typeof options !== "object" || options === null)) {
    const e = new TypeError('The "options" argument must be of type object. Received ' + __received(options));
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  if (!process.connected || parentPort === null) {
    const err = new Error("Channel closed");
    err.code = "ERR_IPC_CHANNEL_CLOSED";
    if (cb) queueMicrotask(() => cb(err));
    else queueMicrotask(() => process.__wjs2_emit("error", err));
    return false;
  }
  if (message === undefined) {
    const e = new TypeError('The "message" argument must be specified');
    e.code = "ERR_MISSING_ARGS"; throw e;
  }
  if (typeof message !== "string" && typeof message !== "object" &&
      typeof message !== "number" && typeof message !== "boolean") {
    const e = new TypeError('The "message" argument must be one of type string, object, number, or boolean.');
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  if (handle !== undefined && handle !== null) {
    const e = new TypeError("This handle type cannot be sent");
    e.code = "ERR_INVALID_HANDLE_TYPE"; throw e;
  }
  try {
    parentPort.postMessage(message);
  } catch (e) {
    const err = e instanceof Error ? e : new Error(String(e));
    if (!err.code) err.code = "ERR_IPC_CHANNEL_CLOSED";
    if (cb) queueMicrotask(() => cb(err));
    else queueMicrotask(() => process.__wjs2_emit("error", err));
    return false;
  }
  if (cb) queueMicrotask(() => cb(null));
  return true;
};
process.disconnect = () => {
  if (!process.connected) return;
  process.connected = false;
  try { parentPort.close(); } catch {}
  process.__wjs2_emit("disconnect");
};
parentPort.on("message", (message) => {
  if (message !== null && typeof message === "object" && !Array.isArray(message) &&
      Object.keys(message).length === 1 && message.__wjs2_fork_ctl === "disconnect") {
    if (process.connected) {
      process.connected = false;
      try { parentPort.close(); } catch {}
      process.__wjs2_emit("disconnect");
    }
    return;
  }
  // NODE_ 前缀分流（子进程侧与父端 __onForkMessage 同口径：cluster
  // NODE_CLUSTER 信封走 internalMessage；listen-twice 套件点名）。
  if (message !== null && typeof message === "object" && !Array.isArray(message) &&
      typeof message.cmd === "string" && message.cmd.indexOf("NODE_") === 0) {
    process.__wjs2_emit("internalMessage", message);
    return;
  }
  process.__wjs2_emit("message", message);
});
parentPort.on("close", () => {
  if (process.connected) {
    process.connected = false;
    process.__wjs2_emit("disconnect");
  }
});
// 内部监听不续命子会话（worker 空转即退，真机口径：无用户监听的子进程
// 脚本结束即退，迟发消息即 ERR_IPC_CHANNEL_CLOSED；有用户监听才续命）：
// newListener 已置 listening 位，此处复位；process 系手写表（无 newListener
// 事件），故直包 message 订阅入口（once/addListener 走 on，removeListener
// 走 off）；投递走 listenerCount 门控，不受 counting 位影响。
// 注意：本块在外层模板字符串内，禁用模板字面量与插值写法。
try { __wjs2_port_unlisten(parentPort.__id); } catch {}
const __ppId = parentPort.__id;
const __procListen = () => { try { __wjs2_port_listen(__ppId); } catch {} };
const __procUnlisten = () => {
  if (process.listenerCount("message") === 0) { try { __wjs2_port_unlisten(__ppId); } catch {} }
};
const __procOn = process.on;
process.on = function (type, cb) {
  if (type === "message" && typeof cb === "function") __procListen();
  return __procOn.call(this, type, cb);
};
process.addListener = process.on;
const __procOff = process.off;
process.off = function (type, cb) {
  const r = __procOff.call(this, type, cb);
  if (type === "message") __procUnlisten();
  return r;
};
process.removeListener = process.off;
const __procRemoveAll = process.removeAllListeners;
process.removeAllListeners = function (type) {
  const r = __procRemoveAll.call(this, type);
  if (type === undefined || type === "message") __procUnlisten();
  return r;
};
await import(__mod);
`;
function __normForkOpts(opts) {
  const o = { execPath: process.execPath, killSignal: "SIGTERM", silent: false, signal: null, killSigname: "SIGTERM", killSigno: 15 };
  if (opts === undefined || opts === null) return o;
  if (typeof opts !== "object") {
    const err = new TypeError("The \"options\" argument must be of type object");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // 线程底座：cwd/env/execArgv/silent/stdio/serialization/timeout/detached 等
  // 接受忽略（同进程线程，无独立进程环境；stdio 恒 null，见 `fork` 文档）；
  // \0 照验（reject-null-bytes 套件）。
  if (opts.killSignal !== undefined) o.killSignal = String(opts.killSignal);
  if (opts.cwd !== undefined && opts.cwd !== null) {
    if (typeof opts.cwd !== "string") throw new ERR_INVALID_ARG_TYPE("options.cwd", "string", opts.cwd);
    __nullCheck(opts.cwd, "options.cwd", "must be a string, Uint8Array, or URL without null bytes");
  }
  if (opts.argv0 !== undefined && opts.argv0 !== null) {
    if (typeof opts.argv0 !== "string") throw new ERR_INVALID_ARG_TYPE("options.argv0", "string", opts.argv0);
    __nullCheck(opts.argv0, "options.argv0");
  }
  if (opts.execPath !== undefined) { __nullCheck(String(opts.execPath), "options.execPath"); o.execPath = String(opts.execPath); }
  if (opts.execArgv !== undefined) {
    if (!Array.isArray(opts.execArgv)) throw new ERR_INVALID_ARG_TYPE("options.execArgv", "Array", opts.execArgv);
    opts.execArgv.forEach((a, i) => __nullCheck(String(a), `options.execArgv[${i}]`));
  }
  // env：透传 worker 快照（fork 炸弹案：旧"值忽略"致自定义 env 丢失，
  // 子复走父分支指数 fork；真机 env 缺省即 process.env 拷贝）。
  // \0 照验（reject-null-bytes 套件）。
  if (opts.env !== undefined && opts.env !== null) {
    for (const k in opts.env) {
      const v = opts.env[k];
      if (v === undefined) continue;
      __nullCheck(k, `options.env['${k}']`);
      __nullCheck(typeof v === "string" ? v : String(v), `options.env['${k}']`);
    }
    o.env = { ...opts.env };
  }
  if (opts.silent !== undefined) o.silent = !!opts.silent;
  if (opts.signal !== undefined) {
    if (!(opts.signal instanceof AbortSignal)) {
      throw new ERR_INVALID_ARG_TYPE("options.signal", "AbortSignal", opts.signal);
    }
    o.signal = opts.signal;
  }
  const hit = __sigResolve(o.killSignal);
  if (hit) { o.killSigname = hit.name; o.killSigno = hit.signo; }
  return o;
}
export function fork(modulePath, args, opts) {
  if (modulePath === undefined || modulePath === null ||
      (typeof modulePath !== "string" && !(modulePath instanceof URL))) {
    const err = new TypeError("The \"modulePath\" argument must be of type string or URL");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // args/options 归一（真机 fork 口径：缺席即 []/{}；纯对象回落 options；
  // 余下非数组即 ARG_TYPE/Array；options 数组与非对象即 ARG_TYPE）。
  if (args === undefined || args === null) { args = []; }
  else if (typeof args === "object" && !Array.isArray(args)) { opts = args; args = []; }
  else if (!Array.isArray(args)) { throw new ERR_INVALID_ARG_TYPE("args", "Array", args); }
  if (opts === undefined || opts === null) opts = {};
  else if (typeof opts !== "object" || Array.isArray(opts)) { throw new ERR_INVALID_ARG_TYPE("options", "object", opts); }
  if (modulePath instanceof URL && modulePath.protocol !== "file:") {
    const err = new TypeError("The \"modulePath\" argument must be a file URL");
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  __nullCheck(String(modulePath instanceof URL ? modulePath.href : modulePath), "modulePath", "must be a string, Uint8Array, or URL without null bytes");
  for (let i = 0; i < (args || []).length; i++) __nullCheck(String(args[i]), `args[${i}]`);
  const o = __normForkOpts(opts);
  const modStr = modulePath instanceof URL ? modulePath.href : String(modulePath);
  const fileUrl = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(modStr) ? modStr : pathToFileURL(modStr).href;
  const argsArr = [...(args || [])].map(String);
  const proc = new ChildProcess();
  proc.__forkChild = true;
  proc.__killSignal = o.killSignal;
  proc.spawnfile = o.execPath;
  proc.spawnargs = [o.execPath, fileUrl, ...argsArr];
  // 预中止：线程不起（真机不起子进程同理），异步 error + exit/close(null, killSignal)
  //（fork-abort-signal 套件 exit mustCall(null, 'SIGTERM'/'SIGKILL')）。
  if (o.signal && o.signal.aborted) {
    const signame = o.killSigname;
    const reason = o.signal.reason;
    queueMicrotask(() => {
      proc.__emitAbort(reason);
      if (typeof proc.onexit === "function") proc.onexit(null, signame);
      if (typeof proc.onclose === "function") proc.onclose(null, signame);
    });
    return proc;
  }
  // silent（或 stdio: 'pipe'）：stdout/stderr 挂无数据管形流——pipe/unpipe 形状
  // 在（silent 套件 `child.stderr.pipe(process.stderr, {end:false})` 必须可用）；
  // 数据面偏差记档：线程底座子输出直走共享 stdio。
  if (o.silent) {
    proc.stdout = __forkNullStream();
    proc.stderr = __forkNullStream();
  }
  const src = __FORK_CHILD_SRC
    .replace("__FORK_MOD__", () => JSON.stringify(fileUrl))
    .replace("__FORK_ARGV__", () => JSON.stringify(argsArr));
  const worker = new Worker(src, { eval: true, __wjs2_forkChild: true, env: o.env });
  proc.__worker = worker;
  proc.__connected = true;
  // 非 silent（stdio 继承）：stdout/stderr 恒 null（真机 26 逐项：fork 未 silent
  // 时 `c.stdout === null` 三面全真——继承无管道句柄；silent 才挂管形流）。
  proc.stdin = null;
  if (!o.silent) {
    proc.stdout = null;
    proc.stderr = null;
  }
  proc.channel = { ref() {}, unref() {} };
  worker.on("message", (m) => proc.__onForkMessage(m));
  worker.on("error", (e) => {
    if (typeof proc.onerror === "function") proc.onerror(e);
  });
  worker.on("exit", (code) => proc.__onForkExit(code));
  // abort（node：error 先行 + terminate；exit(null, killSignal) 由 abort 路径
  // 自发——线程 exit 事件被 __exitDone 旗拦下）。
  if (o.signal) {
    o.signal.addEventListener("abort", () => {
      if (proc.__exitDone) return;
      proc.__exitDone = true;
      try { worker.terminate(); } catch { /* 已退即走下 */ }
      proc.__emitAbort(o.signal.reason);
      if (typeof proc.onexit === "function") proc.onexit(null, o.killSigname);
      if (typeof proc.onclose === "function") proc.onclose(null, o.killSigname);
    }, { once: true });
  }
  return proc;
}
// fork silent 管形流（无数据——见 `fork` 文档偏差；pipe/unpipe/on 形状在）。
function __forkNullStream() {
  const listeners = {};
  return {
    on(ev, cb) { (listeners[ev] ||= []).push(cb); return this; },
    once(ev, cb) { const w = (...a) => { this.off(ev, w); cb(...a); }; w.__wjs2_orig = cb; return this.on(ev, w); },
    off(ev, cb) {
      const l = listeners[ev];
      if (l) {
        let i = l.findIndex((f) => f === cb || f.__wjs2_orig === cb);
        while (i >= 0) { l.splice(i, 1); i = l.findIndex((f) => f === cb || f.__wjs2_orig === cb); }
      }
      return this;
    },
    removeListener(ev, cb) { return this.off(ev, cb); },
    pipe(dest) { return dest; },
    unpipe() { return this; },
    setEncoding() { return this; },
    pause() { return this; },
    resume() { return this; },
    read() { return null; },
    destroy() { return this; },
    get readable() { return false; },
    get destroyed() { return false; },
  };
}
export default { execSync, spawnSync, spawn, exec, execFile, execFileSync, fork, ChildProcess };
