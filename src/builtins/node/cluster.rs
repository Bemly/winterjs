//! `node:cluster`：Bun 🟡对等面（10e）。
//!
//! 落法：骑 `node:worker_threads` Worker（`child.fork` 同构 bootstrap），
//! 真多进程语义不做（书面记档）：无独立进程/env 隔离/signal 语义；
//! `worker.process` 为最小桩（pid=线程 id）；`listening` 事件不发
//! （server 共享需 fd 传递，线程底座无此能力）；scheduling 仅存值。
//! Worker 判定走 `workerData.__wjs_cluster`（不污染 `process.env`）；
//! `fork(env)` 的 env 在子端合入 `process.env`（底座 env 表进程共享，
//! 可见性属偏差，测试用唯一键名隔离）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";
import { Worker as ThreadWorker, workerData, isMainThread } from "node:worker_threads";
import { pathToFileURL } from "node:url";

export const SCHED_NONE = 1;
export const SCHED_RR = 2;

// 子会话入口（`child.fork` 同构；占位经 replacer 填 JSON，见 §4.44）。
const __CLUSTER_CHILD_SRC = `
import { parentPort, workerData } from "node:worker_threads";
const __mod = __CLUSTER_MOD__;
const __cargs = __CLUSTER_ARGS__;
const __cenv = __CLUSTER_ENV__;
process.argv = [process.execPath, __mod, ...__cargs];
for (const [k, v] of Object.entries(__cenv)) {
  try { process.env[k] = String(v); } catch {}
}
process.connected = true;
process.send = (message, ...rest) => {
  let cb = null;
  for (const a of rest) if (typeof a === "function") cb = a;
  if (!process.connected || parentPort === null) {
    const err = new Error("Channel closed");
    err.code = "ERR_IPC_CHANNEL_CLOSED";
    if (cb) queueMicrotask(() => cb(err));
    return false;
  }
  try {
    parentPort.postMessage(message);
  } catch (e) {
    const err = e instanceof Error ? e : new Error(String(e));
    if (!err.code) err.code = "ERR_IPC_CHANNEL_CLOSED";
    if (cb) queueMicrotask(() => cb(err));
    return false;
  }
  if (cb) queueMicrotask(() => cb(null));
  return true;
};
function __clusterDoDisconnect(local) {
  if (!process.connected) return;
  process.connected = false;
  if (!local) {
    try { parentPort.postMessage({ __wjs_cluster_ctl: "disconnect" }); } catch {}
  }
  queueMicrotask(() => { try { parentPort.close(); } catch {} });
  process.__wjs_emit("disconnect");
}
process.disconnect = () => __clusterDoDisconnect(false);
if (parentPort !== null) {
  parentPort.on("message", (message) => {
    if (message !== null && typeof message === "object" && !Array.isArray(message) &&
        message.__wjs_cluster_ctl === "disconnect") {
      __clusterDoDisconnect(true);
      return;
    }
    process.__wjs_emit("message", message);
  });
  parentPort.on("close", () => {
    if (process.connected) {
      process.connected = false;
      process.__wjs_emit("disconnect");
    }
  });
  try { parentPort.postMessage({ __wjs_cluster_ctl: "online" }); } catch {}
}
await import(__mod);
`;

const __isClusterWorker =
  !isMainThread && !!workerData && !!workerData.__wjs_cluster;
const __myId = __isClusterWorker ? workerData.__wjs_cluster.id : 0;

export const isWorker = __isClusterWorker;
export const isPrimary = !__isClusterWorker;
export const isMaster = !__isClusterWorker;

let __sched = SCHED_RR;
// settings 单对象原地更新（具名导出活绑定 + 实例 getter 同源）。
const __settings = {};
let __nextId = 1;
const __workers = {};

class ClusterWorker extends EventEmitter {
  constructor(id, w) {
    super();
    this.id = id;
    this.__w = w;
    this.__connected = true;
    this.__dead = false;
    this.__online = false;
    this.suicide = false;
    this.exitedAfterDisconnect = false;
    const self = this;
    this.process = {
      pid: w ? w.threadId : 0,
      connected: true,
      kill(signal) { self.kill(signal); },
    };
  }
  send(message, ...rest) {
    let cb = null;
    for (const a of rest) if (typeof a === "function") cb = a;
    if (!this.__connected || this.__w === null) {
      const err = new Error("Channel closed");
      err.code = "ERR_IPC_CHANNEL_CLOSED";
      if (cb) queueMicrotask(() => cb(err));
      return false;
    }
    try {
      this.__w.postMessage(message);
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e));
      if (!err.code) err.code = "ERR_IPC_CHANNEL_CLOSED";
      if (cb) queueMicrotask(() => cb(err));
      return false;
    }
    if (cb) queueMicrotask(() => cb(null));
    return true;
  }
  disconnect() {
    if (!this.__connected) return;
    this.suicide = true;
    try { this.__w.postMessage({ __wjs_cluster_ctl: "disconnect" }); } catch {}
    this.__onDisconnect();
  }
  kill(signal) {
    this.suicide = true;
    if (this.__w !== null) {
      try { this.__w.terminate(); } catch {}
    }
  }
  isConnected() { return this.__connected; }
  isDead() { return this.__dead; }
  __onDisconnect() {
    if (!this.__connected) return;
    this.__connected = false;
    this.process.connected = false;
    this.emit("disconnect");
    cluster.emit("disconnect", this);
  }
  __onExit(code) {
    if (this.__connected) this.__onDisconnect();
    this.__dead = true;
    if (!this.exitedAfterDisconnect && this.suicide) this.exitedAfterDisconnect = true;
    delete __workers[this.id];
    this.emit("exit", code, null);
    cluster.emit("exit", this, code, null);
  }
}

