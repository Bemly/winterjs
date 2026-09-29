//! 本体第二批（WinterJS2.shell/hex/time/retry/graph/git/oauth/transpile/log/mime/cookie/httpdate）。
//!
//! 范围（用户拍板一口气全加）：首批（wstd）之后，真缺口中剩余可安全暴露面。
//! 全员直用树内轮子，零新增依赖；错误 plain `TypeError`（权限类原样透传，
//! 薄壳按前缀还原类名）。大面另案（§6 铁律或体量所限）：tokio 族（`JSContext`
//! 禁跨线程）/askama（编译期）/simd-json（`JSON` 已覆盖）/gix-clone（网络进度
//! 面）/binrw-动态 schema·zerocopy（裸重解释）/russh·httparse（非直引）/
//! governor·metrics（serve 行为内）/data-url 系（URL/fetch/TextEncoder 已覆盖）/
//! pathdiff·dunce（`node:path` 已覆盖）/miette 系（行为面）/构建期件。
//! 手动堆（graph）沿 mem 轮 id 表托管，裸指针不出 JS。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};

// ── natives ────────────────────────────────────────────────────────────────
// UNSAFE-BOUNDARY：全部 JSNative 入口经 `wrap_cx` + `Frame::from_raw`（结构性边界块）；
// 前置：调用方 realm 内 + 参数槽 rooted 后才分配；
// 覆盖：`tests/wsys.rs`（正常/报错/边界）。

pub(crate) fn arg_str(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    Some(value_to_string(cx, frame.arg(i)))
}

pub(crate) fn arg_num(frame: &Frame, i: u32) -> Option<f64> {
    if frame.argc() <= i || !frame.arg(i).is_number() {
        return None;
    }
    Some(frame.arg(i).to_number())
}

pub(crate) fn set_json(cx: &mut JSContext, frame: &Frame, v: &serde_json::Value) {
    let text = v.to_string();
    frame.set_rval({
        rooted!(&in(cx) let mut out = UndefinedValue());
        text.to_jsval(cx, out.handle_mut());
        out.get()
    });
}

pub(crate) fn set_str(cx: &mut JSContext, frame: &Frame, s: &str) {
    frame.set_rval({
        rooted!(&in(cx) let mut out = UndefinedValue());
        s.to_jsval(cx, out.handle_mut());
        out.get()
    });
}

pub(crate) fn set_num(frame: &Frame, n: f64) {
    frame.set_rval(mozjs::jsval::DoubleValue(n));
}

// ── shell.expand（shellexpand；$VAR 逐个过 env 门）─────────────────────────

/// 扫描 `$V` / `${V}` 形变量名（纯函数，单测覆盖；`$$` 转义跳过）。
pub fn scan_vars(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            if i + 1 < b.len() && b[i + 1] == b'$' {
                i += 2;
                continue;
            }
            if i + 1 < b.len() && b[i + 1] == b'{' {
                if let Some(end) = s[i + 2..].find('}') {
                    let name = &s[i + 2..i + 2 + end];
                    if !name.is_empty()
                        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        out.push(name.to_string());
                    }
                    i += 3 + end;
                    continue;
                }
            } else {
                let mut j = i + 1;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
                if j > i + 1 {
                    out.push(s[i + 1..j].to_string());
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out.sort();
    out.dedup();
    out
}

/// `__wjs2_wsys_shell_expand(s)`（`~` + env；沙箱内未授权变量即拒）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_shell_hex_faces`。
pub unsafe extern "C" fn shell_expand(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "shell expand") else {
        return false;
    };
    for v in scan_vars(&s) {
        if let Err(msg) = crate::permissions::check_env(&v) {
            report_error(&mut cx, &msg);
            return false;
        }
    }
    tracing::debug!(target: "winterjs2::wsys", src_len = s.len(), "shell expand");
    match shellexpand::full(&s) {
        Ok(out) => {
            set_str(&mut cx, &frame, &out);
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: shell expand failed: {e}"));
            false
        }
    }
}

// ── hex（const-hex）────────────────────────────────────────────────────────

