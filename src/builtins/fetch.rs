//! `fetch()`：http(s) 经 `reqwest`（§2 门控：rustls-no-provider + ring，由本模块装 provider）；
//! file:/data: 同步直给（仍走 promise 语义）；其余 scheme 友好报错。
//!
//! 并发模型：native 只做参数解析 + spawn 任务 + 存回调；完成经 channel 回事件循环结算。
//! 通道发送端活在 `PlainState`（无 JS 值），回调 resolve/reject 存 `RootedState`（被 GC 追踪）。
//! `Request`/`Response`/`Headers` 真类在 prelude（`builtins/mod.rs`），本模块只做传输 + 交付。

use std::sync::OnceLock;
use std::time::Duration;

use mozjs::context::JSContext;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, ObjectValue, UndefinedValue};
use mozjs::rooted;

use crate::error::Error;
use crate::jsapi_glue::{call_one, call_two, report_error, uint8_array, value_to_string, view_bytes, wrap_cx, Frame};
use crate::state;

/// 任务 → 事件循环的完成包（纯数据，可跨 await）。
pub struct FetchResult {
    pub id: u64,
    pub outcome: Result<FetchOk, String>,
}

pub struct FetchOk {
    pub status: u16,
    pub status_text: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// 快照 body（file:/data:；http(s) 为空，body 走流）。
    pub body: Vec<u8>,
    /// http(s) 流 id（file:/data: 为 None；与 fetch id 同号）。
    pub stream: Option<u64>,
}

/// 事件循环消息：响应头/快照（Result）或流式 chunk/终态（Stream）。
pub enum FetchMsg {
    Result(FetchResult),
    Stream(StreamEvent),
}

pub struct StreamEvent {
    pub id: u64,
    pub kind: StreamKind,
}

pub enum StreamKind {
    Chunk(Vec<u8>),
    Done,
    Failed(String),
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        // §2：TLS provider 由顶层 rustls/ring 提供（reqwest 侧为 no-provider）。
        let _ = rustls::crypto::ring::default_provider().install_default();
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client builds")
    })
}

fn arg_string(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    Some(value_to_string(cx, frame.arg(i)))
}

/// prelude 辅助函数取值（init 时已缓存，见 `runtime.rs`）。
fn helpers() -> Option<(JSVal, JSVal)> {
    state::with_rooted(|s| {
        let a = s.make_response_fn.get();
        let b = s.make_fetch_error_fn.get();
        if a.is_undefined() || b.is_undefined() {
            None
        } else {
            Some((a, b))
        }
    })
}

/// 交付：调 resolve(Response) 或 reject(Error)。前置：realm 内。
/// stream 响应头先建流状态（pull 随后即到；见 `state::stream_add`）。
fn deliver(
    cx: &mut JSContext,
    global: *mut JSObject,
    resolve: JSVal,
    reject: JSVal,
    outcome: Result<FetchOk, String>,
) -> bool {
    let Some((make_response, make_error)) = helpers() else {
        report_error(cx, "failed to load settings: fetch helpers missing (prelude?)");
        return false;
    };
    match outcome {
        Ok(ok) => {
            if let Some(sid) = ok.stream {
                state::stream_add(sid);
            }
            let meta = serde_json::json!({
                "status": ok.status,
                "statusText": ok.status_text,
                "headers": ok.headers,
                "url": ok.url,
                "streamId": ok.stream,
            })
            .to_string();
            rooted!(&in(cx) let mut meta_v = UndefinedValue());
            meta.to_jsval(cx, meta_v.handle_mut());
            let Some(body_obj) = uint8_array(cx, &ok.body) else {
                report_error(cx, "RangeError: cannot allocate response body");
                return false;
            };
            rooted!(&in(cx) let body_v = ObjectValue(body_obj));
            let Some(resp) = call_two(cx, global, make_response, meta_v.get(), body_v.get()) else {
                return false;
            };
            call_one(cx, global, resolve, resp).is_some()
        }
        Err(message) => {
            rooted!(&in(cx) let mut msg_v = UndefinedValue());
            message.to_jsval(cx, msg_v.handle_mut());
            let Some(err) = call_one(cx, global, make_error, msg_v.get()) else {
                return false;
            };
            call_one(cx, global, reject, err).is_some()
        }
    }
}

