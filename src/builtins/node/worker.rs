//! `node:worker_threads`（9f-2 通道核 + 9f-3 Worker）。
//!
//! 落法：端口是跨会话统一路由的 Rust 侧对象——`postMessage` 经 JSON 线
//! （`JSON.stringify(structuredClone(v))`，JS 侧打包）发往对端会话的
//! `worker_rx` 收件箱，事件循环 `dispatch` 经预绑定的 `__ev` 回调
//! （新事件域 checklist §4.36：构造器内 bind；purge……端口无 purge，close 摘 target）。
//! 同会话 MessageChannel 回环同样走事件循环，故投递恒异步（真机同款）。
//! 存活：`counted = open && refed && listening`（事件循环退出条件），
//! `close()/unref()/removeListener` 减数——悬空监听端口 hang 进程是 Node 正确语义。
//!
//! Worker（9f-3）：每 worker 独立 OS 线程 + 完整会话（§4.24 哲学；
//! `runtime::run_worker_thread`），经 `WorkerEvent::W*` 与主会话互通：
//! boot rendezvous（收件箱 + parentPort 就绪才返回）→ `WOnline` → 用户脚本 →
//! `WMsg`/`WError` → `WExit`。文件 worker 走文件管线；eval 串嗅探 ESM→落临时
//! `.mjs`（复用管线，相对导入以 tmpdir 为基），否则经典求值。
//! 退出码：排空 0/未捕获错 1/终止 1/`process.exit(n)`→n；权限继承 CLI 快照。
//!
//! 偏差记档：
//! - 线口径 JSON 信封 v2（9i-2）：BigInt/undefined/Date/Map/Set/ArrayBuffer/
//!   视图（类型保留，字节拷贝）全保留；循环引用/函数/symbol/MarkAsUncloneable
//!   物件/DataCloneError；共享引用变多份拷贝；SAB 只拷贝（变普通 AB，不共享）。
//! - transfer（9i-2）：ArrayBuffer/视图 transfer 即 detach（源归零）；MessagePort
//!   transfer 经邀约槽 + 源 neutered（后用静默）；同/跨会话统一路径（跨会话经源
//!   表项转发器多一跳）；transfer 非数组即忽略，重复/非法项 DataCloneError；
//!   分离中 buffer 出现即 DataCloneError；parentPort transfer 对端语义弱化记档。
//! - `BroadcastChannel`（9i-2）：同名跨会话扇出（发者自收排除，关者止收）；基座
//!   EventEmitter（真机 EventTarget）；`message` 载荷裸值，`onmessage` 收 `{data}`。
//! - workerData 缺省仍 `null`（真机 undefined；存量黑盒已钉）；显式 undefined 走
//!   信封回 undefined。env 数据同信封。
//! - 端口恒 started；`receiveMessageOnPort` 取未刷新的排队项；
//!   `moveMessagePortToContext` 恒返回自身。
//! - worker eval 的 completion 值照脚本语义打印（Node 不打印；`void 0` 收尾即可）。
//! - `terminate()` 只在事件循环检查点生效（同步死循环停不下来，与主线程同限）。
//! - stdin/stdout/stderr 恒 null；execArgv/resourceLimits/stdin/out/err 管道/
//!   trackUnmanagedFds/credentials 接受忽略；`postMessageToThread`/`threadName`/
//!   `markAsUntransferable`/locks 不导出；`stderr/stdout` 不管道。
//! - 无 `error` 监听的 worker 错误即 fatal（Node 同款 unhandled 'error'）。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{get_prop_value, report_error, value_to_string, wrap_cx, Frame};
use crate::state;