/// `__wjs2_wsys_hex_encode(u8)` → 小写 hex。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_shell_hex_faces`。
pub unsafe extern "C" fn hex_encode(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: hex encode requires data");
        return false;
    }
    let Some(data) = crate::jsapi_glue::view_bytes(&mut cx, frame.arg(0), "hex encode") else {
        return false;
    };
    set_str(&mut cx, &frame, &const_hex::encode(&data));
    true
}

/// `__wjs2_wsys_hex_decode(hexStr)` → Uint8Array。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_shell_hex_faces`。
pub unsafe extern "C" fn hex_decode(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "hex decode") else {
        return false;
    };
    match const_hex::decode(s.as_bytes()) {
        Ok(bytes) => match crate::jsapi_glue::uint8_array(&mut cx, &bytes) {
            Some(obj) => {
                frame.set_rval(mozjs::jsval::ObjectValue(obj));
                true
            }
            None => {
                report_error(&mut cx, "RangeError: cannot allocate Uint8Array");
                false
            }
        },
        Err(_) => {
            report_error(&mut cx, "TypeError: hex decode requires even-length [0-9a-fA-F]");
            false
        }
    }
}

// ── time（jiff）────────────────────────────────────────────────────────────

/// `__wjs2_wsys_time_now()` → 纪元毫秒.
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_time_retry_faces`。
pub unsafe extern "C" fn time_now(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let _cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    set_num(&frame, jiff::Timestamp::now().as_millisecond() as f64);
    true
}

/// `__wjs2_wsys_time_parse(s)` → RFC3339 纪元毫秒.
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_time_retry_faces`。
pub unsafe extern "C" fn time_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "time parse") else {
        return false;
    };
    match s.parse::<jiff::Timestamp>() {
        Ok(ts) => {
            set_num(&frame, ts.as_millisecond() as f64);
            true
        }
        Err(_) => {
            report_error(&mut cx, "TypeError: time parse requires RFC3339 (e.g. 2026-01-02T03:04:05Z)");
            false
        }
    }
}

/// `__wjs2_wsys_time_format(ms, fmt, tz?)`（strtime；缺省 UTC）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_time_retry_faces`。
pub unsafe extern "C" fn time_format(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(ms), Some(fmt)) = (arg_num(&frame, 0), arg_str(&mut cx, &frame, 1, "time format")) else {
        report_error(&mut cx, "TypeError: time format requires (ms, fmt)");
        return false;
    };
    if !ms.is_finite() {
        report_error(&mut cx, "TypeError: time format requires a finite ms");
        return false;
    }
    let tz = if frame.argc() > 2 {
        let name = value_to_string(&mut cx, frame.arg(2));
        if name.is_empty() {
            jiff::tz::TimeZone::UTC
        } else {
            match jiff::tz::TimeZone::get(&name) {
                Ok(tz) => tz,
                Err(_) => {
                    report_error(&mut cx, "TypeError: time format has unknown time zone");
                    return false;
                }
            }
        }
    } else {
        jiff::tz::TimeZone::UTC
    };
    let ts = match jiff::Timestamp::from_millisecond(ms as i64) {
        Ok(ts) => ts,
        Err(_) => {
            report_error(&mut cx, "TypeError: time format ms out of range");
            return false;
        }
    };
    match jiff::fmt::strtime::format(fmt.as_str(), &ts.to_zoned(tz)) {
        Ok(s) => {
            set_str(&mut cx, &frame, &s);
            true
        }
        Err(_) => {
            report_error(&mut cx, "TypeError: time format has a bad format string");
            false
        }
    }
}

// ── retry.delay（backon 档位数学；等待由 JS setTimeout 做）──────────────────

fn arg_u64(frame: &Frame, i: u32) -> Option<u64> {
    let n = arg_num(frame, i)?;
    if n < 0.0 || n.fract() != 0.0 || n > 10000.0 {
        return None;
    }
    Some(n as u64)
}

