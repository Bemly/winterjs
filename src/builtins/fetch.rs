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
use mozjs::gc::ValueArray;
use mozjs::jsapi::{HandleValueArray, JS_CallFunctionValue, JSObject};
use mozjs::jsval::{JSVal, ObjectValue, UndefinedValue};
use mozjs::rooted;
use mozjs::typedarray::{CreateWith, TypedArray, Uint8};

use crate::builtins::encoding::view_bytes;
use crate::error::Error;
use crate::jsapi_glue::{raw_handle, raw_handle_mut, report_error, value_to_string, wrap_cx, Frame};
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
    pub body: Vec<u8>,
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

/// 调单参函数 `fun(arg)`（this=global；返回 rval；失败清场并 None）。
pub(crate) fn call_one(
    cx: &mut JSContext,
    global: *mut JSObject,
    fun: JSVal,
    arg: JSVal,
) -> Option<JSVal> {
    rooted!(&in(cx) let fun_root = fun);
    rooted!(&in(cx) let arg_root = arg);
    rooted!(&in(cx) let mut rval = UndefinedValue());
    // SAFETY: 单实参直构（§4.9）；fun/arg 为有效 rooted 值；rval 为 rooted 出参
    let args = HandleValueArray::from(unsafe { raw_handle(arg_root.as_ptr()) });
    let ok = unsafe {
        JS_CallFunctionValue(
            cx.raw_cx(),
            raw_handle(&global),
            raw_handle(fun_root.as_ptr()),
            &args,
            raw_handle_mut(rval.as_ptr()),
        )
    };
    if ok { Some(rval.get()) } else { None }
}

/// 调双参函数 `fun(a, b)`（fire_due 的 ValueArray 模式；事件循环上下文可用）。
fn call_two(
    cx: &mut JSContext,
    global: *mut JSObject,
    fun: JSVal,
    a: JSVal,
    b: JSVal,
) -> Option<JSVal> {
    rooted!(&in(cx) let fun_root = fun);
    rooted!(&in(cx) let argv = ValueArray::new([a, b]));
    rooted!(&in(cx) let mut rval = UndefinedValue());
    let args_array = HandleValueArray {
        length_: 2,
        // SAFETY: argv 为栈上 Rooted 槽，存活到调用返回，元素被 GC 追踪
        elements_: argv.as_ptr().cast(),
    };
    // SAFETY: cx/global/fun 均有效；rval 为 rooted 出参
    let ok = unsafe {
        JS_CallFunctionValue(
            cx.raw_cx(),
            raw_handle(&global),
            raw_handle(fun_root.as_ptr()),
            &args_array,
            raw_handle_mut(rval.as_ptr()),
        )
    };
    if ok { Some(rval.get()) } else { None }
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

/// 由字节建 Uint8Array（1 个边界 `unsafe`，见 §6 审计）。
pub(crate) fn uint8_array(cx: &mut JSContext, bytes: &[u8]) -> Option<*mut JSObject> {
    rooted!(&in(cx) let mut obj: *mut JSObject = std::ptr::null_mut());
    // SAFETY: realm 内创建；obj 为 rooted 出参；bytes 存活到调用返回
    let ok = unsafe {
        TypedArray::<Uint8, *mut JSObject>::create(cx, CreateWith::Slice(bytes), obj.handle_mut())
    };
    if ok.is_err() || obj.is_null() {
        None
    } else {
        Some(obj.get())
    }
}

/// 交付：调 resolve(Response) 或 reject(Error)。前置：realm 内。
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
            let meta = serde_json::json!({
                "status": ok.status,
                "statusText": ok.status_text,
                "headers": ok.headers,
                "url": ok.url,
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
    msg: FetchResult,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), Error> {
    let Some((resolve, reject)) = state::take_fetch_callback(msg.id) else {
        state::fetch_unpend();
        return Ok(());
    };
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

/// `__wjs_fetch_start(url, method, headersJson, bodyU8?, resolve, reject)`。
/// file:/data: 同步结算（仍经 resolve/reject，走 microtask）；http(s) spawn 任务。
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
    // 快路径：file: / data:（同步读 + 同步 settle，promise 仍异步决议）
    if url.starts_with("file:") || url.starts_with("data:") {
        let outcome = fetch_local(&url);
        let global_ptr = global_root.get();
        return deliver(&mut cx, global_ptr, resolve, reject, outcome);
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        let scheme = url.split(':').next().unwrap_or("");
        let msg = if scheme == "blob" {
            format!("blob: URLs need Phase 3c (Blob)")
        } else {
            format!("unsupported URL scheme '{scheme}:' in fetch")
        };
        let global_ptr = global_root.get();
        return deliver(&mut cx, global_ptr, resolve, reject, Err(msg));
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
    handle.spawn(async move {
        let outcome = fetch_http(&url, &method, req_headers, req_body).await;
        let _ = tx.send(FetchResult { id, outcome });
    });
    frame.set_rval(UndefinedValue());
    true
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
    })
}

/// http(s) 抓取（tokio 任务内）。
async fn fetch_http(
    url: &str,
    method: &str,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
) -> Result<FetchOk, String> {
    let mut req = client().request(
        reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| format!("TypeError: bad fetch method '{method}'"))?,
        url,
    );
    for (k, v) in &headers {
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
    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("TypeError: fetch failed: {e}"))?
        .to_vec();
    Ok(FetchOk { status, status_text, url: final_url, headers: out_headers, body })
}
