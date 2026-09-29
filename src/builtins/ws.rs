//! `WebSocket` client（`tokio-tungstenite`，wss 经 webpki roots）。
//!
//! 并发模型同 fetch：native 建连 + 存 target/发送端；读/写任务跑在 tokio 上，
//! 事件经 channel 回事件循环 `dispatch`（调 `on*` 回调）。连接存活计入
//! `ws_open`，事件循环因此不早退；close/error 结算时减数并清理。
//! 偏差（文档记录）：只支持 `on*` 属性回调（无 EventTarget）；`binaryType` 只有
//! `arraybuffer` 生效（`blob` 照样给 ArrayBuffer）；`bufferedAmount` 恒 0。

use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use mozjs::context::JSContext;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::gc::ValueArray;
use mozjs::jsapi::{HandleValueArray, JS_CallFunctionValue, JSObject};
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;



use crate::error::Error;
use crate::jsapi_glue::{raw_handle, raw_handle_mut, report_error, uint8_array, value_to_string, view_bytes, wrap_cx, Frame};
use crate::state;

/// 读任务 → 事件循环（纯数据）。
pub struct WsEvent {
    pub id: u64,
    pub kind: WsKind,
}

pub enum WsKind {
    Opened { protocol: String },
    Text(String),
    Bin(Vec<u8>),
    Closed { code: u16, reason: String, clean: bool },
    Failed(String),
}

/// 事件循环/native → 写任务。
pub enum WsOut {
    Text(String),
    Bin(Vec<u8>),
    Close { code: u16, reason: String },
}

fn arg_string(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    Some(value_to_string(cx, frame.arg(i)))
}

/// `__wjs2_ws_connect(url, protocolsJson, target)` → id（存 target + 发送端 + 起任务）。
pub unsafe extern "C" fn ws_connect(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(url), Some(protos_json)) = (
        arg_string(&mut cx, &frame, 0, "WebSocket"),
        arg_string(&mut cx, &frame, 1, "WebSocket"),
    ) else {
        return false;
    };
    let parsed = match url::Url::parse(&url) {        Ok(u) if u.scheme() == "ws" || u.scheme() == "wss" => u,
        _ => {
            report_error(&mut cx, &format!("SyntaxError: bad WebSocket URL: {url}"));
            return false;
        }
    };
    let protocols: Vec<String> = serde_json::from_str(&protos_json).unwrap_or_default();
    let Some((id, tx)) = state::ws_alloc() else {
        report_error(&mut cx, "failed to load settings: WebSocket driver not installed");
        return false;
    };
    let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<WsOut>();
    state::ws_add_sink(id, out_tx);
    let handle = tokio::runtime::Handle::try_current();
    let Ok(handle) = handle else {
        state::ws_remove(id);
        report_error(&mut cx, "OperationError: no async runtime for WebSocket");
        return false;
    };
    let url_owned = parsed.to_string();
    let use_tls = parsed.scheme() == "wss";
    handle.spawn(async move {
        run_socket(id, &url_owned, protocols, tx, out_rx, use_tls).await;
    });
    frame.set_rval(mozjs::jsval::Int32Value(id as i32));
    true
}

/// 测试接缝（仅 `WINTERJS2_TEST_CA_PEMFILE` 置位时）：给自签 CA 用的 rustls 连接器。
/// 生产行为（未置位）保持 `connect_async` 默认（webpki roots），不受影响。
/// 前置：tokio 任务内（读文件用 `tokio::fs`）。
async fn test_connector() -> Option<tokio_tungstenite::Connector> {
    let path = std::env::var("WINTERJS2_TEST_CA_PEMFILE").ok()?;
    let pem = tokio::fs::read(&path).await.ok()?;
    let mut roots = rustls::RootCertStore::empty();
    // 系统根（`rustls-native-certs` 直引轮子）+ 自签 CA（`rustls-pemfile` 解析）。
    let native = rustls_native_certs::load_native_certs();
    let (added, _) = roots.add_parsable_certificates(native.certs);
    tracing::debug!(target: "winterjs2::ws", added, path, "test CA seam: native roots loaded");
    let mut cursor = std::io::Cursor::new(pem);
    let extra: Vec<_> = rustls_pemfile::certs(&mut cursor).collect::<Result<_, _>>().ok()?;
    roots.add_parsable_certificates(extra);
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    tracing::info!(target: "winterjs2::ws", path, "test CA seam active (wss self-signed)");
    Some(tokio_tungstenite::Connector::Rustls(std::sync::Arc::new(config)))
}