/// 事件循环结算一条完成消息（回调取出即移除；失败清场）。
/// 前置条件：cx 已进入 global 所属 realm。
pub fn settle(
    cx: &mut JSContext,
    global: *mut JSObject,
    msg: FetchMsg,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), Error> {
    match msg {
        FetchMsg::Result(r) => settle_result(cx, global, r, err),
        FetchMsg::Stream(ev) => settle_stream(cx, global, ev, err),
    }
}

/// 响应头/快照/错误结算。
fn settle_result(
    cx: &mut JSContext,
    global: *mut JSObject,
    msg: FetchResult,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), Error> {
    let Some((resolve, reject)) = state::take_fetch_callback(msg.id) else {
        // 回调已被 `fetch_abort` 取走（计数已减），残留消息直接丢弃。
        return Ok(());
    };
    state::fetch_task_done(msg.id);
    let ok = deliver(cx, global, resolve, reject, msg.outcome);
    state::fetch_unpend();
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

/// 流事件结算（chunk 泵 / 终态；失败清场）。前置：同上（realm 内）。
fn settle_stream(
    cx: &mut JSContext,
    global: *mut JSObject,
    ev: StreamEvent,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), Error> {
    // 纯状态泵（TLS 借用已放；JS 调用在后，避 §4.14 嵌套）。
    let pump = match ev.kind {
        StreamKind::Chunk(chunk) => state::stream_on_chunk(ev.id, chunk),
        StreamKind::Done => state::stream_on_finish(ev.id, None),
        StreamKind::Failed(e) => state::stream_on_finish(ev.id, Some(e)),
    };
    let failed = |cx: &mut JSContext| match err {
        crate::runtime::ErrorSource::Script { source, filename } => {
            crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
        }
        crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
    };
    let ok = match pump {
        state::StreamPump::Buffered => true,
        state::StreamPump::Wake(resolve, chunk) => resolve_chunk(cx, global, resolve, &chunk),
        state::StreamPump::DoneAll(items) => items
            .into_iter()
            .map(|(resolve, chunk)| match chunk {
                Some(bytes) => resolve_chunk(cx, global, resolve, &bytes),
                None => resolve_done(cx, global, resolve),
            })
            .all(|b| b),
        state::StreamPump::FailAll(rejects, message) => rejects
            .into_iter()
            .map(|reject| reject_stream(cx, global, reject, &message))
            .all(|b| b),
    };
    if ok { Ok(()) } else { Err(failed(cx)) }
}

/// resolve(chunkU8)（`uint8_array` + `call_one` 集中边界；前置 realm 内）。
fn resolve_chunk(cx: &mut JSContext, global: *mut JSObject, resolve: JSVal, bytes: &[u8]) -> bool {
    let Some(obj) = uint8_array(cx, bytes) else {
        report_error(cx, "RangeError: cannot allocate stream chunk");
        return false;
    };
    rooted!(&in(cx) let chunk_v = ObjectValue(obj));
    call_one(cx, global, resolve, chunk_v.get()).is_some()
}

/// resolve(null)（流终结标记；前置同上）。
fn resolve_done(cx: &mut JSContext, global: *mut JSObject, resolve: JSVal) -> bool {
    rooted!(&in(cx) let mut null_v = UndefinedValue());
    mozjs::jsval::NullValue().to_jsval(cx, null_v.handle_mut());
    call_one(cx, global, resolve, null_v.get()).is_some()
}

/// reject(Error(message))（经 prelude `__wjs_make_fetch_error`；前置同上）。
fn reject_stream(cx: &mut JSContext, global: *mut JSObject, reject: JSVal, message: &str) -> bool {
    let Some((_, make_error)) = helpers() else {
        report_error(cx, "failed to load settings: fetch helpers missing (prelude?)");
        return false;
    };
    rooted!(&in(cx) let mut msg_v = UndefinedValue());
    message.to_jsval(cx, msg_v.handle_mut());
    let Some(err_obj) = call_one(cx, global, make_error, msg_v.get()) else {
        return false;
    };
    call_one(cx, global, reject, err_obj).is_some()
}

/// `__wjs_fetch_pull(streamId, resolve, reject)`：一律走回调结算（有即同步调，无则排队）。
/// resolve 约定：Uint8Array=chunk，null=终结；reject=流错误。前置：realm 内 native。
pub unsafe extern "C" fn fetch_pull(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: fetch pull needs stream id and callbacks");
        return false;
    }
    if !frame.arg(1).is_object() || !frame.arg(2).is_object() {
        report_error(&mut cx, "TypeError: fetch pull needs callable resolve/reject");
        return false;
    }
    let (id, resolve, reject) = (frame.arg(0).to_number() as u64, frame.arg(1), frame.arg(2));
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    let global_ptr = global_root.get();
    let action = state::stream_pull(id, resolve, reject);
    let ok = match action {
        state::StreamPull::Queued => true,
        state::StreamPull::Chunk(chunk) => resolve_chunk(&mut cx, global_ptr, resolve, &chunk),
        state::StreamPull::Done => resolve_done(&mut cx, global_ptr, resolve),
        state::StreamPull::Failed(message) => reject_stream(&mut cx, global_ptr, reject, &message),
    };
    frame.set_rval(UndefinedValue());
    ok
}