/// `__wjs2_wsys_retry_delay(kind, attempt, minMs, maxMs, factor?)` → ms 数。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_time_retry_faces`。
pub unsafe extern "C" fn retry_delay(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    use backon::BackoffBuilder;
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(kind) = arg_str(&mut cx, &frame, 0, "retry delay") else {
        return false;
    };
    let (Some(attempt), Some(min_ms), Some(max_ms)) =
        (arg_u64(&frame, 1), arg_u64(&frame, 2), arg_u64(&frame, 3))
    else {
        report_error(&mut cx, "TypeError: retry delay requires (kind, attempt, minMs, maxMs)");
        return false;
    };
    let factor = match arg_num(&frame, 4) {
        Some(f) if f.is_finite() && f >= 1.0 && f <= 10.0 => f as f32,
        _ => 2.0,
    };
    if max_ms < min_ms {
        report_error(&mut cx, "TypeError: retry delay requires minMs <= maxMs");
        return false;
    }
    let min_d = std::time::Duration::from_millis(min_ms);
    let max_d = std::time::Duration::from_millis(max_ms);
    let ms: Option<u128> = match kind.as_str() {
        "constant" => backon::ConstantBuilder::default()
            .with_delay(min_d)
            .without_max_times()
            .build()
            .nth(attempt as usize)
            .map(|d| d.as_millis()),
        "fibonacci" => backon::FibonacciBuilder::default()
            .with_min_delay(min_d)
            .with_max_delay(max_d)
            .without_max_times()
            .build()
            .nth(attempt as usize)
            .map(|d| d.as_millis()),
        "exponential" => backon::ExponentialBuilder::default()
            .with_factor(factor)
            .with_min_delay(min_d)
            .with_max_delay(max_d)
            .without_max_times()
            .build()
            .nth(attempt as usize)
            .map(|d| d.as_millis()),
        _ => {
            report_error(&mut cx, "TypeError: retry delay kind must be constant|fibonacci|exponential");
            return false;
        }
    };
    match ms {
        Some(ms) => {
            set_num(&frame, ms as f64);
            true
        }
        None => {
            report_error(&mut cx, "TypeError: retry delay attempt out of range");
            false
        }
    }
}

// graph/git 纯搬移拆分（§0.9）：实现住 wsys_graph/wsys_git，原位重导出保调用方不动。
pub use crate::builtins::wsys_git::{git_log, git_rev_parse};
pub use crate::builtins::wsys_graph::{
    graph_add_edge, graph_add_node, graph_counts, graph_create, graph_free, graph_toposort,
};

// ── oauth（oauth2 纯构造面；token HTTP 走 fetch 栈，见 prelude）──────────────

/// `__wjs2_wsys_oauth_authorize_url(authUrl, clientId, redirectUri, scope, state?)`
/// → `{url, state}` JSON。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_oauth_transpile_faces`。
pub unsafe extern "C" fn oauth_authorize_url(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    use oauth2::CsrfToken;
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(auth), Some(cid), Some(redir), Some(scope)) = (
        arg_str(&mut cx, &frame, 0, "oauth authorizeUrl"),
        arg_str(&mut cx, &frame, 1, "oauth authorizeUrl"),
        arg_str(&mut cx, &frame, 2, "oauth authorizeUrl"),
        arg_str(&mut cx, &frame, 3, "oauth authorizeUrl"),
    ) else {
        return false;
    };
    let state = if frame.argc() > 4 {
        value_to_string(&mut cx, frame.arg(4))
    } else {
        String::new()
    };
    let challenge = if frame.argc() > 5 {
        let c = value_to_string(&mut cx, frame.arg(5));
        if c.is_empty() { None } else { Some(c) }
    } else {
        None
    };
    // 端点合法性先验（oauth2 侧 AuthUrl/RedirectUrl 同义，统一走 url 轮子）。
    let mut url = match url::Url::parse(&auth) {
        Ok(u) => u,
        Err(_) => {
            report_error(&mut cx, "TypeError: oauth authorizeUrl requires a valid auth URL");
            return false;
        }
    };
    if url::Url::parse(&redir).is_err() {
        report_error(&mut cx, "TypeError: oauth authorizeUrl requires a valid redirect URL");
        return false;
    }
    // state 缺省走密码学随机（oauth2 轮子），显式则原样透传（测试可复现）。
    let csrf = if state.is_empty() {
        CsrfToken::new_random()
    } else {
        CsrfToken::new(state)
    };
    {
        let mut q = url.query_pairs_mut();
        q.clear();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", &cid);
        q.append_pair("redirect_uri", &redir);
        if !scope.is_empty() {
            q.append_pair("scope", &scope);
        }
        q.append_pair("state", csrf.secret());
        if let Some(ch) = challenge {
            q.append_pair("code_challenge", &ch);
            q.append_pair("code_challenge_method", "S256");
        }
    }
    let v = serde_json::json!({ "url": url.as_str(), "state": csrf.secret() });
    set_json(&mut cx, &frame, &v);
    true
}

