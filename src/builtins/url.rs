//! URL / URLSearchParams：prelude 真类 + Rust 解析 natives。
//! 全字符串进出（复杂值走 JSON 桥），零新增 `unsafe`。
//! 语义：WHATWG URL（`url` crate）+ form 编解码（`form_urlencoded`）。
//! `searchParams` 活视图由 prelude 经 WeakMap 双向同步（见 `PRELUDE`）。

use mozjs::context::JSContext;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};

/// 调用帧第 i 个实参转字符串（缺参即报 TypeError）。
fn arg_string(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    Some(value_to_string(cx, frame.arg(i)))
}

fn set_rval_string(cx: &mut JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

fn parse_url(href: &str, base: Option<&str>) -> Result<String, String> {
    let parsed = match base {
        Some(b) => {
            let base_url = url::Url::parse(b).map_err(|e| format!("TypeError: Invalid base URL: {e}"))?;
            url::Url::options().base_url(Some(&base_url)).parse(href)
        }
        None => url::Url::parse(href),
    };
    parsed.map(|u| u.to_string()).map_err(|e| format!("TypeError: Invalid URL: {e}"))
}

/// `__wjs_url_parse(href, base?)` → 规范 href；非法抛 TypeError。
pub unsafe extern "C" fn url_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(href) = arg_string(&mut cx, &frame, 0, "URL") else {
        return false;
    };
    let base = if frame.argc() > 1 {
        let b = frame.arg(1);
        if b.is_undefined() || b.is_null() {
            None
        } else {
            Some(value_to_string(&mut cx, b))
        }
    } else {
        None
    };
    match parse_url(&href, base.as_deref()) {
        Ok(s) => {
            set_rval_string(&mut cx, &frame, &s);
            true
        }
        Err(e) => {
            report_error(&mut cx, &e);
            false
        }
    }
}

/// `__wjs_url_get(href, component)` → 分量字符串（href 已规范，前解析理论上必成功）。
pub unsafe extern "C" fn url_get(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(href), Some(comp)) = (
        arg_string(&mut cx, &frame, 0, "URL component"),
        arg_string(&mut cx, &frame, 1, "URL component"),
    ) else {
        return false;
    };
    let Ok(u) = url::Url::parse(&href) else {
        report_error(&mut cx, "TypeError: Invalid URL");
        return false;
    };
    let s = match comp.as_str() {
        "href" => u.to_string(),
        "protocol" => format!("{}:", u.scheme()),
        "username" => u.username().to_owned(),
        "password" => u.password().unwrap_or("").to_owned(),
        "host" => match (u.host_str(), u.port()) {
            (Some(h), Some(p)) => format!("{h}:{p}"),
            (Some(h), None) => h.to_owned(),
            (None, _) => String::new(),
        },
        "hostname" => u.host_str().unwrap_or("").to_owned(),
        "port" => u.port().map(|p| p.to_string()).unwrap_or_default(),
        "pathname" => u.path().to_owned(),
        "search" => u.query().map(|q| format!("?{q}")).unwrap_or_default(),
        "hash" => u.fragment().map(|f| format!("#{f}")).unwrap_or_default(),
        "origin" => u.origin().ascii_serialization(),
        _ => {
            report_error(&mut cx, &format!("TypeError: unknown URL component '{comp}'"));
            return false;
        }
    };
    set_rval_string(&mut cx, &frame, &s);
    true
}

/// `__wjs_url_set(href, component, value)` → 新 href（非法赋值按规范静默忽略，不抛）。
pub unsafe extern "C" fn url_set(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(href), Some(comp), Some(value)) = (
        arg_string(&mut cx, &frame, 0, "URL component"),
        arg_string(&mut cx, &frame, 1, "URL component"),
        arg_string(&mut cx, &frame, 2, "URL component"),
    ) else {
        return false;
    };
    let Ok(mut u) = url::Url::parse(&href) else {
        report_error(&mut cx, "TypeError: Invalid URL");
        return false;
    };
    match comp.as_str() {
        "href" => {
            if let Ok(s) = parse_url(&value, None) {
                set_rval_string(&mut cx, &frame, &s);
            } else {
                set_rval_string(&mut cx, &frame, &href);
            }
            return true;
        }
        "protocol" => {
            let _ = u.set_scheme(value.strip_suffix(':').unwrap_or(&value));
        }
        "username" => {
            let _ = u.set_username(&value);
        }
        "password" => {
            let _ = u.set_password(if value.is_empty() { None } else { Some(&value) });
        }
        "host" => set_host_port(&mut u, &value),
        "hostname" => {
            if !value.is_empty() {
                let _ = u.set_host(Some(&value));
            }
        }
        "port" => {
            if value.is_empty() {
                let _ = u.set_port(None);
            } else if let Ok(p) = value.parse::<u16>() {
                let _ = u.set_port(Some(p));
            }
        }
        "pathname" => u.set_path(&value),
        "search" => {
            let q = value.strip_prefix('?').unwrap_or(&value);
            u.set_query(if q.is_empty() { None } else { Some(q) });
        }
        "hash" => {
            if value.is_empty() {
                u.set_fragment(None);
            } else {
                u.set_fragment(Some(value.strip_prefix('#').unwrap_or(&value)));
            }
        }
        _ => {
            report_error(&mut cx, &format!("TypeError: unknown URL component '{comp}'"));
            return false;
        }
    }
    let s = u.to_string();
    set_rval_string(&mut cx, &frame, &s);
    true
}

/// host setter：`example.com:8080` 拆端口；IPv6 原样交 `set_host`。
fn set_host_port(u: &mut url::Url, value: &str) {
    if value.is_empty() {
        return;
    }
    if u.set_host(Some(value)).is_ok() {
        return;
    }
    // 带端口的 host（如 `example.com:8080`；IPv6 不进此分支因为 set_host 已处理）
    if let Some((h, p)) = value.rsplit_once(':')
        && !h.ends_with(']')
        && let Ok(port) = p.parse::<u16>()
        && u.set_host(Some(h)).is_ok()
    {
        let _ = u.set_port(Some(port));
    }
}

/// `__wjs_usp_parse(query)` → `[[k,v],...]` JSON（`?` 前缀可带可不带）。
pub unsafe extern "C" fn usp_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(query) = arg_string(&mut cx, &frame, 0, "URLSearchParams") else {
        return false;
    };
    let q = query.strip_prefix('?').unwrap_or(&query);
    let pairs: Vec<(String, String)> = form_urlencoded::parse(q.as_bytes())
        .into_owned()
        .collect();
    match serde_json::to_string(&pairs) {
        Ok(json) => {
            set_rval_string(&mut cx, &frame, &json);
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: cannot encode query pairs: {e}"));
            false
        }
    }
}

/// `__wjs_usp_serialize(jsonPairs)` → 查询字符串（无 `?`）。
pub unsafe extern "C" fn usp_serialize(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(json) = arg_string(&mut cx, &frame, 0, "URLSearchParams") else {
        return false;
    };
    let pairs: Vec<(String, String)> = match serde_json::from_str(&json) {
        Ok(p) => p,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: bad query pairs: {e}"));
            return false;
        }
    };
    let mut ser = form_urlencoded::Serializer::new(String::new());
    for (k, v) in &pairs {
        ser.append_pair(k, v);
    }
    set_rval_string(&mut cx, &frame, &ser.finish());
    true
}