/// `__wjs_fetch_start(url, method, headersJson, bodyU8?, resolve, reject)` → id。
/// 同步 settled（file:/data:/非法 scheme）返回 0；http(s) 未决返回正 id（abort 用）。
pub unsafe extern "C" fn fetch_start(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(url), Some(method), Some(headers_json)) = (
        arg_string(&mut cx, &frame, 0, "fetch"),
        arg_string(&mut cx, &frame, 1, "fetch"),
        arg_string(&mut cx, &frame, 2, "fetch"),
    ) else {
        return false;
    };
    if frame.argc() < 6 {
        report_error(&mut cx, "TypeError: fetch internals missing callbacks");
        return false;
    }
    let body_v = frame.arg(3);
    let (resolve, reject) = (frame.arg(4), frame.arg(5));
    if !resolve.is_object() || !reject.is_object() {
        report_error(&mut cx, "TypeError: fetch internals missing callbacks");
        return false;
    }
    let body: Option<Vec<u8>> = if body_v.is_undefined() || body_v.is_null() {
        None
    } else {
        match view_bytes(&mut cx, body_v, "fetch body") {
            Some(b) => Some(b),
            None => return false,
        }
    };
    let headers: Vec<(String, String)> = serde_json::from_str(&headers_json).unwrap_or_default();

    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    // 快路径：file: / data:（同步读 + 同步 settle，promise 仍异步决议；id 恒 0）
    if url.starts_with("file:") || url.starts_with("data:") {
        let outcome = fetch_local(&url);
        let global_ptr = global_root.get();
        let ok = deliver(&mut cx, global_ptr, resolve, reject, outcome);
        frame.set_rval(mozjs::jsval::Int32Value(0));
        return ok;
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        let scheme = url.split(':').next().unwrap_or("");
        let msg = if scheme == "blob" {
            format!("blob: URLs need Phase 3c (Blob)")
        } else {
            format!("unsupported URL scheme '{scheme}:' in fetch")
        };
        let global_ptr = global_root.get();
        let ok = deliver(&mut cx, global_ptr, resolve, reject, Err(msg));
        frame.set_rval(mozjs::jsval::Int32Value(0));
        return ok;
    }

    // 慢路径：spawn reqwest 任务
    let Some((id, tx)) = state::fetch_alloc() else {
        report_error(&mut cx, "failed to load settings: fetch driver not installed");
        return false;
    };
    state::push_fetch_callback(id, resolve, reject);
    let req_headers = headers;
    let req_body = body;
    let handle = tokio::runtime::Handle::try_current();
    let Ok(handle) = handle else {
        state::take_fetch_callback(id);
        state::fetch_unpend();
        report_error(&mut cx, "OperationError: no async runtime for fetch");
        return false;
    };
    let join = handle.spawn(async move {
        fetch_http_streaming(id, &url, &method, req_headers, req_body, tx).await;
    });
    state::fetch_task(id, join.abort_handle());
    frame.set_rval(mozjs::jsval::Int32Value(id as i32));
    true
}

/// `__wjs_fetch_abort(id)`：取消未决任务并摘除回调（幂等；外层拒绝由 prelude 侧 `reject`）。
/// 流式 body 的排队 pull 一并拒绝（AbortError；调用时流可能已终结/取消，即静默）。
/// 前置：realm 内（仅做状态操作 + 回调结算，无新任务）。
pub unsafe extern "C" fn fetch_abort(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: fetch abort needs a numeric id");
        return false;
    }
    let id = frame.arg(0).to_number() as u64;
    state::fetch_abort(id);
    // 流等待者拒绝（纯状态取出在上；JS 调用在后，避 §4.14 嵌套）。
    let global = state::global();
    rooted!(&in(cx) let global_root: *mut JSObject = global);
    let global_ptr = global_root.get();
    let (_, waiters) = state::stream_abort(id);
    let mut ok = true;
    for (_, reject) in waiters {
        ok &= reject_stream(&mut cx, global_ptr, reject, "AbortError: fetch aborted");
    }
    frame.set_rval(UndefinedValue());
    ok
}

