//! runtime/serve_session：独立 serve JS 会话（常驻循环 + handler 加载）。

use super::*;
use mozjs::jsapi::JSObject;
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::RootedGuard;
use mozjs::rust::Runtime;
use crate::error::Error;

/// serve 会话循环（plan4 T1）：`pump_once` 复用既有通道结算，serve 通道单独顶排空；
/// 只认停机旗/错误退出，不认 idle（服务器 park；常驻计数见 `serve_hold_server`）。
/// 与 `event_loop` 的 select 分支重复是刻意：run/repl 的收敛语义 load-bearing，
/// 不碰（成功/报错双路径零回归，§4.22）。
async fn serve_loop(
    rt: &mut Runtime,
    global: &RootedGuard<'_, *mut JSObject>,
    err: ErrorSource<'_>,
    fetch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::fetch::FetchMsg>,
    ws_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::ws::WsEvent>,
    watch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::fs::WatchEvent>,
    child_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::child::ChildEvent>,
    net_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::net::NetEvent>,
    worker_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::worker::WorkerEvent>,
    quic_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::quic::QuicEvent>,
    napi_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::napi::asyncwork::NapiEvent>,
    dispatch_rx: &mut tokio::sync::mpsc::UnboundedReceiver<usize>,
    serve_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::serve_bridge::ServeEvent>,
) -> Result<(), Error> {
    use crate::builtins::{fetch, node::child as node_child, node::fs as node_fs, node::net as node_net, node::quic as node_quic, node::worker as node_worker, ws};
    use crate::napi::asyncwork as napi_aw;
    macro_rules! settle_fetch {
        ($msg:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            fetch::settle(&mut realm, global.get(), $msg, err)?;
        }};
    }
    macro_rules! settle_ws {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            ws::dispatch(&mut realm, global.get(), $ev, err)?;
        }};
    }
    macro_rules! settle_watch {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            node_fs::dispatch(&mut realm, global.get(), $ev, err)?;
        }};
    }
    macro_rules! settle_child {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            node_child::dispatch(&mut realm, global.get(), $ev, err)?;
        }};
    }
    macro_rules! settle_net {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            node_net::dispatch(&mut realm, global.get(), $ev, err)?;
        }};
    }
    macro_rules! settle_worker {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            node_worker::dispatch(&mut realm, global.get(), $ev, err)?;
        }};
    }
    macro_rules! settle_quic {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            node_quic::dispatch(&mut realm, global.get(), $ev, err)?;
        }};
    }
    macro_rules! settle_napi {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            napi_aw::dispatch(&mut realm, global.get(), $ev, err)?;
        }};
    }
    macro_rules! settle_dispatch {
        ($ptr:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            // SAFETY: realm 内取 raw cx（jobqueue 装配同款）
            unsafe { crate::dispatch::run_dispatchable((&mut realm).raw_cx(), $ptr) };
        }};
    }
    macro_rules! settle_serve {
        ($ev:expr) => {{
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            crate::serve_bridge::dispatch(&mut realm, global.get(), $ev, err)?;
        }};
    }
    loop {
        let st = pump_once(rt, global, err, fetch_rx, ws_rx, watch_rx, child_rx, net_rx, worker_rx, quic_rx, napi_rx, dispatch_rx).await?;
        if st.exited {
            return Ok(());
        }
        if st.entry_failed {
            break;
        }
        // 未处理 rejection 即跳出（收割路径上报；serve 侧 handler 错误已由驱动转
        // 500，走到这里的是真 bug，fail fast 不带病服务）。
        if state::unhandled_pending() > 0 {
            break;
        }
        // serve 顶排空（同步决议只排 microtask，§4.18：随后必须再 RunJobs）。
        let mut served = 0;
        while let Ok(ev) = serve_rx.try_recv() {
            settle_serve!(ev);
            served += 1;
        }
        // 结算/触发过即回顶（先 RunJobs，不 park）；停机旗只在 quiescent 时收敛
        // ——优雅：在飞请求排空后再退。
        let progressed = st.progressed || st.timers > st.timers_unrefed || served > 0;
        if progressed {
            continue;
        }
        if crate::serve_bridge::serve_shutdown() {
            break;
        }
        // park：最近定时器或任一通道（含 serve）先到者。
        match timers::next_wake() {
            Some(at) => {
                let tokio_at = tokio::time::Instant::from_std(at);
                tokio::select! {
                    _ = tokio::time::sleep_until(tokio_at) => {}
                    msg = fetch_rx.recv() => { if let Some(msg) = msg { settle_fetch!(msg); } }
                    ev = ws_rx.recv() => { if let Some(ev) = ev { settle_ws!(ev); } }
                    wev = watch_rx.recv() => { if let Some(wev) = wev { settle_watch!(wev); } }
                    cev = child_rx.recv() => { if let Some(cev) = cev { settle_child!(cev); } }
                    nev = net_rx.recv() => { if let Some(nev) = nev { settle_net!(nev); } }
                    wev2 = worker_rx.recv() => { if let Some(wev2) = wev2 { settle_worker!(wev2); } }
                    qev = quic_rx.recv() => { if let Some(qev) = qev { settle_quic!(qev); } }
                    nev2 = napi_rx.recv() => { if let Some(nev2) = nev2 { settle_napi!(nev2); } }
                    dptr = dispatch_rx.recv() => { if let Some(ptr) = dptr { settle_dispatch!(ptr); } }
                    sev = serve_rx.recv() => { if let Some(sev) = sev { settle_serve!(sev); } }
                }
            }
            None => {
                tokio::select! {
                    msg = fetch_rx.recv() => { if let Some(msg) = msg { settle_fetch!(msg); } }
                    ev = ws_rx.recv() => { if let Some(ev) = ev { settle_ws!(ev); } }
                    wev = watch_rx.recv() => { if let Some(wev) = wev { settle_watch!(wev); } }
                    cev = child_rx.recv() => { if let Some(cev) = cev { settle_child!(cev); } }
                    nev = net_rx.recv() => { if let Some(nev) = nev { settle_net!(nev); } }
                    wev2 = worker_rx.recv() => { if let Some(wev2) = wev2 { settle_worker!(wev2); } }
                    qev = quic_rx.recv() => { if let Some(qev) = qev { settle_quic!(qev); } }
                    nev2 = napi_rx.recv() => { if let Some(nev2) = nev2 { settle_napi!(nev2); } }
                    dptr = dispatch_rx.recv() => { if let Some(ptr) = dptr { settle_dispatch!(ptr); } }
                    sev = serve_rx.recv() => { if let Some(sev) = sev { settle_serve!(sev); } }
                }
            }
        }
    }
    tracing::info!(target: "winterjs::runtime", open = state::serve_open(), "serve loop drained");

    report_unhandled_rejections(rt, global)
}