/// 会话收件箱事件（方向按收件会话定：`Port*` 给端口属主会话，
/// `W*` 给主会话的 Worker 对象；`WTerminate` 给 worker 会话）。
#[derive(Debug)]
pub enum WorkerEvent {
    PortMsg { to: u64, json: String },
    PortClose { to: u64 },
    /// 源→源会话：迁移承接方地址（`port_accept` 发；源表项升为转发器）。
    PortForward { to: u64, dest_tx: tokio::sync::mpsc::UnboundedSender<WorkerEvent>, dest_id: u64 },
    /// 承接→源会话：承接端已关（源转发器自拆）。
    PortDrop { to: u64 },
    /// 同名扇出（`bc_pub` 发；`to` 为归属会话内的本地 sub id）。
    BcMsg { to: u64, json: String },
    /// worker→主：parentPort 投递（`peer_is_worker` 端口由 `port_post` 改道至此）。
    WMsg { worker_id: u64, json: String },
    /// worker→主：未捕获错误（message 文案；随后必跟 `WExit{1}`）。
    WError { worker_id: u64, message: String },
    /// worker→主：线程结束（成功 0/错误 1/终止 1/`process.exit(n)`→n）。
    WExit { worker_id: u64, code: i32 },
    /// worker→主：会话就绪（boot 完成，用户脚本前）。
    WOnline { worker_id: u64 },
    /// 主→worker：终止（置 `worker_terminated` 旗，检查点退出）。
    WTerminate,
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
        WorkerEvent::PortForward { to, dest_tx, dest_id } => {
            state::port_forward_set(to, dest_tx, dest_id);
            // 升级前攒在源端的排队消息，经现转发路由排空（offer/accept 竞态不断流）。
            let target = state::port_target(to);
            if let Some(target) = target {
                if target.is_object() {
                    rooted!(&in(cx) let t: *mut JSObject = target.to_object());
                    if let Some(fun) = get_prop_value(cx, t.get(), c"__ev") {
                        if with_str_args(cx, global, fun, "forwarded", "").is_none() {
                            return Err(failed(cx));
                        }
                    }
                }
            }
            Ok(())
        }
        WorkerEvent::PortDrop { to } => {
            state::port_drop(to);
            Ok(())
        }
        WorkerEvent::BcMsg { to, json } => {
            let Some(target) = state::bc_target(to) else {
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
        WorkerEvent::WTerminate => {
            // 只应进 worker 会话收件箱；主会话收到即无视（绝不杀主循环）。
            if !state::worker_is_main() {
                state::worker_terminate_flag();
            }
            Ok(())
        }
        WorkerEvent::PortMsg { to, json } => {
            let Some(target) = state::port_target(to) else {
                // 摘 target 后到达的迟到消息：有转发路由即改道（`forwarded` 排空竞态）。
                if let Some((tx, dest)) = state::port_forward_route(to) {
                    let _ = tx.send(WorkerEvent::PortMsg { to: dest, json });
                }
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
        WorkerEvent::WMsg { worker_id, json } => {
            let Some(target) = state::worker_target(worker_id) else {
                return Ok(());
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
        WorkerEvent::WError { worker_id, message } => {
            let Some(target) = state::worker_target(worker_id) else {
                return Ok(());
            };
            if !target.is_object() {
                return Ok(());
            }
            rooted!(&in(cx) let t: *mut JSObject = target.to_object());
            let Some(fun) = get_prop_value(cx, t.get(), c"__ev") else {
                return Err(failed(cx));
            };
            if with_str_args(cx, global, fun, "error", &message).is_none() {
                return Err(failed(cx));
            }
            Ok(())
        }
        WorkerEvent::WOnline { worker_id } => {
            let Some(target) = state::worker_target(worker_id) else {
                return Ok(());
            };
            if !target.is_object() {
                return Ok(());
            }
            rooted!(&in(cx) let t: *mut JSObject = target.to_object());
            let Some(fun) = get_prop_value(cx, t.get(), c"__ev") else {
                return Err(failed(cx));
            };
            if with_str_args(cx, global, fun, "online", "").is_none() {
                return Err(failed(cx));
            }
            Ok(())
        }
        WorkerEvent::WExit { worker_id, code } => {
            if !state::worker_exited(worker_id) {
                return Ok(()); // 重复 Exit 即丢弃
            }
            if let Some(target) = state::worker_target(worker_id) {
                if target.is_object() {
                    rooted!(&in(cx) let t: *mut JSObject = target.to_object());
                    if let Some(fun) = get_prop_value(cx, t.get(), c"__ev") {
                        let code_s = code.to_string();
                        if with_str_args(cx, global, fun, "exit", &code_s).is_none() {
                            return Err(failed(cx));
                        }
                    }
                }
            }
            // purge 一律放派发之后（§4.36 checklist）。
            state::worker_target_remove(worker_id);
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

/// 摘目标。`__wjs_port_detach(id)` → undefined（迁移排空后静默摘除）。
pub unsafe extern "C" fn port_detach(
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
    state::port_detach(id);
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

/// 邀约迁移。`__wjs_port_offer(id)` → nonce 串（失败空串，JS 侧翻 DataCloneError）。
pub unsafe extern "C" fn port_offer(
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
    use mozjs::conversions::ToJSValConvertible as _;
    state::port_offer(id).unwrap_or_default().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 承接迁移。`__wjs_port_accept(nonce)` → 本地 id 串（失败空串）。
pub unsafe extern "C" fn port_accept(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: worker port accept needs a nonce");
        return false;
    }
    use mozjs::conversions::ToJSValConvertible as _;
    let nonce = value_to_string(&mut cx, frame.arg(0));
    state::port_accept(&nonce).map(|id| id.to_string()).unwrap_or_default().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 撤邀约。`__wjs_port_withdraw(nonce)` → boolean。
pub unsafe extern "C" fn port_withdraw(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: worker port withdraw needs a nonce");
        return false;
    }
    let nonce = value_to_string(&mut cx, frame.arg(0));
    frame.set_rval(mozjs::jsval::BooleanValue(state::port_withdraw(&nonce)));
    true
}

/// BC 订阅。`__wjs_bc_sub(name)` → sub id 串（失败空串）。
pub unsafe extern "C" fn bc_sub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: BroadcastChannel needs a name");
        return false;
    }
    use mozjs::conversions::ToJSValConvertible as _;
    let name = value_to_string(&mut cx, frame.arg(0));
    state::bc_sub(name).map(|id| id.to_string()).unwrap_or_default().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// BC 退订。`__wjs_bc_unsub(subId)` → undefined。
pub unsafe extern "C" fn bc_unsub(
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
    state::bc_unsub(id);
    frame.set_rval(UndefinedValue());
    true
}

/// BC 扇出。`__wjs_bc_pub(name, exceptSubId, json)` → undefined。
pub unsafe extern "C" fn bc_pub(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: BroadcastChannel publish needs a name, sender and message");
        return false;
    }
    let name = value_to_string(&mut cx, frame.arg(0));
    let except = value_to_string(&mut cx, frame.arg(1)).parse::<u64>().unwrap_or(u64::MAX);
    let json = value_to_string(&mut cx, frame.arg(2));
    state::bc_pub(&name, except, json);
    frame.set_rval(UndefinedValue());
    true
}

/// BC 旗变更。`__wjs_bc_flags(subId, "listen"|"unlisten"|"ref"|"unref")` → undefined。
pub unsafe extern "C" fn bc_flags(
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
        report_error(&mut cx, "TypeError: BroadcastChannel flags needs a flag");
        return false;
    }
    let what = value_to_string(&mut cx, frame.arg(1));
    state::bc_flags(id, &what);
    frame.set_rval(UndefinedValue());
    true
}

/// BC 登记 JS 目标。`__wjs_bc_attach(subId, target)` → undefined。
pub unsafe extern "C" fn bc_attach(
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
        report_error(&mut cx, "TypeError: BroadcastChannel needs a target object");
        return false;
    }
    state::bc_attach(id, frame.arg(1));
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

/// 父端口信息（主会话空串；worker 会话给 parentPort id）。
/// `__wjs_worker_parent()` → `""` 或 id 串。
pub unsafe extern "C" fn worker_parent(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    use mozjs::conversions::ToJSValConvertible as _;
    state::worker_parent_port().map(|id| id.to_string()).unwrap_or_default().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// workerData（主会话 undefined；worker 会话给克隆 JSON 串）。
/// `__wjs_worker_data()` → undefined 或 JSON 串。
pub unsafe extern "C" fn worker_data(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    match state::worker_data_json() {
        Some(json) => {
            use mozjs::conversions::ToJSValConvertible as _;
            json.to_jsval(&mut cx, frame.rval_mut());
        }
        None => frame.set_rval(UndefinedValue()),
    }
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

// ── Worker 线程（9f-3）───────────────────────────────────────────────────

/// worker 线程 id 分配（进程级；主=0，worker 自 1 单调）。
static NEXT_THREAD_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// worker 线程 boot 包（spawn 线程 move 进去；`runtime` 经槽 transito）。
pub struct WorkerBoot {
    pub worker_id: u64,
    pub thread_id: u64,
    pub data_json: Option<String>,
    pub main_inbox: tokio::sync::mpsc::UnboundedSender<WorkerEvent>,
    pub rendezvous: std::sync::mpsc::Sender<(
        tokio::sync::mpsc::UnboundedSender<WorkerEvent>,
        u64,
    )>,
}

/// boot 落地（`runtime::init_session` 内调；建 parentPort + 记主端点 + 权限继承）。
/// parentPort 对端地址是主会话的 worker_id（`port_post` 改道 `WMsg`）。
pub fn worker_boot_from_slot(boot: WorkerBoot) {
    let parent = state::port_alloc_cross(boot.worker_id, boot.main_inbox.clone());
    state::worker_boot(boot.thread_id, boot.data_json, parent);
    state::with_plain(|p| {
        p.worker_main_inbox = Some(boot.main_inbox);
        p.worker_rendezvous = Some(boot.rendezvous);
    });
}

/// boot 收尾（`init_session` 末调；主会话无操作）：回传 worker 收件箱 +
/// parentPort id（spawn 侧 rendezvous 等齐才返回），再发 `WOnline`。
pub fn worker_booted() {
    if state::worker_is_main() {
        return;
    }
    let (inbox, rendezvous, own_tx, parent) = state::with_plain(|p| {
        (
            p.worker_main_inbox.clone(),
            p.worker_rendezvous.take(),
            p.worker_tx.clone(),
            p.worker_parent_port,
        )
    });
    let (Some(inbox), Some(rendezvous), Some(own_tx), Some(parent)) = (inbox, rendezvous, own_tx, parent) else {
        return;
    };
    // worker_id 反查：parentPort 记录的 peer（alloc 时传入主会话的 worker_id）。
    let wid = state::with_rooted(|s| {
        s.worker_ports.iter().find(|pt| pt.id == parent).map(|pt| pt.peer)
    });
    let Some(wid) = wid else { return };
    let _ = rendezvous.send((own_tx, parent));
    let _ = inbox.send(WorkerEvent::WOnline { worker_id: wid });
}

/// 主→worker 投递寻址（句柄收件箱 + parentPort id）。
fn worker_route(worker_id: u64) -> Option<(tokio::sync::mpsc::UnboundedSender<WorkerEvent>, u64)> {
    let inbox = state::worker_inbox(worker_id)?;
    let parent = state::worker_parent_port_of(worker_id)?;
    Some((inbox, parent))
}

/// 起 worker。`__wjs_worker_spawn(src, evalFlag, dataJson)` → `"workerId threadId"`。
/// `src` 为文件路径（evalFlag=0）或源码（evalFlag=1）；dataJson 为空即无 workerData。
pub unsafe extern "C" fn worker_spawn(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: Worker needs a filename, eval flag and data");
        return false;
    }
    let src = value_to_string(&mut cx, frame.arg(0));
    let is_eval = value_to_string(&mut cx, frame.arg(1)) == "1";
    let data_raw = value_to_string(&mut cx, frame.arg(2));
    let data_json = if data_raw.is_empty() { None } else { Some(data_raw) };
    let main_inbox = match state::with_plain(|p| p.worker_tx.clone()) {
        Some(tx) => tx,
        None => {
            report_error(&mut cx, "OperationError: worker channel is not initialized");
            return false;
        }
    };
    let worker_id = state::with_plain(|p| {
        p.worker_next_id += 1;
        p.worker_next_id
    });
    let thread_id =
        NEXT_THREAD_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let (rendezvous_tx, rendezvous_rx) = std::sync::mpsc::channel();
    let boot = WorkerBoot {
        worker_id,
        thread_id,
        data_json,
        main_inbox: main_inbox.clone(),
        rendezvous: rendezvous_tx,
    };
    let spec = crate::runtime::WorkerThreadSpec::new(worker_id, src, is_eval, boot);
    crate::runtime::run_worker_thread(spec);
    // 等 worker 会话就绪（收件箱 + parentPort；超时即起线程失败）。
    match rendezvous_rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok((inbox_tx, parent_port)) => {
            state::worker_handle_add(state::WorkerHandle {
                worker_id,
                thread_id,
                inbox_tx,
                parent_port,
                counted: true,
                exited: false,
            });
            use mozjs::conversions::ToJSValConvertible as _;
            format!("{worker_id} {thread_id}").to_jsval(&mut cx, frame.rval_mut());
            true
        }
        Err(_) => {
            report_error(&mut cx, "OperationError: worker thread failed to start");
            false
        }
    }
}

/// 登记 Worker JS 目标。`__wjs_worker_attach(workerId, target)` → undefined。
pub unsafe extern "C" fn worker_attach(
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
        report_error(&mut cx, "TypeError: worker attach needs a target object");
        return false;
    }
    state::worker_target_add(id, frame.arg(1));
    frame.set_rval(UndefinedValue());
    true
}

/// 主→worker 投递（parentPort 收）。`__wjs_worker_post(workerId, json)` → boolean。
pub unsafe extern "C" fn worker_post(
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
        report_error(&mut cx, "TypeError: worker postMessage needs a message");
        return false;
    }
    let json = value_to_string(&mut cx, frame.arg(1));
    let ok = match worker_route(id) {
        Some((inbox, parent)) => inbox
            .send(WorkerEvent::PortMsg { to: parent, json })
            .is_ok(),
        None => false,
    };
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// 终止 worker（`WTerminate`；退出码经 `WExit` 事件回传，恒 1）。
/// `__wjs_worker_terminate(workerId)` → boolean（已退出即 false）。
pub unsafe extern "C" fn worker_terminate(
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
    let ok = match state::worker_inbox(id) {
        Some(inbox) => inbox.send(WorkerEvent::WTerminate).is_ok(),
        None => false,
    };
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// worker ref/unref（主循环续命开关）。`__wjs_worker_set_ref(id, "1"/"0")`。
pub unsafe extern "C" fn worker_set_ref(
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
    let refed = frame.argc() >= 2 && value_to_string(&mut cx, frame.arg(1)) == "1";
    state::worker_set_ref(id, refed);
    frame.set_rval(UndefinedValue());
    true
}

/// worker 线程 id。`__wjs_worker_tid(workerId)` → 数字串（已退出即空串）。
pub unsafe extern "C" fn worker_tid(
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
    use mozjs::conversions::ToJSValConvertible as _;
    state::worker_tid(id).map(|t| t.to_string()).unwrap_or_default().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 监听装上。`__wjs_port_listen(id)` → undefined。
pub unsafe extern "C" fn port_listen(
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
    state::port_listen(id);
    frame.set_rval(UndefinedValue());
    true
}

/// 监听卸完。`__wjs_port_unlisten(id)` → undefined。
pub unsafe extern "C" fn port_unlisten(
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
    state::port_unlisten(id);
    frame.set_rval(UndefinedValue());
    true
}

/// 是否引用中。`__wjs_port_has_ref(id)` → boolean。
pub unsafe extern "C" fn port_has_ref(
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
    frame.set_rval(mozjs::jsval::BooleanValue(state::port_has_ref(id)));
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

// ── 9i-2 线信封 v2（仍是单 JSON 串，Rust 通道零改动）─────────────────────
// 自定义打包走 JSON（BigInt/undefined/Date/Map/Set/ArrayBuffer/视图/
// MessagePort 全保留；循环引用/函数/symbol/不可克隆内置报 DataCloneError；
// 共享引用变多份拷贝，记档）。transfer 标记内联 + 顶层信封带 nonce，接收侧
// reviver 认领。
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

function __normTransfer(transfer) {
  if (transfer === undefined) return [];
  if (!Array.isArray(transfer)) return []; // 真机：非数组忽略（实测）
  const out = [];
  const seen = new Set();
  for (const t of transfer) {
    if (seen.has(t)) __dataCloneErr("Duplicate transferable");
    seen.add(t);
    if (t instanceof MessagePort) {
      if (t.__neutered || t.__closed) __dataCloneErr("Transferred port");
      out.push({ kind: "port", obj: t });
    } else if (ArrayBuffer.isView(t)) {
      const b = t.buffer;
      if (__isSAB(b)) __dataCloneErr("SharedArrayBuffer transfer");
      if (b.detached) __dataCloneErr("Detached buffer transfer");
      out.push({ kind: "view", obj: t });
    } else if (__isSAB(t)) {
      __dataCloneErr("SharedArrayBuffer transfer");
    } else if (t instanceof ArrayBuffer) {
      if (t.detached) __dataCloneErr("Detached buffer transfer");
      out.push({ kind: "buf", obj: t });
    } else {
      __dataCloneErr("Transfer item");
    }
  }
  return out;
}

function __packValue(v, st) {
  const ti = (v !== null && (typeof v === "object" || typeof v === "function")) ? st.tmap.get(v) : undefined;
  if (ti !== undefined) {
    st.seenTransfer.add(ti.obj);
    if (ti.kind === "port") {
      const nonce = String(__wjs_port_offer(ti.id));
      if (nonce === "") __dataCloneErr("Transferred port");
      st.neuterPorts.push({ o: ti.obj, live: true });
      return { __wjs_xfer: st.nonce, k: "port", id: nonce };
    }
    if (ti.kind === "buf") {
      const bytes = __b64encode(new Uint8Array(ti.obj));
      __detachBuf(ti.obj);
      return { __wjs_xfer: st.nonce, k: "buf", b: bytes };
    }
    // view：整 underlying buffer 字节 + 偏移复原（transfer 即整块 detach）。
    const b = ti.obj.buffer;
    const bytes = __b64encode(new Uint8Array(b));
    __detachBuf(b);
    return { __wjs_xfer: st.nonce, k: "view", t: ti.obj.constructor.name, b: bytes, o: ti.obj.byteOffset, n: ti.obj.byteLength };
  }
  if (typeof v === "function" || typeof v === "symbol") __dataCloneErr(String(v));
  if ((typeof v === "object" && v !== null) || typeof v === "function") {
    if (__uncloneable.has(v)) __dataCloneErr("This object");
  }
  if ((typeof v === "object" && v !== null) && __denyClone(v)) __dataCloneErr("This value");
  if (typeof v === "bigint") return { __wjs_xfer: st.nonce, k: "big", v: String(v) };
  if (v === undefined) return { __wjs_xfer: st.nonce, k: "undef" };
  if (v instanceof Date) return { __wjs_xfer: st.nonce, k: "date", v: v.toISOString() };
  if (v instanceof Map) {
    if (st.path.has(v)) __dataCloneErr("Circular value");
    st.path.add(v);
    try {
      return { __wjs_xfer: st.nonce, k: "map", v: [...v].map(([k2, v2]) => [__packValue(k2, st), __packValue(v2, st)]) };
    } finally {
      st.path.delete(v);
    }
  }
  if (v instanceof Set) {
    if (st.path.has(v)) __dataCloneErr("Circular value");
    st.path.add(v);
    try {
      return { __wjs_xfer: st.nonce, k: "set", v: [...v].map((x) => __packValue(x, st)) };
    } finally {
      st.path.delete(v);
    }
  }
  if (v instanceof MessagePort) __dataCloneErr("MessagePort without transfer");
  if (v instanceof ArrayBuffer) {
    if (v.detached) __dataCloneErr("Detached buffer");
    return { __wjs_xfer: st.nonce, k: "buf", b: __b64encode(new Uint8Array(v)) };
  }
  if (ArrayBuffer.isView(v)) {
    const b = v.buffer;
    if (b.detached) __dataCloneErr("Detached buffer");
    return { __wjs_xfer: st.nonce, k: "view", t: v.constructor.name, b: __b64encode(new Uint8Array(b)), o: v.byteOffset, n: v.byteLength };
  }
  if (Array.isArray(v)) {
    if (st.path.has(v)) __dataCloneErr("Circular value");
    st.path.add(v);
    try {
      return v.map((x) => __packValue(x, st));
    } finally {
      st.path.delete(v);
    }
  }
  if (v !== null && typeof v === "object") {
    if (st.path.has(v)) __dataCloneErr("Circular value");
    st.path.add(v);
    try {
      const out = {};
      for (const k of Object.keys(v)) out[k] = __packValue(v[k], st);
      return out;
    } finally {
      st.path.delete(v);
    }
  }
  return v;
}

function __unpackValue(v, st) {
  if (Array.isArray(v)) return v.map((x) => __unpackValue(x, st));
  if (v !== null && typeof v === "object") {
    if (v.__wjs_xfer === st.nonce) {
      switch (v.k) {
        case "big": return BigInt(v.v);
        case "undef": return undefined;
        case "date": return new Date(v.v);
        case "map": return new Map(v.v.map(([k2, v2]) => [__unpackValue(k2, st), __unpackValue(v2, st)]));
        case "set": return new Set(v.v.map((x) => __unpackValue(x, st)));
        case "buf": return __b64decode(v.b).buffer;
        case "view": {
          const u8 = __b64decode(v.b);
          const Ctor = __VIEW_CTORS[v.t] || Uint8Array;
          const o = Number(v.o) || 0;
          const n = Number(v.n);
          if (Ctor === DataView) return new DataView(u8.buffer, o, Number.isFinite(n) ? n : undefined);
          return new Ctor(u8.buffer, o, Number.isFinite(n) ? n : undefined);
        }
        case "port": {
          const local = String(__wjs_port_accept(String(v.id)));
          if (local === "") __dataCloneErr("Transferred port");
          return new MessagePort(local);
        }
        default: break;
      }
    }
    const out = {};
    for (const k of Object.keys(v)) out[k] = __unpackValue(v[k], st);
    return out;
  }
  return v;
}

function __toWire(value, transfer) {
  const list = __normTransfer(transfer);
  const st = { nonce: `w${++__wireSeq}x${Math.floor(Math.random() * 36 ** 6).toString(36)}`, tmap: new Map(), path: new Set(), neuterPorts: [], seenTransfer: new Set() };
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
      const nonce = String(__wjs_port_offer(t.obj.__id));
      if (nonce !== "") {
        __wjs_port_withdraw(nonce);
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
    json = JSON.stringify({ __wjs_env: st.nonce, d: tree }) ?? "null";
  } catch {
    __dataCloneErr("This value");
  }
  for (const p of st.neuterPorts) {
    try { p.live ? p.o.__neuter() : p.o.__neuterDead(); } catch { /* 忽略 */ }
  }
  return json;
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
  if (raw !== null && typeof raw === "object" && typeof raw.__wjs_env === "string" && "d" in raw) {
    return __unpackValue(raw.d, { nonce: raw.__wjs_env });
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
    this.on("newListener", (ev) => { if (ev === "message") __wjs_port_listen(__id); });
    this.on("removeListener", (ev) => {
      if (ev === "message" && this.listenerCount("message") === 0) __wjs_port_unlisten(__id);
    });
    __wjs_port_attach(__id, this);
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
    });
  }
  __ev(kind, payload) {
    if (kind === "forwarded") {
      // 迁移升级到达：源端排队消息经现转发路由排空后静默摘除。
      const pending = this.__queue.splice(0);
      for (const wire of pending) {
        try { __wjs_port_post(this.__id, String(wire)); } catch { /* 丢弃 */ }
      }
      try { __wjs_port_detach(this.__id); } catch { /* 忽略 */ }
      return;
    }
    if (kind !== "message") return;
    if (this.__closed || this.__dropDead) return;
    this.__queue.push(payload);
    this.__maybeFlush();
  }
  postMessage(value, transfer) {
    if (this.__closed || this.__neutered) return;
    const wire = __toWire(value, transfer);
    __wjs_port_post(this.__id, wire);
  }
  start() {}
  close() {
    if (this.__closed || this.__neutered) return;
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
    try {
      return Boolean(__wjs_port_has_ref(this.__id));
    } catch {
      return true;
    }
  }
}

export function receiveMessageOnPort(port) {
  if (!(port instanceof MessagePort)) {
    const err = new TypeError("The \"port\" argument must be an instance of MessagePort");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
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
    const [a, b] = String(__wjs_port_pair()).split(" ");
    this.port1 = new MessagePort(a);
    this.port2 = new MessagePort(b);
  }
}

// ── 9i-2 BroadcastChannel（同名跨会话扇出；发者自收排除，关者止收）────────
// 形态偏差记档：基座沿用 EventEmitter（与本仓 MessagePort 一致；真机为
// EventTarget，`String(bc)` 形状不同）；`message` 事件载荷为裸值（真机为
// MessageEvent），`onmessage` 回调收 `{ data }` 兼容形；无 `close` 事件。

export class BroadcastChannel extends EventEmitter {
  constructor(name) {
    super();
    if (arguments.length === 0 || name === undefined) {
      const err = new TypeError(`The "name" argument must be specified`);
      err.code = "ERR_MISSING_ARGS";
      throw err;
    }
    name = String(name);
    const sub = String(__wjs_bc_sub(name));
    if (sub === "") {
      throw new Error("OperationError: BroadcastChannel is not initialized");
    }
    this.name = name;
    this.__sub = sub;
    this.__closed = false;
    this.__onmessage = null;
    this.__ev = this.__ev.bind(this);
    this.on("newListener", (ev) => { if (ev === "message") __wjs_bc_flags(sub, "listen"); });
    this.on("removeListener", (ev) => {
      if (ev === "message" && this.listenerCount("message") === 0) __wjs_bc_flags(sub, "unlisten");
    });
    __wjs_bc_attach(sub, this);
  }
  get onmessage() { return this.__onmessage; }
  set onmessage(fn) {
    if (this.__onmessage !== null) this.removeListener("message", this.__onmessageWrap);
    this.__onmessage = (typeof fn === "function") ? fn : null;
    if (this.__onmessage !== null) {
      this.__onmessageWrap = (value) => { try { this.__onmessage({ data: value }); } catch { /* 忽略 */ } };
      this.on("message", this.__onmessageWrap);
    } else {
      this.__onmessageWrap = null;
    }
  }
  __ev(kind, payload) {
    if (kind !== "message") return;
    if (this.__closed) return;
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
    if (this.__closed) return;
    __wjs_bc_pub(this.name, this.__sub, __toWire(value));
  }
  close() {
    if (this.__closed) return;
    this.__closed = true;
    __wjs_bc_unsub(this.__sub);
  }
  ref() {
    __wjs_bc_flags(this.__sub, "ref");
    return this;
  }
  unref() {
    __wjs_bc_flags(this.__sub, "unref");
    return this;
  }
}

export const isMainThread = __wjs_worker_is_main();
export const threadId = Number(__wjs_worker_thread_id());
const __parentId = String(__wjs_worker_parent());
export const parentPort = (__parentId === "") ? null : new MessagePort(__parentId);
const __dataRaw = __wjs_worker_data();
export const workerData = (__dataRaw === undefined || __dataRaw === "") ? null : __fromWire(String(__dataRaw));
export const resourceLimits = {};
export const SHARE_ENV = Symbol("SHARE_ENV");

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

function __workerFilePath(filename) {
  if (typeof filename === "string") return { src: filename, isEval: "0" };
  if (typeof filename === "object" && filename !== null && typeof filename.href === "string") {
    const href = String(filename.href);
    if (href.startsWith("file://")) {
      try {
        return { src: decodeURIComponent(new URL(href).pathname), isEval: "0" };
      } catch { /* 落下走报错 */ }
    }
  }
  const err = new TypeError(`The "filename" argument must be of type string or an instance of URL.`);
  err.code = "ERR_INVALID_ARG_TYPE";
  throw err;
}

export class Worker extends EventEmitter {
  constructor(filename, options = {}) {
    super();
    if (options === null || (typeof options !== "object" && typeof options !== "function")) {
      const err = new TypeError(`The "options" argument must be of type object.`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    const { src, isEval } = options.eval
      ? { src: String(filename), isEval: "1" }
      : __workerFilePath(filename);
    let dataJson = "";
    if (options.workerData !== undefined) {
      dataJson = __toWire(options.workerData, options.transferList);
    }
    let ids;
    ids = String(__wjs_worker_spawn(src, isEval, dataJson)).split(" ");
    this.__id = ids[0];
    this.__tid = Number(ids[1]);
    this.__exited = null;
    this.__termWaiters = [];
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
      this.emit("error", new Error(String(payload)));
    } else if (kind === "exit") {
      const code = Number(payload);
      this.__exited = code;
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
  get threadId() { return this.__tid; }
  get resourceLimits() { return {}; }
  get stdin() { return null; }
  get stdout() { return null; }
  get stderr() { return null; }
}

const __api = {
  isMainThread, threadId, parentPort, workerData, resourceLimits, SHARE_ENV,
  MessagePort, MessageChannel, BroadcastChannel, Worker, markAsUncloneable,
  moveMessagePortToContext, receiveMessageOnPort,
  setEnvironmentData, getEnvironmentData,
};
export default __api;
"#;