async fn run_socket(
    id: u64,
    url: &str,
    protocols: Vec<String>,
    tx: tokio::sync::mpsc::UnboundedSender<WsEvent>,
    mut out_rx: tokio::sync::mpsc::UnboundedReceiver<WsOut>,
    use_tls: bool,
) {
    let mut req = match url.into_client_request() {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(WsEvent { id, kind: WsKind::Failed(format!("websocket error: {e}")) });
            return;
        }
    };
    if !protocols.is_empty() {
        let joined = protocols.join(", ");
        if let Ok(v) = joined.parse() {
            req.headers_mut().insert("Sec-WebSocket-Protocol", v);
        }
    }
    let (stream, resp) = match tokio_tungstenite::connect_async_tls_with_config(
        req,
        None,
        false,
        // wss 且测试接缝置位才给自定义连接器；其余（ws/生产 wss）走默认。
        if use_tls { test_connector().await } else { None },
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(WsEvent { id, kind: WsKind::Failed(format!("websocket error: {e}")) });
            return;
        }
    };
    let protocol = resp
        .headers()
        .get("Sec-WebSocket-Protocol")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let _ = tx.send(WsEvent { id, kind: WsKind::Opened { protocol } });
    let (mut sink, mut stream) = stream.split();
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(t))) => {
                        let _ = tx.send(WsEvent { id, kind: WsKind::Text(t.to_string()) });
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(b))) => {
                        let _ = tx.send(WsEvent { id, kind: WsKind::Bin(b.to_vec()) });
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(frame))) => {
                        let (code, reason) = frame
                            .map(|f| (f.code.into(), f.reason.to_string()))
                            .unwrap_or((1005, String::new()));
                        let _ = tx.send(WsEvent { id, kind: WsKind::Closed { code, reason, clean: true } });
                        break;
                    }
                    Some(Ok(_)) => {} // Ping/Pong/Frame：tungstenite 自动回 pong
                    Some(Err(e)) => {
                        let _ = tx.send(WsEvent { id, kind: WsKind::Closed { code: 1006, reason: e.to_string(), clean: false } });
                        break;
                    }
                    None => {
                        let _ = tx.send(WsEvent { id, kind: WsKind::Closed { code: 1006, reason: String::new(), clean: false } });
                        break;
                    }
                }
            }
            out = out_rx.recv() => {
                match out {
                    Some(WsOut::Text(t)) => {
                        if sink.send(tokio_tungstenite::tungstenite::Message::Text(t.into())).await.is_err() {
                            let _ = tx.send(WsEvent { id, kind: WsKind::Closed { code: 1006, reason: String::new(), clean: false } });
                            break;
                        }
                    }
                    Some(WsOut::Bin(b)) => {
                        if sink.send(tokio_tungstenite::tungstenite::Message::Binary(b.into())).await.is_err() {
                            let _ = tx.send(WsEvent { id, kind: WsKind::Closed { code: 1006, reason: String::new(), clean: false } });
                            break;
                        }
                    }
                    Some(WsOut::Close { code, reason }) => {
                        let frame = (code != 1005).then(|| tokio_tungstenite::tungstenite::protocol::frame::CloseFrame {
                            code: code.into(),
                            reason: reason.clone().into(),
                        });
                        let _ = sink.send(tokio_tungstenite::tungstenite::Message::Close(frame)).await;
                        // 等对端回 close，上限 5s（对端已关则直接结算）
                        match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Close(frame)))) => {
                                let (code, reason) = frame
                                    .map(|f| (f.code.into(), f.reason.to_string()))
                                    .unwrap_or((code, reason));
                                let _ = tx.send(WsEvent { id, kind: WsKind::Closed { code, reason, clean: true } });
                            }
                            _ => {
                                let _ = tx.send(WsEvent { id, kind: WsKind::Closed { code, reason, clean: false } });
                            }
                        }
                        break;
                    }
                    None => break,
                }
            }
        }
    }
}

/// `__wjs2_ws_send(id, kind, payload)`：kind 0=文本，1=二进制。
pub unsafe extern "C" fn ws_send(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: WebSocket send needs id, kind and payload");
        return false;
    }
    let Some(id) = arg_u64(&mut cx, &frame, 0, "WebSocket send") else {
        return false;
    };
    let kind = frame.arg(1).to_number() as u32;
    let out = if kind == 0 {
        WsOut::Text(value_to_string(&mut cx, frame.arg(2)))
    } else {
        match view_bytes(&mut cx, frame.arg(2), "WebSocket data") {
            Some(b) => WsOut::Bin(b),
            None => return false,
        }
    };
    match state::ws_send(id, out) {
        true => {
            frame.set_rval(UndefinedValue());
            true
        }
        false => {
            report_error(&mut cx, "InvalidStateError: WebSocket is not open");
            false
        }
    }
}

/// `__wjs2_ws_close(id, code, reason)`：未知 id 静默成功（幂等 close）。
pub unsafe extern "C" fn ws_close(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 || !frame.arg(1).is_number() {
        report_error(&mut cx, "TypeError: WebSocket close needs id, code and reason");
        return false;
    }
    let Some(id) = arg_u64(&mut cx, &frame, 0, "WebSocket close") else {
        return false;
    };
    let code = frame.arg(1).to_number() as u16;
    let reason = value_to_string(&mut cx, frame.arg(2));
    state::ws_close(id, code, reason);
    frame.set_rval(UndefinedValue());
    true
}

