//! 独立 serve 的 JS 桥（plan4 T1）：axum fallback → 通道 → JS 会话（`Request→Response`）。
//!
//! 分层铁律（plan4 §2）：Transport 只见 `http` 类型与字节，Bridge 只传纯数据帧，
//! `Request` 组装/`Response` 拆解只在 JS 线程（dispatch 内）发生。axum 线程
//! 永不碰 TLS state（§4.153-review）：响应通道随 `Head` 事件过界，JS 侧落表。
//!
//! 请求体复用 fetch 流机制（`fetch_streams` + `__wjs2_fetch_pull`，零新状态机）：
//! Head 分发即 `stream_add`，Chunk/End/Fail 经 `fetch::settle` 原样泵入，
//! JS 侧 `ReadableStream` 拉取。响应侧三 native（head/push/fail） Dram：
//! 未知 id 一律静默成功（过期响应：客户端已走或已终结，不报错）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use mozjs::context::JSContext;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::error::Error;
use crate::jsapi_glue::{call_one, get_prop_value, report_error, uint8_array, value_to_string, view_bytes, wrap_cx, Frame};
use crate::state;

/// 请求头（纯数据，可跨线程）。
#[derive(Debug)]
pub struct ServeReqHead {
    pub id: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// WS upgrade 尝试（中间件置位；普通请求 false，见 T4）。
    pub upgrade: bool,
}

/// 响应头（纯数据；`oneshot` 一次交付）。
#[derive(Debug, PartialEq)]
pub struct ServeRespHead {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

impl ServeRespHead {
    /// 500 短路头（handler 抛错/非法状态/客户端已走时的终态形状）。
    pub fn internal_error() -> Self {
        ServeRespHead { status: 500, headers: vec![] }
    }
}

/// 响应体帧（`mpsc` 流；Fail 即提前截断，axum 侧记 warn）。
#[derive(Debug, PartialEq)]
pub enum ServeBodyMsg {
    Chunk(Vec<u8>),
    End,
    Fail(String),
}

/// 响应通道（随 Head 事件过界，JS 线程落表；纯 Rust，无 JS 值）。
pub struct ServeRespTx {
    pub head_tx: Option<tokio::sync::oneshot::Sender<ServeRespHead>>,
    pub body_tx: tokio::sync::mpsc::UnboundedSender<ServeBodyMsg>,
    /// Upgrade 决策通道（中间件 await；普通请求 None，见 T4）。
    pub upgrade_tx: Option<tokio::sync::oneshot::Sender<ServeUpgrade>>,
}

/// Upgrade 决策（Accept 捆绑桥接两端，Decline 走普通管线）。
pub enum ServeUpgrade {
    Accept {
        ws_id: u64,
        ev_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsEvent>,
        out_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsOut>,
    },
    Decline,
}

/// axum 线程 → JS 会话事件（纯数据）。
pub enum ServeEvent {
    Head { head: ServeReqHead, resp: ServeRespTx },
    Chunk(u64, Vec<u8>),
    End(u64),
    Fail(u64, String),
    /// 停机唤醒（无副作用，只打断 park 使停机旗收敛；见 serve.rs 收尾）。
    Wake,
}

static SERVE_TX: OnceLock<Mutex<Option<tokio::sync::mpsc::UnboundedSender<ServeEvent>>>> =
    OnceLock::new();
static SERVE_IDS: AtomicU64 = AtomicU64::new(0);
static SERVE_SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn serve_tx_slot() -> &'static Mutex<Option<tokio::sync::mpsc::UnboundedSender<ServeEvent>>> {
    SERVE_TX.get_or_init(|| Mutex::new(None))
}

/// 发布会话收件箱（serve 会话起时一次；axum 线程经此投递）。
pub fn publish_serve_tx(tx: tokio::sync::mpsc::UnboundedSender<ServeEvent>) {
    *serve_tx_slot().lock().expect("serve tx slot") = Some(tx);
}