/// `__wjs2_wsys_oauth_pkce()` → `{challenge, verifier}`（S256）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_oauth_transpile_faces`。
pub unsafe extern "C" fn oauth_pkce(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    use oauth2::PkceCodeChallenge;
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    set_json(
        &mut cx,
        &frame,
        &serde_json::json!({ "challenge": challenge.as_str(), "verifier": verifier.secret() }),
    );
    true
}

// ── transpile（loader oxc 管线复用）─────────────────────────────────────────

/// `__wjs2_wsys_transpile(src, filename?)` → 转译后 JS（缺省 `input.ts`）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_oauth_transpile_faces`。
pub unsafe extern "C" fn transpile(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(src) = arg_str(&mut cx, &frame, 0, "transpile") else {
        return false;
    };
    let filename = if frame.argc() > 1 {
        value_to_string(&mut cx, frame.arg(1))
    } else {
        "input.ts".to_string()
    };
    if filename.is_empty() || filename.contains('\0') {
        report_error(&mut cx, "TypeError: transpile requires a filename");
        return false;
    }
    tracing::debug!(target: "winterjs2::wsys", src_len = src.len(), filename_len = filename.len(), "transpile");
    match crate::loader::transpile::load_js(&src, &filename, std::path::Path::new(&filename)) {
        Ok(loaded) => {
            set_str(&mut cx, &frame, &loaded.js);
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: transpile failed: {e}"));
            false
        }
    }
}

// ── log（tracing；4k 截断）──────────────────────────────────────────────────

fn log_capped(level: &str, msg: &str) {
    let short = if msg.len() > 4096 { &msg[..4096] } else { msg };
    match level {
        "debug" => tracing::debug!(target: "winterjs2::js", "{short}"),
        "info" => tracing::info!(target: "winterjs2::js", "{short}"),
        "warn" => tracing::warn!(target: "winterjs2::js", "{short}"),
        _ => tracing::error!(target: "winterjs2::js", "{short}"),
    }
}

/// `__wjs2_wsys_log(level, msg)`（level 越界即错）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_misc2_faces`。
pub unsafe extern "C" fn wlog(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(level), Some(msg)) = (
        arg_str(&mut cx, &frame, 0, "log"),
        arg_str(&mut cx, &frame, 1, "log"),
    ) else {
        return false;
    };
    match level.as_str() {
        "debug" | "info" | "warn" | "error" => {
            log_capped(&level, &msg);
            frame.set_rval(UndefinedValue());
            true
        }
        _ => {
            report_error(&mut cx, "TypeError: log level must be debug|info|warn|error");
            false
        }
    }
}

// ── mime / cookie / httpdate ───────────────────────────────────────────────

/// `__wjs2_wsys_mime_lookup(path)` → MIME 串（fallback octet-stream）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_misc2_faces`。
pub unsafe extern "C" fn mime_lookup(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(p) = arg_str(&mut cx, &frame, 0, "mime lookup") else {
        return false;
    };
    set_str(&mut cx, &frame, &mime_guess::from_path(&p).first_or_octet_stream().to_string());
    true
}

