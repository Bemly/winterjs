//! serve H3 面（T3 QUIC 同端口；调用方保 endpoint 存活）。
//!
//! 本面全路径引用（axum/quinn/h3 系），无 use 块。


/// H3（plan4 T3）：同一 Router 经 QUIC/UDP 同端口服务（`h3-axum` example 形态）。
/// 调用方保证 endpoint 存活；返回即 accept 循环结束（endpoint.close 后）。
/// 优雅关闭由调用方 `endpoint.close()` 驱动，在飞连接随 QUIC 关闭而收尾。
pub(crate) async fn serve_h3(app: axum::Router, endpoint: quinn::Endpoint) {
    while let Some(incoming) = endpoint.accept().await {
        let app = app.clone();
        tokio::spawn(async move {
            let conn = match incoming.await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(target: "winterjs::serve", "QUIC handshake failed: {e}");
                    return;
                }
            };
            let peer = conn.remote_address();
            tracing::debug!(target: "winterjs::serve", %peer, "H3 QUIC connection");
            let h3_conn = match h3::server::builder().build(h3_quinn::Connection::new(conn)).await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(target: "winterjs::serve", %peer, "H3 handshake failed: {e}");
                    return;
                }
            };
            tracing::debug!(target: "winterjs::serve", %peer, "H3 connection established");
            tokio::pin!(h3_conn);
            loop {
                match h3_conn.accept().await {
                    Ok(Some(resolver)) => {
                        tracing::debug!(target: "winterjs::serve", %peer, "H3 request accepted");
                        let app = app.clone();
                        tokio::spawn(async move {
                            if let Err(e) = h3_axum::serve_h3_with_axum(app, resolver).await {
                                tracing::warn!(target: "winterjs::serve", %peer, "H3 request failed: {e}");
                            }
                        });
                    }
                    Ok(None) => break,
                    Err(e) => {
                        if h3_axum::is_graceful_h3_close(&e) {
                            tracing::debug!(target: "winterjs::serve", %peer, "H3 closed gracefully");
                        } else {
                            tracing::warn!(target: "winterjs::serve", %peer, "H3 connection error: {e:?}");
                        }
                        break;
                    }
                }
            }
        });
    }
}