/// file:/data: 抓取（同步）。
fn fetch_local(url: &str) -> Result<FetchOk, String> {
    if let Some(rest) = url.strip_prefix("file:") {
        let parsed = url::Url::parse(url).map_err(|e| format!("TypeError: bad file URL: {e}"))?;
        let path = parsed
            .to_file_path()
            .map_err(|_| "TypeError: bad file URL".to_string())?;
        let _ = rest;
        return match fs_err::read(&path) {
            Ok(body) => Ok(FetchOk {
                status: 200,
                status_text: "OK".into(),
                url: url.to_owned(),
                headers: vec![],
                body,
                stream: None,
            }),
            Err(e) => Err(format!("TypeError: fetch failed: {e}")),
        };
    }
    // data:
    let data_url =
        data_url::DataUrl::process(url).map_err(|e| format!("TypeError: bad data: URL ({e:?})"))?;
    let mime = data_url.mime_type().to_string();
    let (bytes, _) = data_url
        .decode_to_vec()
        .map_err(|e| format!("TypeError: bad data: body ({e:?})"))?;
    Ok(FetchOk {
        status: 200,
        status_text: "OK".into(),
        url: url.to_owned(),
        headers: vec![("content-type".into(), mime)],
        body: bytes,
        stream: None,
    })
}

/// http(s) 抓取（tokio 任务内）：响应头即发 Result（带 stream id），body 边下边吐。
async fn fetch_http_streaming(
    id: u64,
    url: &str,
    method: &str,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    tx: tokio::sync::mpsc::UnboundedSender<FetchMsg>,
) {
    let outcome = http_head(url, method, &headers, body).await;
    let (resp, head) = match outcome {
        Ok((resp, head)) => (resp, head),
        Err(e) => {
            let _ = tx.send(FetchMsg::Result(FetchResult { id, outcome: Err(e) }));
            state::fetch_task_done(id);
            return;
        }
    };
    let head = FetchOk { stream: Some(id), ..head };
    if tx.send(FetchMsg::Result(FetchResult { id, outcome: Ok(head) })).is_err() {
        state::fetch_task_done(id);
        return;
    }
    use futures::StreamExt as _;
    let mut stream = resp.bytes_stream();
    loop {
        match stream.next().await {
            Some(Ok(chunk)) => {
                if tx.send(FetchMsg::Stream(StreamEvent { id, kind: StreamKind::Chunk(chunk.to_vec()) })).is_err() {
                    break;
                }
            }
            Some(Err(e)) => {
                let _ = tx.send(FetchMsg::Stream(StreamEvent {
                    id,
                    kind: StreamKind::Failed(format!("TypeError: fetch failed: {e}")),
                }));
                break;
            }
            None => {
                let _ = tx.send(FetchMsg::Stream(StreamEvent { id, kind: StreamKind::Done }));
                break;
            }
        }
    }
    state::fetch_task_done(id);
}

/// 建请求并发头（body 全 buffered 发；响应体走流，见上）。
async fn http_head(
    url: &str,
    method: &str,
    headers: &[(String, String)],
    body: Option<Vec<u8>>,
) -> Result<(reqwest::Response, FetchOk), String> {
    let mut req = client().request(
        reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| format!("TypeError: bad fetch method '{method}'"))?,
        url,
    );
    for (k, v) in headers {
        let name = reqwest::header::HeaderName::from_bytes(k.as_bytes())
            .map_err(|_| format!("TypeError: bad header name '{k}'"))?;
        let value = reqwest::header::HeaderValue::from_str(v)
            .map_err(|_| format!("TypeError: bad header value for '{k}'"))?;
        req = req.header(name, value);
    }
    if let Some(b) = body {
        req = req.body(b);
    }
    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            "TimeoutError: fetch timed out".to_string()
        } else {
            format!("TypeError: fetch failed: {e}")
        }
    })?;
    let status = resp.status().as_u16();
    let status_text = resp
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_owned();
    let final_url = resp.url().to_string();
    let mut out_headers = Vec::new();
    for (k, v) in resp.headers().iter() {
        out_headers.push((k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()));
    }
    Ok((resp, FetchOk { status, status_text, url: final_url, headers: out_headers, body: Vec::new(), stream: None }))
}
