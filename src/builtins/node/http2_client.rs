//! http2 客户端域（连接选项 + h2_connect；对齐 http2.rs；纯搬移）。

use mozjs::jsval::JSVal;
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;
use super::http2::{BodyFeeds, BodyMsg, ChanBody, Exec, TokioIo, b64, b64dec, ensure_provider, h2_err_msg_rst, headers_json, read_body, report_error_static, opt_num, set_rval_str};
use super::net::{NetCmd, NetEvent, NetKind};
use std::collections::HashMap;
use std::sync::Arc;

// ── 客户端 ──────────────────────────────────────────────────────────────────

/// 连接选项 JSON：`{tls?: {ca?, rejectUnauthorized?, servername?}}`。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ConnectOpts {
    tls: Option<TlsClientOpts>,
}

#[derive(Debug, Default, serde::Deserialize, Clone)]
#[serde(default)]
struct TlsClientOpts {
    #[serde(rename = "ca")]
    ca_pem: Option<String>,
    #[serde(rename = "rejectUnauthorized")]
    reject_unauthorized: Option<bool>,
    servername: Option<String>,
}

/// 开流请求 JSON：`{method, path, scheme, authority, waitTrailers, headers: [[k,v]]}`。
#[derive(Debug, serde::Deserialize)]
struct OpenReq {
    method: String,
    #[serde(default)]
    path: String,
    #[serde(default = "default_scheme")]
    scheme: String,
    #[serde(default)]
    authority: String,
    #[serde(rename = "waitTrailers", default)]
    wait_trailers: bool,
    // 值可为 string 或 array（set-cookie 等多值头；数组展开为多条线，
    // node 客户端同口径）。此前 String 收不了 sequence 即
    // "bad open params: invalid type: sequence"（cookies/multiheaders 套件）。
    #[serde(default)]
    headers: Vec<Vec<serde_json::Value>>,
}

/// OpenReq 头展开：string 值单条，array 值逐元素展开。
fn openreq_headers(req: &OpenReq) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pair in &req.headers {
        let Some(name) = pair.first().and_then(|v| v.as_str()) else { continue };
        let values: Vec<String> = match pair.get(1) {
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            Some(serde_json::Value::Number(n)) => vec![n.to_string()],
            Some(serde_json::Value::Bool(b)) => vec![b.to_string()],
            // 客户端 cookie 数组已在 JS 侧 "; " 并串，其余数组展开为多条线
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    other => other.to_string(),
                })
                .collect(),
            _ => continue,
        };
        for v in values {
            out.push((name.to_ascii_lowercase(), v));
        }
    }
    out
}

fn default_scheme() -> String {
    "http".into()
}