/// 撤下收件箱（会话收尾；后继投递即失败不堆积，rendezvous  parked 同哲学 §4.49）。
pub fn unpublish_serve_tx() {
    *serve_tx_slot().lock().expect("serve tx slot") = None;
}

/// axum 线程取投递端（None=会话未起或已收尾，请求直接 500/503 由调用方定）。
pub fn serve_tx_global() -> Option<tokio::sync::mpsc::UnboundedSender<ServeEvent>> {
    serve_tx_slot().lock().expect("serve tx slot").clone()
}

/// 请求 id（进程级单调不复用；axum 多线程并发安全）。
pub fn next_serve_id() -> u64 {
    SERVE_IDS.fetch_add(1, Ordering::SeqCst) + 1
}

/// 停机旗（主线程置位，serve_loop 见旗收尾，不等 idle）。
pub fn serve_shutdown() -> bool {
    SERVE_SHUTDOWN.load(Ordering::SeqCst)
}

pub fn set_serve_shutdown() {
    SERVE_SHUTDOWN.store(true, Ordering::SeqCst);
}

/// 响应状态合法性（200-599；沿 prelude `Response` 构造器口径，非法即 500 短路）。
pub fn valid_status(status: u16) -> bool {
    (200..600).contains(&status)
}

/// 响应头 JSON（`{status, headers:[[k,v]]}`；`__wjs2_serve_head` 实参形）。
#[derive(Debug, PartialEq)]
struct ServeHeadMeta {
    status: u16,
    headers: Vec<(String, String)>,
}

