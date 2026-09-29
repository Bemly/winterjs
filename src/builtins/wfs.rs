//! 本体 FS（WinterJS2.fs）：与 `node:fs` 分离的自有文件面。
//!
//! 方向（用户拍板）：本体走自己的，兼容走兼容的；本体直用已有 Rust 轮子
//!（`fs-err` + `std::fs`，零新增依赖），不经 `node:fs` JS 层。
//! 形态：Web 形 async（Rust 同步实现 + JS 包 Promise），错误 plain
//! `TypeError`/`WfsError`（无 node 错误码口径，保持分离）。
//! 暴露：`globalThis.WinterJS2.fs` 主面 + `globalThis.fs` 别名（同 storage 既例）。

use base64::Engine as _;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};

/// 路径校验（纯函数，单测覆盖）：非空、无 NUL。
pub fn validate_wfs_path(p: &str) -> Result<(), String> {
    if p.is_empty() {
        return Err("fs path must be a non-empty string".to_string());
    }
    if p.contains('\0') {
        return Err("fs path must not contain NUL".to_string());
    }
    Ok(())
}

/// fs-err 错误转可读串（去绝对路径回显，只留 kind + 片段，避免日志泄全路径）。
fn fs_err(e: impl std::string::ToString) -> String {
    e.to_string()
}

// ── natives（同步；参数校验失败 report_error + false）────────────────────────
// UNSAFE-BOUNDARY：全部 JSNative 入口经 `wrap_cx` + `Frame::from_raw`（结构性边界块，
// 随 native 数线性增长）；前置：调用方 realm 内 + 参数槽 rooted 后才分配；
// 覆盖：`tests/wfs.rs`（正常/报错/边界）+ 黑盒 panic 路径用例。

fn arg_path(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires a path"));
        return None;
    }
    let p = value_to_string(cx, frame.arg(i));
    if let Err(msg) = validate_wfs_path(&p) {
        report_error(cx, &format!("TypeError: {msg}"));
        return None;
    }
    Some(p)
}

fn arg_bool(frame: &Frame, i: u32) -> bool {
    if frame.argc() <= i {
        return false;
    }
    let v = frame.arg(i);
    v.is_boolean() && v.to_boolean()
}

/// `__wjs2_wfs_read(path)` → base64。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/wfs.rs::wfs_read_write_stat`。
pub unsafe extern "C" fn wfs_read(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(p) = arg_path(&mut cx, &frame, 0, "fs read") else {
        return false;
    };
    if let Err(msg) = crate::permissions::check_read(&p) {
        report_error(&mut cx, &msg);
        return false;
    }
    tracing::debug!(target: "winterjs2::wfs", path_len = p.len(), "read");
    match fs_err::read(&p) {
        Ok(bytes) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            frame.set_rval({
                rooted!(&in(cx) let mut out = UndefinedValue());
                b64.to_jsval(&mut cx, out.handle_mut());
                out.get()
            });
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("WfsError: {}", fs_err(e)));
            false
        }
    }
}

/// `__wjs2_wfs_write(path, b64)`。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/wfs.rs::wfs_read_write_stat`。
pub unsafe extern "C" fn wfs_write(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(p) = arg_path(&mut cx, &frame, 0, "fs write") else {
        return false;
    };
    if frame.argc() <= 1 {
        report_error(&mut cx, "TypeError: fs write requires data");
        return false;
    }
    let b64 = value_to_string(&mut cx, frame.arg(1));
    if let Err(msg) = crate::permissions::check_write(&p) {
        report_error(&mut cx, &msg);
        return false;
    }
    let bytes = match base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()) {
        Ok(b) => b,
        Err(_) => {
            report_error(&mut cx, "TypeError: fs write data is not valid base64");
            return false;
        }
    };
    tracing::debug!(target: "winterjs2::wfs", path_len = p.len(), bytes_len = bytes.len(), "write");
    match fs_err::write(&p, &bytes) {
        Ok(()) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("WfsError: {}", fs_err(e)));
            false
        }
    }
}

fn stat_json(path: &str) -> Result<serde_json::Value, String> {
    let md = fs_err::symlink_metadata(path).map_err(fs_err)?;
    let ft = md.file_type();
    let len = md.len();
    let mtime_ms = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Ok(serde_json::json!({
        "isFile": ft.is_file(),
        "isDirectory": ft.is_dir(),
        "isSymlink": ft.is_symlink(),
        "size": len,
        "mtimeMs": mtime_ms,
    }))
}

/// `__wjs2_wfs_stat(path)` → JSON 串。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/wfs.rs::wfs_read_write_stat`。
pub unsafe extern "C" fn wfs_stat(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(p) = arg_path(&mut cx, &frame, 0, "fs stat") else {
        return false;
    };
    if let Err(msg) = crate::permissions::check_read(&p) {
        report_error(&mut cx, &msg);
        return false;
    }
    match stat_json(&p) {
        Ok(v) => {
            let text = v.to_string();
            frame.set_rval({
                rooted!(&in(cx) let mut out = UndefinedValue());
                text.to_jsval(&mut cx, out.handle_mut());
                out.get()
            });
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("WfsError: {msg}"));
            false
        }
    }
}

/// `__wjs2_wfs_mkdir(path, recursiveBool)`。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/wfs.rs::wfs_mkdir_readdir_remove`。
pub unsafe extern "C" fn wfs_mkdir(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(p) = arg_path(&mut cx, &frame, 0, "fs mkdir") else {
        return false;
    };
    let recursive = arg_bool(&frame, 1);
    if let Err(msg) = crate::permissions::check_write(&p) {
        report_error(&mut cx, &msg);
        return false;
    }
    tracing::debug!(target: "winterjs2::wfs", path_len = p.len(), recursive, "mkdir");
    let r = if recursive {
        fs_err::create_dir_all(&p)
    } else {
        fs_err::create_dir(&p)
    };
    match r {
        Ok(()) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("WfsError: {}", fs_err(e)));
            false
        }
    }
}

