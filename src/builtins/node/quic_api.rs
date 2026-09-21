//! quic JS API 面（h3_respond/request、sess_open、stream 系、dgram 系 natives；对齐 quic.rs；纯搬移）。

use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use base64::Engine as _;
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;
use super::quic::{arg_id, arg_json};
use super::quic_driver::{h3_headers_json, QuicH3Cmd, QuicSessCmd, QuicStreamCmd};

/// `__wjs_quic_h3_respond(sessId, streamId, json)` → undefined（服务端回 H3 响应；
/// json `{status, headers, body(b64)}`；会话已摘/非 H3 即 ERR_INVALID_STATE）。
pub unsafe extern "C" fn quic_h3_respond(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(sess) = arg_id(&mut cx, &frame, 0) else {
        return false;
    };
    let Some(stream) = arg_id(&mut cx, &frame, 1) else {
        return false;
    };
    let v = match arg_json(&mut cx, &frame, 2, "quic h3 respond") {
        Some(v) => v,
        None => return false,
    };
    let status = v.get("status").and_then(|x| x.as_u64()).unwrap_or(200).min(599) as u16;
    let headers = h3_headers_json(v.get("headers").cloned().unwrap_or(serde_json::Value::Object(Default::default())));
    let body = v.get("body").and_then(|x| x.as_str()).unwrap_or("");
    let body = match base64::engine::general_purpose::STANDARD.decode(body) {
        Ok(b) => b,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: quic h3 respond body is not base64 ({e})"));
            return false;
        }
    };
    let Some(tx) = state::quic_sess_h3_cmd(sess) else {
        report_error(&mut cx, "ERR_INVALID_STATE: quic h3 session is gone");
        return false;
    };
    let _ = tx.send(QuicH3Cmd::Respond { stream, status, headers, body });
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs_quic_h3_request(sessId, json)` → 流 id 串（客户端发 H3 请求；
/// json `{method, path, headers, body(b64)}`；响应经 `H3Response` 到流目标）。
pub unsafe extern "C" fn quic_h3_request(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_h3_respond
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(sess) = arg_id(&mut cx, &frame, 0) else {
        return false;
    };
    let v = match arg_json(&mut cx, &frame, 1, "quic h3 request") {
        Some(v) => v,
        None => return false,
    };
    let method = v.get("method").and_then(|x| x.as_str()).unwrap_or("GET").to_string();
    let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("/").to_string();
    let headers = h3_headers_json(v.get("headers").cloned().unwrap_or(serde_json::Value::Object(Default::default())));
    let body = v.get("body").and_then(|x| x.as_str()).unwrap_or("");
    let body = match base64::engine::general_purpose::STANDARD.decode(body) {
        Ok(b) => b,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: quic h3 request body is not base64 ({e})"));
            return false;
        }
    };
    let Some(tx) = state::quic_sess_h3_cmd(sess) else {
        report_error(&mut cx, "ERR_INVALID_STATE: quic h3 session is gone");
        return false;
    };
    let stream = state::quic_alloc_id();
    state::quic_stream_insert(stream, sess, state::QuicStreamDir::Bidi);
    let _ = tx.send(QuicH3Cmd::Request { stream, method, path, headers, body });
    use mozjs::conversions::ToJSValConvertible as _;
    stream.to_string().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 本地开流。`__wjs_quic_sess_open(sessId, "bidi"|"uni")` → 流 id 串
/// （就绪经 `StreamOpened` 事件；会话已摘即 `ERR_INVALID_STATE` 错）。
pub unsafe extern "C" fn quic_sess_open(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let sess = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let dir = if frame.argc() >= 2 { value_to_string(&mut cx, frame.arg(1)) } else { String::new() };
    let bidi = match dir.as_str() {
        "bidi" => true,
        "uni" => false,
        _ => {
            report_error(&mut cx, "TypeError: direction must be bidi or uni");
            return false;
        }
    };
    let dir_enum =
        if bidi { state::QuicStreamDir::Bidi } else { state::QuicStreamDir::Send };
    let stream = state::quic_alloc_id();
    state::quic_stream_insert(stream, sess, dir_enum);
    let cmd = if bidi {
        QuicSessCmd::OpenBidi { stream }
    } else {
        QuicSessCmd::OpenUni { stream }
    };
    if !state::quic_sess_cmd(sess, cmd) {
        state::quic_stream_remove(stream);
        report_error(&mut cx, "ERR_INVALID_STATE: Session is closed. New streams cannot be opened.");
        return false;
    }
    use mozjs::conversions::ToJSValConvertible as _;
    stream.to_string().to_jsval(&mut cx, frame.rval_mut());
    true
}

/// 登记流 JS 目标。`__wjs_quic_stream_attach(id, target)` → undefined。
pub unsafe extern "C" fn quic_stream_attach(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    if frame.argc() < 2 || !frame.arg(1).is_object() {
        report_error(&mut cx, "TypeError: quic stream attach needs a target object");
        return false;
    }
    state::quic_stream_target_add(id, frame.arg(1));
    frame.set_rval(UndefinedValue());
    true
}

/// 流写。`__wjs_quic_stream_write(id, uint8)` → boolean（流已收尾即 false）。
pub unsafe extern "C" fn quic_stream_write(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: quic stream write needs data");
        return false;
    }
    let bytes = match crate::jsapi_glue::view_bytes(&mut cx, frame.arg(1), "quic stream write") {
        Some(b) => b,
        None => return false,
    };
    frame.set_rval(mozjs::jsval::BooleanValue(
        state::quic_stream_write_cmd(id, QuicStreamCmd::Write(bytes)),
    ));
    true
}

/// 写端 finish。`__wjs_quic_stream_finish(id)` → undefined（已收尾即无操作）。
pub unsafe extern "C" fn quic_stream_finish(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    state::quic_stream_write_cmd(id, QuicStreamCmd::Finish);
    frame.set_rval(UndefinedValue());
    true
}

/// 码实参（number/bigint 形态；非法即 `ERR_OUT_OF_RANGE` 错，None）。
fn arg_code(cx: &mut JSContext, frame: &Frame, i: u32) -> Option<u64> {
    if frame.argc() <= i {
        return Some(0);
    }
    let v = frame.arg(i);
    let n = if v.is_number() {
        value_to_string(cx, v).parse::<i64>().ok()?
    } else if v.is_bigint() {
        value_to_string(cx, v).trim_end_matches('n').parse::<i64>().ok()?
    } else {
        return None;
    };
    if n < 0 || n > 0xFFFF_FFFF {
        return None;
    }
    Some(n as u64)
}

/// 写端 reset。`__wjs_quic_stream_reset(id, code)` → undefined。
pub unsafe extern "C" fn quic_stream_reset(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let code = match arg_code(&mut cx, &frame, 1) {
        Some(c) => c,
        None => {
            report_error(&mut cx, "ERR_OUT_OF_RANGE: reset code must be an integer in range");
            return false;
        }
    };
    state::quic_stream_write_cmd(id, QuicStreamCmd::Reset(code));
    frame.set_rval(UndefinedValue());
    true
}

/// 读端 stop。`__wjs_quic_stream_stop(id, code)` → undefined。
pub unsafe extern "C" fn quic_stream_stop(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    let code = match arg_code(&mut cx, &frame, 1) {
        Some(c) => c,
        None => {
            report_error(&mut cx, "ERR_OUT_OF_RANGE: stop code must be an integer in range");
            return false;
        }
    };
    state::quic_stream_read_cmd(id, QuicStreamCmd::Stop(code));
    frame.set_rval(UndefinedValue());
    true
}

/// 发数据报（超限静默丢弃，Node 同款）。`__wjs_quic_sess_send_dgram(id, uint8)` → boolean。
pub unsafe extern "C" fn quic_sess_send_dgram(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: sendDatagram needs data");
        return false;
    }
    let bytes = match crate::jsapi_glue::view_bytes(&mut cx, frame.arg(1), "sendDatagram") {
        Some(b) => b,
        None => return false,
    };
    let ok = match state::quic_sess_conn(id) {
        Some(conn) => conn.send_datagram(bytes.into()).is_ok(),
        None => false,
    };
    frame.set_rval(mozjs::jsval::BooleanValue(ok));
    true
}

/// 数据报上限（禁收发即 0，Node 同款）。`__wjs_quic_sess_max_dgram(id)` → 数字串。
pub unsafe extern "C" fn quic_sess_max_dgram(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 quic_listen
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let id = match arg_id(&mut cx, &frame, 0) {
        Some(id) => id,
        None => return false,
    };
    use mozjs::conversions::ToJSValConvertible as _;
    let max = state::quic_sess_conn(id).and_then(|c| c.max_datagram_size()).unwrap_or(0);
    max.to_string().to_jsval(&mut cx, frame.rval_mut());
    true
}