fn parse_head(meta: &str) -> Option<ServeHeadMeta> {
    let v: serde_json::Value = serde_json::from_str(meta).ok()?;
    let status = u16::try_from(v.get("status")?.as_u64()?).ok()?;
    let headers = v
        .get("headers")?
        .as_array()?
        .iter()
        .map(|p| {
            Some((
                p.get(0)?.as_str()?.to_owned(),
                p.get(1)?.as_str()?.to_owned(),
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ServeHeadMeta { status, headers })
}

/// 摘表（计数-1）+ 请求流 EOF 唤醒（resolve-null，无拒绝：handler 侧
/// for-await 正常结束，不造 unhandled）。返回摘到的响应通道。
fn serve_finish(
    cx: &mut JSContext,
    global: *mut JSObject,
    id: u64,
) -> Option<ServeRespTx> {
    let resp = state::serve_take(id)?;
    match state::stream_on_finish(id, None) {
        state::StreamPump::DoneAll(items) => {
            for (resolve, chunk) in items {
                match chunk {
                    Some(bytes) => resolve_chunk(cx, global, resolve, &bytes),
                    None => resolve_done(cx, global, resolve),
                }
            }
        }
        _ => {}
    }
    Some(resp)
}

/// resolve(chunkU8)（fetch.rs `resolve_chunk` 同款集中边界；前置 realm 内）。
fn resolve_chunk(cx: &mut JSContext, global: *mut JSObject, resolve: JSVal, bytes: &[u8]) {
    let Some(obj) = uint8_array(cx, bytes) else {
        report_error(cx, "RangeError: cannot allocate serve chunk");
        return;
    };
    rooted!(&in(cx) let chunk_v = mozjs::jsval::ObjectValue(obj));
    call_one(cx, global, resolve, chunk_v.get());
}

/// resolve(null)（流终结标记；前置同上）。
fn resolve_done(cx: &mut JSContext, global: *mut JSObject, resolve: JSVal) {
    rooted!(&in(cx) let mut null_v = UndefinedValue());
    mozjs::jsval::NullValue().to_jsval(cx, null_v.handle_mut());
    call_one(cx, global, resolve, null_v.get());
}

/// 事件循环结算（前置：cx 已进入 global 所属 realm；调用方 pump 内持 AutoRealm）。
pub fn dispatch(
    cx: &mut JSContext,
    global: *mut JSObject,
    ev: ServeEvent,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), Error> {
    use crate::builtins::fetch::{self, FetchMsg, StreamEvent, StreamKind};
    match ev {
        ServeEvent::Head { head, resp } => {
            state::serve_head(head.id, resp);
            // 请求体流占位（pull 随后即到；重复建幂等，见 `stream_add`）。
            state::stream_add(head.id);
            dispatch_head(cx, global, head, err)
        }
        ServeEvent::Chunk(id, bytes) => {
            fetch::settle(cx, global, FetchMsg::Stream(StreamEvent { id, kind: StreamKind::Chunk(bytes) }), err)
        }
        ServeEvent::End(id) => {
            fetch::settle(cx, global, FetchMsg::Stream(StreamEvent { id, kind: StreamKind::Done }), err)
        }
        ServeEvent::Fail(id, message) => {
            fetch::settle(
                cx,
                global,
                FetchMsg::Stream(StreamEvent { id, kind: StreamKind::Failed(message.clone()) }),
                err,
            )?;
            // 响应侧失败收尾：客户端已走或 axum 侧错——可达部分 500 短路，
            // 体侧 Fail（axum 记 warn 截断；请求流侧已在上行 reject）。
            if let Some(resp) = state::serve_take(id) {
                if let Some(tx) = resp.head_tx {
                    let _ = tx.send(ServeRespHead::internal_error());
                }
                let _ = resp.body_tx.send(ServeBodyMsg::Fail(message.clone()));
            }
            Ok(())
        }
        ServeEvent::Wake => Ok(()),
    }
}

/// Head 分发：调 prelude 驱动 `__wjs2_serve_on_head(id, metaJson, streamId)`。
/// 驱动内完成 Request 组装 + fetch 调用 + 响应排空（全异步，Rust 只 transport）。
fn dispatch_head(
    cx: &mut JSContext,
    global: *mut JSObject,
    head: ServeReqHead,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), Error> {
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    let global_ptr = global_root.get();
    let Some(driver) = get_prop_value(cx, global_ptr, c"__wjs2_serve_on_head") else {
        return Err(Error::Other("serve helper __wjs2_serve_on_head missing (prelude?)".into()));
    };
    let meta = serde_json::json!({
        "method": head.method,
        "url": head.url,
        "headers": head.headers,
        "upgrade": head.upgrade,
    })
    .to_string();
    rooted!(&in(cx) let mut meta_v = UndefinedValue());
    meta.to_jsval(cx, meta_v.handle_mut());
    rooted!(&in(cx) let mut id_v = UndefinedValue());
    (head.id as f64).to_jsval(cx, id_v.handle_mut());
    rooted!(&in(cx) let mut sid_v = UndefinedValue());
    (head.id as f64).to_jsval(cx, sid_v.handle_mut());
    // 三参驱动（id, metaJson, streamId；streamId 与请求 id 同号，见 plan4 §2）。
    let ok = crate::jsapi_glue::call_three(cx, global_ptr, driver, id_v.get(), meta_v.get(), sid_v.get()).is_some();
    if ok {
        return Ok(());
    }
    Err(match err {
        crate::runtime::ErrorSource::Script { source, filename } => {
            crate::jsapi_glue::pending_exception_error(cx, global_ptr, source, filename)
        }
        crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
    })
}

/// `__wjs2_serve_head(id, metaJson)`：投递响应头。幂等：未知 id（过期响应）
/// 静默成功；非法状态即 500 短路（含头+空体），调用方无需再推。
/// UNSAFE-BOUNDARY：引擎回调帧 + 会话 env；裸指针只在 realm 内解引用；
/// 覆盖测试：`tests/serve.rs::phase11_serve_dynamic_*`（head 非法/重复终结行）。
pub unsafe extern "C" fn serve_head(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper。
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_number() || !frame.arg(1).is_string() {
        report_error(&mut cx, "TypeError: serve head needs id and meta JSON");
        return false;
    }
    let id = frame.arg(0).to_number() as u64;
    let meta = value_to_string(&mut cx, frame.arg(1));
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    let global_ptr = global_root.get();
    let Some(head_tx) = state::serve_take_head(id) else {
        frame.set_rval(UndefinedValue());
        return true;
    };
    let head = match parse_head(&meta) {
        Some(m) if valid_status(m.status) => ServeRespHead { status: m.status, headers: m.headers },
        _ => {
            let _ = head_tx.send(ServeRespHead::internal_error());
            if let Some(resp) = serve_finish(&mut cx, global_ptr, id) {
                let _ = resp.body_tx.send(ServeBodyMsg::End);
            }
            frame.set_rval(UndefinedValue());
            return true;
        }
    };
    if head_tx.send(head).is_err() {
        // 客户端已走：收尾（计数-1 + 请求流 EOF 唤醒）。
        serve_finish(&mut cx, global_ptr, id);
    }
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs2_serve_push(id, chunkU8|null)`：推响应体 chunk；null 即终结并摘表。
/// 发送失败（接收端已走）即收尾；未知 id 静默成功（过期响应）。
/// UNSAFE-BOUNDARY：同上；覆盖测试同 `serve_head`。
pub unsafe extern "C" fn serve_push(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上。
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: serve push needs id and chunk");
        return false;
    }
    let id = frame.arg(0).to_number() as u64;
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    let global_ptr = global_root.get();
    let Some(body_tx) = state::serve_body_tx(id) else {
        frame.set_rval(UndefinedValue());
        return true;
    };
    if frame.arg(1).is_null_or_undefined() {
        let _ = body_tx.send(ServeBodyMsg::End);
        serve_finish(&mut cx, global_ptr, id);
        frame.set_rval(UndefinedValue());
        return true;
    }
    let Some(bytes) = view_bytes(&mut cx, frame.arg(1), "serve push chunk") else {
        return false;
    };
    if body_tx.send(ServeBodyMsg::Chunk(bytes)).is_err() {
        serve_finish(&mut cx, global_ptr, id);
    }
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs2_serve_fail(id, message)`：handler 失败 → 500 短路（头未发则发 500 头 +
/// 消息体，已发则截断体）。未知 id 静默成功。UNSAFE-BOUNDARY：同上。
pub unsafe extern "C" fn serve_fail(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上。
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: serve fail needs id and message");
        return false;
    }
    let id = frame.arg(0).to_number() as u64;
    let message = value_to_string(&mut cx, frame.arg(1));
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    let global_ptr = global_root.get();
    if let Some(resp) = serve_finish(&mut cx, global_ptr, id) {
        if let Some(tx) = resp.head_tx {
            let _ = tx.send(ServeRespHead::internal_error());
        }
        if !message.is_empty() {
            let _ = resp.body_tx.send(ServeBodyMsg::Chunk(message.into_bytes()));
        }
        let _ = resp.body_tx.send(ServeBodyMsg::End);
        // upgrade 挂起即决议 Decline（中间件不再空等 30s，直接走 500 短路）。
        if let Some(tx) = resp.upgrade_tx {
            let _ = tx.send(ServeUpgrade::Decline);
        }
    }
    // 工厂已挂靠而未配对的 socket 一并回收（计数归还）。
    if let Some((ws_id, _, _)) = serve_ws_take(id) {
        state::ws_remove(ws_id);
    }
    frame.set_rval(UndefinedValue());
    true
}

/// handler 模块求值 + 双认 fetch（`default.fetch` 优先，回落具名 `fetch`；
/// §0 四项-1）。缺其一即启动期可读错，不静默 500。落点为
/// `globalThis.__wjs2_serve_fetch`（global 本身是 GC 根，免新 slot、免 trace 改；
/// 用户覆盖即自担，`__wjs2_*` 内名前缀惯例）。
pub fn load_serve_handler(
    cx: &mut JSContext,
    global: *mut JSObject,
    url: &url::Url,
) -> Result<(), Error> {
    use mozjs::realm::AutoRealm;
    // SAFETY: global 为本会话 global（调用方 run_serve_session 的 rooted guard；
    // 裸指针回 realm 间无 JSAPI 调用，无 GC 间隙；jobqueue.rs:62 同款）。
    let mut realm = AutoRealm::new(&mut *cx, std::ptr::NonNull::new(global).expect("serve global"));
    let rcx: &mut JSContext = &mut realm;
    let ns = crate::modules::require_esm(rcx, url)?;
    if !ns.is_object() {
        return Err(Error::Other(format!(
            "serve handler '{}': namespace is not an object",
            url.as_str()
        )));
    }
    // §4.80 纪律：裸 JSVal 先入槽再做一切分配型调用。
    rooted!(&in(rcx) let ns_root: *mut JSObject = ns.to_object());
    let fetch = match get_prop_value(rcx, ns_root.get(), c"default") {
        Some(d) if d.is_object() => {
            rooted!(&in(rcx) let d_root: *mut JSObject = d.to_object());
            get_prop_value(rcx, d_root.get(), c"fetch")
        }
        _ => None,
    }
    .filter(|f| f.is_object())
    .or_else(|| get_prop_value(rcx, ns_root.get(), c"fetch").filter(|f| f.is_object()));
    let Some(fetch) = fetch else {
        return Err(Error::Other(format!(
            "serve handler '{}': must export fetch (default {{ fetch }} or named fetch)",
            url.as_str()
        )));
    };
    rooted!(&in(rcx) let fetch_root = fetch);
    if !crate::jsapi_glue::set_prop_value(rcx, global, c"__wjs2_serve_fetch", fetch_root.get()) {
        return Err(Error::Other("serve handler: cannot stash fetch fn".into()));
    }
    Ok(())
}

// ── T4 服务端 WS（upgrade 决策 + socket 挂靠；事件/发送/计数全复用 ws.rs）──

/// 待接管 socket（serve_id → 桥接端；factory 与 accept 之间过界）。
struct WsPending {
    ws_id: u64,
    ev_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsEvent>,
    out_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsOut>,
}

static SERVE_WS_PENDING: OnceLock<Mutex<HashMap<u64, WsPending>>> = OnceLock::new();

fn ws_pending_slot() -> &'static Mutex<HashMap<u64, WsPending>> {
    SERVE_WS_PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 挂靠待接管 socket（后到覆盖，先到者摘计数）。
pub fn serve_ws_stage(
    serve_id: u64,
    ws_id: u64,
    ev_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsEvent>,
    out_rx: tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsOut>,
) {
    let old = ws_pending_slot().lock().expect("serve ws pending").insert(
        serve_id,
        WsPending { ws_id, ev_tx, out_rx },
    );
    if let Some(old) = old {
        state::ws_remove(old.ws_id);
    }
}

/// 取走待接管（accept/decline/fail 收口；未知 id 回 None）。
pub fn serve_ws_take(
    serve_id: u64,
) -> Option<(
    u64,
    tokio::sync::mpsc::UnboundedSender<crate::builtins::ws::WsEvent>,
    tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsOut>,
)> {
    ws_pending_slot()
        .lock()
        .expect("serve ws pending")
        .remove(&serve_id)
        .map(|p| (p.ws_id, p.ev_tx, p.out_rx))
}

/// `__wjs2_serve_ws_create(serveId)` → wsId：分配 ws 表项 + 发送端并挂靠。
/// 工厂（`__wjs2_serve_socket`）调用；101 前未配对由 decline/fail 回收。
/// UNSAFE-BOUNDARY：引擎回调帧 + 会话 env；覆盖测试：`tests/serve.rs::phase11_serve_ws_echo`。
pub unsafe extern "C" fn serve_ws_create(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper。
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: serve socket needs a serve id");
        return false;
    }
    let serve_id = frame.arg(0).to_number() as u64;
    let Some((ws_id, ev_tx)) = state::ws_alloc() else {
        report_error(&mut cx, "OperationError: WebSocket driver not installed");
        return false;
    };
    let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<crate::builtins::ws::WsOut>();
    state::ws_add_sink(ws_id, out_tx);
    serve_ws_stage(serve_id, ws_id, ev_tx, out_rx);
    frame.set_rval(mozjs::jsval::Int32Value(ws_id as i32));
    true
}

/// `__wjs2_serve_ws_accept(serveId)`：handler 返回 socket 即接受升级 → 决策 Accept。
/// 无挂靠（未调工厂）即抛错走 500；会话已走即静默回收。
/// UNSAFE-BOUNDARY：同上；覆盖测试同 `serve_ws_create`。
pub unsafe extern "C" fn serve_ws_accept(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上。
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: serve accept needs a serve id");
        return false;
    }
    let serve_id = frame.arg(0).to_number() as u64;
    let Some((ws_id, ev_tx, out_rx)) = serve_ws_take(serve_id) else {
        report_error(&mut cx, "TypeError: serve upgrade without socket (need __wjs2_serve_socket + 101)");
        return false;
    };
    let Some(resp) = state::serve_take(serve_id) else {
        // 会话已走（超时/客户端消失）：静默回收，不报错。
        state::ws_remove(ws_id);
        frame.set_rval(UndefinedValue());
        return true;
    };
    let Some(tx) = resp.upgrade_tx else {
        state::ws_remove(ws_id);
        report_error(&mut cx, "TypeError: serve accept on a non-upgrade request");
        return false;
    };
    if tx.send(ServeUpgrade::Accept { ws_id, ev_tx, out_rx }).is_err() {
        state::ws_remove(ws_id);
    }
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs2_serve_ws_decline(serveId)`：返回 Response → 决策 Decline（走普通管线）+ 挂靠回收。
/// 未知 id/已决议一律静默成功（幂等）。
/// UNSAFE-BOUNDARY：同上；覆盖测试同 `serve_ws_create`。
pub unsafe extern "C" fn serve_ws_decline(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上。
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: serve decline needs a serve id");
        return false;
    }
    let serve_id = frame.arg(0).to_number() as u64;
    if let Some((ws_id, _, _)) = serve_ws_take(serve_id) {
        state::ws_remove(ws_id);
    }
    if let Some(tx) = state::serve_take_upgrade(serve_id) {
        let _ = tx.send(ServeUpgrade::Decline);
    }
    frame.set_rval(UndefinedValue());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn serve_status_gates() {
        assert!(valid_status(200));
        assert!(valid_status(599));
        assert!(!valid_status(199));
        assert!(!valid_status(600));
        assert!(!valid_status(0));
    }

    #[test]
    fn serve_head_json_roundtrip() {
        let m = parse_head(r#"{"status":201,"headers":[["x-a","b"],["c","d"]]}"#).unwrap();
        assert_eq!(m.status, 201);
        assert_eq!(m.headers, vec![("x-a".to_owned(), "b".to_owned()), ("c".to_owned(), "d".to_owned())]);
        assert!(parse_head(r#"{"status":"x","headers":[]}"#).is_none());
        assert!(parse_head(r#"{"status":200}"#).is_none());
        assert!(parse_head("nope").is_none());
        assert!(parse_head(r#"{"status":70000,"headers":[]}"#).is_none());
    }

    #[test]
    #[serial]
    fn serve_ids_monotonic_and_tx_publish() {
        let a = next_serve_id();
        let b = next_serve_id();
        assert!(b > a);
        assert!(serve_tx_global().is_none());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        publish_serve_tx(tx);
        assert!(serve_tx_global().is_some());
        unpublish_serve_tx();
        assert!(serve_tx_global().is_none());
    }
}