/// serve JS 会话入口（plan4 T1）：调用方已起独立 OS 线程（16MB 栈，§4.24）并进
/// LocalSet；此处 init 会话 → handler 双认加载 → 常驻循环，返回即会话终。
/// `startup` 一次性通知：handler 就绪（或失败）先行，主线程据此起 axum。
pub async fn run_serve_session(
    handler: std::path::PathBuf,
    serve_rx: &mut tokio::sync::mpsc::UnboundedReceiver<crate::serve_bridge::ServeEvent>,
    startup: std::sync::mpsc::Sender<Result<(), String>>,
) -> Result<(), Error> {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "winterjs".into());
    let init = init_session(vec![exe])?;
    // 声明顺序即 drop 逆序（§4.22：`rt` 先于 `engine` drop 即炸；各返回点走 end_session）。
    let engine = init.engine;
    let mut rt = init.rt;
    let _state_guard = init.state_guard;
    // SAFETY: `init_session` 返回到此重 root 之间无任何 JSAPI 调用（无 GC 间隙）。
    rooted!(&in(rt.cx()) let global = init.global_ptr);
    let mut fetch_rx = init.fetch_rx;
    let mut ws_rx = init.ws_rx;
    let mut watch_rx = init.watch_rx;
    let mut child_rx = init.child_rx;
    let mut net_rx = init.net_rx;
    let mut worker_rx = init.worker_rx;
    let mut quic_rx = init.quic_rx;
    let mut napi_rx = init.napi_rx;
    let mut dispatch_rx = init.dispatch_rx;
    let url = crate::loader::resolve::entry_url(&handler)
        .map_err(|e| Error::Other(format!("cannot resolve --handler '{}': {e}", handler.display())))?;
    tracing::info!(target: "winterjs::runtime", handler = %handler.display(), url = url.as_str(), "serve handler load");
    let loaded = {
        let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
        crate::serve_bridge::load_serve_handler(&mut realm, global.get(), &url)
    };
    match loaded {
        Ok(()) => {
            let _ = startup.send(Ok(()));
        }
        Err(e) => {
            let _ = startup.send(Err(e.to_string()));
            end_session(rt, engine);
            return Err(e);
        }
    }
    state::serve_hold_server();
    let r = serve_loop(
        &mut rt,
        &global,
        ErrorSource::Module { url: url.as_str() },
        &mut fetch_rx,
        &mut ws_rx,
        &mut watch_rx,
        &mut child_rx,
        &mut net_rx,
        &mut worker_rx,
        &mut quic_rx,
        &mut napi_rx,
        &mut dispatch_rx,
        serve_rx,
    )
    .await;
    state::serve_release_server();
    end_session(rt, engine);
    r
}
