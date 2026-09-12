//! `node:worker_threads` 消息通道核（9f-2；Worker spawn 见 9f-3）。
//!
//! 落法：端口是跨会话统一路由的 Rust 侧对象——`postMessage` 经 JSON 线
//! （`JSON.stringify(structuredClone(v))`，JS 侧打包）发往对端会话的
//! `worker_rx` 收件箱，事件循环 `dispatch` 经预绑定的 `__ev` 回调
//! （新事件域 checklist §4.36：构造器内 bind；purge……端口无 purge，close 摘 target）。
//! 同会话 MessageChannel 回环同样走事件循环，故投递恒异步（真机同款）。
//! 存活：open + ref'd 端口计 `worker_open`（事件循环退出条件），`close()/unref()`
//! 减数——悬空端口 hang 进程是 Node 正确语义，测试必须 close。
//!
//! 偏差记档（9f-2）：
//! - 线口径 JSON：循环引用/BigInt/function/symbol 不可投递（clone 侧即拦，
//!   `DataCloneError` 具名错，无 DOMException 码）；transfer 列表接受忽略
//!   （端口不可转移，值一律拷贝）。
//! - 端口恒 started（`start()` 空操作；Node 默认 paused 到监听/显式 start）。
//! - `receiveMessageOnPort` 只取尚未 microtask 刷出的排队项（已派发即 `undefined`）。
//! - `moveMessagePortToContext` 恒返回端口本身（分发本就上下文无关）。
//! - `BroadcastChannel`/`postMessageToThread`/`threadName`/`markAsUntransferable`/
//!   locks 不导出；`Worker`/`parentPort` 非空形态 9f-3 接线。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{get_prop_value, report_error, value_to_string, wrap_cx, Frame};
use crate::state;

/// 会话收件箱事件（9f-3 加 Worker/Exit/Online 变体）。
#[derive(Debug)]
pub enum WorkerEvent {
    PortMsg { to: u64, json: String },
    PortClose { to: u64 },
}

/// 进程级环境数据（`setEnvironmentData` 跨线程共享；值走 JSON 线）。
static ENV_DATA: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn env_data() -> &'static Mutex<HashMap<String, String>> {
    ENV_DATA.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_str_args(
    cx: &mut JSContext,
    global: *mut JSObject,
    fun: JSVal,
    kind: &str,
    payload: &str,
) -> Option<JSVal> {
    use mozjs::conversions::ToJSValConvertible as _;
    rooted!(&in(cx) let mut a = UndefinedValue());
    rooted!(&in(cx) let mut b = UndefinedValue());
    kind.to_jsval(cx, a.handle_mut());
    payload.to_jsval(cx, b.handle_mut());
    crate::jsapi_glue::call_two(cx, global, fun, a.get(), b.get())
}

/// 事件分发（事件循环 `pump_once` 内调用；目标已摘即丢弃）。
pub fn dispatch(
    cx: &mut JSContext,
    global: *mut JSObject,
    ev: WorkerEvent,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), crate::error::Error> {
    let failed = |cx: &mut JSContext| match err {
        crate::runtime::ErrorSource::Script { source, filename } => {
            crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
        }
        crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
    };
    match ev {
        WorkerEvent::PortClose { to } => {
            state::port_peer_closed(to);
            Ok(())
        }
        WorkerEvent::PortMsg { to, json } => {
            let Some(target) = state::port_target(to) else {
                return Ok(()); // 关闭/摘除后到达即丢弃
            };
            if !target.is_object() {
                return Ok(());
            }
            rooted!(&in(cx) let t: *mut JSObject = target.to_object());
            let Some(fun) = get_prop_value(cx, t.get(), c"__ev") else {
                return Err(failed(cx));
            };
            if with_str_args(cx, global, fun, "message", &json).is_none() {
                return Err(failed(cx));
            }
            Ok(())
        }
    }
}

/// 数值 id 实参（字符串形态，§4.33 约定）。
fn arg_id(cx: &mut JSContext, frame: &Frame, i: u32) -> Option<u64> {
    if frame.argc() <= i {
        report_error(cx, "TypeError: worker port needs an id");
        return None;
    }
    value_to_string(cx, frame.arg(i)).parse::<u64>().ok().or_else(|| {
        report_error(cx, "TypeError: worker port id must be a port id string");
        None
    })
}

