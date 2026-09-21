
// ── 9i-2 BroadcastChannel（同名跨会话扇出；发者自收排除，关者止收）────────
// 形态偏差记档：基座沿用 EventEmitter（与本仓 MessagePort 一致；真机为
// EventTarget，`String(bc)` 形状不同）；`message` 事件载荷为裸值（真机为
// MessageEvent），`onmessage` 回调收 `{ data }` 兼容形；无 `close` 事件。

// 10f 对拍：BroadcastChannel.prototype 方法族 this 品牌门（broadcastchannel
// 套件 Reflect.get/apply 面，node ERR_INVALID_THIS 口径）。
const __bcInvalidThis = () => {
  const err = new TypeError('Value of "this" must be of type BroadcastChannel');
  err.code = "ERR_INVALID_THIS";
  return err;
};
const __bcState = new WeakSet();

export class BroadcastChannel extends EventEmitter {
  constructor(name) {
    super();
    // node broadcastchannel 套件口径：仅无参即 ERR_MISSING_ARGS；显式
    // undefined 合法（name "undefined"）；Symbol 走 `${name}` ToString 即抛
    //（本引擎转换错文案与 V8 不同，此处按真机原文补齐——套件正则点名）。
    if (arguments.length === 0) {
      const err = new TypeError(`The "name" argument must be specified`);
      err.code = "ERR_MISSING_ARGS";
      throw err;
    }
    if (typeof name === "symbol") {
      throw new TypeError("Cannot convert a Symbol value to a string");
    }
    name = `${name}`;
    const sub = String(__wjs_bc_sub(name));
    if (sub === "") {
      throw new Error("OperationError: BroadcastChannel is not initialized");
    }
    __bcState.add(this);
    this.__name = name;
    this.__sub = sub;
    this.__closed = false;
    this.__onmessage = null;
    // 无监听到达的广播排队（receiveMessageOnPort 同步收信——broadcastchannel
    // 套件 `receiveMessageOnPort(bc2).message` 点名；node 底层即 MessagePort）。
    this.__queue = [];
    this.__flushScheduled = false;
    this.__ev = this.__ev.bind(this);
    this.on("newListener", (ev) => { if (ev === "message") __wjs_bc_flags(sub, "listen"); });
    // newListener 在入表前触发（§4.47）：延迟一轮再刷队。
    this.on("newListener", (ev) => { if (ev === "message") queueMicrotask(() => this.__maybeFlush()); });
    this.on("removeListener", (ev) => {
      if (ev === "message" && this.listenerCount("message") === 0) __wjs_bc_flags(sub, "unlisten");
    });
    __wjs_bc_attach(sub, this);
  }
  __maybeFlush() {
    if (this.__flushScheduled || this.__queue.length === 0) return;
    if (this.listenerCount("message") === 0) return;
    this.__flushScheduled = true;
    queueMicrotask(() => {
      this.__flushScheduled = false;
      this.__flushQueue();
    });
  }
  __flushQueue() {
    while (this.__queue.length > 0) {
      // 逐条查关（wpt：onmessage 内 close 阻断同端口已排队任务）。
      if (this.__closed) return;
      const raw = this.__queue.shift();
      let value;
      try {
        value = __fromWire(String(raw));
      } catch (err) {
        this.emit("messageerror", err instanceof Error ? err : new Error("worker message is not valid JSON"));
        continue;
      }
      this.emit("message", value);
    }
  }
  get name() {
    if (!__bcState.has(this)) throw __bcInvalidThis();
    return this.__name;
  }
  get onmessage() {
    if (!__bcState.has(this)) throw __bcInvalidThis();
    return this.__onmessage;
  }
  set onmessage(fn) {
    if (!__bcState.has(this)) throw __bcInvalidThis();
    if (this.__onmessage !== null) this.removeListener("message", this.__onmessageWrap);
    this.__onmessage = (typeof fn === "function") ? fn : null;
    if (this.__onmessage !== null) {
      // 真机口径：onmessage 收真 MessageEvent（data/target，broadcastchannel
      // 套件逐项）；EE 'message' 载荷裸值不变。
      this.__onmessageWrap = (value) => {
        try {
          this.__onmessage(new globalThis.MessageEvent("message", { data: value, __wjsTarget: this }));
        } catch { /* 忽略 */ }
      };
      this.on("message", this.__onmessageWrap);
    } else {
      this.__onmessageWrap = null;
    }
  }
  // EventTarget 双面（同 MessagePort：message 系 MessageEvent，自定义 CustomEvent）。
  addEventListener(type, fn) {
    if (typeof fn !== "function") return;
    const t = String(type);
    const isMsg = t === "message" || t === "messageerror";
    const per = (this.__etWrap ??= new WeakMap());
    let byType = per.get(fn);
    if (!byType) { byType = new Map(); per.set(fn, byType); }
    if (byType.has(t)) return;
    const wrap = (value) => {
      const ev = isMsg
        ? new globalThis.MessageEvent(t, { data: value, __wjsTarget: this })
        : new globalThis.CustomEvent(t, { detail: value });
      fn.call(this, ev);
    };
    byType.set(t, wrap);
    this.on(t, wrap);
  }
  removeEventListener(type, fn) {
    const wrap = this.__etWrap?.get(fn)?.get(String(type));
    if (wrap) {
      this.off(String(type), wrap);
      this.__etWrap.get(fn).delete(String(type));
    }
  }
  __ev(kind, payload) {
    if (kind !== "message") return;
    if (this.__closed) return;
    // 无监听即排队（同步收信面）；监听到达由 newListener 延迟刷（§4.47）。
    if (this.listenerCount("message") === 0) {
      this.__queue.push(payload);
      return;
    }
    let value;
    try {
      value = __fromWire(String(payload));
    } catch (e) {
      this.emit("messageerror", e instanceof Error ? e : new Error("worker message is not valid JSON"));
      return;
    }
    this.emit("message", value);
  }
  postMessage(value) {
    if (!__bcState.has(this)) throw __bcInvalidThis();
    // node broadcastchannel 套件口径：仅无参即抛；显式 undefined 合法（可投递）。
    if (arguments.length === 0) {
      const err = new TypeError(`The "message" argument must be specified`);
      err.code = "ERR_MISSING_ARGS";
      throw err;
    }
    if (this.__closed) throw new Error("BroadcastChannel is closed");
    __wjs_bc_pub(this.name, this.__sub, __toWire(value));
  }
  close() {
    if (!__bcState.has(this)) throw __bcInvalidThis();
    if (this.__closed) return;
    this.__closed = true;
    __wjs_bc_unsub(this.__sub);
  }
  ref() {
    if (!__bcState.has(this)) throw __bcInvalidThis();
    __wjs_bc_flags(this.__sub, "ref");
    return this;
  }
  unref() {
    if (!__bcState.has(this)) throw __bcInvalidThis();
    __wjs_bc_flags(this.__sub, "unref");
    return this;
  }
  // node 口径：inspect 形 "BroadcastChannel { name: 'x', active: true|false }"
  //（broadcastchannel 套件逐字；active 非真实属性——inspect 定制面）。
  [Symbol.for("nodejs.util.inspect.custom")]() {
    return `BroadcastChannel { name: ${__inspectQuote(this.__name)}, active: ${!this.__closed} }`;
  }
}

