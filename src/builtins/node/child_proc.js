import { Worker } from "node:worker_threads";
import { pathToFileURL } from "node:url";
import * as fs from "node:fs";
import __osDefault from "node:os";
import { getSystemErrorName as __uvName } from "node:util";
import errors from 'node:internal/errors';
import { addAbortListener } from 'node:internal/events/abort_listener';
const {
  codes: {
    ERR_INVALID_ARG_TYPE: { HideStackFramesError: ERR_INVALID_ARG_TYPE },
    ERR_INVALID_ARG_VALUE: { HideStackFramesError: ERR_INVALID_ARG_VALUE },
    ERR_IPC_ONE_PIPE,
    ERR_INVALID_HANDLE_TYPE,
    ERR_MISSING_ARGS,
  },
} = errors;
const __SIGS = __osDefault.constants.signals;
function __b64dec(s) {
  s = String(s).replace(/-/g, "+").replace(/_/g, "/");
  while (s.length % 4) s += "=";
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
function __b64enc(u8) {
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
  return btoa(s);
}
function __normExecOpts(opts) {
  // node execFile 口径（v26 源码对拍）：{encoding:'utf8', ...opts} 展开合并——
  // opts 带 encoding 键（即便 undefined）即覆盖为 undefined → Buffer（实测
  // probe：{encoding:undefined} 走 Buffer，{} 走 utf8 串）；键缺席 → utf8 串。
  const o = { encoding: "utf8", timeoutMs: 0, shell: true, maxBuffer: 1024 * 1024, inputB64: null, killSignal: "SIGTERM", killSigno: 15 };
  if (opts === undefined || opts === null) { o.env = __childEnv(o); return o; }
  if (typeof opts === "string") { o.encoding = opts; return o; }
  if (opts !== null && typeof opts === "object" && "encoding" in opts) o.encoding = opts.encoding;
  if (opts.timeout !== undefined) o.timeoutMs = Number(opts.timeout);
  if (opts.shell !== undefined) { __nullCheck(opts.shell, "options.shell"); o.shell = opts.shell; }
  if (opts.maxBuffer !== undefined) o.maxBuffer = Number(opts.maxBuffer);
  if (opts.cwd !== undefined) { __nullCheck(String(opts.cwd), "options.cwd", "must be a string, Uint8Array, or URL without null bytes"); o.cwd = String(opts.cwd); }
  // argv0：校验后忽略（异步 exec 族 argv0 落地另案；reject-null-bytes 套件只断抛错）。
  if (opts.argv0 !== undefined && opts.argv0 !== null) {
    if (typeof opts.argv0 !== "string") throw new ERR_INVALID_ARG_TYPE("options.argv0", "string", opts.argv0);
    __nullCheck(opts.argv0, "options.argv0");
  }
  if (opts.env !== undefined) o.env = { ...opts.env };
  // killSignal：undefined/null/合法信号过；类型先行 ARG_TYPE，落空 UNKNOWN_SIGNAL
  //（sanitizeKillSignal 口径；exec timeout-kill 套件）。
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
    o.killSignal = hit.name; o.killSigno = hit.signo;
  }
  // signal：undefined 过；余下必须 AbortSignal 实例（真机 validateAbortSignal 口径，
  // null 也抛——abortcontroller 系套件 {}/函数形 ARG_TYPE）。
  if (opts.signal !== undefined) {
    if (!(opts.signal instanceof AbortSignal)) {
      throw new ERR_INVALID_ARG_TYPE("options.signal", "AbortSignal", opts.signal);
    }
    o.signal = opts.signal;
  }
  if (opts.input !== undefined && opts.input !== null) {
    const b = opts.input;
    if (typeof b !== "string" && !(b instanceof Uint8Array) && !(b instanceof ArrayBuffer) && !ArrayBuffer.isView(b)) {
      throw new ERR_INVALID_ARG_TYPE("options.input", ["string", "Buffer", "TypedArray", "DataView", "ArrayBuffer"], b);
    }
    const u8 = typeof b === "string" ? new TextEncoder().encode(b)
      : (b instanceof Uint8Array ? b
        : (b instanceof ArrayBuffer ? new Uint8Array(b)
          : new Uint8Array(b.buffer, b.byteOffset, b.byteLength)));
    o.inputB64 = __b64enc(u8);
  }
  o.env = __childEnv(o);
  return o;
}
function __normSpawnOpts(opts) {
  // 缺省 encoding "buffer"（真机口径；exec 系另为 utf8，不串）。
  const o = { encoding: "buffer", timeoutMs: 0, shell: false, shellPath: null, maxBuffer: 1024 * 1024, inputB64: null, killSigno: 15, killSigname: "SIGTERM", argv0: null, cwd: null, detached: false, stdioInherit: [false, false, false] };
  if (opts === undefined || opts === null) { o.env = __childEnv(o); return o; }
  if (opts.encoding !== undefined) o.encoding = opts.encoding;
  // 字符串选项（cwd/argv0）：undefined/null 过，余下非串即 ARG_TYPE + \0 校验。
  for (const k of ["cwd", "argv0"]) {
    const v = opts[k];
    if (v === undefined || v === null) continue;
    if (typeof v !== "string") {
      throw new ERR_INVALID_ARG_TYPE(`options.${k}`, "string", v);
    }
    __nullCheck(v, `options.${k}`, k === "cwd" ? "must be a string, Uint8Array, or URL without null bytes" : undefined);
    o[k === "cwd" ? "cwd" : "argv0"] = v;
  }
  // 布尔选项（detached/windowsHide/windowsVerbatimArguments）：undefined/null/布尔过。
  // detached 真传 native；windows 系 unix 忽略（记档）。
  for (const k of ["detached", "windowsHide", "windowsVerbatimArguments"]) {
    const v = opts[k];
    if (v === undefined || v === null) continue;
    if (typeof v !== "boolean") {
      throw new ERR_INVALID_ARG_TYPE(`options.${k}`, "boolean", v);
    }
    if (k === "detached") o.detached = v;
  }
  // shell：undefined/null/布尔/字符串过（字符串即 shell 路径）+ \0 校验；
  // 余下 ARG_TYPE。
  if (opts.shell !== undefined && opts.shell !== null) {
    if (typeof opts.shell === "boolean") { o.shell = opts.shell; o.shellPath = null; }
    else if (typeof opts.shell === "string") { __nullCheck(opts.shell, "options.shell"); o.shell = true; o.shellPath = opts.shell; }
    else throw new ERR_INVALID_ARG_TYPE("options.shell", ["boolean", "string"], opts.shell);
  }
  // uid/gid：undefined/null/非负整数过（值忽略，记档）；非 number 即 ARG_TYPE，
  // 非整数/负数/NaN/Inf 即 RANGE。
  for (const k of ["uid", "gid"]) {
    const v = opts[k];
    if (v === undefined || v === null) continue;
    if (typeof v !== "number") throw new ERR_INVALID_ARG_TYPE(`options.${k}`, "number", v);
    if (!Number.isInteger(v) || v < 0) {
      const e = new RangeError(`The value of "options.${k}" is out of range. It must be a non-negative integer. Received ${v}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
  }
  // timeout：undefined/null/非负整数过；非 number 即 ARG_TYPE；
  // 负数/NaN/Inf/小数即 RANGE。
  if (opts.timeout !== undefined && opts.timeout !== null) {
    const v = opts.timeout;
    if (typeof v !== "number") throw new ERR_INVALID_ARG_TYPE("options.timeout", "number", v);
    if (!Number.isInteger(v) || v < 0) {
      const e = new RangeError(`The value of "options.timeout" is out of range. It must be a non-negative integer. Received ${v}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    o.timeoutMs = v;
  }
  // maxBuffer：undefined/null/非负数/Infinity/小数过（Infinity 即不限）；
  // 非 number 即 ARG_TYPE；NaN/负数/-Inf 即 RANGE。小数下取整
  //（整数长度下与原值等价，Rust 侧 u64 收敛）。
  if (opts.maxBuffer !== undefined && opts.maxBuffer !== null) {
    const v = opts.maxBuffer;
    if (typeof v !== "number") throw new ERR_INVALID_ARG_TYPE("options.maxBuffer", "number", v);
    if (Number.isNaN(v) || v < 0) {
      const e = new RangeError(`The value of "options.maxBuffer" is out of range. It must be a non-negative number. Received ${v}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    o.maxBuffer = Number.isFinite(v) ? Math.floor(v) : null;
  }
  // killSignal：undefined/null/合法信号（名大小写不敏感/表中数字）过；
  // 类型先行（布尔/数组/对象/函数即 ARG_TYPE，真机口径），
  // 再查表落空即 ERR_UNKNOWN_SIGNAL。
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
  if (opts.env !== undefined) o.env = { ...opts.env };
  // stdio：字符串整体形（inherit/ignore/pipe）或三元数组（逐流；超界忽略）；
  // 余下收敛 pipe（校验面另案）。同步族只用透传标记（inherit 即不捕获）。
  o.stdioInherit = [false, false, false];
  const __stdioOne = (v) => v === "inherit" ? "inherit" : "pipe";
  if (opts.stdio !== undefined && opts.stdio !== null) {
    if (typeof opts.stdio === "string") {
      const m = __stdioOne(opts.stdio);
      o.stdioInherit = [m === "inherit", m === "inherit", m === "inherit"];
    } else if (Array.isArray(opts.stdio)) {
      for (let i = 0; i < 3; i++) {
        if (i < opts.stdio.length) o.stdioInherit[i] = __stdioOne(opts.stdio[i]) === "inherit";
      }
    }
  }
  if (opts.input !== undefined && opts.input !== null) {
    const b = opts.input;
    if (typeof b !== "string" && !(b instanceof Uint8Array) && !(b instanceof ArrayBuffer) && !ArrayBuffer.isView(b)) {
      throw new ERR_INVALID_ARG_TYPE("options.input", ["string", "Buffer", "TypedArray", "DataView", "ArrayBuffer"], b);
    }
    const u8 = typeof b === "string" ? new TextEncoder().encode(b)
      : (b instanceof Uint8Array ? b
        : (b instanceof ArrayBuffer ? new Uint8Array(b)
          : new Uint8Array(b.buffer, b.byteOffset, b.byteLength)));
    o.inputB64 = __b64enc(u8);
  }
  o.env = __childEnv(o);
  return o;
}
// 信号归一（os.signals 表单源）：名大小写不敏感、数字须命中表值；
// 命中回 {name, signo}，余下 null（含布尔/数组/对象/函数）。
function __sigResolve(v) {
  if (typeof v === "string") {
    const up = v.toUpperCase();
    for (const k of Object.keys(__SIGS)) if (k === up) return { name: k, signo: __SIGS[k] };
    return null;
  }
  if (typeof v === "number" && Number.isInteger(v)) {
    for (const k of Object.keys(__SIGS)) if (__SIGS[k] === v) return { name: k, signo: v };
  }
  return null;
}
// \0 校验（node validateArgumentNullCheck 口径：仅字符串含 \0 才抛；
// reason 缺省 'must be a string without null bytes'，cwd/modulePath 系另传）。
function __nullCheck(s, name, reason) {
  if (typeof s === "string" && s.includes("\0")) {
    throw new ERR_INVALID_ARG_VALUE(name, s, reason ?? "must be a string without null bytes");
  }
}
// 自举翻译（input/timeout/maxbuf 套件：子进程即自身时，Node 形 argv
// （`-e`/`-p` 脚本/裸文件）映射到本仓全 flag CLI；他家二进制原样透传。
// `-e` extras 透传（本仓 --eval 尾参作脚本 argv，最佳 effort）。
function __selfArgv(file, args) {
  if (file !== process.execPath) return [file, args];
  const a = [...args];
  // 前导 node 运行时旗（`--pending-deprecation file`/`--experimental-x -e …`）原样
  // 透传，CLI 起点剥除 + 补 --run/-e→--eval（cli::strip_node_compat_args）。
  if (a[0] === "-e" || a[0] === "-p") return [file, ["--eval", ...a.slice(1)]];
  if (a[0] !== undefined && !String(a[0]).startsWith("-")) return [file, ["--run", ...a]];
  return [file, a];
}
// 同步族错误 errno 表（真机实测；余下 -4094）。
function __syncErrno(code) {
  return { ENOENT: -2, EACCES: -13, ENOBUFS: -55, ETIMEDOUT: -60 }[code] ?? -4094;
}
function __toOut(b64, encoding) {
  const bytes = __b64dec(b64 || "");
  // 真机口径：encoding 缺省（spawnSync）即 "buffer"，回真 Buffer
  // （deepStrictEqual 裸 Uint8Array 即不等，原型不同）。
  if (encoding === "buffer" || encoding === null || encoding === undefined) return Buffer.from(bytes);
  return new TextDecoder(String(encoding)).decode(bytes);
}
function __spawnError(cmd, r, encoding) {
  const msg = r.timedOut
    ? `Command failed: ${cmd}\nTimed out`
    : `Command failed: ${cmd}${r.status !== null ? ` (exit ${r.status})` : ""}`;
  const err = new Error(msg + (r.stderr_b64 ? "\n" + new TextDecoder().decode(__b64dec(r.stderr_b64)) : ""));
  err.status = r.status;
  err.signal = r.signal;
  err.stdout = __toOut(r.stdout_b64, encoding);
  err.stderr = __toOut(r.stderr_b64, encoding);
  if (r.spawnErr) {
    const code = (r.spawnErr.match(/^([A-Z_]+): /) || [])[1] || "UNKNOWN";
    err.code = code;
    err.errno = __syncErrno(code);
  }
  throw err;
}
// shell 串分段自举翻译（execsync-maxbuf / chunk-problem 管道形）：
// 顶层 `|` 切段（引号感知，`||` 不切），每段走 argv 翻译；非自举段原样。
// `$VAR`/`${VAR}`（含引号包裹形）按 env 解引用判定；解不出且名为 NODE 即
// 自举标记（测试 helper `$NODE` 口径）。段内 node 兼容旗（--expose-* 等）
// 剥除后走 -e/-p/裸文件规则（CLI 入口同款，见 cli::strip_node_compat_args）。
const __WJS_ACTIONS = [
  "--run", "--eval", "--config", "--completions", "--man", "--add", "--install", "--publish",
  "--login", "--upgrade", "--init", "--repl", "--test", "--lint", "--fmt", "--serve",
];
function __splitShell(s, sep) {
  const segs = [];
  let cur = "", q = null;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (q !== null) {
      cur += c;
      if (c === q) q = null;
    } else if (c === '"' || c === "'") {
      q = c; cur += c;
    } else if (sep === "|" && c === "|" && s[i + 1] === "|") {
      cur += "||"; i++;
    } else if (sep === "|" && c === "|") {
      segs.push(cur); cur = "";
    } else {
      cur += c;
    }
  }
  segs.push(cur);
  return segs;
}
function __splitArgs(s) {
  const out = [];
  let cur = "", q = null;
  const flush = () => { if (cur !== "") { out.push(cur); cur = ""; } };
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (q !== null) {
      cur += c;
      if (c === q) { q = null; flush(); }
    } else if (c === '"' || c === "'") {
      q = c; cur += c;
    } else if (c === " " || c === "\t") {
      flush();
    } else {
      cur += c;
    }
  }
  flush();
  return out;
}
// 解引用一层引号（可多层）+ `$VAR`/`${VAR}` 查 env；`$NODE` 无值即自举标记。
function __derefTok(t, env) {
  let u = t;
  for (;;) {
    if (u.length >= 2 && ((u.startsWith('"') && u.endsWith('"')) || (u.startsWith("'") && u.endsWith("'")))) {
      u = u.slice(1, -1);
      continue;
    }
    break;
  }
  if (u.startsWith("$")) {
    const m = /^\$\{([A-Za-z_][A-Za-z0-9_]*)\}$/.exec(u) || /^\$([A-Za-z_][A-Za-z0-9_]*)$/.exec(u);
    if (m) {
      if (env && Object.prototype.hasOwnProperty.call(env, m[1])) return String(env[m[1]]);
      if (m[1] === "NODE") return "$NODE";
      return u;
    }
  }
  return u;
}
function __selfSeg(seg, env) {
  const toks = __splitArgs(seg);
  if (toks.length === 0) return seg;
  const binRes = __derefTok(toks[0], env);
  const selfBin = binRes === process.execPath || binRes === "$NODE";
  if (!selfBin) return seg;
  // node 运行时旗（`--flag` / `--flag=value` 整 token）原样保留在前，交 CLI
  // 剥除并记录（cli::strip_node_compat_args——getOptionValue/execArgv 读回）；
  // 本仓动作旗在场即已是 winterjs 形，不动。
  let rest = toks.slice(1);
  const flags = [];
  while (rest.length > 0 && /^--[a-z]/.test(rest[0])) {
    if (__WJS_ACTIONS.includes(rest[0].split("=")[0])) return seg;
    flags.push(rest[0]);
    // 取值形空格分隔值随旗走（名单同 cli::COMPAT_VALUE_FLAGS）。
    if (rest[0] === "--max-http-header-size" && rest.length > 1) { flags.push(rest[1]); rest = rest.slice(1); }
    rest = rest.slice(1);
  }
  if (rest.length === 0) return seg;
  const head = [toks[0], ...flags].join(" ");
  const fm = /^-([A-Za-z]+)$/.exec(rest[0]);
  if (fm && /^[pe]+$/.test(fm[1])) return `${head} --eval ${rest.slice(1).join(" ")}`;
  if (!rest[0].startsWith("-")) return `${head} --run ${rest.join(" ")}`;
  return seg;
}
function __selfCmd(cmd, env) {
  const s = String(cmd);
  // 旧单段正则已由分段翻译替代（管道形 chunk-problem 根因）；空串原样。
  if (s === "") return cmd;
  return __splitShell(s, "|").map((seg) => __selfSeg(seg, env)).join(" | ");
}
// send 参数校验（node target.send/_send 口径，父子双侧共用；send-type-error
// 套件逐项：options 非对象即 ARG_TYPE；message 缺席 MISSING_ARGS、非
// string/object/number/boolean 即 ARG_TYPE；非空句柄无 fd 移交即
// INVALID_HANDLE_TYPE）。
function __validateSendOptions(options) {
  if (options !== undefined && (typeof options !== "object" || options === null)) {
    throw new ERR_INVALID_ARG_TYPE("options", "object", options);
  }
}
function __validateSendMessage(message, handle) {
  if (message === undefined) throw new ERR_MISSING_ARGS("message");
  if (typeof message !== "string" && typeof message !== "object" &&
      typeof message !== "number" && typeof message !== "boolean") {
    throw new ERR_INVALID_ARG_TYPE("message", ["string", "object", "number", "boolean"], message);
  }
  if (handle !== undefined && handle !== null) throw new ERR_INVALID_HANDLE_TYPE();
}
// node 口径（实测 probe）：abort 错 = Error 实例，name='AbortError'、
// code='ABORT_ERR'、cause=signal.reason（原生 abort() 为 DOMException AbortError）。
function __abortError(reason) {
  const err = new Error("This operation was aborted");
  err.name = "AbortError";
  err.code = "ABORT_ERR";
  err.cause = reason;
  return err;
}
export function execSync(cmd, opts) {
  const o = __normExecOpts(opts);
  __nullCheck(String(cmd), "command");
  // execSync 缺省 Buffer（真机实测；exec 异步缺省 utf8）：未显式给编码即改 buffer。
  if (typeof opts !== "string" && (opts === undefined || opts === null || opts.encoding === undefined)) o.encoding = "buffer";
  cmd = __selfCmd(String(cmd), o.env ?? process.env);
  const r = JSON.parse(__wjs_cp_exec(cmd, JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: !!o.shell, input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  if (r.spawnErr || r.timedOut || r.status !== 0) __spawnError(cmd, r, o.encoding);
  return __toOut(r.stdout_b64, o.encoding);
}
export function spawnSync(file, args, opts) {
  if (args !== undefined && args !== null && !Array.isArray(args)) { opts = args; args = []; }
  const o = __normSpawnOpts(opts);
  __nullCheck(String(file), "file");
  for (let i = 0; i < (args || []).length; i++) __nullCheck(String(args[i]), `args[${i}]`);
  const f = String(file);
  const origArgs = [...(args || [])].map(String);
  const [f2, a2] = __selfArgv(f, origArgs);
  const r = JSON.parse(__wjs_cp_spawn(f2, JSON.stringify(a2), JSON.stringify({
    cwd: o.cwd ?? null, env: o.env ?? null, timeout_ms: o.timeoutMs,
    shell: o.shell, shell_path: o.shellPath, kill_signo: o.killSigno,
    kill_signame: o.killSigname, argv0: o.argv0, detached: !!o.detached,
    stdin_inherit: !!o.stdioInherit[0], stdout_inherit: !!o.stdioInherit[1],
    stderr_inherit: !!o.stdioInherit[2],
    input_b64: o.inputB64, max_buffer: o.maxBuffer,
  })));
  const stdout = __toOut(r.stdout_b64, o.encoding);
  const stderr = __toOut(r.stderr_b64, o.encoding);
  // 真机口径（实测）：成功 output=[null, stdout, stderr]，失败 output=null；
  // error 挂 code/errno/syscall/path（message 非枚举）.
  const out = {
    pid: r.pid,
    output: null,
    stdout, stderr,
    status: r.status,
    signal: r.timedOut ? o.killSigname : r.signal,
  };
  if (!r.spawnErr && !r.timedOut) out.output = [null, stdout, stderr];
  if (r.spawnErr && !r.timedOut) {
    const code = (r.spawnErr.match(/^([A-Z_]+): /) || [])[1] || "UNKNOWN";
    out.error = new Error(`spawnSync ${f} ${code}`);
    out.error.code = code;
    out.error.errno = __syncErrno(code);
    out.error.syscall = `spawnSync ${f}`;
    out.error.path = f;
    // 真机口径（spawnsync.js 点名）：spawnargs 为参数数组（不含 file 本体）。
    out.error.spawnargs = origArgs;
  }
  if (r.timedOut && !out.error) {
    out.error = new Error(`spawnSync ${f} ETIMEDOUT`);
    out.error.code = "ETIMEDOUT";
    out.error.errno = -60;
    out.error.syscall = `spawnSync ${f}`;
    out.error.path = f;
    out.error.spawnargs = origArgs;
  }
  return out;
}
// legacy Readable 面（node 真机口径）：Web ReadableStream 外壳，供
// spawnPromisified 系套件（setEncoding + on('data')）与 CLI 自省使用。
// data 缺省 Buffer（§4.83），setEncoding 后为串；'data' 挂载即流动泵。
function __legacyReadable(web) {
  const listeners = {};
  let flowing = false;
  let paused = false;
  let ended = false;
  let destroyed = false;
  let enc = null;
  let reader = null;
  // paused 读缓冲（flush-stdio 套件：on('readable') + read() 循环；flowing 期
  // read() 恒 null，数据走 'data'）。
  let buf = [];
  let pumping = false;
  let pipes = null;
  const emit = (ev, ...args) => {
    for (const l of [...(listeners[ev] || [])]) {
      try { l(...args); } catch {}
    }
  };
  async function pump() {
    if (reader === null) reader = web.getReader();
    if (pumping) return;
    pumping = true;
    try {
      for (;;) {
        while (buf.length > 0 && flowing && !paused && !destroyed) emit("data", buf.shift());
        if (destroyed) return;
        let r;
        try { r = await reader.read(); } catch (e) { emit("error", e); return; }
        if (r.done) {
          while (buf.length > 0 && flowing && !paused && !destroyed) emit("data", buf.shift());
          ended = true;
          emit("end");
          // close 递延一轮（真机先 end 后 close 异步序；迟挂 close 仍到。
          // destroy() 的同步 close 保持——用户主动销毁即时语义）。
          queueMicrotask(() => emit("close"));
          if (!destroyed && !flowing && buf.length > 0) emit("readable");
          return;
        }
        let chunk = Buffer.from(r.value);
        if (enc !== null) chunk = chunk.toString(enc);
        buf.push(chunk);
        if (flowing && !paused && !destroyed) {
          while (buf.length > 0 && flowing && !paused && !destroyed) emit("data", buf.shift());
        } else if (!destroyed) {
          emit("readable");
        }
      }
    } finally { pumping = false; }
  }
  const api = {
    on(ev, cb) {
      (listeners[ev] ||= []).push(cb);
      if (ev === "data") { flowing = true; pump(); }
      else if (ev === "readable" || ev === "end" || ev === "close") {
        // 'end'/'close' 单监听也要泵（kill 套件：只挂 end 即要 EOF；
        // 非 flowing，不改数据流向，只观测终止）。
        pump();
      }
      return api;
    },
    once(ev, cb) {
      const w = (...a) => { api.off(ev, w); cb(...a); };
      return api.on(ev, w);
    },
    off(ev, cb) {
      const l = listeners[ev];
      if (l) { const i = l.indexOf(cb); if (i !== -1) l.splice(i, 1); }
      return api;
    },
    removeListener(ev, cb) { return api.off(ev, cb); },
    removeAllListeners(ev) {
      if (ev !== undefined) delete listeners[ev];
      else for (const k of Object.keys(listeners)) delete listeners[k];
      return api;
    },
    setEncoding(e) { enc = e === null ? null : String(e); return api; },
    pause() { paused = true; return api; },
    resume() { paused = false; flowing = true; pump(); return api; },
    // 最小 pipe 面（stdio-inherit 套件 `child.stderr.pipe(process.stderr)`；
    // 数据经 write 透传，end 默认透传）。
    pipe(dest, opts) {
      const onData = (d) => { try { dest.write(d); } catch {} };
      const onEnd = () => { try { if (!opts || opts.end !== false) dest.end(); } catch {} };
      api.on("data", onData);
      api.on("end", onEnd);
      ((pipes ||= [])).push({ dest, onData, onEnd });
      return dest;
    },
    unpipe(dest) {
      if (!pipes) return api;
      const rest = [];
      for (const p of pipes) {
        if (dest !== undefined && p.dest !== dest) { rest.push(p); continue; }
        api.off("data", p.onData);
        api.off("end", p.onEnd);
      }
      pipes = rest;
      return api;
    },
    destroy() {
      if (destroyed) return api;
      destroyed = true;
      try { if (reader !== null) reader.cancel(); } catch {}
      emit("close");
      return api;
    },
    // paused 读（flowing 期恒 null；暂停期取缓冲，空即 null）。
    read() {
      pump();
      if (flowing && !paused) return null;
      if (buf.length > 0) return buf.shift();
      return null;
    },
    get destroyed() { return destroyed; },
    // execFile collect 的串化判定读此位（node 流同形；setEncoding 后即真）。
    get readableEncoding() { return enc; },
    // 兼容桩（pipe-dataflow 套件直改 `stdout._handle.readStart` 断言永不调用；
    // 本泵模型不经过 readStart，桩恒静默）。
    _handle: { readStart() {}, readStop() {} },
  };
  return api;
}

// legacy Writable 面（node stdin 真机口径：Socket 形——stdin 套件直调
// `cat.stdin.write('hello')`/`.end()` 并断 writable/readable 位；web
// WritableStream 无这些成员）。write 回调恒异步触发（§4.74）。
function __legacyWritable(id) {
  const listeners = {};
  let destroyed = false;
  let ended = false;
  let buffered = 0;
  const emit = (ev, ...args) => {
    for (const l of [...(listeners[ev] || [])]) {
      try { l(...args); } catch {}
    }
  };
  const api = {
    on(ev, cb) { (listeners[ev] ||= []).push(cb); return api; },
    once(ev, cb) {
      const w = (...a) => { api.off(ev, w); cb(...a); };
      w.__wjs_orig = cb;
      return api.on(ev, w);
    },
    off(ev, cb) {
      const l = listeners[ev];
      if (l) {
        let i = l.findIndex((f) => f === cb || f.__wjs_orig === cb);
        while (i >= 0) { l.splice(i, 1); i = l.findIndex((f) => f === cb || f.__wjs_orig === cb); }
      }
      return api;
    },
    removeListener(ev, cb) { return api.off(ev, cb); },
    removeAllListeners(ev) {
      if (ev !== undefined) delete listeners[ev];
      else for (const k of Object.keys(listeners)) delete listeners[k];
      return api;
    },
    write(chunk, cb) {
      if (destroyed || ended) {
        const err = Object.assign(new Error("write after end"), { code: "ERR_STREAM_WRITE_AFTER_END" });
        if (typeof cb === "function") queueMicrotask(() => cb(err));
        else emit("error", err);
        return false;
      }
      const u8 = typeof chunk === "string" ? new TextEncoder().encode(chunk)
        : (chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk?.buffer ?? chunk));
      const ok = __wjs_child_stdin_write(id, __b64enc(u8));
      if (!ok) {
        const err = Object.assign(new Error("This socket has been ended by the other party"), { code: "EPIPE" });
        if (typeof cb === "function") queueMicrotask(() => cb(err));
        else emit("error", err);
        return false;
      }
      if (typeof cb === "function") queueMicrotask(() => cb(null));
      // 背压（big-write-end 套件：恒 true 即 `while(write)` 死循环）：
      // 16KB 高水位（Socket 缺省），超即 false + 下轮记账清零发 drain。
      // task 侧无写完成回执，“入队即走”近似——投递保序不受记账影响
      // （unbounded 通道 FIFO，end 关排在写后）。
      buffered += u8.length;
      if (buffered > 16384) {
        queueMicrotask(() => { buffered = 0; emit("drain"); });
        return false;
      }
      return true;
    },
    end(chunk, cb) {
      if (chunk !== undefined && chunk !== null) api.write(chunk);
      if (ended) return api;
      ended = true;
      __wjs_child_stdin_close(id);
      queueMicrotask(() => emit("finish"));
      if (typeof cb === "function") queueMicrotask(() => cb());
      return api;
    },
    destroy() {
      if (destroyed) return api;
      destroyed = true;
      __wjs_child_stdin_close(id);
      emit("close");
      return api;
    },
    get writable() { return !destroyed && !ended; },
    get writableEnded() { return ended; },
    get destroyed() { return destroyed; },
    // stdin 是 Socket（Duplex）的写半部：readable 恒 false（stdin 套件点名）。
    get readable() { return false; },
  };
  return api;
}
export class ChildProcess {
  #id = 0;
  #killed = false;
  #onexit = null;
  #onclose = null;
  #onerror = null;
  #onspawn = null;
  // 多监听列表（on 累积/off 摘除；单分发位经 __install 落 fan-out）。
  #exitL = [];
  #closeL = [];
  // close 到达记录（流 end 后迟挂 close 即时重放，见 on；真机 exit→stdio 关→
  // close 异步序在本仓同派发内完成，记录补迟挂一拍）。
  #closeArgs = null;
  #errorL = [];
  #spawnL = [];
  #msgL = [];
  #internalL = [];
  #discL = [];
  #onmessage = null;
  #ondisconnect = null;
  #oninternal = null;
  exitCode = null;
  signalCode = null;
  spawnfile = null;
  spawnargs = null;
  channel = null;
  // fork 面（worker 线程底座，见文末 `fork`）：置位后 message/disconnect/
  // send/disconnect/kill/unref 走线程通道；spawn 子进程保持原语义。
  __forkChild = false;
  __worker = null;
  __connected = false;
  __exitCode = null;
  // fork 出口单发旗（abort 路径自 synth exit 后，线程 WExit 被拦）。
  __exitDone = false;
  __killSignal = "SIGTERM";
  __init(id, stdio) {
    this.#id = id;
    this.__initStreams(stdio, id);
    // close 记录常驻（迟挂重放：到达即记，不等首监听）。
    this.__install("close");
    // node 口径：spawn 成功后 pid 为自有数据属性（hasOwn true）；
    // 未成功（id=0）保持原型 getter 的 undefined。
    if (id !== 0) {
      try { Object.defineProperty(this, "pid", { value: __wjs_child_pid(id), writable: true, configurable: true, enumerable: true }); } catch {}
    }
    return this;
  }
  // 流初始化（正常 spawn 与死句柄路径共用；id=0 即死句柄——native 查表恒失败，
  // 写返回 false。cwd 套件错误路径仍要 child.stdout.setEncoding 可用）。
  __initStreams(stdio, id) {
    const id2 = id;
    // 超时杀落位（Rust dispatch 在 Exited 前调）：自有箭头属性而非原型方法——
    // dispatch 以 global 为 this 调钩子（§4.34），#killed 须走闭包捕获。
    this.__markKilled = () => { this.#killed = true; };
    // pipe 口径：stdout/stderr 为 live ReadableStream（Rust 泵按块 enqueue，
    // exit 前残留必达，见 dispatch）；stdin 为 WritableStream（写失败即子进程已走）。
    const mkOut = (push, close) => {
      let ctl = null;
      const stream = new ReadableStream({ start(c) { ctl = c; }, cancel() {} });
      this[push] = (b64) => { try { ctl.enqueue(__b64dec(b64)); } catch {} };
      this[close] = () => { try { ctl.close(); } catch {} };
      return __legacyReadable(stream);
    };
    if (stdio[1] === "pipe") this.stdout = mkOut("__pushOut", "__closeOut");
    else this.stdout = null;
    if (stdio[2] === "pipe") this.stderr = mkOut("__pushErr", "__closeErr");
    else this.stderr = null;
    if (stdio[0] === "pipe") {
      this.stdin = __legacyWritable(id2);
    } else this.stdin = null;
    // node 口径：stdio 数组恒在（与 stdin/stdout/stderr 同一对象；spawn-error
    //套件在 ENOENT 路径亦断言）。
    this.stdio = [this.stdin, this.stdout, this.stderr];
    return this;
  }
  // error 事件（spawn 预检失败/abort；无监听即抛——node 'error' 语义）。
  __emitError(err) {
    if (typeof this.onerror === "function") this.onerror(err);
    else throw err;
  }
  __emitAbort(reason) { this.__emitError(__abortError(reason)); }
  // 死句柄 close（spawn 预检失败：close(-errno, null)；exit 不发——真机 probe）。
  __emitDeadClose(errno) {
    if (typeof this.onclose === "function") this.onclose(errno ?? -2, null);
  }
  // node 口径：spawn 未成功（#id=0 占位）pid 恒 undefined（execFile ENOENT
  // 套件 typeof 点名；真机 ChildProcess 在 spawn 成功前根本无 pid 属性）
  get pid() { return this.#id === 0 ? undefined : __wjs_child_pid(this.#id); }
  get killed() { return this.#killed; }
  kill(signal) {
    if (this.__forkChild) {
      if (this.__exitCode !== null) return false;
      this.#killed = true;
      try { this.__worker.terminate(); } catch { return false; }
      return true;
    }
    // 信号名经 os.signals 表归一为数字；未知信号抛 ERR_UNKNOWN_SIGNAL
    //（真机 convertToValidSignal 口径；0 为存在性检查直接透传）。
    let sig = "15";
    if (signal !== undefined) {
      if (signal === 0) sig = "0";
      else {
        const hit = __sigResolve(signal);
        if (!hit) {
          const e = new TypeError(`Unknown signal: ${String(signal)}`);
          e.code = "ERR_UNKNOWN_SIGNAL"; throw e;
        }
        sig = String(hit.signo);
      }
    }
    const ok = __wjs_child_kill(this.#id, sig);
    if (ok) this.#killed = true;
    return ok;
  }
  // node internal/child_process.js ChildProcess.prototype.spawn 逐字口径：
  // validateObject(options) → stdio 归一（含 ipc 检出）→ 有 ipc 才验 envPairs →
  //验 file（string）→ 验 args（array）→ 起进程。校验序即语义（constructor 套件
  //逐块点名）。起进程段复用模块级 spawn(file, args, opts) 的归一/预检/自举/
  //native 落地（stdio 传复合形态时走 __spawnInto）。
  spawn(options) {
    if (typeof options !== "object" || options === null) {
      throw new ERR_INVALID_ARG_TYPE("options", "object", options);
    }
    // stdio 归一：string/array 均可；4 元 ipc 形保留（envPairs 校验用）。
    let stdioOpt = options.stdio !== undefined ? options.stdio : "pipe";
    let hasIpc = false;
    if (typeof stdioOpt === "string") {
      if (stdioOpt === "ipc") hasIpc = true;
    } else if (Array.isArray(stdioOpt)) {
      hasIpc = stdioOpt.includes("ipc");
    } else {
      throw new ERR_INVALID_ARG_VALUE("stdio", stdioOpt);
    }
    if (hasIpc) {
      if (options.envPairs !== undefined) {
        if (!Array.isArray(options.envPairs)) {
          throw new ERR_INVALID_ARG_TYPE("options.envPairs", "Array", options.envPairs);
        }
      }
    }
    if (typeof options.file !== "string") {
      throw new ERR_INVALID_ARG_TYPE("options.file", "string", options.file);
    }
    let args;
    if (options.args === undefined) args = [];
    else {
      if (!Array.isArray(options.args)) {
        throw new ERR_INVALID_ARG_TYPE("options.args", "Array", options.args);
      }
      args = options.args;
    }
    // 落地：与模块级 spawn 同一道 __spawnInto（proc=this，事件接线直挂 this）。
    __spawnInto(this, options.file, args, __normSpawnAsyncOpts({
      cwd: options.cwd, detached: options.detached, stdio: stdioOpt,
      shell: options.shell, uid: options.uid, gid: options.gid,
      windowsHide: options.windowsHide,
      windowsVerbatimArguments: options.windowsVerbatimArguments,
    }));
    return 0;
  }
  __idOf() { return this.#id; }
  // node 口径：显式资源管理（`using cat = spawn(...)`）——dispose 即 kill()
  //（destroy 套件；asyncDispose 同步落定）。
  [Symbol.dispose]() { try { this.kill(); } catch {} }
  [Symbol.asyncDispose]() { try { this.kill(); } catch {} return Promise.resolve(); }
  // node 口径：起进程成功后 nextTick 发 'spawn'（onSpawnNT）——早于任何
  // data/exit/close 分发（spawn-event 套件 didSpawn 门）。
  __emitSpawn() {
    if (typeof this.onspawn === "function") {
      try { this.onspawn(); } catch (e) { this.__emitError(e); }
    }
  }
  on(event, cb) {
    if (typeof cb !== "function") throw new TypeError("listener must be a function");
    // node 口径：同事件多监听并存（spawn-event 套件挂两个 'spawn'；旧单槽
    //实现后挂顶掉先挂，didSpawn 永 false）。列表累积 + fan-out 落分发位。
    if (event === "exit") { this.#exitL.push(cb); this.__install("exit"); }
    else if (event === "close") {
      this.#closeL.push(cb); this.__install("close");
      // close 已到后迟挂即时重放（流 end 后挂 close 形；once 包裹亦经此路）。
      if (this.#closeArgs !== null) {
        const self = this;
        queueMicrotask(() => {
          if (self.#closeL.includes(cb)) { try { cb(...self.#closeArgs); } catch {} }
        });
      }
    }
    else if (event === "error") { this.#errorL.push(cb); this.__install("error"); }
    else if (event === "spawn") { this.#spawnL.push(cb); this.__install("spawn"); }
    else if (event === "message" || event === "disconnect" || event === "internalMessage") {
      if (!this.__forkChild) {
        // spawn 子进程无 fd-passing 通道（记档缺口）：监听即明错，不静默吞
        throw Object.assign(new Error("ERR_NOT_SUPPORTED: child IPC channel not supported (use fork)"), { code: "ERR_NOT_SUPPORTED" });
      }
      if (event === "message") { this.#msgL.push(cb); this.__install("message"); }
      else if (event === "internalMessage") { this.#internalL.push(cb); this.__install("internalMessage"); }
      else { this.#discL.push(cb); this.__install("disconnect"); }
    }
    else throw new Error(`NotSupportedError: ChildProcess event '${event}' (exit/close/error/spawn/message/disconnect/internalMessage)`);
    return this;
  }
  // 监听列表扇出到单分发位（exit/close 走访问器 wrap 落码；空表即摘除）。
  __install(event) {
    if (event === "exit") {
      const ls = [...this.#exitL];
      this.onexit = ls.length ? ((code, signal) => { for (const fn of ls) fn(code, signal); }) : null;
    } else if (event === "close") {
      const self = this;
      const ls = [...this.#closeL];
      // 常驻记录（空表亦装：无监听到达仍记，供迟挂重放；同步扇出时序不动）。
      this.onclose = ((code, signal) => {
        self.#closeArgs = [code, signal];
        for (const fn of ls) fn(code, signal);
      });
    } else if (event === "error") {
      const ls = [...this.#errorL];
      this.onerror = ls.length ? ((...a) => { for (const fn of ls) fn(...a); }) : null;
    } else if (event === "spawn") {
      const ls = [...this.#spawnL];
      this.onspawn = ls.length ? (() => { for (const fn of ls) fn(); }) : null;
    } else if (event === "message") {
      const ls = [...this.#msgL];
      this.#onmessage = ls.length ? ((m) => { for (const fn of ls) fn(m); }) : null;
    } else if (event === "internalMessage") {
      const ls = [...this.#internalL];
      this.#oninternal = ls.length ? ((m) => { for (const fn of ls) fn(m); }) : null;
    } else if (event === "disconnect") {
      const ls = [...this.#discL];
      this.#ondisconnect = ls.length ? (() => { for (const fn of ls) fn(); }) : null;
    }
  }
  once(event, cb) {
    if (typeof cb !== "function") throw new TypeError("listener must be a function");
    const self = this;
    const wrapped = (...args) => { self.off(event, wrapped); cb(...args); };
    wrapped.__wjs_orig = cb;
    return this.on(event, wrapped);
  }
  off(event, cb) {
    const match = (fn) => fn === cb || (typeof fn === "function" && fn.__wjs_orig === cb);
    const drop = (ls) => { const i = ls.findIndex(match); if (i >= 0) ls.splice(i, 1); };
    if (event === "exit") { drop(this.#exitL); this.__install("exit"); }
    else if (event === "close") { drop(this.#closeL); this.__install("close"); }
    else if (event === "error") { drop(this.#errorL); this.__install("error"); }
    else if (event === "spawn") { drop(this.#spawnL); this.__install("spawn"); }
    else if (event === "message") { drop(this.#msgL); this.__install("message"); }
    else if (event === "internalMessage") { drop(this.#internalL); this.__install("internalMessage"); }
    else if (event === "disconnect") { drop(this.#discL); this.__install("disconnect"); }
    return this;
  }
  removeListener(event, cb) { return this.off(event, cb); }
  // node 口径（kill-sigwinch 套件）：清指定事件（缺省全清）监听。
  removeAllListeners(event) {
    if (event === undefined) {
      for (const e of ["exit", "close", "error", "spawn", "message", "disconnect", "internalMessage"]) this.__clearAll(e);
    } else {
      if (!["exit", "close", "error", "spawn", "message", "disconnect", "internalMessage"].includes(event)) {
        throw new Error(`NotSupportedError: ChildProcess event '${event}' (exit/close/error/spawn/message/disconnect/internalMessage)`);
      }
      this.__clearAll(event);
    }
    return this;
  }
  __clearAll(event) {
    if (event === "exit") { this.#exitL.length = 0; this.__install("exit"); }
    else if (event === "close") { this.#closeL.length = 0; this.__install("close"); }
    else if (event === "error") { this.#errorL.length = 0; this.__install("error"); }
    else if (event === "spawn") { this.#spawnL.length = 0; this.__install("spawn"); }
    else if (event === "message") { this.#msgL.length = 0; this.__install("message"); }
    else if (event === "internalMessage") { this.#internalL.length = 0; this.__install("internalMessage"); }
    else if (event === "disconnect") { this.#discL.length = 0; this.__install("disconnect"); }
  }
  // 手动派发（execfile 套件直调 child.emit('close', …)；真机 EventEmitter 口径，
  // 走访问器 wrap 以便 exitCode/signalCode 落定）。
  emit(event, ...args) {
    if (event === "exit" && typeof this.onexit === "function") { this.onexit(...args); return true; }
    if (event === "close" && typeof this.onclose === "function") { this.onclose(...args); return true; }
    if (event === "error" && typeof this.onerror === "function") { this.onerror(...args); return true; }
    if (event === "spawn" && typeof this.onspawn === "function") { this.onspawn(...args); return true; }
    return false;
  }
  // exit/close 经访问器 wrap：落定退出码（直接赋值亦生效，Node 的 exitCode 语义）。
  // node 口径：回调双参 (code, signal)；null/undefined 的位不动（exit 用旧值，
  // close 用 null——真机 close 在 signal 死亡时 exitCode 仍 null）。
  set onexit(cb) {
    this.#onexit = (typeof cb === "function") ? ((code, signal) => {
      if (code !== undefined && code !== null) this.exitCode = code;
      if (signal !== undefined && signal !== null) this.signalCode = signal;
      cb(code, signal);
    }) : cb;
  }
