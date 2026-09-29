import { EventEmitter } from "node:events";
import { codes } from "node:internal/errors";
import { inspect as __wjs2Inspect } from "node:util";

// node util.inspect 的字符串形（单引号——BroadcastChannel inspect 定制用）。
function __inspectQuote(s) {
  return __wjs2Inspect(String(s), { quotes: "single" });
}

const __uncloneable = new WeakSet();
export function markAsUncloneable(obj) {
  if ((typeof obj === "object" && obj !== null) || typeof obj === "function") {
    __uncloneable.add(obj);
  }
}

// 10f：node 26 具名面（mark-as-untransferable 套件）——标记 AB 不可转移，
// 转移即 DataCloneError(25) 且**不 detach**；`isMarkedAsUntransferable` 查询。
const __untransferable = new WeakSet();
export function markAsUntransferable(obj) {
  if ((typeof obj === "object" && obj !== null) || typeof obj === "function") {
    __untransferable.add(obj);
  }
}
export function isMarkedAsUntransferable(obj) {
  return __untransferable.has(obj);
}

function __dataCloneErr(message) {
  // 10f 对拍：真 DataCloneError = DOMException（constructor.name/code 25/instanceof
  // Error 三面都要对，transfer-self 套件逐项断言）；消息为全量原文。
  throw new globalThis.DOMException(message, "DataCloneError");
}

// ── 9i-2 线信封 v2（仍是单 JSON 串，Rust 通道零改动）─────────────────────
// 自定义打包走 JSON（BigInt/undefined/Date/Map/Set/ArrayBuffer/视图/
// MessagePort 全保留；循环/共享引用保留同一性——容器先序 id，重访即 `ref`
// marker（M5 vitest 牵引：任务图天生带环；此前抛错/多份拷贝， wire 兼容：
// 无环消息形状逐字节不变）；函数/symbol/MarkAsUncloneable 物件任何位置出现
// 即 DataCloneError；transfer 标记内联 + 顶层信封带 nonce，接收侧 reviver 认领。
// ArrayBuffer/视图：transfer 即 detach（源归零，真机同款），否则拷贝字节；
// SAB 只拷贝（变普通 AB，记档）；分离中（detached）出现即 DataCloneError。
// MessagePort：仅 transfer 可投递（offer/accept 邀约槽 + 源 neutered 静默），
// 同会话与跨会话统一路径（跨会话经源表项转发器多一跳，对端无感知）。
// 函数/symbol/markAsUncloneable 物件任何位置出现即 DataCloneError。

let __wireSeq = 0;
const __VIEW_CTORS = {
  Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array,
  Int32Array, Uint32Array, Float32Array, Float64Array,
  BigInt64Array, BigUint64Array, DataView,
};

function __isSAB(v) {
  return typeof SharedArrayBuffer !== "undefined" && (v instanceof SharedArrayBuffer);
}
// 本引擎 structuredClone 不支持 BigInt（实测 DataCloneError），故可克隆性由
// 下方 walk 全权判定；此处仅列 walk 须显式拒绝的内置（structuredClone 口径）。
function __denyClone(v) {
  if (v instanceof Promise) return true;
  if (typeof WeakMap !== "undefined" && v instanceof WeakMap) return true;
  if (typeof WeakSet !== "undefined" && v instanceof WeakSet) return true;
  if (typeof WeakRef !== "undefined" && v instanceof WeakRef) return true;
  if (typeof FinalizationRegistry !== "undefined" && v instanceof FinalizationRegistry) return true;
  return false;
}
function __b64encode(u8) {
  return Buffer.from(u8.buffer, u8.byteOffset, u8.byteLength).toString("base64");
}
function __b64decode(b64) {
  return new Uint8Array(Buffer.from(String(b64), "base64"));
}
function __detachBuf(ab) {
  if (typeof ab.transfer === "function") ab.transfer();
  else structuredClone(ab, { transfer: [ab] });
}

