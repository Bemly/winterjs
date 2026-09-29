//! http2 服务端域（监听选项 + h2_listen；对齐 http2.rs；纯搬移）。

use mozjs::jsval::JSVal;
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;
use super::http2::{TokioIo, ensure_provider, opt_num, serve_conn, set_rval_str};
use super::net::{NetEvent, NetKind};

/// 监听选项 JSON：`{tls?: {cert, key}}`（h2c 缺省）。
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ListenOpts {
    tls: Option<TlsServerOpts>,
}

#[derive(Debug, Default, serde::Deserialize, Clone)]
#[serde(default)]
struct TlsServerOpts {
    cert: Option<String>,
    key: Option<String>,
}

/// `__wjs2_h2_listen(port, host, optsJson, target)` → server id。
pub unsafe extern "C" fn h2_listen(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: h2 listen internals missing target");
        return false;
    }
    let Some(port) = opt_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: h2 listen: port must be a number");
        return false;
    };
    let host = value_to_string(&mut cx, frame.arg(1));
    let opts: ListenOpts =
        serde_json::from_str(&value_to_string(&mut cx, frame.arg(2))).unwrap_or_default();
    let target = frame.arg(3);
    ensure_provider();
    // TLS 配置预检（fail fast；h2c 无配置）
    let tls_cfg: Option<std::sync::Arc<rustls::ServerConfig>> = match opts.tls {
        None => None,
        Some(t) => {
            let (Some(cert), Some(key)) = (t.cert, t.key) else {
                report_error(&mut cx, "TypeError: http2 secure server needs { key, cert }");
                return false;
            };
            match crate::builtins::node::tls::server_config_h2(&cert, &key) {
                Ok(c) => Some(std::sync::Arc::new(c)),
                Err(e) => {
                    report_error(&mut cx, &e);
                    return false;
                }
            }
        }
    };
    let Some((id, ev_tx)) = state::net_alloc() else {
        report_error(&mut cx, "OperationError: net driver not installed");
        return false;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        report_error(&mut cx, "OperationError: no async runtime for h2 listen");
        return false;
    };
    let mut cmd_rx = state::net_socket_add(id, target);
    set_rval_str(&mut cx, &frame, &id.to_string());
    handle.spawn(async move {
        let bound = tokio::net::TcpListener::bind((host.as_str(), port as u16)).await;
        let Ok(listener) = bound else {
            let e = bound.unwrap_err();
            let code = crate::builtins::node::fs::io_code(&e);
            let _ = ev_tx.send(NetEvent {
                id,
                kind: NetKind::ServerError { code: code.into(), msg: format!("{code}: {e}") },
            });
            let _ = ev_tx.send(NetEvent { id, kind: NetKind::ServerClose });
            return;
        };
        let local = listener
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse::<std::net::SocketAddr>().expect("literal addr"));
        let _ = ev_tx.send(NetEvent {
            id,
            kind: NetKind::Listening { addr: local.ip().to_string(), port: local.port() },
        });
        // 存活连接表（server close 时逐个 Close；conn 退出经 done 通道摘除）
        let mut live: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let (done_tx, mut done_rx) =
            tokio::sync::mpsc::unbounded_channel::<u64>();
        let acceptor = tls_cfg.map(tokio_rustls::TlsAcceptor::from);
        loop {
            tokio::select! {
                acc = listener.accept() => {
                    let Ok((stream, peer)) = acc else { continue };
                    let (conn_id, conn_cmd_rx) = state::net_conn_add();
                    // conn 复用 server 的 target（connClose/aborted 事件在
                    // ServerClose purge 后仍可达；node：server.close 不杀活连接）
                    if let Some(t) = state::net_target(id) {
                        state::net_target_add(conn_id, t);
                    }
                    live.insert(conn_id);
                    let ev2 = ev_tx.clone();
                    let done2 = done_tx.clone();
                    if let Some(acc) = acceptor.clone() {
                        let fut = async move {
                            match acc.accept(stream).await {
                                Err(_) => {
                                    state::net_purge(conn_id);
                                    let _ = done2.send(conn_id);
                                }
                                Ok(tls) => {
                                    serve_conn(TokioIo::new(tls), id, conn_id, peer, ev2, conn_cmd_rx).await;
                                    let _ = done2.send(conn_id);
                                }
                            }
                        };
                        tokio::spawn(fut);
                    } else {
                        let fut = async move {
                            serve_conn(TokioIo::new(stream), id, conn_id, peer, ev2, conn_cmd_rx).await;
                            let _ = done2.send(conn_id);
                        };
                        tokio::spawn(fut);
                    }
                }
                done = done_rx.recv() => {
                    if let Some(cid) = done {
                        live.remove(&cid);
                    }
                }
                _ = cmd_rx.recv() => break,
            }
        }
        // node 默认：server.close() 不杀活连接，各 conn 由对端关闭后自行退出
        let _ = ev_tx.send(NetEvent { id, kind: NetKind::ServerClose });
    });
    true
}