/// 取数值实参（非数值即报 TypeError）。
fn arg_u64(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<u64> {
    if frame.argc() <= i || !frame.arg(i).is_number() {
        report_error(cx, &format!("TypeError: {what} needs a numeric id"));
        return None;
    }
    Some(frame.arg(i).to_number() as u64)
}

/// 事件循环分发一条 Ws 事件（经 prelude `__wjs2_ws_emit` 更新状态 + 调回调；
/// target 缺失也做 bookkeeping；失败清场）。
/// 前置条件：cx 已进入 global 所属 realm。
pub fn dispatch(
    cx: &mut JSContext,
    global: *mut JSObject,
    ev: WsEvent,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), Error> {
    let emit = state::with_rooted(|s| s.ws_emit_fn.get());
    if emit.is_undefined() {
        state::ws_remove(ev.id);
        return Ok(());
    }
    // kind 与 prelude `__wjs2_ws_emit` 对齐
    let (prop, kind, json, bin): (&std::ffi::CStr, &str, String, Option<Vec<u8>>) = match ev.kind {
        WsKind::Opened { protocol } => (
            c"onopen",
            "open",
            serde_json::json!({ "protocol": protocol }).to_string(),
            None,
        ),
        WsKind::Text(text) => (
            c"onmessage",
            "message-text",
            serde_json::json!({ "text": text }).to_string(),
            None,
        ),
        WsKind::Bin(bytes) => (c"onmessage", "message-bin", "{}".into(), Some(bytes)),
        WsKind::Closed { code, reason, clean } => (
            c"onclose",
            "close",
            serde_json::json!({ "code": code, "reason": reason, "clean": clean }).to_string(),
            None,
        ),
        WsKind::Failed(message) => {
            let json = serde_json::json!({ "message": message }).to_string();
            let ok_err = fire_emit(cx, global, emit, ev.id, c"onerror", "error", json, None);
            let ok_close = fire_emit(
                cx,
                global,
                emit,
                ev.id,
                c"onclose",
                "close",
                serde_json::json!({ "code": 1006u16, "reason": message, "clean": false }).to_string(),
                None,
            );
            state::ws_remove(ev.id);
            if ok_err && ok_close {
                return Ok(());
            }
            return Err(match err {
                crate::runtime::ErrorSource::Script { source, filename } => {
                    crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
                }
                crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
            });
        }
    };
    let terminal = kind == "close";
    let ok = fire_emit(cx, global, emit, ev.id, prop, kind, json, bin);
    if terminal {
        state::ws_remove(ev.id);
    }
    if ok {
        return Ok(());
    }
    Err(match err {
        crate::runtime::ErrorSource::Script { source, filename } => {
            crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
        }
        crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
    })
}

/// 调 `__wjs2_ws_emit(id, prop, kind, json, binU8)`（fire_due 的 rooted ValueArray 模式）。
fn fire_emit(
    cx: &mut JSContext,
    global: *mut JSObject,
    emit: JSVal,
    id: u64,
    prop: &std::ffi::CStr,
    kind: &str,
    json: String,
    bin: Option<Vec<u8>>,
) -> bool {
    rooted!(&in(cx) let mut id_v = UndefinedValue());
    (id as i32).to_jsval(cx, id_v.handle_mut());
    rooted!(&in(cx) let mut prop_v = UndefinedValue());
    prop.to_str().unwrap_or("").to_jsval(cx, prop_v.handle_mut());
    rooted!(&in(cx) let mut kind_v = UndefinedValue());
    kind.to_jsval(cx, kind_v.handle_mut());
    rooted!(&in(cx) let mut json_v = UndefinedValue());
    json.to_jsval(cx, json_v.handle_mut());
    let bin_v = match bin {
        Some(bytes) => match uint8_array(cx, &bytes) {
            Some(o) => mozjs::jsval::ObjectValue(o),
            None => return false,
        },
        None => UndefinedValue(),
    };
    rooted!(&in(cx) let bin_root = bin_v);
    rooted!(&in(cx) let fun_root = emit);
    rooted!(&in(cx) let argv =
        ValueArray::new([id_v.get(), prop_v.get(), kind_v.get(), json_v.get(), bin_root.get()]));
    rooted!(&in(cx) let mut rval = UndefinedValue());
    let args_array = HandleValueArray {
        length_: 5,
        // SAFETY: argv 为栈上 Rooted 槽，存活到调用返回，元素被 GC 追踪
        elements_: argv.as_ptr().cast(),
    };
    // SAFETY: cx/global/fun 均有效；rval 为 rooted 出参
    unsafe {
        JS_CallFunctionValue(
            cx.raw_cx(),
            raw_handle(&global),
            raw_handle(fun_root.as_ptr()),
            &args_array,
            raw_handle_mut(rval.as_ptr()),
        )
    }
}