/// 客户端 session 驱动（conn 与命令同 task `select!`；终结即 Close 事件）。
/// 10f：response 事件带真 flags（END_STREAM→5）、trailers 事件、session 终结时
/// 未完结流补 aborted（先于 close，§4.52 顺序）。
async fn drive_session<S>(
    io: TokioIo<S>,
    id: u64,
    ev_tx: tokio::sync::mpsc::UnboundedSender<NetEvent>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<NetCmd>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let hs = hyper::client::conn::http2::Builder::new(Exec).handshake::<_, ChanBody>(io);
    let (sender, conn) = match hs.await {
        Ok(t) => t,
        Err(e) => {
            let _ = ev_tx.send(NetEvent {
                id,
                kind: NetKind::Error {
                    code: "ERR_HTTP2_CONNECT".into(),
                    msg: format!("ERR_HTTP2_CONNECT: {e}"),
                },
            });
            if state::net_close_once(id) {
                let _ = ev_tx.send(NetEvent { id, kind: NetKind::H2SessionClose });
            }
            return;
        }
    };
    let _ = ev_tx.send(NetEvent { id, kind: NetKind::Connect { local: None } });
    // 开流登记：stream_id → 响应体是否已完结（session 死时未完结者报 aborted）。
    let open: Arc<std::sync::Mutex<HashMap<u64, bool>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    // 上传体通道（waitTrailers 悬置流；trailer 命令回注）。
    let uploads: BodyFeeds = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    // 在途请求任务句柄（H2RespondReset → abort → hyper RST(CANCEL)）
    let stream_tasks: Arc<std::sync::Mutex<HashMap<u64, tokio::task::AbortHandle>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    tokio::pin!(conn);
    loop {
        tokio::select! {
            r = &mut conn => {
                let _ = r;
                if state::net_close_once(id) {
                    for (stream_id, done) in open.lock().unwrap().iter() {
                        if !*done {
                            let _ = ev_tx.send(NetEvent {
                                id,
                                kind: NetKind::H2Stream {
                                    stream_id: *stream_id,
                                    what: "aborted".into(),
                                    payload: String::new(),
                                },
                            });
                        }
                    }
                    let _ = ev_tx.send(NetEvent { id, kind: NetKind::H2SessionClose });
                }
                return;
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(NetCmd::H2Open { stream_id, headers, body_b64 }) => {
                        let req: OpenReq = match serde_json::from_str(&headers) {
                            Ok(r) => r,
                            Err(e) => {
                                let _ = ev_tx.send(NetEvent {
                                    id,
                                    kind: NetKind::H2Stream {
                                        stream_id,
                                        what: "error".into(),
                                        payload: serde_json::json!({
                                            "code": "ERR_HTTP2_STREAM_ERROR",
                                            "msg": format!("bad open params: {e}"),
                                        })
                                        .to_string(),
                                    },
                                });
                                continue;
                            }
                        };
                        let body = b64dec(&body_b64).unwrap_or_default();
                        let parsed_headers = openreq_headers(&req);
                        // :authority 经完整 URI 带（hyper h2 客户端不合成伪头）；
                        // authority 为空回落 host 头（host-only 请求，host 回落套件）。
                        let authority = if req.authority.is_empty() {
                            parsed_headers
                                .iter()
                                .find(|(k, _)| k == "host")
                                .map(|(_, v)| v.clone())
                                .unwrap_or_default()
                        } else {
                            req.authority.clone()
                        };
                        let uri = format!("{}://{}{}", req.scheme, authority, req.path);
                        let mut builder = http::Request::builder()
                            .method(req.method.as_str())
                            .uri(uri.as_str());
                        for (k, v) in parsed_headers {
                            builder = builder.header(k.as_str(), v.as_str());
                        }
                        // 请求体经 ChanBody：空体 done=true → is_end_stream → END_STREAM
                        // 随头出线（GET 系即时开流）；非体先 Data 后 End；
                        // waitTrailers 悬置 EndPending 等 trailer 命令再闭流。
                        let (utx, urx) = tokio::sync::mpsc::unbounded_channel::<BodyMsg>();
                        // done=true 仅空体无 trailer（END_STREAM 随头）；有体即 false，
                        // 否则 poll_frame 首轮即 None，Data 永不到线。
                        let mut done = true;
                        if !body.is_empty() {
                            let _ = utx.send(BodyMsg::Data(bytes::Bytes::from(body)));
                            done = false;
                            if req.wait_trailers {
                                let _ = utx.send(BodyMsg::EndPending);
                            } else {
                                let _ = utx.send(BodyMsg::End(None));
                            }
                        } else if req.wait_trailers {
                            let _ = utx.send(BodyMsg::EndPending);
                            done = false;
                        }
                        if req.wait_trailers {
                            uploads.lock().await.insert(stream_id, utx);
                        }
                        let request = builder.body(ChanBody { rx: urx, done });
                        let request = match request {
                            Ok(r) => r,
                            Err(e) => {
                                let _ = ev_tx.send(NetEvent {
                                    id,
                                    kind: NetKind::H2Stream {
                                        stream_id,
                                        what: "error".into(),
                                        payload: serde_json::json!({
                                            "code": "ERR_HTTP2_STREAM_ERROR",
                                            "msg": format!("bad request: {e}"),
                                        })
                                        .to_string(),
                                    },
                                });
                                continue;
                            }
                        };
                        let ev2 = ev_tx.clone();
                        let open2 = open.clone();
                        open.lock().unwrap().insert(stream_id, false);
                        let mut sender = sender.clone();
                        let task = tokio::spawn(async move {
                            match sender.send_request(request).await {
                                Err(e) => {
                                    let (msg, rst) = h2_err_msg_rst(&e);
                                    let _ = ev2.send(NetEvent {
                                        id,
                                        kind: NetKind::H2Stream {
                                            stream_id,
                                            what: "error".into(),
                                            payload: serde_json::json!({
                                                "code": "ERR_HTTP2_STREAM_ERROR",
                                                "msg": msg,
                                                "rst": rst,
                                            })
                                            .to_string(),
                                        },
                                    });
                                }
                                Ok(resp) => {
                                    let status = resp.status().as_u16();
                                    let head = headers_json(resp.headers());
                                    // 先收完体再发 response 事件（通道序保证
                                    // response 先于 data/end，§4.35 不变）：
                                    // 空体且无 trailer ⇒ END_STREAM 随头出线，
                                    // flags = 4|1 = 5（head-request/204/304 套件
                                    // 断言 flags 5）；否则 4。hyper Incoming 无
                                    // END_STREAM 预判，只能事后推断（偏差记档）。
                                    let read = read_body(resp.into_body()).await;
                                    let (b, trailers, body_err) = match read {
                                        Ok((b, t)) => (b, t, None),
                                        Err((e, rst)) => (Vec::new(), Vec::new(), Some((e, rst))),
                                    };
                                    let flags = if body_err.is_none() && b.is_empty() && trailers.is_empty() { 5 } else { 4 };
                                    let _ = ev2.send(NetEvent {
                                        id,
                                        kind: NetKind::H2Stream {
                                            stream_id,
                                            what: "response".into(),
                                            payload: serde_json::json!({ "status": status, "flags": flags, "headers": head }).to_string(),
                                        },
                                    });
                                    match body_err {
                                        None => {
                                            if !b.is_empty() {
                                                let _ = ev2.send(NetEvent {
                                                    id,
                                                    kind: NetKind::H2Stream {
                                                        stream_id,
                                                        what: "data".into(),
                                                        payload: b64(&b),
                                                    },
                                                });
                                            }
                                            if !trailers.is_empty() {
                                                let _ = ev2.send(NetEvent {
                                                    id,
                                                    kind: NetKind::H2Stream {
                                                        stream_id,
                                                        what: "trailers".into(),
                                                        payload: serde_json::to_string(&trailers)
                                                            .unwrap_or_else(|_| "[]".into()),
                                                    },
                                                });
                                            }
                                            let _ = ev2.send(NetEvent {
                                                id,
                                                kind: NetKind::H2Stream {
                                                    stream_id,
                                                    what: "end".into(),
                                                    payload: String::new(),
                                                },
                                            });
                                            open2.lock().unwrap().insert(stream_id, true);
                                        }
                                        Some((e, rst)) => {
                                            // NO_ERROR 干净收尾按子串判，其余按流错误上报
                                            if e.contains("NO_ERROR") {
                                                let _ = ev2.send(NetEvent {
                                                    id,
                                                    kind: NetKind::H2Stream {
                                                        stream_id,
                                                        what: "end".into(),
                                                        payload: String::new(),
                                                    },
                                                });
                                                open2.lock().unwrap().insert(stream_id, true);
                                            } else {
                                                let _ = ev2.send(NetEvent {
                                                    id,
                                                    kind: NetKind::H2Stream {
                                                        stream_id,
                                                        what: "error".into(),
                                                        payload: serde_json::json!({
                                                            "code": "ERR_HTTP2_STREAM_ERROR",
                                                            "msg": e,
                                                            "rst": rst,
                                                        })
                                                        .to_string(),
                                                    },
                                                });
                                                open2.lock().unwrap().insert(stream_id, true);
                                            }
                                        }
                                    }
                                }
                            }
                        });
                        stream_tasks.lock().unwrap().insert(stream_id, task.abort_handle());
                    }
                    Some(NetCmd::H2RespondReset { stream_id, .. }) => {
                        // 客户端流取消：abort 在途请求任务 → hyper RST(CANCEL)
                        if let Some(h) = stream_tasks.lock().unwrap().remove(&stream_id) {
                            h.abort();
                        }
                        open.lock().unwrap().insert(stream_id, true);
                    }
                    Some(NetCmd::H2OpenTrailers { stream_id, trailers_json }) => {
                        let trailers: Vec<(String, String)> =
                            serde_json::from_str(&trailers_json).unwrap_or_default();
                        let tx = uploads.lock().await.remove(&stream_id);
                        if let Some(tx) = tx {
                            let _ = tx.send(BodyMsg::End(Some(trailers)));
                        }
                    }
                    Some(NetCmd::Close) | None => {
                        if state::net_close_once(id) {
                            for (stream_id, done) in open.lock().unwrap().iter() {
                                if !*done {
                                    let _ = ev_tx.send(NetEvent {
                                        id,
                                        kind: NetKind::H2Stream {
                                            stream_id: *stream_id,
                                            what: "aborted".into(),
                                            payload: String::new(),
                                        },
                                    });
                                }
                            }
                            let _ = ev_tx.send(NetEvent { id, kind: NetKind::H2SessionClose });
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// `__wjs2_h2_connect(host, port, optsJson, target)` → session id。
pub unsafe extern "C" fn h2_connect(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: h2 connect internals missing target");
        return false;
    }
    let host = value_to_string(&mut cx, frame.arg(0));
    let Some(port) = opt_num(&frame, 1) else {
        report_error(&mut cx, "TypeError: h2 connect: port must be a number");
        return false;
    };
    let opts: ConnectOpts =
        serde_json::from_str(&value_to_string(&mut cx, frame.arg(2))).unwrap_or_default();
    let target = frame.arg(3);
    ensure_provider();
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for h2 connect");
        return false;
    };
    let cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    handle.spawn(async move {
        let tcp = match tokio::net::TcpStream::connect((host.as_str(), port as u16)).await {
            Ok(s) => s,
            Err(e) => {
                let code = crate::builtins::node::fs::io_code(&e);
                let _ = ev_tx.send(NetEvent {
                    id,
                    kind: NetKind::Error { code: code.into(), msg: format!("{code}: {e}") },
                });
                if state::net_close_once(id) {
                    let _ = ev_tx.send(NetEvent { id, kind: NetKind::H2SessionClose });
                }
                return;
            }
        };
        if let Some(t) = opts.tls {
            let reject = t.reject_unauthorized.unwrap_or(true);
            let cfg = match crate::builtins::node::tls::client_config_h2(t.ca_pem.as_deref(), reject) {
                Ok(c) => c,
                Err(e) => {
                    report_error_static(&ev_tx, id, &e);
                    return;
                }
            };
            let servername = t.servername.unwrap_or_else(|| host.clone());
            let name = match rustls::pki_types::ServerName::try_from(servername.clone()) {
                Ok(n) => n,
                Err(e) => {
                    report_error_static(
                        &ev_tx,
                        id,
                        &format!("ERR_TLS_HANDSHAKE: bad servername '{servername}': {e}"),
                    );
                    return;
                }
            };
            let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(cfg));
            match connector.connect(name, tcp).await {
                Err(e) => {
                    report_error_static(&ev_tx, id, &format!("ERR_TLS_HANDSHAKE: {e}"));
                }
                Ok(tls) => drive_session(TokioIo::new(tls), id, ev_tx, cmd_rx).await,
            }
        } else {
            drive_session(TokioIo::new(tcp), id, ev_tx, cmd_rx).await;
        }
    });
    true
}