export const isMainThread = __wjs_worker_is_main();
export const threadId = Number(__wjs_worker_thread_id());
// 10f：本线程构造名（主线程 null；worker 无名亦 null——thread-name 套件）。
export const threadName = __wjs_worker_name() || null;
const __parentId = String(__wjs_worker_parent());
export const parentPort = (__parentId === "") ? null : new MessagePort(__parentId);
const __dataRaw = __wjs_worker_data();
export const workerData = (__dataRaw === undefined || __dataRaw === "") ? null : __fromWire(String(__dataRaw));
export const resourceLimits = {};
export const SHARE_ENV = Symbol("SHARE_ENV");

// node worker bootstrap 口径：worker 内的进程操作族换 UNSUPPORTED_OPERATION 桩
// （unsupported-things 套件逐项点名：disabled 位/() 形/无 () 属性形三种）。
// 本模块在 worker 内首次求值时装上（worker 线程同一 prelude，__parentId 非空即 worker）。
if (__parentId !== "") {
  // 10f 对拍 + fork 兼容（§4.132 坑八）：fork 子进程（`__wjs_forkChild`）有
  // IPC 通道——send/disconnect/channel/connected 是正道，不装桩；fork 的
  // 子端 src 随后覆写它们（getter 桩会让 strict 赋值直接炸，fork 全灭）。
  const __proc = globalThis.process;
  const __isFork = __wjs_worker_is_fork() === true;
  const __throwUnsupported = (msg) => {
    const err = new Error(msg);
    err.code = "ERR_WORKER_UNSUPPORTED_OPERATION";
    throw err;
  };
  for (const fn of ["abort", "chdir", "setuid", "seteuid",
    "setgid", "setegid", "setgroups", "initgroups"]) {
    const stub = function () { __throwUnsupported(`process.${fn}() is not supported in workers`); };
    stub.disabled = true;
    try { __proc[fn] = stub; } catch { /* 只读面跳过 */ }
  }
  if (!__isFork) {
    for (const fn of ["send", "disconnect"]) {
      const stub = function () { __throwUnsupported(`process.${fn}() is not supported in workers`); };
      stub.disabled = true;
      try { __proc[fn] = stub; } catch { /* 只读面跳过 */ }
    }
    for (const prop of ["channel", "connected"]) {
      try {
        Object.defineProperty(__proc, prop, {
          get() { __throwUnsupported(`process.${prop} is not supported in workers`); },
          configurable: true,
        });
      } catch { /* 同上 */ }
    }
  }
  try {
    __proc.umask = (mask) => {
      if (mask !== undefined) {
        __throwUnsupported("Setting process.umask() is not supported in workers");
      }
      return __wjs_umask();
    };
  } catch { /* 同上 */ }
  for (const k of ["_startProfilerIdleNotifier", "_stopProfilerIdleNotifier",
    "_debugProcess", "_debugPause", "_debugEnd"]) {
    try { delete __proc[k]; } catch { /* 同上 */ }
  }
}