function __normTransfer(transfer, srcPort) {
  if (transfer === undefined) return [];
  if (!Array.isArray(transfer)) return []; // 真机：非数组忽略（实测）
  const out = [];
  const seen = new Set();
  for (const t of transfer) {
    if (seen.has(t)) {
      // 10f 对拍：node 逐类型文案（transfer-duplicate 套件正则逐字）。
      __dataCloneErr(t instanceof MessagePort
        ? "Transfer list contains duplicate MessagePort"
        : `${__cloneLabel(t)} appears twice in the transfer list`);
    }
    seen.add(t);
    if (t instanceof MessagePort) {
      // 10f 对拍：源端口/已关闭端口两形 node 文案逐字（transfer-self/
      // transfer-closed 套件断 code 25 + constructor.name DOMException）。
      if (t === srcPort) __dataCloneErr("Transfer list contains source port");
      if (t.__neutered || t.__closed) {
        __dataCloneErr("MessagePort in transfer list is already detached");
      }
      out.push({ kind: "port", obj: t });
    } else if (ArrayBuffer.isView(t)) {
      const b = t.buffer;
      if (__isSAB(b)) __dataCloneErr("SharedArrayBuffer could not be cloned.");
      if (b.detached) __dataCloneErr("Detached buffer could not be cloned.");
      // 10f：池 AB 不可转移（Buffer.from 小串池化；真机 markAsUntransferable 口径）
      if (globalThis.__wjs2_bufPooled?.has(b)) __dataCloneErr("Pooled buffer could not be cloned.");
      out.push({ kind: "view", obj: t });
    } else if (__isSAB(t)) {
      __dataCloneErr("SharedArrayBuffer could not be cloned.");
    } else if (t instanceof ArrayBuffer) {
      // 10f：markAsUntransferable 标记的 AB 拒转移（不 detach——套件断
      // byteLength 不变；code 25/DataCloneError 由 __dataCloneErr 统一）。
      if (__untransferable.has(t)) __dataCloneErr("ArrayBuffer could not be cloned.");
      if (t.detached) __dataCloneErr("Detached buffer could not be cloned.");
      if (globalThis.__wjs2_bufPooled?.has(t)) __dataCloneErr("Pooled buffer could not be cloned.");
      out.push({ kind: "buf", obj: t });
    } else {
      // transfer-guards 套件（10f）：net.Socket/net.Server 在 transfer list 即
      // ERR_WORKER_HANDLE_NOT_TRANSFERABLE（node kTransferList 断言族；net 侧
      // 构造时经 globalThis.__wjs2_netXfer 登记，文案 errors.js 逐字）。
      const __xt = (globalThis.__wjs2_netXfer && typeof globalThis.__wjs2_netXfer.get === "function")
        ? globalThis.__wjs2_netXfer.get(t) : undefined;
      if (__xt !== undefined) {
        const e = new Error(`${__xt} cannot be transferred in its current state; it must be a freshly created or accepted handle that has not started reading and has no pending writes`);
        e.code = "ERR_WORKER_HANDLE_NOT_TRANSFERABLE";
        throw e;
      }
      __dataCloneErr(`${__cloneLabel(t)} could not be cloned.`);
    }
  }
  return out;
}
function __cloneLabel(t) {
  if (t === null) return "null";
  const tn = typeof t;
  if (tn === "object") {
    const n = t.constructor && t.constructor.name ? t.constructor.name : "Object";
    return `an instance of ${n}`;
  }
  return `type ${tn}`;
}