/// `__wjs2_wsys_cookie_parse(header)` → 首 cookie `{name,value}` JSON。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_misc2_faces`。
pub unsafe extern "C" fn cookie_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(h) = arg_str(&mut cx, &frame, 0, "cookie parse") else {
        return false;
    };
    let first = h.split(';').next().unwrap_or("").trim();
    match cookie::Cookie::parse(first) {
        Ok(c) => {
            set_json(
                &mut cx,
                &frame,
                &serde_json::json!({ "name": c.name(), "value": c.value() }),
            );
            true
        }
        Err(_) => {
            report_error(&mut cx, "TypeError: cookie parse requires name=value");
            false
        }
    }
}

/// `__wjs2_wsys_cookie_serialize(name, value, optsJson?)` 。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_misc2_faces`。
pub unsafe extern "C" fn cookie_serialize(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(name), Some(value)) = (
        arg_str(&mut cx, &frame, 0, "cookie serialize"),
        arg_str(&mut cx, &frame, 1, "cookie serialize"),
    ) else {
        return false;
    };
    if name.is_empty() || name.contains([';', '=', ' ', '\0']) {
        report_error(&mut cx, "TypeError: cookie serialize requires a token name");
        return false;
    }
    let opts: serde_json::Value = if frame.argc() > 2 {
        serde_json::from_str(&value_to_string(&mut cx, frame.arg(2))).unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };
    let mut b = cookie::Cookie::build((name, value));
    if let Some(p) = opts.get("path").and_then(|v| v.as_str()) {
        b = b.path(p.to_string());
    }
    if let Some(m) = opts.get("maxAge").and_then(|v| v.as_i64()) {
        b = b.max_age(cookie::time::Duration::seconds(m));
    }
    if opts.get("httpOnly").and_then(|v| v.as_bool()).unwrap_or(false) {
        b = b.http_only(true);
    }
    if opts.get("secure").and_then(|v| v.as_bool()).unwrap_or(false) {
        b = b.secure(true);
    }
    if let Some(s) = opts.get("sameSite").and_then(|v| v.as_str()) {
        match s.to_ascii_lowercase().as_str() {
            "strict" => b = b.same_site(cookie::SameSite::Strict),
            "lax" => b = b.same_site(cookie::SameSite::Lax),
            "none" => b = b.same_site(cookie::SameSite::None),
            _ => {
                report_error(&mut cx, "TypeError: cookie sameSite must be Strict|Lax|None");
                return false;
            }
        }
    }
    set_str(&mut cx, &frame, &b.to_string());
    true
}

/// `__wjs2_wsys_httpdate_parse(s)` → 纪元毫秒.
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_misc2_faces`。
pub unsafe extern "C" fn httpdate_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(s) = arg_str(&mut cx, &frame, 0, "httpdate parse") else {
        return false;
    };
    match httpdate::parse_http_date(&s) {
        Ok(t) => match t.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => {
                set_num(&frame, d.as_millis() as f64);
                true
            }
            Err(_) => {
                report_error(&mut cx, "TypeError: httpdate is before the epoch");
                false
            }
        },
        Err(_) => {
            report_error(&mut cx, "TypeError: httpdate parse requires an IMF date");
            false
        }
    }
}

/// `__wjs2_wsys_httpdate_format(ms)` → IMF 串.
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_misc2_faces`。
pub unsafe extern "C" fn httpdate_format(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(ms) = arg_num(&frame, 0) else {
        report_error(&mut cx, "TypeError: httpdate format requires ms");
        return false;
    };
    if !ms.is_finite() || ms < 0.0 {
        report_error(&mut cx, "TypeError: httpdate format requires ms >= 0");
        return false;
    }
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64);
    set_str(&mut cx, &frame, &httpdate::fmt_http_date(t));
    true
}

#[cfg(test)]
mod tests {
    use super::scan_vars;

    #[test]
    fn wsys_scan_vars_shapes() {
        assert_eq!(scan_vars("hi $A ${B} $$C $"), vec!["A", "B"]);
        assert_eq!(scan_vars("no vars"), Vec::<String>::new());
        assert_eq!(scan_vars("$A $A"), vec!["A"]);
    }
}