// 子端当前 worker 桩（send/disconnect 走 parentPort；只读 id）。
class ClusterWorkerSide extends EventEmitter {
  constructor(id) {
    super();
    this.id = id;
    this.suicide = false;
    this.exitedAfterDisconnect = false;
    const self = this;
    this.process = {
      pid: 0,
      connected: true,
      kill(signal) {},
    };
  }
  send(message, ...rest) {
    if (typeof process.send === "function") return process.send(message, ...rest);
    return false;
  }
  disconnect() {
    if (typeof process.disconnect === "function") process.disconnect();
  }
  kill(signal) {}
  isConnected() { return !!process.connected; }
  isDead() { return false; }
}

class Cluster extends EventEmitter {
  get SCHED_NONE() { return SCHED_NONE; }
  get SCHED_RR() { return SCHED_RR; }
  get workers() { return __workers; }
  get settings() { return { ...__settings }; }
  get schedulingPolicy() { return __sched; }
  set schedulingPolicy(v) {
    if (v !== SCHED_NONE && v !== SCHED_RR) {
      const err = new TypeError("schedulingPolicy must be SCHED_NONE or SCHED_RR");
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    __sched = v;
  }
  get worker() { return __isClusterWorker ? __sideWorker : undefined; }
  get isWorker() { return __isClusterWorker; }
  get isPrimary() { return !__isClusterWorker; }
  get isMaster() { return !__isClusterWorker; }
  setupPrimary(settings) {
    if (settings !== undefined && (typeof settings !== "object" || settings === null)) {
      const err = new TypeError("settings must be an object");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    for (const k of Object.keys(__settings)) delete __settings[k];
    Object.assign(__settings, settings ?? {});
  }
  setupMaster(settings) { this.setupPrimary(settings); }
  fork(env) {
    if (__isClusterWorker) {
      // 线程底座不支持 worker 内再 fork（见模块头注）。
      const err = new Error("fork is not available inside cluster workers");
      err.code = "ERR_NOT_SUPPORTED";
      throw err;
    }
    if (env !== undefined && (typeof env !== "object" || env === null)) {
      const err = new TypeError("env must be an object");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const id = __nextId++;
    const exec = __settings.exec ?? process.argv[1];
    if (typeof exec !== "string") {
      const err = new Error("cluster.fork needs an entry file (process.argv[1])");
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    const fileUrl = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(exec) ? exec : pathToFileURL(exec).href;
    const argsArr = [...(__settings.args ?? [])].map(String);
    const envObj = {};
    for (const [k, v] of Object.entries(env ?? {})) envObj[String(k)] = String(v);
    const src = __CLUSTER_CHILD_SRC
      .replace("__CLUSTER_MOD__", () => JSON.stringify(fileUrl))
      .replace("__CLUSTER_ARGS__", () => JSON.stringify(argsArr))
      .replace("__CLUSTER_ENV__", () => JSON.stringify(envObj));
    const w = new ThreadWorker(src, { eval: true, workerData: { __wjs_cluster: { id } }, __wjs_forkChild: true });
    const worker = new ClusterWorker(id, w);
    __workers[id] = worker;
    w.on("message", (m) => {
      if (m !== null && typeof m === "object" && !Array.isArray(m) && typeof m.__wjs_cluster_ctl === "string") {
        if (m.__wjs_cluster_ctl === "online") {
          worker.__online = true;
          worker.emit("online");
          cluster.emit("online", worker);
        } else if (m.__wjs_cluster_ctl === "disconnect") {
          worker.suicide = true;
          worker.__onDisconnect();
        }
        return;
      }
      worker.emit("message", m);
      cluster.emit("message", worker, m);
    });
    w.on("error", (e) => { worker.emit("error", e); });
    w.on("exit", (code) => { worker.__onExit(code); });
    queueMicrotask(() => { cluster.emit("fork", worker); });
    return worker;
  }
  disconnect(cb) {
    for (const w of Object.values(__workers)) {
      try { w.disconnect(); } catch {}
    }
    if (typeof cb === "function") queueMicrotask(cb);
  }
}

const cluster = new Cluster();
const __sideWorker = __isClusterWorker ? new ClusterWorkerSide(__myId) : undefined;

export default cluster;
export { __workers as workers, __settings as settings };
export const Worker = ClusterWorker;
"#;
