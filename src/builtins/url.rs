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

// ── URLPattern（`urlpattern` 0.6 直引，Deno 官方实现；plan3 §5 专项）──
// 桥接约定（全字符串进出，复杂值走 JSON 桥）：JS 侧做 WebIDL 重载分流与
// 取值（用户 getter 抛错天然透传），Rust 侧只做解析/匹配/序列化。
// pattern 注册表随会话存活（fd 表同款口径，无显式释放）。

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static PATTERNS: RefCell<HashMap<u64, urlpattern::UrlPattern>> =
        RefCell::new(HashMap::new());
    static PATTERN_NEXT: RefCell<u64> = RefCell::new(1);
}

fn pattern_alloc(pat: urlpattern::UrlPattern) -> u64 {
    PATTERNS.with(|m| {
        PATTERN_NEXT.with(|n| {
            let mut n = n.borrow_mut();
            let id = *n;
            *n += 1;
            m.borrow_mut().insert(id, pat);
            id
        })
    })
}

fn pattern_with<T>(id: u64, f: impl FnOnce(&urlpattern::UrlPattern) -> T) -> Option<T> {
    PATTERNS.with(|m| m.borrow().get(&id).map(f))
}

/// init 字典 JSON → UrlPatternInit（缺键即 None；base_url 另设）。
fn init_from_json(v: &serde_json::Value) -> urlpattern::UrlPatternInit {
    let get = |k: &str| {
        v.get(k).and_then(|x| {
            if x.is_null() {
                None
            } else if let Some(s) = x.as_str() {
                Some(s.to_owned())
            } else {
                Some(value_json_to_string(x))
            }
        })
    };
    urlpattern::UrlPatternInit {
        protocol: get("protocol"),
        username: get("username"),
        password: get("password"),
        hostname: get("hostname"),
        port: get("port"),
        pathname: get("pathname"),
        search: get("search"),
        hash: get("hash"),
        base_url: None,
    }
}

/// 非字符串 JSON 值转字符串（WebIDL DOMString 宽容口径：数字/布尔直转）。
fn value_json_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn parse_base(s: &str) -> Option<url::Url> {
    url::Url::parse(s).ok()
}

/// `__wjs_urlpattern_parse(inputJson, baseJsonOrNull, ignoreCaseBool)`
/// → `{id,protocol,username,password,hostname,port,pathname,search,hash,
/// hasRegExpGroups}` JSON；非法抛 TypeError（原文，JS 侧包码）。
pub unsafe extern "C" fn urlpattern_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let input_json = frame.arg(0);
    let input_s = value_to_string(&mut cx, input_json);
    let base_s = if frame.argc() > 1 {
        let b = frame.arg(1);
        if b.is_null() || b.is_undefined() {
            None
        } else {
            Some(value_to_string(&mut cx, b))
        }
    } else {
        None
    };
    let ignore_case = if frame.argc() > 2 {
        frame.arg(2).to_boolean()
    } else {
        false
    };
    let input_v: serde_json::Value = match serde_json::from_str(&input_s) {
        Ok(v) => v,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: bad pattern input: {e}"));
            return false;
        }
    };
    // base 给出但非法即抛（真机口径；与 test/exec 的吞错回 false/null 不同）。
    let base_url = match base_s.as_deref() {
        Some(b) => match parse_base(b) {
            Some(u) => Some(u),
            None => {
                report_error(&mut cx, "TypeError: invalid base URL");
                return false;
            }
        },
        None => None,
    };
    let init = if let Some(s) = input_v.as_str() {
        match urlpattern::UrlPatternInit::parse_constructor_string::<regex::Regex>(s, base_url) {
            Ok(init) => init,
            Err(e) => {
                report_error(&mut cx, &format!("TypeError: {e}"));
                return false;
            }
        }
    } else if input_v.is_object() {
        let mut init = init_from_json(&input_v);
        init.base_url = base_url;
        init
    } else {
        report_error(&mut cx, "TypeError: bad pattern input");
        return false;
    };
    let options = urlpattern::UrlPatternOptions {
        ignore_case,
        ..Default::default()
    };
    match urlpattern::UrlPattern::<regex::Regex>::parse(init, options) {
        Ok(pat) => {
            let out = serde_json::json!({
                "protocol": pat.protocol(),
                "username": pat.username(),
                "password": pat.password(),
                "hostname": pat.hostname(),
                "port": pat.port(),
                "pathname": pat.pathname(),
                "search": pat.search(),
                "hash": pat.hash(),
                "hasRegExpGroups": pat.has_regexp_groups(),
                "id": pattern_alloc(pat),
            });
            set_rval_string(&mut cx, &frame, &out.to_string());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: {e}"));
            false
        }
    }
}

/// match 输入 JSON → UrlPatternMatchInput（非法输入回 Err，调用方按 test/exec
/// 语义分别处理：test/exec 一律吞错回 false/null，只有 JS 侧 dict+base 才抛）。
fn match_input(
    input_v: &serde_json::Value,
    base_s: Option<&str>,
) -> Result<urlpattern::UrlPatternMatchInput, String> {
    if let Some(s) = input_v.as_str() {
        let url = match base_s {
            Some(b) => {
                let base = parse_base(b).ok_or_else(|| "bad base".to_owned())?;
                url::Url::options()
                    .base_url(Some(&base))
                    .parse(s)
                    .map_err(|e| e.to_string())?
            }
            None => url::Url::parse(s).map_err(|e| e.to_string())?,
        };
        Ok(urlpattern::UrlPatternMatchInput::Url(url))
    } else if input_v.is_object() {
        Ok(urlpattern::UrlPatternMatchInput::Init(init_from_json(input_v)))
    } else {
        Err("bad input".to_owned())
    }
}