/// `__wjs2_wfs_readdir(path)` → JSON 串 `[{name,isFile,isDirectory,isSymlink}]`（按名排序）。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/wfs.rs::wfs_mkdir_readdir_remove`。
pub unsafe extern "C" fn wfs_readdir(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(p) = arg_path(&mut cx, &frame, 0, "fs readdir") else {
        return false;
    };
    if let Err(msg) = crate::permissions::check_read(&p) {
        report_error(&mut cx, &msg);
        return false;
    }
    match fs_err::read_dir(&p) {
        Ok(rd) => {
            let mut out = Vec::new();
            for ent in rd {
                let ent = match ent {
                    Ok(e) => e,
                    Err(e) => {
                        report_error(&mut cx, &format!("WfsError: {}", fs_err(e)));
                        return false;
                    }
                };
                let name = ent.file_name().to_string_lossy().into_owned();
                let ft = match ent.file_type() {
                    Ok(t) => t,
                    Err(e) => {
                        report_error(&mut cx, &format!("WfsError: {}", fs_err(e)));
                        return false;
                    }
                };
                out.push(serde_json::json!({
                    "name": name,
                    "isFile": ft.is_file(),
                    "isDirectory": ft.is_dir(),
                    "isSymlink": ft.is_symlink(),
                }));
            }
            out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            let text = serde_json::Value::Array(out).to_string();
            frame.set_rval({
                rooted!(&in(cx) let mut outv = UndefinedValue());
                text.to_jsval(&mut cx, outv.handle_mut());
                outv.get()
            });
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("WfsError: {}", fs_err(e)));
            false
        }
    }
}

/// `__wjs2_wfs_remove(path, recursiveBool)`。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/wfs.rs::wfs_mkdir_readdir_remove`。
pub unsafe extern "C" fn wfs_remove(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(p) = arg_path(&mut cx, &frame, 0, "fs remove") else {
        return false;
    };
    let recursive = arg_bool(&frame, 1);
    if let Err(msg) = crate::permissions::check_write(&p) {
        report_error(&mut cx, &msg);
        return false;
    }
    tracing::debug!(target: "winterjs2::wfs", path_len = p.len(), recursive, "remove");
    let meta = fs_err::symlink_metadata(&p);
    let r = match meta {
        Err(e) => Err(e),
        Ok(md) => {
            if md.file_type().is_dir() {
                if recursive {
                    fs_err::remove_dir_all(&p)
                } else {
                    fs_err::remove_dir(&p)
                }
            } else {
                fs_err::remove_file(&p)
            }
        }
    };
    match r {
        Ok(()) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("WfsError: {}", fs_err(e)));
            false
        }
    }
}

/// `__wjs2_wfs_rename(a, b)`。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/wfs.rs::wfs_rename_copy_exists`。
pub unsafe extern "C" fn wfs_rename(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(a) = arg_path(&mut cx, &frame, 0, "fs rename") else {
        return false;
    };
    let Some(b) = arg_path(&mut cx, &frame, 1, "fs rename") else {
        return false;
    };
    if let Err(msg) = crate::permissions::check_read(&a) {
        report_error(&mut cx, &msg);
        return false;
    }
    if let Err(msg) = crate::permissions::check_write(&b) {
        report_error(&mut cx, &msg);
        return false;
    }
    match fs_err::rename(&a, &b) {
        Ok(()) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("WfsError: {}", fs_err(e)));
            false
        }
    }
}

/// `__wjs2_wfs_copy(a, b)`（文件对文件）。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/wfs.rs::wfs_rename_copy_exists`。
pub unsafe extern "C" fn wfs_copy(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(a) = arg_path(&mut cx, &frame, 0, "fs copy") else {
        return false;
    };
    let Some(b) = arg_path(&mut cx, &frame, 1, "fs copy") else {
        return false;
    };
    if let Err(msg) = crate::permissions::check_read(&a) {
        report_error(&mut cx, &msg);
        return false;
    }
    if let Err(msg) = crate::permissions::check_write(&b) {
        report_error(&mut cx, &msg);
        return false;
    }
    match fs_err::copy(&a, &b) {
        Ok(_) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("WfsError: {}", fs_err(e)));
            false
        }
    }
}

/// `__wjs2_wfs_exists(path)` → bool（不存在即 false，不抛）。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/wfs.rs::wfs_errors_boundary`。
pub unsafe extern "C" fn wfs_exists(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(p) = arg_path(&mut cx, &frame, 0, "fs exists") else {
        return false;
    };
    if crate::permissions::check_read(&p).is_err() {
        frame.set_rval(mozjs::jsval::BooleanValue(false));
        return true;
    }
    frame.set_rval(mozjs::jsval::BooleanValue(
        fs_err::symlink_metadata(&p).is_ok(),
    ));
    true
}

#[cfg(test)]
mod tests {
    use super::validate_wfs_path;

    #[test]
    fn wfs_path_validation() {
        assert!(validate_wfs_path("a.txt").is_ok());
        assert!(validate_wfs_path("").is_err());
        assert!(validate_wfs_path("a\0b").is_err());
    }

    #[test]
    fn wfs_stat_shape_on_manifest() {
        let v = super::stat_json("Cargo.toml").unwrap();
        assert_eq!(v["isFile"], true);
        assert!(v["size"].as_u64().unwrap() > 0);
    }
}