export function setEnvironmentData(key, value) {
  if (typeof key !== "string") {
    const err = new TypeError(`The "key" argument must be of type string. Received type ${typeof key}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  __wjs_worker_env_set(key, __toWire(value));
}
export function getEnvironmentData(key) {
  if (typeof key !== "string") {
    const err = new TypeError(`The "key" argument must be of type string. Received type ${typeof key}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const raw = __wjs_worker_env_get(key);
  if (raw === undefined) return undefined;
  return __fromWire(String(raw));
}

// node lib/internal/worker.js 口径：URL(data:)/URL(file:)/绝对串/./.. 串四路，
// 其余 ERR_WORKER_PATH（file:///data: 串带 Wrap 提示）；非法类型 ARG_TYPE 数组形。
function __workerFilePath(filename) {
  const isUrlLike = typeof filename === "object" && filename !== null && typeof filename.href === "string";
  if (isUrlLike && String(filename.href).startsWith("data:")) {
    // node 口径：data: URL 解码为源码走 eval（utf8/base64 双形）。
    const href = String(filename.href);
    const comma = href.indexOf(",");
    const meta = href.slice(5, comma === -1 ? href.length : comma);
    const payload = comma === -1 ? "" : href.slice(comma + 1);
    const decoded = meta.endsWith(";base64") ? atob(payload) : decodeURIComponent(payload);
    return { src: decoded, isEval: "1" };
  }
  if (isUrlLike) {
    const href = String(filename.href);
    if (href.startsWith("file://")) {
      try {
        return { src: decodeURIComponent(new URL(href).pathname), isEval: "0" };
      } catch { /* 落下走报错 */ }
    }
    // node 口径：URL 实例非 file: 即 ERR_INVALID_URL_SCHEME（unsupported-path 套件）。
    throw new codes.ERR_INVALID_URL_SCHEME("file");
  }
  if (typeof filename === "string") {
    if (filename.startsWith("/") || /^\.\.?[\\/]/.test(filename)) {
      return { src: filename, isEval: "0" };
    }
    const hint = (filename.startsWith("file://") ? " Wrap file:// URLs with `new URL`." : "") +
      (filename.startsWith("data:text/javascript") ? " Wrap data: URLs with `new URL`." : "");
    const err = new TypeError(
      `The worker script or module filename must be an absolute path or a relative path starting with './' or '../'.${hint} Received "${filename}"`);
    err.code = "ERR_WORKER_PATH";
    throw err;
  }
  throw new codes.ERR_INVALID_ARG_TYPE("filename", ["string", "URL"], filename);
}

export class Worker extends EventEmitter {
  // 子线程标出流（M5 vitest 牵引：threads 池 `worker.stdout.pipe(logger)` 无守卫，
  // `streamFlushed` 等 end/close；无数据流——子输出直走共享 stdio，见 `__end` 记档）。
  // 仅 `new Worker(..., { stdout: true })` 时具现（Node 形），否则保持 null。
  __stdout = null;
  __stderr = null;
  constructor(filename, options = {}) {
    super();
    if (options === null || (typeof options !== "object" && typeof options !== "function")) {
      const err = new TypeError(`The "options" argument must be of type object.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    // node lib/internal/worker.js 校验序：eval 门 → filename 路由 → env/name/execArgv。
    if (options.eval && typeof filename !== "string") {
      throw new codes.ERR_INVALID_ARG_VALUE("options.eval", options.eval, "must be false when 'filename' is not a string");
    }
    const { src, isEval } = options.eval
      ? { src: String(filename), isEval: "1" }
      : __workerFilePath(filename);
    // env（node internal/worker.js 逐字口径）：对象（含数组）逐值 `${v}` 化；
    // null/undefined 继承——本仓落成创建时快照（worker 隔离，10f process-env）；
    // SHARE_ENV 共享真 env（快照 wire 置空即原语义）；其余 ARG_TYPE（message
    // 由 errors.js 逐字渲染，套件断全文）。
    const envOpt = options.env;
    let envJson;
    if (typeof envOpt === "object" && envOpt !== null) {
      const envObj = {};
      for (const [k, v] of Object.entries(envOpt)) envObj[k] = `${v}`;
      envJson = JSON.stringify(envObj);
    } else if (envOpt == null) {
      envJson = JSON.stringify(process.env);
    } else if (envOpt === SHARE_ENV) {
      envJson = "";
    } else {
      throw new codes.ERR_INVALID_ARG_TYPE("options.env", ["object", "undefined", "null", "worker_threads.SHARE_ENV"], envOpt);
    }
    if (options.name !== undefined && typeof options.name !== "string") {
      throw new codes.ERR_INVALID_ARG_TYPE("options.name", "string", options.name);
    }
    if (options.execArgv !== undefined &&
        (!Array.isArray(options.execArgv) || options.execArgv.some((s) => typeof s !== "string"))) {
      throw new codes.ERR_INVALID_ARG_TYPE("options.execArgv", ["string[]"], options.execArgv);
    }
    let dataJson = "";
    if (options.workerData !== undefined) {
      dataJson = __toWire(options.workerData, options.transferList);
    }
    let ids;
    ids = String(__wjs_worker_spawn(src, isEval, dataJson, options.name ?? "", options.__wjs_forkChild ? "1" : "0", envJson)).split(" ");
    this.__id = ids[0];
    this.__tid = Number(ids[1]);
    this.__exited = null;
    this.__name = options.name ?? null;
    this.__rl = options.resourceLimits ?? null;
    this.__termWaiters = [];
    if (options.stdout) this.__stdout = new __WorkerStdio();
    if (options.stderr) this.__stderr = new __WorkerStdio();
    this.__ev = this.__ev.bind(this);
    __wjs_worker_attach(this.__id, this);
  }
  __ev(kind, payload) {
    if (kind === "message") {
      let value;
      try {
        value = __fromWire(String(payload));
      } catch (e) {
        this.emit("messageerror", e instanceof Error ? e : new Error("worker message is not valid JSON"));
        return;
      }
      this.emit("message", value);
    } else if (kind === "error") {
      // 10f 对拍：Rust 侧 "Kind: message" 前缀还原原错误类（SyntaxError 套件
      // 断 err.constructor === SyntaxError）；裸文本回 Error（uncaught-exception
      // 套件断 String(err) === 'Error: foo'）；`__wjs_prim:` 信封还原原始值
      // （error-primitive 套件断 err === 42 / Symbol.for('a') 等）。
      const text = String(payload);
      if (text.startsWith("__wjs_prim:")) {
        this.emit("error", __wjs_primFromText(text.slice(11)));
        return;
      }
      const m = /^(SyntaxError|TypeError|RangeError|EvalError|ReferenceError|URIError|AggregateError): ([\s\S]*)$/.exec(text);
      this.emit("error", m ? new globalThis[m[1]](m[2]) : new Error(text));
    } else if (kind === "exit") {
      const code = Number(payload);
      this.__exited = code;
      if (this.__stdout) this.__stdout.__end();
      if (this.__stderr) this.__stderr.__end();
      const waiters = this.__termWaiters.splice(0);
      for (const w of waiters) {
        try { w(code); } catch { /* 忽略 */ }
      }
      this.emit("exit", code);
    } else if (kind === "online") {
      this.emit("online");
    }
  }
  postMessage(value, transfer) {
    const wire = __toWire(value, transfer);
    __wjs_worker_post(this.__id, wire);
  }
  terminate() {
    try { __wjs_worker_terminate(this.__id); } catch { /* 已退出即走下 */ }
    if (this.__exited !== null) return Promise.resolve(this.__exited);
    return new Promise((resolve) => { this.__termWaiters.push(resolve); });
  }
  ref() {
    __wjs_worker_set_ref(this.__id, "1");
    return this;
  }
  unref() {
    __wjs_worker_set_ref(this.__id, "0");
    return this;
  }
  get threadId() {
    // 10f 对拍：退出后 threadId 恒 -1（safe-getters 套件点名；真机口径）。
    return this.__exited === null ? this.__tid : -1;
  }
  // 10f 对拍：threadName 运行期 = options.name（无名 null），exit 后恒 null；
  // resourceLimits = 传入对象（缺省 {}，退出后 {}——resource-limits 套件）。
  get threadName() { return this.__exited === null ? (this.__name ?? null) : null; }
  get resourceLimits() { return this.__rl ?? {}; }
  get stdin() { return null; }
  get stdout() { return this.__stdout; }
  get stderr() { return this.__stderr; }
}
// MessageEvent 的 source/ports 校验需要 MessagePort 品牌（主线程无全局
// MessagePort，模块求值期登记到隐藏槽；求值前无从有端口，校验恒 TypeError）。
globalThis.__wjs_MessagePort = MessagePort;
// 10f：BroadcastChannel 是 Web 全局（node 主/worker 线程均全局，messaging
// 套件裸 `new BroadcastChannel(...)`；与 worker_threads 导出同一类）。
globalThis.BroadcastChannel = BroadcastChannel;
// 无数据标出流（见 Worker 注释）：pipe/unpipe 形状 + end/close 一次性语义；
// 数据永不流动（子输出直走共享 stdio），`__end` 在 worker 退出时由分发调用。
class __WorkerStdio {
  constructor() {
    this.__listeners = {};
    this.readableEnded = false;
    this.destroyed = false;
  }
  on(ev, cb) {
    if (typeof cb !== "function") throw new TypeError("listener must be a function");
    (this.__listeners[String(ev)] ??= []).push(cb);
    return this;
  }
  once(ev, cb) {
    if (typeof cb !== "function") throw new TypeError("listener must be a function");
    const self = this;
    const wrapped = (...args) => { self.off(ev, wrapped); cb(...args); };
    wrapped.__wjs_orig = cb;
    return this.on(ev, wrapped);
  }
  off(ev, cb) {
    const list = this.__listeners[String(ev)];
    if (list) {
      let i = list.findIndex((l) => l === cb || l.__wjs_orig === cb);
      while (i >= 0) { list.splice(i, 1); i = list.findIndex((l) => l === cb || l.__wjs_orig === cb); }
    }
    return this;
  }
  pipe(dest) { return dest; }
  unpipe() { return this; }
  __end() {
    if (this.readableEnded) return;
    this.readableEnded = true;
    this.destroyed = true;
    for (const ev of ["end", "close"]) {
      for (const cb of [...(this.__listeners[ev] ?? [])]) {
        try { cb(); } catch {}
      }
    }
    this.__listeners = {};
  }
}

const __api = {
  isMainThread, threadId, threadName, parentPort, workerData, resourceLimits, SHARE_ENV,
  MessagePort, MessageChannel, BroadcastChannel, Worker, markAsUncloneable,
  markAsUntransferable, isMarkedAsUntransferable,
  moveMessagePortToContext, receiveMessageOnPort,
  setEnvironmentData, getEnvironmentData,
};
export default __api;
