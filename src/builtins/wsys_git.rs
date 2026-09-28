//! 本体 git 只读面（WinterJS.git.revParse/log；gix 底座，clone 另案）。
//!
//! 纯搬移拆分自 wsys.rs（§0.9 超限拆分；调用方经 wsys 原位重导出）。
//! UNSAFE-BOUNDARY：全部 JSNative 入口经 `wrap_cx` + `Frame::from_raw`
//!（结构性边界块）；前置：调用方 realm 内 + 参数槽 rooted 后才分配；
//! 读仓走 `permissions::check_read`；覆盖：`tests/wsys.rs::wsys_git_faces`。

use mozjs::context::JSContext;
use mozjs::jsval::JSVal;

use crate::builtins::wsys::{arg_num, arg_str, set_json, set_str};
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};

// ── git（gix 只读三件；clone 另案）──────────────────────────────────────────

fn git_open(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<gix::Repository> {
    let p = if frame.argc() <= i {
        None
    } else {
        Some(value_to_string(cx, frame.arg(i)))
    }?;
    if p.is_empty() {
        report_error(cx, &format!("TypeError: {what} requires a repo path"));
        return None;
    }
    if let Err(msg) = crate::permissions::check_read(&p) {
        report_error(cx, &msg);
        return None;
    }
    match gix::open(&p) {
        Ok(repo) => Some(repo),
        Err(e) => {
            report_error(cx, &format!("TypeError: git open failed: {e}"));
            None
        }
    }
}

/// `__wjs_wsys_git_rev_parse(path, rev)` → sha。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_git_faces`。
pub unsafe extern "C" fn git_rev_parse(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(repo) = git_open(&mut cx, &frame, 0, "git revParse") else {
        return false;
    };
    let Some(rev) = arg_str(&mut cx, &frame, 1, "git revParse") else {
        return false;
    };
    match repo.rev_parse_single(rev.as_str()) {
        Ok(id) => {
            set_str(&mut cx, &frame, &id.to_string());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: git rev not found: {e}"));
            false
        }
    }
}

/// `__wjs_wsys_git_log(path, rev, n)` → `[{sha,title}]` JSON（n 封顶 100）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_git_faces`。
pub unsafe extern "C" fn git_log(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(repo) = git_open(&mut cx, &frame, 0, "git log") else {
        return false;
    };
    let Some(rev) = arg_str(&mut cx, &frame, 1, "git log") else {
        return false;
    };
    let n = match arg_num(&frame, 2) {
        Some(n) if n.is_finite() && n >= 0.0 && n.fract() == 0.0 => (n as usize).min(100),
        _ => 10,
    };
    let tip = match repo.rev_parse_single(rev.as_str()) {
        Ok(id) => id,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: git rev not found: {e}"));
            return false;
        }
    };
    let walk = match repo.rev_walk([tip.detach()]).all() {
        Ok(walk) => walk,
        Err(e) => {
            report_error(&mut cx, &format!("TypeError: git walk failed: {e}"));
            return false;
        }
    };
    let mut out = Vec::new();
    for info in walk.take(n) {
        let info = match info {
            Ok(info) => info,
            Err(e) => {
                report_error(&mut cx, &format!("TypeError: git walk failed: {e}"));
                return false;
            }
        };
        let commit = match info.object() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let title = commit
            .message()
            .map(|m| m.title.to_string())
            .unwrap_or_default();
        out.push(serde_json::json!({ "sha": commit.id().to_string(), "title": title }));
    }
    set_json(&mut cx, &frame, &serde_json::Value::Array(out));
    true
}