function __packValue(v, st) {
  const ti = (v !== null && (typeof v === "object" || typeof v === "function")) ? st.tmap.get(v) : undefined;
  if (ti !== undefined) {
    st.seenTransfer.add(ti.obj);
    if (ti.kind === "port") {
      const nonce = String(__wjs2_port_offer(ti.id));
      if (nonce === "") __dataCloneErr("MessagePort in transfer list is already detached");
      st.neuterPorts.push({ o: ti.obj, live: true });
      return { __wjs2_xfer: st.nonce, k: "port", id: nonce };
    }
    if (ti.kind === "buf") {
      const bytes = __b64encode(new Uint8Array(ti.obj));
      __detachBuf(ti.obj);
      return { __wjs2_xfer: st.nonce, k: "buf", b: bytes };
    }
    // view：整 underlying buffer 字节 + 偏移复原（transfer 即整块 detach）。
    const b = ti.obj.buffer;
    const bytes = __b64encode(new Uint8Array(b));
    __detachBuf(b);
    return { __wjs2_xfer: st.nonce, k: "view", t: ti.obj.constructor.name, b: bytes, o: ti.obj.byteOffset, n: ti.obj.byteLength };
  }
  if (typeof v === "function" || typeof v === "symbol") __dataCloneErr(`${String(v)} could not be cloned.`);
  if ((typeof v === "object" && v !== null) || typeof v === "function") {
    if (__uncloneable.has(v)) __dataCloneErr("object could not be cloned.");
  }
  if ((typeof v === "object" && v !== null) && __denyClone(v)) __dataCloneErr("object could not be cloned.");
  if (typeof v === "bigint") return { __wjs2_xfer: st.nonce, k: "big", v: String(v) };
  if (v === undefined) return { __wjs2_xfer: st.nonce, k: "undef" };
  if (v instanceof Date) return { __wjs2_xfer: st.nonce, k: "date", v: v.toISOString() };
  if (v instanceof Map) {
    // 容器先序 id：首访编号，祖先/共享重访即 `ref`（解码侧同序注册，恒后向引用）。
    const hit = st.path.get(v);
    if (hit !== undefined) return { __wjs2_xfer: st.nonce, k: "ref", id: hit };
    st.path.set(v, st.nextId++);
    return { __wjs2_xfer: st.nonce, k: "map", v: [...v].map(([k2, v2]) => [__packValue(k2, st), __packValue(v2, st)]) };
  }
  if (v instanceof Set) {
    const hit = st.path.get(v);
    if (hit !== undefined) return { __wjs2_xfer: st.nonce, k: "ref", id: hit };
    st.path.set(v, st.nextId++);
    return { __wjs2_xfer: st.nonce, k: "set", v: [...v].map((x) => __packValue(x, st)) };
  }
  if (v instanceof MessagePort) {
    // 裸端口 vs 容器内端口文案分形（node V8 序列化器口径，broadcastchannel
    // 套件 /Object that needs transfer was found/ 点名后者）。
    __dataCloneErr(st.depth > 0 ? "Object that needs transfer was found" : "MessagePort could not be cloned.");
  }
  if (typeof SharedArrayBuffer === "function" && v instanceof SharedArrayBuffer) {
    // 10f 对拍：node postMessage(SAB) = 品牌保真的**副本**（真共享内存需跨线程
    // 底座，记档）；无 SAB 全局时不可达（typeof 守卫，§4.58）。
    return { __wjs2_xfer: st.nonce, k: "sab", b: __b64encode(new Uint8Array(v)) };
  }
  if (v instanceof ArrayBuffer) {
    if (v.detached) __dataCloneErr("Detached buffer could not be cloned.");
    return { __wjs2_xfer: st.nonce, k: "buf", b: __b64encode(new Uint8Array(v)) };
  }
  if (ArrayBuffer.isView(v)) {
    const b = v.buffer;
    if (b.detached) __dataCloneErr("Detached buffer could not be cloned.");
    return { __wjs2_xfer: st.nonce, k: "view", t: v.constructor.name, b: __b64encode(new Uint8Array(b)), o: v.byteOffset, n: v.byteLength };
  }
  if (Array.isArray(v)) {
    const hit = st.path.get(v);
    if (hit !== undefined) return { __wjs2_xfer: st.nonce, k: "ref", id: hit };
    st.path.set(v, st.nextId++);
    return v.map((x) => __packValue(x, st));
  }
  if (v !== null && typeof v === "object") {
    const hit = st.path.get(v);
    if (hit !== undefined) return { __wjs2_xfer: st.nonce, k: "ref", id: hit };
    st.path.set(v, st.nextId++);
    const out = {};
    st.depth++;
    for (const k of Object.keys(v)) out[k] = __packValue(v[k], st);
    st.depth--;
    return out;
  }
  return v;
}