/// 建直连端口对。`__wjs_port_pair()` → `"a b"`（两端各计 1 存活）。
pub unsafe extern "C" fn port_pair(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    use mozjs::conversions::ToJSValConvertible as _;
    match state::port_pair() {
        Some((a, b)) => {
            format!("{a} {b}").to_jsval(&mut cx, frame.rval_mut());
            true
        }
        None => {
            report_error(&mut cx, "OperationError: worker channel is not initialized");
            false
        }
    }
}

/// 登记 JS 目标。`__wjs_port_attach(id, target)` → undefined。
pub unsafe extern "C" fn port_attach(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    if frame.argc() < 2 || !frame.arg(1).is_object() {
        report_error(&mut cx, "TypeError: worker port needs a target object");
        return false;
    }
    state::port_attach(id, frame.arg(1));
    frame.set_rval(UndefinedValue());
    true
}

/// 投递。`__wjs_port_post(id, json)` → boolean（对端已关即 false，不抛）。
pub unsafe extern "C" fn port_post(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: worker port post needs a message");
        return false;
    }
    let json = value_to_string(&mut cx, frame.arg(1));
    frame.set_rval(mozjs::jsval::BooleanValue(state::port_post(id, json)));
    true
}

/// 关闭。`__wjs_port_close(id)` → boolean（首次 true）。
pub unsafe extern "C" fn port_close(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    frame.set_rval(mozjs::jsval::BooleanValue(state::port_close(id)));
    true
}

/// 取消引用。`__wjs_port_unref(id)` → undefined。
pub unsafe extern "C" fn port_unref(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    state::port_unref(id);
    frame.set_rval(UndefinedValue());
    true
}

/// 重新引用。`__wjs_port_ref(id)` → undefined。
pub unsafe extern "C" fn port_ref(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    state::port_ref(id);
    frame.set_rval(UndefinedValue());
    true
}

/// 本线程是否主线程。`__wjs_worker_is_main()` → boolean。
pub unsafe extern "C" fn worker_is_main(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let _ = &mut cx;
    frame.set_rval(mozjs::jsval::BooleanValue(state::worker_is_main()));
    true
}

/// 本线程 id（数字字符串；主=0）。`__wjs_worker_thread_id()` → `"0"`。
pub unsafe extern "C" fn worker_thread_id(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    use mozjs::conversions::ToJSValConvertible as _;
    state::worker_thread_id().to_string().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 父端口信息（9f-2 主线程恒空串；9f-3 worker 线程给端口 id）。
/// `__wjs_worker_parent()` → `""`。
pub unsafe extern "C" fn worker_parent(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    use mozjs::conversions::ToJSValConvertible as _;
    "".to_jsval(&mut cx, frame.rval_mut());
    true
}

/// workerData（9f-2 主线程恒 undefined；9f-3 给 JSON 串）。
/// `__wjs_worker_data()` → undefined。
pub unsafe extern "C" fn worker_data(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let _ = &mut cx;
    frame.set_rval(UndefinedValue());
    true
}

/// 环境数据写。`__wjs_worker_env_set(key, json)` → undefined。
pub unsafe extern "C" fn env_set(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: setEnvironmentData needs a key and value");
        return false;
    }
    let key = value_to_string(&mut cx, frame.arg(0));
    let json = value_to_string(&mut cx, frame.arg(1));
    if let Ok(mut m) = env_data().lock() {
        m.insert(key, json);
    }
    frame.set_rval(UndefinedValue());
    true
}

/// 环境数据读。`__wjs_worker_env_get(key)` → json 串或 undefined。
pub unsafe extern "C" fn env_get(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: getEnvironmentData needs a key");
        return false;
    }
    let key = value_to_string(&mut cx, frame.arg(0));
    let got = env_data().lock().ok().and_then(|m| m.get(&key).cloned());
    match got {
        Some(json) => {
            use mozjs::conversions::ToJSValConvertible as _;
            json.to_jsval(&mut cx, frame.rval_mut());
        }
        None => frame.set_rval(UndefinedValue()),
    }
    true
}

/// 内嵌 ESM 源（`node:worker_threads`，9f-2：通道核，无 Worker）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";