fn pattern_id(frame: &Frame) -> Option<u64> {
    (frame.argc() > 0)
        .then(|| frame.arg(0))
        .and_then(|v| {
            if v.is_number() {
                Some(v.to_number() as u64)
            } else {
                None
            }
        })
}

/// `__wjs_urlpattern_test(idNum, inputJson, baseJsonOrNull)` → 布尔；
/// 输入非法一律回 false（真机口径；dict+base 的抛错由 JS 侧先行）。
pub unsafe extern "C" fn urlpattern_test(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(id), input_s, base_s) = (
        pattern_id(&frame),
        value_to_string(&mut cx, frame.arg(1)),
        if frame.argc() > 2 {
            let b = frame.arg(2);
            if b.is_null() || b.is_undefined() {
                None
            } else {
                Some(value_to_string(&mut cx, b))
            }
        } else {
            None
        },
    ) else {
        frame.set_rval(mozjs::jsval::BooleanValue(false));
        return true;
    };
    let ok = pattern_with(id, |pat| {
        let input_v: serde_json::Value = serde_json::from_str(&input_s).ok()?;
        let input = match_input(&input_v, base_s.as_deref()).ok()?;
        pat.test(input).ok()
    })
    .flatten()
    .unwrap_or(false);
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// 分量结果 → `{input, groups}` JSON（未命中组记 null，JS 侧转 undefined；
/// groups 键恒字典序——`serde_json::Map` 即 BTree，真机组序系其内部哈希桶
/// artifact（同名集异序可得异序，已实证不可复刻），此处记档为确定性偏离）。
fn component_json(c: &urlpattern::UrlPatternComponentResult) -> serde_json::Value {
    let groups: serde_json::Map<String, serde_json::Value> = c
        .groups
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.clone()
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            )
        })
        .collect();
    serde_json::json!({"input": c.input, "groups": groups})
}

/// `__wjs_urlpattern_exec(idNum, inputJson, baseJsonOrNull)`
/// → 8 分量 JSON 或 `"null"`（未命中/输入非法；JS 侧组装 `inputs`）。
pub unsafe extern "C" fn urlpattern_exec(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(id), input_s, base_s) = (
        pattern_id(&frame),
        value_to_string(&mut cx, frame.arg(1)),
        if frame.argc() > 2 {
            let b = frame.arg(2);
            if b.is_null() || b.is_undefined() {
                None
            } else {
                Some(value_to_string(&mut cx, b))
            }
        } else {
            None
        },
    ) else {
        set_rval_string(&mut cx, &frame, "null");
        return true;
    };
    let out = pattern_with(id, |pat| {
        let input_v: serde_json::Value = serde_json::from_str(&input_s).ok()?;
        let input = match_input(&input_v, base_s.as_deref()).ok()?;
        let res = pat.exec(input).ok()??;
        Some(
            serde_json::json!({
                "protocol": component_json(&res.protocol),
                "username": component_json(&res.username),
                "password": component_json(&res.password),
                "hostname": component_json(&res.hostname),
                "port": component_json(&res.port),
                "pathname": component_json(&res.pathname),
                "search": component_json(&res.search),
                "hash": component_json(&res.hash),
            })
            .to_string(),
        )
    })
    .flatten()
    .unwrap_or_else(|| "null".to_owned());
    set_rval_string(&mut cx, &frame, &out);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlpattern_init_shapes() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"pathname":"/foo/:id"}"#).unwrap();
        let init = init_from_json(&v);
        assert_eq!(init.pathname.as_deref(), Some("/foo/:id"));
        assert!(init.protocol.is_none());
        let pat = urlpattern::UrlPattern::<regex::Regex>::parse(init, Default::default()).unwrap();
        assert_eq!(pat.pathname(), "/foo/:id");
        assert_eq!(pat.protocol(), "*");
        assert!(!pat.has_regexp_groups());
    }

    #[test]
    fn urlpattern_str_and_groups() {
        let init = urlpattern::UrlPatternInit::parse_constructor_string::<regex::Regex>(
            "https://ex.com/foo/*",
            None,
        )
        .unwrap();
        let pat = urlpattern::UrlPattern::<regex::Regex>::parse(init, Default::default()).unwrap();
        assert_eq!(pat.hostname(), "ex.com");
        let url = url::Url::parse("https://ex.com/foo/42").unwrap();
        let res = pat
            .exec(urlpattern::UrlPatternMatchInput::Url(url))
            .unwrap()
            .unwrap();
        assert_eq!(res.pathname.input, "/foo/42");
        assert_eq!(res.pathname.groups.get("0").unwrap().as_deref(), Some("42"));
    }

    #[test]
    fn urlpattern_miss_and_bad() {
        let init = urlpattern::UrlPatternInit::parse_constructor_string::<regex::Regex>(
            "https://ex.com/foo/*",
            None,
        )
        .unwrap();
        let pat = urlpattern::UrlPattern::<regex::Regex>::parse(init, Default::default()).unwrap();
        let url = url::Url::parse("https://other.com/foo/42").unwrap();
        assert!(
            pat.exec(urlpattern::UrlPatternMatchInput::Url(url))
                .unwrap()
                .is_none()
        );
        // `[` 经 init 字典是宽容解析（真机 `new URLPattern({pathname:"["})`
        // 不抛；严格的是 constructor-string 入口，此处走 init 口径断言）。
        let mut init = urlpattern::UrlPatternInit::default();
        init.pathname = Some("[".to_owned());
        assert!(urlpattern::UrlPattern::<regex::Regex>::parse(init, Default::default()).is_ok());
    }
}