function __unpackValue(v, st) {
  if (Array.isArray(v)) {
    // 与打包侧同先序注册（先占位再填子项，祖先后向引用恒可解）。
    const a = [];
    st.refs[st.nextId++] = a;
    for (let i = 0; i < v.length; i++) a[i] = __unpackValue(v[i], st);
    return a;
  }
  if (v !== null && typeof v === "object") {
    if (v.__wjs2_xfer === st.nonce) {
      switch (v.k) {
        case "ref": return st.refs[v.id];
        case "big": return BigInt(v.v);
        case "undef": return undefined;
        case "date": return new Date(v.v);
        case "map": {
          const m = new Map();
          st.refs[st.nextId++] = m;
          for (const [k2, v2] of v.v) m.set(__unpackValue(k2, st), __unpackValue(v2, st));
          return m;
        }
        case "set": {
          const s = new Set();
          st.refs[st.nextId++] = s;
          for (const x of v.v) s.add(__unpackValue(x, st));
          return s;
        }
        case "buf": return __b64decode(v.b).buffer;
        case "sab": {
          // 10f 对拍：SharedArrayBuffer 品牌 roundtrip（副本语义，真共享记档）。
          const u8s = __b64decode(v.b);
          const sab = new SharedArrayBuffer(u8s.length);
          new Uint8Array(sab).set(u8s);
          return sab;
        }
        case "view": {
          const u8 = __b64decode(v.b);
          const Ctor = __VIEW_CTORS[v.t] || Uint8Array;
          const o = Number(v.o) || 0;
          // 10f 修：wire n 是 byteLength——typed array 第三参是**元素数**，
          // 按 BPE 折算（此前 Int32/Float64 系全 OOB，workerData 求值中断连带
          // 整模块 class 声明区 TDZ 级联）。
          const bpe = Ctor.BYTES_PER_ELEMENT || 1;
          const n = Number(v.n);
          if (Ctor === DataView) return new DataView(u8.buffer, o, Number.isFinite(n) ? n : undefined);
          return new Ctor(u8.buffer, o, Number.isFinite(n) ? n / bpe : undefined);
        }
        case "port": {
          const local = String(__wjs2_port_accept(String(v.id)));
          if (local === "") __dataCloneErr("MessagePort in transfer list is already detached");
          return new MessagePort(local);
        }
        default: break;
      }
    }
    const out = {};
    st.refs[st.nextId++] = out;
    for (const k of Object.keys(v)) out[k] = __unpackValue(v[k], st);
    return out;
  }
  return v;
}

function __toWire(value, transfer, srcPort) {
  const list = __normTransfer(transfer, srcPort);
  const st = { nonce: `w${++__wireSeq}x${Math.floor(Math.random() * 36 ** 6).toString(36)}`, tmap: new Map(), path: new Map(), nextId: 0, neuterPorts: [], seenTransfer: new Set(), depth: 0 };
  for (const t of list) {
    if (t.kind === "port") st.tmap.set(t.obj, { kind: "port", id: t.obj.__id, obj: t.obj });
    else st.tmap.set(t.obj, t);
  }
  // 可克隆性由 __packValue walk 全权判定（本引擎 structuredClone 不支持 BigInt，
  // 此处不再探路，见 __denyClone）。
  const tree = __packValue(value, st);
  // transfer 清单内但消息未引用者：照样生效（buffer detach、端口 neutered；
  // 端口 offer 后无承接者即 withdraw 回收槽，真机同款"转走即失效"）。
  for (const t of list) {
    if (st.seenTransfer.has(t.obj)) continue;
    if (t.kind === "port") {
      const nonce = String(__wjs2_port_offer(t.obj.__id));
      if (nonce !== "") {
        __wjs2_port_withdraw(nonce);
        st.neuterPorts.push({ o: t.obj, live: false });
      }
    } else if (t.kind === "buf") {
      __detachBuf(t.obj);
    } else if (t.kind === "view") {
      __detachBuf(t.obj.buffer);
    }
  }
  let json;
  try {
    // 顶层信封：nonce 随信封走（嵌套 marker 认领用；单 JSON 串，通道零改动）。
    json = JSON.stringify({ __wjs2_env: st.nonce, d: tree }) ?? "null";
  } catch {
    __dataCloneErr("object could not be cloned.");
  }
  for (const p of st.neuterPorts) {
    try { p.live ? p.o.__neuter() : p.o.__neuterDead(); } catch { /* 忽略 */ }
  }
  return json;
}