const __uncloneable = new WeakSet();
export function markAsUncloneable(obj) {
  if ((typeof obj === "object" && obj !== null) || typeof obj === "function") {
    __uncloneable.add(obj);
  }
}

function __dataCloneErr(what) {
  const err = new Error(`${what} could not be cloned.`);
  err.name = "DataCloneError";
  throw err;
}
function __toWire(value) {
  if (typeof value === "function" || typeof value === "symbol") __dataCloneErr(String(value));
  if ((typeof value === "object" && value !== null) || typeof value === "function") {
    if (__uncloneable.has(value)) __dataCloneErr("This object");
  }
  let cloned;
  try {
    cloned = structuredClone(value);
  } catch {
    __dataCloneErr("This value");
  }
  try {
    return JSON.stringify(cloned) ?? "null";
  } catch {
    __dataCloneErr("This value");
  }
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
    this.__queue = [];
    this.__flushScheduled = false;
    this.__ev = this.__ev.bind(this);
    // paused 口径：无 message 监听时只排队不刷；监听到达即开闸（Node 同款）。
    // 注意 newListener 在监听入表*之前*触发，故延迟一轮 microtask 再刷。
    this.on("newListener", (ev) => { if (ev === "message") queueMicrotask(() => this.__maybeFlush()); });
    __wjs_port_attach(__id, this);
  }
  __maybeFlush() {
    if (this.__flushScheduled || this.__queue.length === 0) return;
    if (this.listenerCount("message") === 0) return;
    this.__flushScheduled = true;
    queueMicrotask(() => {
      this.__flushScheduled = false;
      while (this.__queue.length > 0) {
        const raw = this.__queue.shift();
        let value;
        try {
          value = JSON.parse(raw);
        } catch {
          this.emit("messageerror", new Error("worker message is not valid JSON"));
          continue;
        }
        this.emit("message", value);
      }
    });
  }
  __ev(kind, payload) {
    if (kind !== "message") return;
    if (this.__closed) return;
    this.__queue.push(payload);
    this.__maybeFlush();
  }
  postMessage(value, transfer) {
    void transfer;
    if (this.__closed) return;
    const wire = __toWire(value);
    __wjs_port_post(this.__id, wire);
  }
  start() {}
  close() {
    if (this.__closed) return;
    this.__closed = true;
    this.__queue.length = 0;
    __wjs_port_close(this.__id);
    this.emit("close");
  }
  ref() {
    __wjs_port_ref(this.__id);
    return this;
  }
  unref() {
    __wjs_port_unref(this.__id);
    return this;
  }
  hasRef() {
    return true;
  }
}

export function receiveMessageOnPort(port) {
  if (!(port instanceof MessagePort)) {
    const err = new TypeError("The \"port\" argument must be an instance of MessagePort");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (port.__queue.length === 0) return undefined;
  return { message: JSON.parse(port.__queue.shift()) };
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
    const [a, b] = String(__wjs_port_pair()).split(" ");
    this.port1 = new MessagePort(a);
    this.port2 = new MessagePort(b);
  }
}

export const isMainThread = __wjs_worker_is_main();
export const threadId = Number(__wjs_worker_thread_id());
const __parentId = String(__wjs_worker_parent());
export const parentPort = (__parentId === "") ? null : new MessagePort(__parentId);
const __dataRaw = __wjs_worker_data();
export const workerData = (__dataRaw === undefined) ? null : JSON.parse(String(__dataRaw));
export const resourceLimits = {};
export const SHARE_ENV = Symbol("SHARE_ENV");

export function setEnvironmentData(key, value) {
  if (typeof key !== "string") {
    const err = new TypeError(`The "key" argument must be of type string. Received type ${typeof key}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  __wjs_worker_env_set(key, __toWire(value === undefined ? null : value));
}
export function getEnvironmentData(key) {
  if (typeof key !== "string") {
    const err = new TypeError(`The "key" argument must be of type string. Received type ${typeof key}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const raw = __wjs_worker_env_get(key);
  if (raw === undefined) return undefined;
  return JSON.parse(String(raw));
}

const __api = {
  isMainThread, threadId, parentPort, workerData, resourceLimits, SHARE_ENV,
  MessagePort, MessageChannel, markAsUncloneable,
  moveMessagePortToContext, receiveMessageOnPort,
  setEnvironmentData, getEnvironmentData,
};
export default __api;
"#;
