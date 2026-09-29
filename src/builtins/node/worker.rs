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
//! - 线口径 JSON 信封 v2（9i-2；M5 循环保留升级）：BigInt/undefined/Date/Map/Set/
//!   ArrayBuffer/视图（类型保留，字节拷贝）全保留；循环/共享引用保留同一性
//!   （先序 id + `ref` marker，真机口径；此前多份拷贝/循环抛错，9i-2 黑盒已同步
//!   升级）；函数/symbol/MarkAsUncloneable 物件仍 DataCloneError；SAB 只拷贝
//!   （变普通 AB，不共享）。
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

/// 建直连端口对。`__wjs2_port_pair()` → `"a b"`（两端各计 1 存活）。
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

/// 登记 JS 目标。`__wjs2_port_attach(id, target)` → undefined。
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

/// 摘目标。`__wjs2_port_detach(id)` → undefined（迁移排空后静默摘除）。
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

/// 投递。`__wjs2_port_post(id, json)` → boolean（对端已关即 false，不抛）。
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

/// 关闭。`__wjs2_port_close(id)` → boolean（首次 true）。
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

/// 取消引用。`__wjs2_port_unref(id)` → undefined。
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

/// 重新引用。`__wjs2_port_ref(id)` → undefined。
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

/// 邀约迁移。`__wjs2_port_offer(id)` → nonce 串（失败空串，JS 侧翻 DataCloneError）。
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

/// 承接迁移。`__wjs2_port_accept(nonce)` → 本地 id 串（失败空串）。
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

/// 撤邀约。`__wjs2_port_withdraw(nonce)` → boolean。
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

/// BC 订阅。`__wjs2_bc_sub(name)` → sub id 串（失败空串）。
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

/// BC 退订。`__wjs2_bc_unsub(subId)` → undefined。
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

/// BC 扇出。`__wjs2_bc_pub(name, exceptSubId, json)` → undefined。
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

/// BC 同步收信口（`receiveMessageOnPort` 对 BC 的底座；10f）。
/// `__wjs2_bc_try_recv(subId)` → 本会话 pending 队首 wire 串（空串 = 无）。
pub unsafe extern "C" fn bc_try_recv(
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
    match state::bc_try_recv(id) {
        Some(wire) => wire.to_jsval(&mut cx, frame.rval_mut()),
        None => String::new().to_jsval(&mut cx, frame.rval_mut()),
    }
    true
}

/// BC 旗变更。`__wjs2_bc_flags(subId, "listen"|"unlisten"|"ref"|"unref")` → undefined。
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

/// BC 登记 JS 目标。`__wjs2_bc_attach(subId, target)` → undefined。
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

/// 本线程是否主线程。`__wjs2_worker_is_main()` → boolean。
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

/// 本线程 id（数字字符串；主=0）。`__wjs2_worker_thread_id()` → `"0"`。
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

/// worker 构造名（`threadName` 导出；主会话空串 → JS 侧映射 null）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn worker_name(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    use mozjs::conversions::ToJSValConvertible as _;
    let n = if state::worker_is_main() {
        String::new()
    } else {
        state::worker_name().unwrap_or_default()
    };
    n.to_jsval(&mut cx, frame.rval_mut());
    true
}

/// fork 子进程标记（`process.send` 等 IPC 面不装 UNSUPPORTED 桩；10f）。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn worker_is_fork(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    use mozjs::conversions::ToJSValConvertible as _;
    let v = if state::worker_is_main() { false } else { state::worker_is_fork() };
    v.to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 父端口信息（主会话空串；worker 会话给 parentPort id）。
/// `__wjs2_worker_parent()` → `""` 或 id 串。
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
/// `__wjs2_worker_data()` → undefined 或 JSON 串。
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

/// worker env 快照读（`process_.rs` env 代理底座）。`__wjs2_worker_env_snapshot()`
/// → JSON 对象串（快照模式）或 undefined（主会话/SHARE_ENV——真 env 直读）。
pub unsafe extern "C" fn env_snapshot(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 port_pair
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let _ = &mut cx;
    match state::with_plain(|p| p.worker_env_json.clone()) {
        Some(json) => {
            use mozjs::conversions::ToJSValConvertible as _;
            json.to_jsval(&mut cx, frame.rval_mut());
        }
        None => frame.set_rval(UndefinedValue()),
    }
    true
}

/// 环境数据写。`__wjs2_worker_env_set(key, json)` → undefined。
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

/// 环境数据读。`__wjs2_worker_env_get(key)` → json 串或 undefined。
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

use super::worker_term::{term_request, worker_route};
pub use super::worker_term::{
    WorkerBoot, term_bind_cx, term_install, term_slot_register,
    term_tls_bind, term_tls_flagged, worker_boot_from_slot, worker_booted,
    worker_spawn,
};


/// 登记 Worker JS 目标。`__wjs2_worker_attach(workerId, target)` → undefined。
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

/// 主→worker 投递（parentPort 收）。`__wjs2_worker_post(workerId, json)` → boolean。
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
/// 10f：置共享旗 + 请求引擎中断（忙循环斩断，见 term_request）——idle 路径
/// 仍走收件箱 WTerminate 检查点。`__wjs2_worker_terminate(workerId)` → boolean。
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
    term_request(id);
    let ok = match state::worker_inbox(id) {
        Some(inbox) => inbox.send(WorkerEvent::WTerminate).is_ok(),
        None => false,
    };
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// worker ref/unref（主循环续命开关）。`__wjs2_worker_set_ref(id, "1"/"0")`。
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

/// worker 线程 id。`__wjs2_worker_tid(workerId)` → 数字串（已退出即空串）。
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

/// 监听装上。`__wjs2_port_listen(id)` → undefined。
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

/// 监听卸完。`__wjs2_port_unlisten(id)` → undefined。
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

/// 是否引用中。`__wjs2_port_has_ref(id)` → boolean。
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

/// 同步收信口（`receiveMessageOnPort` 底座；10f）。`__wjs2_port_try_recv(id)`
/// → 本端口 pending 表队首 wire 串（空串 = 无）。UNSAFE-BOUNDARY：纯 Rust
/// 队列进出，无 JS 值存留；覆盖测试 `tests/node/worker.rs` 同步收信。
pub unsafe extern "C" fn port_try_recv(
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
    match state::port_try_recv(id) {
        Some(wire) => wire.to_jsval(&mut cx, frame.rval_mut()),
        None => String::new().to_jsval(&mut cx, frame.rval_mut()),
    }
    true
}

/// 内嵌 ESM 源（`node:worker_threads`，9f-2：通道核，无 Worker）。

/// 内嵌 ESM 源（`node:worker_threads`；§0.9 按域分块：`worker_clone.js`
/// structuredClone 线信封 + `worker_ports.js`（BC/Worker/stdio/出口），concat 字节恒等）。
pub const SOURCE: &str = concat!(
    include_str!("worker_clone.js"),
    include_str!("worker_ports.js"),
);
