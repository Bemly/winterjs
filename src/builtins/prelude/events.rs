//! DOM 事件（Event 家族/EventTarget/AbortSignal-Controller）（prelude 分域；拼接顺序见 mod.rs）。
pub const EVENTS_JS: &str = r#"
// ---- M5: 全局 Event / EventTarget / CustomEvent（Node 平坦派发口径）----
// Node 的 EventTarget 不实现捕获/冒泡 propagation path（官方文档明言）：
// capture 选项仅为 removeEventListener 匹配保留；listener 收函数或 {handleEvent}。
// 事件状态走共享 WeakMap（Event 与 EventTarget 跨类要读写字段，# 私有够不着；
// 与既有 __wjs2_abortState 同风格，前缀避免污染全局面）。
const __wjs2_eventState = new WeakMap();
const __wjs2_etState = new WeakMap();
globalThis.Event = class Event {
  constructor(type, options = {}) {
    if (arguments.length === 0) throw new TypeError("Event requires at least 1 argument, but only 0 were passed");
    const o = options ?? {};
    __wjs2_eventState.set(this, {
      type: String(type),
      bubbles: !!o.bubbles,
      cancelable: !!o.cancelable,
      composed: !!o.composed,
      defaultPrevented: false,
      stopped: false,
      immediate: false,
      dispatching: false,
      timeStamp: Date.now(),
      target: null,
      currentTarget: null,
    });
  }
  get type() { return __wjs2_eventState.get(this).type; }
  get bubbles() { return __wjs2_eventState.get(this).bubbles; }
  get cancelable() { return __wjs2_eventState.get(this).cancelable; }
  get composed() { return __wjs2_eventState.get(this).composed; }
  get timeStamp() { return __wjs2_eventState.get(this).timeStamp; }
  get defaultPrevented() { return __wjs2_eventState.get(this).defaultPrevented; }
  get target() { return __wjs2_eventState.get(this).target; }
  get currentTarget() { return __wjs2_eventState.get(this).currentTarget; }
  get srcElement() { return __wjs2_eventState.get(this).target; }
  get isTrusted() { return false; }
  preventDefault() {
    const s = __wjs2_eventState.get(this);
    if (s.cancelable) s.defaultPrevented = true;
  }
  stopPropagation() { __wjs2_eventState.get(this).stopped = true; }
  stopImmediatePropagation() {
    const s = __wjs2_eventState.get(this);
    s.stopped = true;
    s.immediate = true;
  }
};
globalThis.CustomEvent = class CustomEvent extends Event {
  #detail;
  constructor(type, options = {}) {
    super(type, options);
    this.#detail = (options ?? {}).detail ?? null;
  }
  get detail() { return this.#detail; }
};
// undici webidl 口径的值回显（MessageEvent 校验文案；真机逐形实测）：
// instanceOf 消息 = `"` + inspect(v, {quotes:'double'}) + `"`（"str" 形串自带
// 双引号故现 `""str""`；数字/容器仅外包一对）；not-iterable 用裸 inspect。
// 覆盖套件点名的形状（标量/空容器/数组/类实例），完整 inspect 面在 util。
const __wjs2_insp = (v) => {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  const t = typeof v;
  if (t === "string") {
    const body = v.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
    return `"${body}"`;
  }
  if (t === "number" || t === "boolean" || t === "bigint") return String(v);
  if (t === "symbol") return v.toString();
  if (t === "function") return `[Function: ${v.name || "(anonymous)"}]`;
  if (Array.isArray(v)) return `[ ${v.map((x) => __wjs2_insp(x)).join(", ")} ]`;
  const n = v.constructor && v.constructor.name && v.constructor.name !== "Object" ? v.constructor.name : null;
  const keys = Object.keys(v);
  const body = keys.length === 0 ? "" : ` ${keys.map((k) => `${k}: ${__wjs2_insp(v[k])}`).join(", ")} `;
  return n ? `${n} {${body}}` : `{${body}}`;
};
const __wjs2_inspQuoted = (v) => `"${__wjs2_insp(v)}"`;
// Rust 侧取 symbol 描述（ToString 对 symbol 抛 TypeError；JS 侧 toString 合法）。
globalThis.__wjs2_symToString = (v) => (typeof v === "symbol") ? v.toString() : null;
// 10f：全局 MessageEvent（node 26 主/worker 线程均全局；message-port/
// message-event 套件逐项对拍）。source/ports 须 MessagePort 实例——品牌经
// worker 模块求值期登记的 `__wjs2_MessagePort` 隐藏槽判定（主线程无全局
// MessagePort；求值前无从有端口，非 null source 即 TypeError 正确）。
globalThis.MessageEvent = class MessageEvent extends Event {
  #data; #origin; #lastEventId; #source; #ports;
  constructor(type, init = {}) {
    if (arguments.length === 0) throw new TypeError("MessageEvent requires at least 1 argument, but only 0 were passed");
    super(type, init);
    const o = init ?? {};
    this.#data = o.data ?? null;
    this.#origin = String(o.origin ?? "");
    this.#lastEventId = String(o.lastEventId ?? "");
    const src = o.source ?? null;
    if (src !== null) {
      const M = globalThis.__wjs2_MessagePort;
      if (!M || !(src instanceof M)) {
        throw new TypeError(`MessageEvent constructor: Expected eventInitDict.source (${__wjs2_inspQuoted(src)}) to be an instance of MessagePort.`);
      }
    }
    this.#source = src;
    let ports = o.ports;
    if (ports !== undefined && ports !== null) {
      if (typeof ports[Symbol.iterator] !== "function") {
        throw new TypeError(`MessageEvent constructor: eventInitDict.ports (${__wjs2_insp(ports)}) is not iterable.`);
      }
      const list = [...ports];
      for (let i = 0; i < list.length; i++) {
        const M2 = globalThis.__wjs2_MessagePort;
        if (!M2 || !(list[i] instanceof M2)) {
          throw new TypeError(`MessageEvent constructor: Expected eventInitDict.ports[${i}] (${__wjs2_inspQuoted(list[i])}) to be an instance of MessagePort.`);
        }
      }
      this.#ports = list;
    } else {
      this.#ports = [];
    }
    // 内部派发目标（`__wjs2Target` 不属 WebIDL 字典面，仅宿主 port 桥使用）。
    const tgt = o.__wjs2Target;
    if (tgt) __wjs2_eventState.get(this).target = tgt;
  }
  get data() { return this.#data; }
  get origin() { return this.#origin; }
  get lastEventId() { return this.#lastEventId; }
  get source() { return this.#source; }
  get ports() { return this.#ports; }
};
// WebSocket CloseEvent（import-websocket 套件：node:http 重导出与全局同一性）。
// WebIDL 口径：code/reason/wasClean 只读，缺省 0/""/false。
globalThis.CloseEvent = class CloseEvent extends Event {
  #code; #reason; #wasClean;
  constructor(type, init = {}) {
    if (arguments.length === 0) throw new TypeError("CloseEvent requires at least 1 argument, but only 0 were passed");
    super(type, init);
    const o = init ?? {};
    this.#code = o.code ?? 0;
    this.#reason = String(o.reason ?? "");
    this.#wasClean = o.wasClean ?? false;
  }
  get code() { return this.#code; }
  get reason() { return this.#reason; }
  get wasClean() { return this.#wasClean; }
};
globalThis.EventTarget = class EventTarget {
  constructor() {
    __wjs2_etState.set(this, new Map());
  }
  addEventListener(type, listener, options = {}) {
    if (arguments.length < 2) throw new TypeError("addEventListener requires at least 2 arguments");
    if (typeof listener !== "function" && (typeof listener !== "object" || listener === null || typeof listener.handleEvent !== "function")) {
      throw new TypeError("addEventListener: listener must be a function or an object with handleEvent");
    }
    const o = typeof options === "boolean" ? { capture: options } : (options ?? {});
    if (o.signal?.aborted) return;
    // Proxy 目标无表决不抛（mustNotMutate 包裹的 signal 形 addEventListener；
    // 监听记代理身份下，触发侧 miss 即 benign——abort 竞速由 aborted 轮询门覆盖）。
    let st = __wjs2_etState.get(this);
    if (!st) { st = new Map(); __wjs2_etState.set(this, st); }
    const key = String(type);
    const list = st.get(key) ?? [];
    if (list.some((e) => e.listener === listener && e.capture === !!o.capture)) return;
    const entry = { listener, once: !!o.once, capture: !!o.capture, signal: o.signal ?? null, removed: false };
    list.push(entry);
    st.set(key, list);
    if (o.signal) o.signal.addEventListener("abort", () => this.removeEventListener(key, listener, options), { once: true });
  }
  removeEventListener(type, listener, options = {}) {
    const o = typeof options === "boolean" ? { capture: options } : (options ?? {});
    const st = __wjs2_etState.get(this);
    if (!st) return;
    const list = st.get(String(type));
    if (!list) return;
    const i = list.findIndex((e) => e.listener === listener && e.capture === !!o.capture && !e.removed);
    if (i >= 0) {
      list[i].removed = true;
      list.splice(i, 1);
    }
  }
  dispatchEvent(event) {
    if (!(event instanceof Event)) throw new TypeError("dispatchEvent requires an Event instance");
    const es = __wjs2_eventState.get(event);
    if (es.dispatching) throw new Error("InvalidStateError: event is already being dispatched");
    const st = __wjs2_etState.get(this);
    if (!st) throw new TypeError("dispatchEvent called on non-EventTarget");
    es.target = this;
    es.dispatching = true;
    const list = (st.get(es.type) ?? []).slice();
    try {
      for (const entry of list) {
        if (es.immediate || entry.removed) continue;
        if (entry.signal?.aborted) continue;
        if (entry.once) this.removeEventListener(es.type, entry.listener, { capture: entry.capture });
        es.currentTarget = this;
        if (typeof entry.listener === "function") {
          entry.listener.call(this, event);
        } else {
          entry.listener.handleEvent(event);
        }
      }
    } finally {
      es.dispatching = false;
      es.currentTarget = null;
    }
    return !(es.cancelable && es.defaultPrevented);
  }
};

"#;