// 10f：worker 未捕获的**原始值**（throw 42 / "boom" / Symbol.for('a') 等）经
// Rust 捕获点打包 `__wjs2_prim:{json}` 信封（Error::Script kind=None 且非对象
// 异常时 message 即信封），error 事件按类还原（error-primitive 套件逐类型断
// 同一性；注册 Symbol 经 Symbol.for 还原即跨线程同一）。
function __wjs2_primFromText(json) {
  let o;
  try { o = JSON.parse(json); } catch { return undefined; }
  switch (o.t) {
    case "num": return Number(o.v);
    case "str": return String(o.v);
    case "bool": return !!o.v;
    case "big": return BigInt(o.v);
    case "nil": return null;
    case "undef": return undefined;
    case "sym": return Symbol.for(o.v);
    default: return undefined;
  }
}

function __fromWire(json) {
  let raw;
  try {
    raw = JSON.parse(String(json));
  } catch {
    const err = new Error("worker message is not valid JSON");
    err.name = "MessageError";
    throw err;
  }
  if (raw !== null && typeof raw === "object" && typeof raw.__wjs2_env === "string" && "d" in raw) {
    return __unpackValue(raw.d, { nonce: raw.__wjs2_env, refs: [], nextId: 0 });
  }
  // v1 载荷（本二进制内不产生；防御性直通，无 marker 可误认）。
  return raw;
}

export class MessagePort extends EventEmitter {
  constructor(__id) {
    super();
    if (typeof __id !== "string") {
      const err = new TypeError("MessagePort needs an internal port id");
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__id = __id;
    this.__closed = false;
    this.__neutered = false;
    this.__queue = [];
    this.__flushScheduled = false;
    this.__ev = this.__ev.bind(this);
    // paused 口径：无 message 监听时只排队不刷；监听到达即开闸（Node 同款）。
    // 注意 newListener 在监听入表*之前*触发，故延迟一轮 microtask 再刷。
    this.on("newListener", (ev) => { if (ev === "message") queueMicrotask(() => this.__maybeFlush()); });
    // 监听门控计数：有 message 监听才续命事件循环（worker 空转即退，Node 口径）。
    this.on("newListener", (ev) => { if (ev === "message") __wjs2_port_listen(__id); });
    this.on("removeListener", (ev) => {
      if (ev === "message" && this.listenerCount("message") === 0) __wjs2_port_unlisten(__id);
    });
    __wjs2_port_attach(__id, this);
  }
  // onmessage 兼容面（真机语义：赋值即隐式开始流动——经 addEventListener 走
  // newListener 开闸；回调收真 MessageEvent（data/target/ports，真机逐项
  // 实测），`message` EE 载荷裸值不变）。
  get onmessage() { return this.__onmessage; }
  set onmessage(fn) {
    if (this.__onmessage) this.removeListener("message", this.__onmessageWrap);
    this.__onmessage = (typeof fn === "function") ? fn : null;
    if (this.__onmessage) {
      this.__onmessageWrap = (value) => {
        this.__onmessage(new globalThis.MessageEvent("message", { data: value, __wjs2Target: this }));
      };
      this.on("message", this.__onmessageWrap);
    } else {
      this.__onmessageWrap = null;
    }
  }
  // EventTarget 双面（真机：'message'/'messageerror' 收 MessageEvent(data)，
  // 自定义类型经 emit 桥收 CustomEvent(detail)——message-port 套件逐项）。
  // 同 fn 跨类型各留各的包装（fn→Map(type→wrap)，remove 精确摘除）。
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
        ? new globalThis.MessageEvent(t, { data: value, __wjs2Target: this })
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
  __neuter() {
    // 迁移后源端：静默（真机同款 no-op），不摘对端。
    if (this.__neutered) return;
    this.__neutered = true;
    this.__queue.length = 0;
  }
  __neuterDead() {
    // 未引用转让（withdraw，无承接者）：静默 + 后续到达丢弃。
    this.__neuter();
    this.__dropDead = true;
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
      const raw = this.__queue.shift();
      let value;
      try {
        value = __fromWire(raw);
      } catch (e) {
        this.emit("messageerror", e instanceof Error ? e : new Error("worker message is not valid JSON"));
        continue;
      }
      this.emit("message", value);
    }
  }
  __ev(kind, payload) {
    if (kind === "forwarded") {
      // 迁移升级到达：源端排队消息经现转发路由排空后静默摘除。
      const pending = this.__queue.splice(0);
      for (const wire of pending) {
        try { __wjs2_port_post(this.__id, String(wire)); } catch { /* 丢弃 */ }
      }
      try { __wjs2_port_detach(this.__id); } catch { /* 忽略 */ }
      return;
    }
    if (kind !== "message") return;
    if (this.__closed || this.__dropDead) return;
    this.__queue.push(payload);
    this.__maybeFlush();
  }
  postMessage(value, transfer) {
    if (this.__closed || this.__neutered) return;
    const wire = __toWire(value, transfer, this);
    // 10f 对拍：本地 pair 经 Rust pending 表投递——`receiveMessageOnPort`
    // 同步可收（receive-message 套件），事件派发仍由 pump 逐轮驱动（node 的
    // task 级节奏；纯微任务链式 ping-pong 会饿死定时器，infinite-loop 实证）。
    __wjs2_port_post(this.__id, wire);
  }
  start() {}
  close() {
    if (this.__closed || this.__neutered) return;
    this.__closed = true;
    this.__queue.length = 0;
    __wjs2_port_close(this.__id);
    this.emit("close");
  }
  ref() {
    __wjs2_port_ref(this.__id);
    return this;
  }
  unref() {
    __wjs2_port_unref(this.__id);
    return this;
  }
  hasRef() {
    try {
      return Boolean(__wjs2_port_has_ref(this.__id));
    } catch {
      return true;
    }
  }
}

export function receiveMessageOnPort(port) {
  // 10f 对拍：BroadcastChannel 亦收（node BC 底层即 MessagePort，
  // broadcastchannel 套件 `receiveMessageOnPort(bc2)` 点名）。
  if (!(port instanceof MessagePort) && !(port instanceof BroadcastChannel)) {
    const err = new TypeError("The \"port\" argument must be a MessagePort instance");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // 10f：优先 Rust pending 表（postMessage 本地路由的同步收信口）；再退 JS
  // 队列（迁移/BC 排队/历史路径）。
  if (port instanceof MessagePort) {
    const wire = __wjs2_port_try_recv(port.__id);
    if (wire !== "") return { message: __fromWire(wire) };
  }
  if (port instanceof BroadcastChannel) {
    const wire = __wjs2_bc_try_recv(port.__sub);
    if (wire !== "") return { message: __fromWire(wire) };
  }
  if (port.__queue.length === 0) return undefined;
  return { message: __fromWire(port.__queue.shift()) };
}

export function moveMessagePortToContext(port, context) {
  if (!(port instanceof MessagePort)) {
    const err = new TypeError("The \"port\" argument must be an instance of MessagePort");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  void context;
  return port;
}

export class MessageChannel {
  constructor() {
    const [a, b] = String(__wjs2_port_pair()).split(" ");
    this.port1 = new MessagePort(a);
    this.port2 = new MessagePort(b);
  }
}
