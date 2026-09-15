//! `node:fs` + `node:fs/promises`：同步核心（`fs-err` 增强报错，码经 `io_code`）。
//! 错误约定：native 报 `"{CODE}: {detail}"`，prelude 包装成带
//! `.code/.syscall/.path` 的 Error（Node 形状）；`fs/promises` 为同语义 async 包裹
//! （底层同步实现，文档记录；lint 脚本量级无感）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};
use crate::state;

/// `io::Error` → Node 码（raw_os_error 优先，Kind 兜底；纯函数，单元测试覆盖）。
pub fn io_code(e: &std::io::Error) -> &'static str {
    if let Some(errno) = e.raw_os_error() {
        return match errno {
            1 => "EPERM",
            2 => "ENOENT",
            3 => "ESRCH", // 10f os.getPriority(-1)（kill 系同理备用）
            9 => "EBADF",
            // EADDRINUSE：macOS 48 / Linux 98（双平台绑定冲突，9d net）
            48 | 98 => "EADDRINUSE",
            // ECONNREFUSED：macOS 61 / Linux 111（net/http 客户端拒连）
            61 | 111 => "ECONNREFUSED",
            13 => "EACCES",
            17 => "EEXIST",
            18 => "EXDEV",
            20 => "ENOTDIR",
            21 => "EISDIR",
            22 => "EINVAL",
            24 => "EMFILE",
            28 => "ENOSPC",
            32 => "EPIPE",
            36 => "ENAMETOOLONG",
            38 => "ENOSYS", // 10f 非 unix 的 priority 桩（win 记档）
            39 => "ENOTEMPTY",
            40 => "ELOOP",
            _ => "UNKNOWN",
        };
    }
    match e.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        std::io::ErrorKind::IsADirectory => "EISDIR",
        std::io::ErrorKind::InvalidInput => "EINVAL",
        std::io::ErrorKind::OutOfMemory => "ENOMEM",
        _ => "UNKNOWN",
    }
}

/// 权限类别（Phase 8-b）：native 对路径的访问方式，边界检查用。
enum PermClass {
    Read,
    Write,
}

/// 路径实参 + 权限检查（`--allow-*` 沙箱；拒绝即上报并返回 None）。
fn arg_path_checked(
    cx: &mut mozjs::context::JSContext,
    frame: &Frame,
    i: u32,
    what: &str,
    class: PermClass,
) -> Option<String> {
    let path = arg_path(cx, frame, i, what)?;
    let result = match class {
        PermClass::Read => crate::permissions::check_read(&path),
        PermClass::Write => crate::permissions::check_write(&path),
    };
    if let Err(msg) = result {
        report_error(cx, &msg);
        return None;
    }
    Some(path)
}

/// 路径实参（string|URL；`file:` URL 转本地路径；BufferSource 拒之）。
fn arg_path(cx: &mut mozjs::context::JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} needs a path"));
        return None;
    }
    let v = frame.arg(i);
    let s = value_to_string(cx, v);
    // file: URL 形如 file:///tmp/x → 本地路径（Node 行为）。
    if let Ok(url) = url::Url::parse(&s)
        && url.scheme() == "file"
    {
        return url.to_file_path().map(|p| p.to_string_lossy().into_owned()).ok().or_else(|| {
            report_error(cx, &format!("TypeError: {what}: bad file URL"));
            None
        });
    }
    Some(s)
}

/// 数据实参（string 按 utf8；Uint8Array/ArrayBuffer 视图；其余 TypeError）。
fn arg_bytes(
    cx: &mut mozjs::context::JSContext,
    frame: &Frame,
    i: u32,
    what: &str,
) -> Option<Vec<u8>> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} needs data"));
        return None;
    }
    let v = frame.arg(i);
    if v.is_string() {
        return Some(value_to_string(cx, v).into_bytes());
    }
    match view_bytes(cx, v, &format!("{what} data")) {
        Some(b) => Some(b),
        None => None,
    }
}

/// `io::Error` 上报（`{CODE}: {syscall} '{path}': {detail}`；prelude 拆 CODE）。
fn report_io(cx: &mut mozjs::context::JSContext, syscall: &str, path: &str, e: std::io::Error) {
    report_error(cx, &format!("{}: {syscall} '{path}': {e}", io_code(&e)));
}

/// Uint8Array 返回值（`crypto` 同款小 helper，不跨模块引）。
fn set_rval_bytes(cx: &mut mozjs::context::JSContext, frame: &Frame, out: &[u8]) -> bool {
    rooted!(&in(cx) let mut obj: *mut mozjs::jsapi::JSObject = std::ptr::null_mut());
    // SAFETY: realm 内创建；obj 为 rooted 出参；out 存活到调用返回
    let ok = unsafe {
        mozjs::typedarray::TypedArray::<mozjs::typedarray::Uint8, *mut mozjs::jsapi::JSObject>::create(
            cx,
            mozjs::typedarray::CreateWith::Slice(out),
            obj.handle_mut(),
        )
    };
    if ok.is_err() || obj.is_null() {
        report_error(cx, "RangeError: cannot allocate output");
        return false;
    }
    frame.set_rval(mozjs::jsval::ObjectValue(obj.get()));
    true
}

/// 字符串返回值。
fn set_rval_str(cx: &mut mozjs::context::JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

/// `__wjs_fs_read_file(path)` → Uint8Array。
pub unsafe extern "C" fn fs_read_file(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "readFile", PermClass::Read) else {
        return false;
    };
    match fs_err::read(&path) {
        Ok(bytes) => set_rval_bytes(&mut cx, &frame, &bytes),
        Err(e) => {
            report_io(&mut cx, "open", &path, e);
            false
        }
    }
}

/// `__wjs_fs_write_file(path, dataU8, modeNum?)`（mode 仅 unix 生效；0/undefined 跳过）。
pub unsafe extern "C" fn fs_write_file(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(path), Some(data)) = (
        arg_path_checked(&mut cx, &frame, 0, "writeFile", PermClass::Write),
        arg_bytes(&mut cx, &frame, 1, "writeFile"),
    ) else {
        return false;
    };
    let mode = if frame.argc() > 2 && frame.arg(2).is_number() {
        frame.arg(2).to_number() as u32
    } else {
        0
    };
    if let Err(e) = fs_err::write(&path, &data) {
        report_io(&mut cx, "open", &path, e);
        return false;
    }
    #[cfg(unix)]
    if mode != 0 {
        use std::os::unix::fs::PermissionsExt as _;
        let perm = std::fs::Permissions::from_mode(mode & 0o7777);
        if let Err(e) = std::fs::set_permissions(&path, perm) {
            report_io(&mut cx, "chmod", &path, e);
            return false;
        }
    }
    let _ = mode;
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs_fs_append_file(path, dataU8)`。
pub unsafe extern "C" fn fs_append_file(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(path), Some(data)) = (
        arg_path_checked(&mut cx, &frame, 0, "appendFile", PermClass::Write),
        arg_bytes(&mut cx, &frame, 1, "appendFile"),
    ) else {
        return false;
    };
    let r = std::fs::OpenOptions::new().create(true).append(true).open(&path);
    match r {
        Ok(mut f) => {
            use std::io::Write as _;
            match f.write_all(&data) {
                Ok(()) => {
                    frame.set_rval(UndefinedValue());
                    true
                }
                Err(e) => {
                    report_io(&mut cx, "write", &path, e.into());
                    false
                }
            }
        }
        Err(e) => {
            report_io(&mut cx, "open", &path, e);
            false
        }
    }
}

/// 元信息 JSON（stat/lstat 共用；时间毫秒 f64；mode 八进制数）。
fn stat_json(md: &std::fs::Metadata, path: &str) -> String {
    use std::time::UNIX_EPOCH;
    let ms = |t: std::io::Result<std::time::SystemTime>| {
        t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    };
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::MetadataExt as _;
        md.mode()
    };
    #[cfg(not(unix))]
    let mode = if md.is_dir() { 0o777 } else { 0o666 };
    let _ = path;
    // 9c: unix 元字段（MetadataExt/FileTypeExt 全 std；非 unix 归零记档）
    #[cfg(unix)]
    let extra = {
        use std::os::unix::fs::FileTypeExt as _;
        use std::os::unix::fs::MetadataExt as _;
        serde_json::json!({
            "dev": md.dev(), "ino": md.ino(), "nlink": md.nlink(),
            "uid": md.uid(), "gid": md.gid(), "rdev": md.rdev(),
            "blksize": md.blksize(), "blocks": md.blocks(),
            "isFifo": md.file_type().is_fifo(),
            "isSocket": md.file_type().is_socket(),
            "isBlock": md.file_type().is_block_device(),
            "isChar": md.file_type().is_char_device(),
        })
    };
    #[cfg(not(unix))]
    let extra = serde_json::json!({
        "dev": 0, "ino": 0, "nlink": 1, "uid": 0, "gid": 0, "rdev": 0,
        "blksize": 4096, "blocks": 0,
        "isFifo": false, "isSocket": false, "isBlock": false, "isChar": false,
    });
    let mut v = serde_json::json!({
        "size": md.len(),
        "mtimeMs": ms(md.modified()),
        "atimeMs": ms(md.accessed()),
        "birthtimeMs": ms(md.created()),
        "isFile": md.is_file(),
        "isDirectory": md.is_dir(),
        "isSymlink": md.is_symlink(),
        "mode": mode,
    });
    if let Some(obj) = extra.as_object() {
        for (k, val) in obj {
            v[k.as_str()] = val.clone();
        }
    }
    v.to_string()
}

/// `__wjs_fs_stat(path, followLinksBool)` → 元 JSON（stat/lstat 由 prelude 分流）。
pub unsafe extern "C" fn fs_stat(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "stat", PermClass::Read) else {
        return false;
    };
    let follow = frame.argc() < 2 || frame.arg(1) == mozjs::jsval::BooleanValue(true);
    let md = if follow { fs_err::metadata(&path) } else { fs_err::symlink_metadata(&path) };
    match md {
        Ok(md) => {
            set_rval_str(&mut cx, &frame, &stat_json(&md, &path));
            true
        }
        Err(e) => {
            report_io(&mut cx, "stat", &path, e);
            false
        }
    }
}

/// `__wjs_fs_statfs(path)` → StatsFs JSON（M5 vitest 牵引）。
/// unix 经已批准轮子 `nix::sys::statvfs`（§8 直引，零新依赖）；`type` 取
/// filesystem_id（nix 0.31 未暴露 f_type 魔数，记档）；非 unix 报 ENOSYS。
pub unsafe extern "C" fn fs_statfs(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "statfs", PermClass::Read) else {
        return false;
    };
    #[cfg(unix)]
    {
        match nix::sys::statvfs::statvfs(path.as_str()) {
            Ok(st) => {
                let json = serde_json::json!({
                    "type": st.filesystem_id(),
                    "bsize": st.block_size(),
                    "blocks": st.blocks(),
                    "bfree": st.blocks_free(),
                    "bavail": st.blocks_available(),
                    "files": st.files(),
                    "ffree": st.files_free(),
                })
                .to_string();
                set_rval_str(&mut cx, &frame, &json);
                true
            }
            Err(e) => {
                report_io(&mut cx, "statfs", &path, std::io::Error::from(e));
                false
            }
        }
    }
    #[cfg(not(unix))]
    {
        report_io(
            &mut cx,
            "statfs",
            &path,
            std::io::Error::new(std::io::ErrorKind::Unsupported, "statfs not implemented on this platform"),
        );
        false
    }
}

/// `__wjs_fs_mkdir(path, recursiveBool)`。
pub unsafe extern "C" fn fs_mkdir(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "mkdir", PermClass::Write) else {
        return false;
    };
    let recursive = frame.argc() > 1 && frame.arg(1).to_boolean();
    let r = if recursive { fs_err::create_dir_all(&path) } else { fs_err::create_dir(&path) };
    match r {
        Ok(()) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_io(&mut cx, "mkdir", &path, e);
            false
        }
    }
}

/// `__wjs_fs_rm(path, recursiveBool, forceBool)`。
pub unsafe extern "C" fn fs_rm(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "rm", PermClass::Write) else {
        return false;
    };
    let argb = |i: u32| frame.argc() > i && frame.arg(i).to_boolean();
    let (recursive, force) = (argb(1), argb(2));
    let meta = std::fs::symlink_metadata(&path);
    match meta {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && force => {
            frame.set_rval(UndefinedValue());
            return true;
        }
        Err(e) => {
            report_io(&mut cx, "lstat", &path, e);
            return false;
        }
        Ok(md) => {
            let r = if md.is_dir() && !md.is_symlink() {
                if recursive {
                    fs_err::remove_dir_all(&path)
                } else {
                    fs_err::remove_dir(&path)
                }
            } else if let Err(e) = fs_err::remove_file(&path) {
                Err(e)
            } else {
                Ok(())
            };
            match r {
                Ok(()) => {
                    frame.set_rval(UndefinedValue());
                    true
                }
                Err(e) => {
                    report_io(&mut cx, "rm", &path, e);
                    false
                }
            }
        }
    }
}

/// `__wjs_fs_readdir(path, withTypesBool)` → 名数组 / `[name, isDir, isFile][]` JSON。
pub unsafe extern "C" fn fs_readdir(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "readdir", PermClass::Read) else {
        return false;
    };
    let with_types = frame.argc() > 1 && frame.arg(1).to_boolean();
    match fs_err::read_dir(&path) {
        Ok(rd) => {
            let mut names: Vec<String> = Vec::new();
            let mut typed: Vec<serde_json::Value> = Vec::new();
            for entry in rd {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        report_io(&mut cx, "readdir", &path, e);
                        return false;
                    }
                };
                let name = entry.file_name().to_string_lossy().into_owned();
                if with_types {
                    let ft = match entry.file_type() {
                        Ok(t) => t,
                        Err(e) => {
                            report_io(&mut cx, "readdir", &path, e);
                            return false;
                        }
                    };
                    typed.push(serde_json::json!([name, ft.is_dir(), ft.is_file(), ft.is_symlink()]));
                } else {
                    names.push(name);
                }
            }
            if with_types {
                set_rval_str(&mut cx, &frame, &serde_json::Value::Array(typed).to_string());
            } else {
                names.sort();
                set_rval_str(
                    &mut cx,
                    &frame,
                    &serde_json::to_string(&names).unwrap_or_else(|_| "[]".into()),
                );
            }
            true
        }
        Err(e) => {
            report_io(&mut cx, "scandir", &path, e);
            false
        }
    }
}

/// `__wjs_fs_rename(old, new)`。
pub unsafe extern "C" fn fs_rename(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(old), Some(new)) = (
        arg_path_checked(&mut cx, &frame, 0, "rename", PermClass::Read),
        arg_path_checked(&mut cx, &frame, 1, "rename", PermClass::Write),
    ) else {
        return false;
    };
    match fs_err::rename(&old, &new) {
        Ok(()) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_io(&mut cx, "rename", &old, e);
            false
        }
    }
}

/// `__wjs_fs_copy_file(src, dst)`。
pub unsafe extern "C" fn fs_copy_file(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(src), Some(dst)) = (
        arg_path_checked(&mut cx, &frame, 0, "copyFile", PermClass::Read),
        arg_path_checked(&mut cx, &frame, 1, "copyFile", PermClass::Write),
    ) else {
        return false;
    };
    match fs_err::copy(&src, &dst) {
        Ok(_) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_io(&mut cx, "copyfile", &src, e);
            false
        }
    }
}

/// `__wjs_fs_exists(path)` → boolean（缺失/非法一律 false，永不抛；文档记录）。
pub unsafe extern "C" fn fs_exists(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let exists = if frame.argc() < 1 || !frame.arg(0).is_string() {
        false
    } else {
        let p = value_to_string(&mut cx, frame.arg(0));
        if let Err(msg) = crate::permissions::check_read(&p) {
            report_error(&mut cx, &msg);
            return false;
        }
        std::path::Path::new(&p).exists()
    };
    frame.set_rval(mozjs::jsval::BooleanValue(exists));
    true
}

/// `__wjs_fs_realpath(path)` → 规范串。
pub unsafe extern "C" fn fs_realpath(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "realpath", PermClass::Read) else {
        return false;
    };
    match std::fs::canonicalize(&path) {
        Ok(p) => {
            set_rval_str(&mut cx, &frame, &p.to_string_lossy());
            true
        }
        Err(e) => {
            report_io(&mut cx, "lstat", &path, e);
            false
        }
    }
}

/// `__wjs_fs_mkdtemp(prefix)` → 唯一目录串（pid+随机 8 字节 hex；0700）。
pub unsafe extern "C" fn fs_mkdtemp(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(prefix) = arg_path_checked(&mut cx, &frame, 0, "mkdtemp", PermClass::Write) else {
        return false;
    };
    for _ in 0..8 {
        let mut rand = [0u8; 8];
        if getrandom::fill(&mut rand).is_err() {
            report_error(&mut cx, "OperationError: cannot get random values");
            return false;
        }
        let name = format!("{prefix}{}-{:x}", std::process::id(), u64::from_ne_bytes(rand));
        match std::fs::create_dir(&name) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    let _ = std::fs::set_permissions(&name, std::fs::Permissions::from_mode(0o700));
                }
                set_rval_str(&mut cx, &frame, &name);
                return true;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                report_io(&mut cx, "mkdir", &name, e);
                return false;
            }
        }
    }
    report_error(&mut cx, "OperationError: mkdtemp failed (collisions)");
    false
}

// ── Phase 9c：同步面增补（fd 表 + link/symlink/readlink/truncate/utimes/chmod/
// access/open/close/read/write/fstat/fchmod/futimes/fsync）——全 std，无新 crate
//（dependencies2.md §9c 口径：fs-err + std + tokio fs）。
// 记档偏差：fd 为本运行时合成号（自 3 起单调，不复用最小号）；access 的 X_OK
// 近似为 mode 搜索位判定；write with O_APPEND 走 cursor 写。

/// fd 表（合成 fd → File；进程级静态，进程退出由 OS 回收，§4.8 同口径）。
fn fd_table() -> std::sync::MutexGuard<'static, std::collections::BTreeMap<i32, std::fs::File>> {
    static TABLE: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<i32, std::fs::File>>> =
        std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
        .lock()
        .unwrap()
}

static FD_NEXT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(3);

fn fd_num(cx: &mut mozjs::context::JSContext, frame: &Frame, i: u32, what: &str) -> Option<i32> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} needs a file descriptor"));
        return None;
    }
    let v = frame.arg(i);
    if !v.is_number() {
        report_error(cx, &format!("TypeError: {what}: fd must be a number"));
        return None;
    }
    Some(v.to_number() as i32)
}

/// 数字实参（可选，缺省回默认值）。
fn opt_f64(frame: &Frame, i: u32) -> Option<f64> {
    let v = frame.arg(i);
    if v.is_number() { Some(v.to_number()) } else { None }
}

fn bad_fd(cx: &mut mozjs::context::JSContext, syscall: &str) {
    report_error(cx, &format!("EBADF: {syscall}: bad file descriptor"));
}

/// `__wjs_fs_read_link(path)` → 目标字符串。
pub unsafe extern "C" fn fs_read_link(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "readlink", PermClass::Read) else {
        return false;
    };
    match fs_err::read_link(&path) {
        Ok(t) => {
            set_rval_str(&mut cx, &frame, &t.to_string_lossy());
            true
        }
        Err(e) => {
            report_io(&mut cx, "readlink", &path, e);
            false
        }
    }
}

/// `__wjs_fs_link(src, dst)` → 硬链接。
pub unsafe extern "C" fn fs_link(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(src) = arg_path_checked(&mut cx, &frame, 0, "link", PermClass::Read) else {
        return false;
    };
    let Some(dst) = arg_path_checked(&mut cx, &frame, 1, "link", PermClass::Write) else {
        return false;
    };
    match fs_err::hard_link(&src, &dst) {
        Ok(()) => true,
        Err(e) => {
            report_io(&mut cx, "link", &dst, e);
            false
        }
    }
}

/// `__wjs_fs_symlink(target, path)` → 符号链接（type 参数 unix 忽略，Node 同款）。
pub unsafe extern "C" fn fs_symlink(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(target) = arg_path(&mut cx, &frame, 0, "symlink") else {
        return false;
    };
    let Some(path) = arg_path_checked(&mut cx, &frame, 1, "symlink", PermClass::Write) else {
        return false;
    };
    match std::os::unix::fs::symlink(&target, &path) {
        Ok(()) => true,
        Err(e) => {
            report_io(&mut cx, "symlink", &path, e);
            false
        }
    }
}

/// `__wjs_fs_truncate(path, len)` → 截断到 len（缺省 0）。
pub unsafe extern "C" fn fs_truncate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "truncate", PermClass::Write) else {
        return false;
    };
    let len = opt_f64(&frame, 1).unwrap_or(0.0).max(0.0) as u64;
    let open = std::fs::OpenOptions::new().write(true).open(&path);
    match open {
        Ok(f) => match f.set_len(len) {
            Ok(()) => true,
            Err(e) => {
                report_io(&mut cx, "truncate", &path, e);
                false
            }
        },
        Err(e) => {
            report_io(&mut cx, "open", &path, e);
            false
        }
    }
}

fn ms_to_system_time(ms: f64) -> std::time::SystemTime {
    use std::time::{Duration, UNIX_EPOCH};
    if ms <= 0.0 {
        UNIX_EPOCH
    } else {
        UNIX_EPOCH + Duration::from_secs_f64(ms / 1000.0)
    }
}

/// `__wjs_fs_utimes(path, atimeMs, mtimeMs)` → File::set_times（std 1.75+）。
pub unsafe extern "C" fn fs_utimes(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "utimes", PermClass::Write) else {
        return false;
    };
    let atime = opt_f64(&frame, 1).unwrap_or(0.0);
    let mtime = opt_f64(&frame, 2).unwrap_or(atime);
    let open = std::fs::OpenOptions::new().write(true).open(&path);
    match open {
        Ok(f) => {
            let times = std::fs::FileTimes::new()
                .set_accessed(ms_to_system_time(atime))
                .set_modified(ms_to_system_time(mtime));
            match f.set_times(times) {
                Ok(()) => true,
                Err(e) => {
                    report_io(&mut cx, "utimes", &path, e);
                    false
                }
            }
        }
        Err(e) => {
            report_io(&mut cx, "open", &path, e);
            false
        }
    }
}

/// `__wjs_fs_chmod(path, mode)` → set_permissions（PermissionsExt mode 位）。
pub unsafe extern "C" fn fs_chmod(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "chmod", PermClass::Write) else {
        return false;
    };
    let mode = opt_f64(&frame, 1).unwrap_or(0o644 as f64) as u32;
    match fs_err::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(mode)) {
        Ok(()) => true,
        Err(e) => {
            report_io(&mut cx, "chmod", &path, e);
            false
        }
    }
}

/// `__wjs_fs_access(path, mode)` → 可达性判定（X_OK 近似 mode 搜索位，记档）。
pub unsafe extern "C" fn fs_access(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "access", PermClass::Read) else {
        return false;
    };
    let mode = opt_f64(&frame, 1).unwrap_or(0.0) as i32;
    let md = match fs_err::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            report_io(&mut cx, "access", &path, e);
            return false;
        }
    };
    // R_OK/W_OK：试探打开（不创建），PermissionDenied 即 EACCES
    if mode & 4 != 0 {
        if let Err(e) = std::fs::OpenOptions::new().read(true).open(&path) {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                report_error(&mut cx, &format!("EACCES: access '{path}': permission denied"));
                return false;
            }
        }
    }
    if mode & 2 != 0 {
        if let Err(e) = std::fs::OpenOptions::new().write(true).open(&path) {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                report_error(&mut cx, &format!("EACCES: access '{path}': permission denied"));
                return false;
            }
        }
    }
    if mode & 1 != 0 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if md.permissions().mode() & 0o111 == 0 {
                report_error(&mut cx, &format!("EACCES: access '{path}': permission denied"));
                return false;
            }
        }
    }
    true
}

/// open flags JSON：{read,write,append,truncate,create,createNew}（JS 层解析 flags 字符串）。
#[derive(serde::Deserialize)]
struct OpenFlags {
    #[serde(default)]
    read: bool,
    #[serde(default)]
    write: bool,
    #[serde(default)]
    append: bool,
    #[serde(default)]
    truncate: bool,
    #[serde(default)]
    create: bool,
    #[serde(default, rename = "createNew")]
    create_new: bool,
}

/// `__wjs_fs_open(path, flagsJson)` → 合成 fd。
pub unsafe extern "C" fn fs_open(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path(&mut cx, &frame, 0, "open") else {
        return false;
    };
    let flags_s = value_to_string(&mut cx, frame.arg(1));
    let Ok(flags) = serde_json::from_str::<OpenFlags>(&flags_s) else {
        report_error(&mut cx, "TypeError: open: bad flags JSON");
        return false;
    };
    // 权限口径：读意图 check_read、写意图 check_write（读写双开则双查）
    if flags.read {
        if let Err(msg) = crate::permissions::check_read(&path) {
            report_error(&mut cx, &msg);
            return false;
        }
    }
    if flags.write || flags.append {
        if let Err(msg) = crate::permissions::check_write(&path) {
            report_error(&mut cx, &msg);
            return false;
        }
    }
    let mut o = std::fs::OpenOptions::new();
    o.read(flags.read);
    o.write(flags.write || flags.append);
    o.append(flags.append);
    if flags.truncate && !flags.append {
        o.truncate(true);
    }
    if flags.create {
        o.create(true);
    }
    if flags.create_new {
        o.create_new(true);
    }
    match o.open(&path) {
        Ok(f) => {
            let fd = FD_NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            fd_table().insert(fd, f);
            set_rval_str(&mut cx, &frame, &fd.to_string());
            true
        }
        Err(e) => {
            report_io(&mut cx, "open", &path, e);
            false
        }
    }
}

/// `__wjs_fs_close(fd)`。
pub unsafe extern "C" fn fs_close(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(fd) = fd_num(&mut cx, &frame, 0, "close") else {
        return false;
    };
    match fd_table().remove(&fd) {
        Some(_) => true,
        None => {
            bad_fd(&mut cx, "close");
            false
        }
    }
}

/// `__wjs_fs_read_fd(fd, length, positionMs)` → Uint8Array（新视图；position -1 = cursor）。
pub unsafe extern "C" fn fs_read_fd(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(fd) = fd_num(&mut cx, &frame, 0, "read") else {
        return false;
    };
    let length = opt_f64(&frame, 1).unwrap_or(0.0).max(0.0) as usize;
    let position = opt_f64(&frame, 2).map(|p| p.max(-1.0) as i64);
    let mut buf = vec![0u8; length];
    {
        let mut table = fd_table();
        let Some(f) = table.get_mut(&fd) else {
            bad_fd(&mut cx, "read");
            return false;
        };
        use std::io::Read as _;
        let n = match position {
            Some(p) if p >= 0 => {
                // pread 语义：不动 cursor（unix read_at；记档：windows seek_read）
                #[cfg(unix)]
                {
                    use std::os::unix::fs::FileExt as _;
                    f.read_at(&mut buf, p as u64)
                }
                #[cfg(not(unix))]
                {
                    use std::io::Seek as _;
                    let _ = f.seek(std::io::SeekFrom::Start(p as u64));
                    f.read(&mut buf)
                }
            }
            _ => f.read(&mut buf),
        };
        match n {
            Ok(n) => {
                buf.truncate(n);
            }
            Err(e) => {
                report_io(&mut cx, "read", "", e);
                return false;
            }
        }
    }
    set_rval_bytes(&mut cx, &frame, &buf)
}

/// `__wjs_fs_write_fd(fd, dataBytes, positionMs)` → 写入字节数（position -1 = cursor）。
pub unsafe extern "C" fn fs_write_fd(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(fd) = fd_num(&mut cx, &frame, 0, "write") else {
        return false;
    };
    let Some(data) = arg_bytes(&mut cx, &frame, 1, "write") else {
        return false;
    };
    let position = opt_f64(&frame, 2).map(|p| p.max(-1.0) as i64);
    let mut table = fd_table();
    let Some(f) = table.get_mut(&fd) else {
        bad_fd(&mut cx, "write");
        return false;
    };
    use std::io::Write as _;
    let n = match position {
        Some(p) if p >= 0 => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileExt as _;
                f.write_at(&data, p as u64)
            }
            #[cfg(not(unix))]
            {
                use std::io::Seek as _;
                let _ = f.seek(std::io::SeekFrom::Start(p as u64));
                f.write(&data)
            }
        }
        _ => f.write(&data),
    };
    match n {
        Ok(n) => {
            set_rval_str(&mut cx, &frame, &n.to_string());
            true
        }
        Err(e) => {
            report_io(&mut cx, "write", "", e);
            false
        }
    }
}

/// `__wjs_fs_ftruncate(fd, len)`。
pub unsafe extern "C" fn fs_ftruncate(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(fd) = fd_num(&mut cx, &frame, 0, "ftruncate") else {
        return false;
    };
    let len = opt_f64(&frame, 1).unwrap_or(0.0).max(0.0) as u64;
    let mut table = fd_table();
    let Some(f) = table.get_mut(&fd) else {
        bad_fd(&mut cx, "ftruncate");
        return false;
    };
    match f.set_len(len) {
        Ok(()) => true,
        Err(e) => {
            report_io(&mut cx, "ftruncate", "", e);
            false
        }
    }
}

/// `__wjs_fs_fstat(fd)` → 元 JSON（stat_json 复用）。
pub unsafe extern "C" fn fs_fstat(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(fd) = fd_num(&mut cx, &frame, 0, "fstat") else {
        return false;
    };
    let table = fd_table();
    let Some(f) = table.get(&fd) else {
        bad_fd(&mut cx, "fstat");
        return false;
    };
    match f.metadata() {
        Ok(md) => {
            set_rval_str(&mut cx, &frame, &stat_json(&md, ""));
            true
        }
        Err(e) => {
            report_io(&mut cx, "fstat", "", e);
            false
        }
    }
}

/// `__wjs_fs_fchmod(fd, mode)` → File::set_permissions。
pub unsafe extern "C" fn fs_fchmod(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(fd) = fd_num(&mut cx, &frame, 0, "fchmod") else {
        return false;
    };
    let mode = opt_f64(&frame, 1).unwrap_or(0o644 as f64) as u32;
    let mut table = fd_table();
    let Some(f) = table.get_mut(&fd) else {
        bad_fd(&mut cx, "fchmod");
        return false;
    };
    match f.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(mode)) {
        Ok(()) => true,
        Err(e) => {
            report_io(&mut cx, "fchmod", "", e);
            false
        }
    }
}

/// `__wjs_fs_futimes(fd, atimeMs, mtimeMs)` → File::set_times。
pub unsafe extern "C" fn fs_futimes(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(fd) = fd_num(&mut cx, &frame, 0, "futimes") else {
        return false;
    };
    let atime = opt_f64(&frame, 1).unwrap_or(0.0);
    let mtime = opt_f64(&frame, 2).unwrap_or(atime);
    let mut table = fd_table();
    let Some(f) = table.get_mut(&fd) else {
        bad_fd(&mut cx, "futimes");
        return false;
    };
    let times = std::fs::FileTimes::new()
        .set_accessed(ms_to_system_time(atime))
        .set_modified(ms_to_system_time(mtime));
    match f.set_times(times) {
        Ok(()) => true,
        Err(e) => {
            report_io(&mut cx, "futimes", "", e);
            false
        }
    }
}

/// `__wjs_fs_fsync(fd, datasyncBool)` → sync_all/sync_data。
pub unsafe extern "C" fn fs_fsync(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(fd) = fd_num(&mut cx, &frame, 0, "fsync") else {
        return false;
    };
    let datasync = frame.argc() > 1 && frame.arg(1).to_boolean();
    let mut table = fd_table();
    let Some(f) = table.get_mut(&fd) else {
        bad_fd(&mut cx, "fsync");
        return false;
    };
    let r = if datasync { f.sync_data() } else { f.sync_all() };
    match r {
        Ok(()) => true,
        Err(e) => {
            report_io(&mut cx, "fsync", "", e);
            false
        }
    }
}

// ── fs.watch（`notify` 线程 → 防抖线程 → 事件循环；300ms 静默窗，与 test --watch 同值）
// 防抖说明：`notify-debouncer-mini`（批准单内轮子）只给 `Any`/`AnyContinuous`
//（kind 丢失），`fs.watch` 的 `rename`/`change` 区分会被吃掉——此处用共享防抖线程
// 手写 coalesce（按 `(id, kind, file)` 键 300ms 静默窗，kind 原样保留；失败事件直通）。
// 线程模型与既有 `notify` 线程一致（Rust 侧多线程只经 channel 与 JS 线程通信，§6 合规）。

/// 防抖窗（与 `testrun.rs` 的 300ms 同值，用户感知一致）。
pub(crate) const WATCH_DEBOUNCE_MS: u64 = 300;

/// 防抖线程输入（分类已做完的纯数据）。
struct RawWatch {
    id: u64,
    kind: WatchKind,
}

type DebounceKey = (u64, String, Option<String>);

/// 共享防抖线程入口（`OnceLock` 懒起，存 `Result` 以兼容 stable；
/// 发送端掉光即退出）。
static DEBOUNCE_TX: std::sync::OnceLock<Result<std::sync::mpsc::Sender<RawWatch>, String>> =
    std::sync::OnceLock::new();

fn debounce_handle(
    js_tx: &tokio::sync::mpsc::UnboundedSender<WatchEvent>,
) -> Result<std::sync::mpsc::Sender<RawWatch>, String> {
    DEBOUNCE_TX
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<RawWatch>();
            let js_tx = js_tx.clone();
            match std::thread::Builder::new()
                .name("wjs-fs-watch-debounce".into())
                .spawn(move || debounce_loop(rx, js_tx))
            {
                Ok(_) => Ok(tx),
                Err(e) => Err(format!("cannot start watch debounce thread: {e}")),
            }
        })
        .clone()
}

fn debounce_loop(
    rx: std::sync::mpsc::Receiver<RawWatch>,
    js_tx: tokio::sync::mpsc::UnboundedSender<WatchEvent>,
) {
    use std::time::{Duration, Instant};
    let window = Duration::from_millis(WATCH_DEBOUNCE_MS);
    // 插入序 Vec（不用 HashMap）：同键只保留首事件并刷新 deadline，刷出按到达序——
    // 新文件 Create+Modify 双事件时 rename（先到）稳定赢（flaky 修，见黑盒 watch 用例）。
    // 事件量极小（人手/测试级），O(n) 扫描可接受。
    let mut pending: Vec<(DebounceKey, (Instant, WatchEvent))> = Vec::new();
    loop {
        let now = Instant::now();
        // 到期即刷（保持到达序）
        let mut i = 0;
        while i < pending.len() {
            if pending[i].1.0 <= now {
                let (_, (_, ev)) = pending.remove(i);
                if js_tx.send(ev).is_err() {
                    return;
                }
            } else {
                i += 1;
            }
        }
        let wait = pending
            .iter()
            .map(|(_, (d, _))| d.checked_duration_since(Instant::now()).unwrap_or(Duration::ZERO))
            .min()
            .unwrap_or(window);
        match rx.recv_timeout(wait) {
            Ok(raw) => {
                let id = raw.id;
                match raw.kind {
                    // 失败直通（不防抖，尽早报错）
                    WatchKind::Failed(msg) => {
                        if js_tx.send(WatchEvent { id, kind: WatchKind::Failed(msg) }).is_err() {
                            return;
                        }
                    }
                    WatchKind::Fired { event, file } => {
                        let key = (id, event.clone(), file.clone());
                        if let Some(slot) = pending.iter_mut().find(|(k, _)| *k == key) {
                            slot.1.0 = Instant::now() + window;
                        } else {
                            pending.push((
                                key,
                                (Instant::now() + window, WatchEvent { id, kind: WatchKind::Fired { event, file } }),
                            ));
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// notify 线程 → 事件循环（纯数据；`kind` 为 `rename`/`change`）。
pub struct WatchEvent {
    pub id: u64,
    pub kind: WatchKind,
}

pub enum WatchKind {
    Fired { event: String, file: Option<String> },
    Failed(String),
}

/// notify 事件 → Node `rename`/`change`（Access/Other 忽略，返回 None）。
fn watch_classify(kind: &notify::EventKind) -> Option<&'static str> {
    match kind {
        notify::EventKind::Create(_) | notify::EventKind::Remove(_) => Some("rename"),
        notify::EventKind::Modify(_) => Some("change"),
        _ => None,
    }
}

/// `__wjs_watch_start(path, recursiveBool, persistentBool, listener)` → id。
/// 路径不存在即报（`watch` 前置校验；`notify` 自身错误走 Failed 事件）。
pub unsafe extern "C" fn watch_start(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 4 || !frame.arg(3).is_object() {
        report_error(&mut cx, "TypeError: watch needs path, flags and listener");
        return false;
    }
    let (Some(path), recursive, persistent, listener) = (
        arg_path_checked(&mut cx, &frame, 0, "watch", PermClass::Read),
        frame.argc() > 1 && frame.arg(1) == mozjs::jsval::BooleanValue(true),
        frame.argc() <= 2 || frame.arg(2) != mozjs::jsval::BooleanValue(false),
        frame.arg(3),
    ) else {
        return false;
    };
    if !std::path::Path::new(&path).exists() {
        report_error(&mut cx, &format!("ENOENT: watch '{path}'"));
        return false;
    }
    let Some((id, tx)) = state::watch_alloc() else {
        report_error(&mut cx, "failed to load settings: watch driver not installed");
        return false;
    };
    let Ok(dtx) = debounce_handle(&tx) else {
        report_error(&mut cx, "OperationError: watch failed (debounce thread)");
        return false;
    };
    // js 直通道退役：事件统一经防抖线程进事件循环；tx 仅用于懒起线程
    drop(tx);
    let mode = if recursive {
        notify::RecursiveMode::Recursive
    } else {
        notify::RecursiveMode::NonRecursive
    };
    let watched = path.clone();
    let build: Result<notify::RecommendedWatcher, String> = (|| {
        use notify::Watcher as _;
        // notify 回调只做分类（纯数据），防抖由共享线程做（300ms 静默窗，kind 保留）。
        let mut watcher =
            notify::RecommendedWatcher::new(move |res: Result<notify::Event, notify::Error>| {
                match res {
                    Ok(ev) => {
                        let Some(kind) = watch_classify(&ev.kind) else {
                            return;
                        };
                        let file = ev.paths.first().and_then(|p| {
                            p.file_name().map(|n| n.to_string_lossy().into_owned())
                        });
                        let _ = dtx.send(RawWatch {
                            id,
                            kind: WatchKind::Fired { event: kind.to_string(), file },
                        });
                    }
                    Err(e) => {
                        let _ = dtx.send(RawWatch { id, kind: WatchKind::Failed(e.to_string()) });
                    }
                }
            }, notify::Config::default())
            .map_err(|e| e.to_string())?;
        watcher.watch(std::path::Path::new(&watched), mode).map_err(|e| e.to_string())?;
        Ok(watcher)
    })();
    match build {
        Ok(driver) => {
            state::watch_add(id, driver, listener, persistent);
            tracing::info!(target: "winterjs::watch", id, path = path.as_str(), recursive, "watch started");
            frame.set_rval(mozjs::jsval::Int32Value(id as i32));
            true
        }
        Err(e) => {
            report_error(&mut cx, &format!("OperationError: watch failed: {e}"));
            false
        }
    }
}

/// `__wjs_watch_close(id)`（幂等；残留事件落空）。
pub unsafe extern "C" fn watch_close(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: watch close needs a numeric id");
        return false;
    }
    state::watch_remove(frame.arg(0).to_number() as u64);
    frame.set_rval(UndefinedValue());
    true
}

/// 事件循环分发一条 watch 事件（监听保留，多次触发；失败摘除并 WARN）。
/// 前置条件：cx 已进入 global 所属 realm（事件循环上下文，`call_two` 合规）。
pub fn dispatch(
    cx: &mut JSContext,
    global: *mut JSObject,
    ev: WatchEvent,
    err: crate::runtime::ErrorSource<'_>,
) -> Result<(), crate::error::Error> {
    use crate::jsapi_glue::call_two;
    let failed = |cx: &mut JSContext| match err {
        crate::runtime::ErrorSource::Script { source, filename } => {
            crate::jsapi_glue::pending_exception_error(cx, global, source, filename)
        }
        crate::runtime::ErrorSource::Module { url } => crate::modules::module_error(cx, url),
    };
    match ev.kind {
        WatchKind::Fired { event, file } => {
            let Some(listener) = state::watch_listener(ev.id) else {
                return Ok(());
            };
            rooted!(&in(cx) let mut event_v = UndefinedValue());
            event.to_jsval(cx, event_v.handle_mut());
            rooted!(&in(cx) let mut file_v = UndefinedValue());
            match file {
                Some(f) => f.to_jsval(cx, file_v.handle_mut()),
                None => mozjs::jsval::NullValue().to_jsval(cx, file_v.handle_mut()),
            }
            if call_two(cx, global, listener, event_v.get(), file_v.get()).is_some() {
                Ok(())
            } else {
                Err(failed(cx))
            }
        }
        WatchKind::Failed(message) => {
            // 溢出类错误：摘除该路（监听不再触发），WARN 留痕后继续循环。
            state::watch_remove(ev.id);
            tracing::warn!(target: "winterjs::watch", id = ev.id, message = message.as_str(), "watch failed, removed");
            Ok(())
        }
    }
}

/// `__wjs_fs_unlink(path)`（`rm` 子集，单文件）。
pub unsafe extern "C" fn fs_unlink(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "unlink", PermClass::Write) else {
        return false;
    };
    match fs_err::remove_file(&path) {
        Ok(()) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_io(&mut cx, "unlink", &path, e);
            false
        }
    }
}

/// `__wjs_fs_rmdir(path, recursiveBool)`（`rm` 子集，目录）。
pub unsafe extern "C" fn fs_rmdir(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "rmdir", PermClass::Write) else {
        return false;
    };
    let recursive = frame.argc() > 1 && frame.arg(1) == mozjs::jsval::BooleanValue(true);
    let r = if recursive { fs_err::remove_dir_all(&path) } else { fs_err::remove_dir(&path) };
    match r {
        Ok(()) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(e) => {
            report_io(&mut cx, "rmdir", &path, e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_code_mapping() {
        assert_eq!(io_code(&std::io::Error::from(std::io::ErrorKind::NotFound)), "ENOENT");
        assert_eq!(io_code(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)), "EACCES");
        assert_eq!(io_code(&std::io::Error::from(std::io::ErrorKind::AlreadyExists)), "EEXIST");
        assert_eq!(io_code(&std::io::Error::from(std::io::ErrorKind::IsADirectory)), "EISDIR");
        assert_eq!(
            io_code(&std::io::Error::from_raw_os_error(39)),
            "ENOTEMPTY"
        );
        // 9c：fd 系读写路径报错面（EBADF）；9d：绑定冲突（macOS/Linux 双 errno）
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(9)), "EBADF");
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(48)), "EADDRINUSE");
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(98)), "EADDRINUSE");
        // 10f：priority 面（ESRCH）+ 非 unix 桩（ENOSYS）
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(3)), "ESRCH");
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(38)), "ENOSYS");
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(9999)), "UNKNOWN");
    }
}

/// 内嵌 ESM 源（`node:fs`；错误带 `.code/.syscall/.path`；偏差见头注）。
pub const SOURCE: &str = r#"
function __fsErr(e, syscall, path) {
  const m = String((e && e.message) || e);
  // 权限拒绝直通（不套 io 形状；Deno NotCapable 同款可读错）
  if (m.startsWith("PermissionError:")) {
    const perr = new Error(m.slice("PermissionError: ".length));
    perr.name = "PermissionError";
    perr.path = path;
    throw perr;
  }
  const code = (m.match(/^([A-Z_]+): /) || [])[1] || "UNKNOWN";
  const rest = m.replace(/^[A-Z_]+: /, "");
  const err = new Error(`${code}: ${syscall} '${path}' ${rest}`.trim());
  err.code = code;
  err.syscall = syscall;
  err.path = path;
  throw err;
}
function __fsCall(syscall, path, fn) {
  try {
    return fn();
  } catch (e) {
    __fsErr(e, syscall, path);
  }
}
function __fsPath(p, what) {
  if (typeof p === "string") return p;
  if (p instanceof URL) {
    if (p.protocol !== "file:") throw new TypeError(`${what}: only file: URLs supported`);
    return p;
  }
  throw new TypeError(`${what}: path must be a string or file: URL`);
}
function __fsData(d, what) {
  if (typeof d === "string") return new TextEncoder().encode(d);
  if (d instanceof Uint8Array) return d;
  if (d instanceof ArrayBuffer) return new Uint8Array(d);
  if (ArrayBuffer.isView(d)) return new Uint8Array(d.buffer, d.byteOffset, d.byteLength);
  throw new TypeError(`${what}: data must be string or BufferSource`);
}
function __fsEncoding(opts) {
  if (opts === undefined || opts === null) return null;
  if (typeof opts === "string") return opts;
  return opts.encoding ?? null;
}
function __fsMode(opts) {
  if (opts && typeof opts === "object" && opts.mode !== undefined) {
    const n = Number(opts.mode);
    if (!Number.isInteger(n) || n < 0) throw new TypeError("mode must be an integer");
    return n;
  }
  return 0;
}
function __fsDecode(bytes, encoding, what) {
  if (encoding === null) {
    // Node 语义：无编码读返回 Buffer（非裸 Uint8Array）——String(buf)/
    // toString() = utf8 内容、isBuffer = true。裸 Uint8Array 会 join 成
    // "byte,byte,…"（vite PostCSS 配置加载 JSON.parse(buf) 实测误判）。
    return Buffer.from(bytes);
  }
  return new TextDecoder(String(encoding)).decode(bytes);
}
class __Stats {
  constructor(j) {
    this.dev = j.dev ?? 0;
    this.ino = j.ino ?? 0;
    this.mode = j.mode;
    this.nlink = j.nlink ?? 1;
    this.uid = j.uid ?? 0;
    this.gid = j.gid ?? 0;
    this.rdev = j.rdev ?? 0;
    this.size = j.size;
    this.blksize = j.blksize ?? 4096;
    this.blocks = j.blocks ?? 0;
    this.atimeMs = j.atimeMs;
    this.mtimeMs = j.mtimeMs;
    this.ctimeMs = j.mtimeMs;
    this.birthtimeMs = j.birthtimeMs;
    this.atime = new Date(j.atimeMs);
    this.mtime = new Date(j.mtimeMs);
    this.ctime = new Date(j.mtimeMs);
    this.birthtime = new Date(j.birthtimeMs);
    this.__f = j.isFile;
    this.__d = j.isDirectory;
    this.__l = j.isSymlink;
    this.__fifo = j.isFifo;
    this.__sock = j.isSocket;
    this.__blk = j.isBlock;
    this.__chr = j.isChar;
  }
  isFile() { return this.__f; }
  isDirectory() { return this.__d; }
  isSymbolicLink() { return this.__l; }
  isFIFO() { return !!this.__fifo; }
  isSocket() { return !!this.__sock; }
  isBlockDevice() { return !!this.__blk; }
  isCharacterDevice() { return !!this.__chr; }
}
class __Dirent {
  constructor(name, isDir, isFile, isLink) {
    this.name = name;
    this.__d = isDir;
    this.__f = isFile;
    this.__l = isLink;
  }
  isFile() { return this.__f; }
  isDirectory() { return this.__d; }
  isSymbolicLink() { return this.__l; }
  isBlockDevice() { return false; }
  isCharacterDevice() { return false; }
  isFIFO() { return false; }
  isSocket() { return false; }
}
// StatsFs 纯数据面（type/bsize/blocks/bfree/bavail/files/ffree，无方法，Node 口径）。
class __StatsFs {
  constructor(j) {
    this.type = j.type ?? 0;
    this.bsize = j.bsize ?? 4096;
    this.blocks = j.blocks ?? 0;
    this.bfree = j.bfree ?? 0;
    this.bavail = j.bavail ?? 0;
    this.files = j.files ?? 0;
    this.ffree = j.ffree ?? 0;
  }
}
// flags 字符串 → OpenFlags JSON（Node 口径子集；`s` 后缀忽略；数字只认本运行时
// constants 暴露的位值，O_CREAT/O_EXCL/O_TRUNC/O_APPEND 用 Linux 位值，记档）。
function __fsFlags(flag, what) {
  if (typeof flag === "number") {
    const rd = (flag & 3) !== 1;
    const wr = (flag & 3) !== 0;
    return JSON.stringify({
      read: rd, write: wr, append: (flag & 1024) !== 0,
      truncate: (flag & 512) !== 0, create: (flag & 64) !== 0, createNew: (flag & 128) !== 0,
    });
  }
  let f = String(flag ?? "r").replace(/s/g, "");
  const base = {
    r: { read: true },
    "r+": { read: true, write: true },
    w: { write: true, truncate: true, create: true },
    "w+": { read: true, write: true, truncate: true, create: true },
    a: { write: true, append: true, create: true },
    "a+": { read: true, write: true, append: true, create: true },
    wx: { write: true, truncate: true, createNew: true },
    "wx+": { read: true, write: true, truncate: true, createNew: true },
    ax: { write: true, append: true, createNew: true },
    "ax+": { read: true, write: true, append: true, createNew: true },
  };
  if (!base[f]) throw new TypeError(`${what}: invalid flag '${flag}'`);
  return JSON.stringify(base[f]);
}
function __fsReadWhole(p, flag) {
  if (flag === undefined || flag === "r") return __wjs_fs_read_file(p);
  const fd = __wjs_fs_open(p, __fsFlags(flag, "readFile"));
  try {
    const parts = [];
    while (true) {
      const chunk = __wjs_fs_read_fd(fd, 1 << 20, -1);
      if (chunk.length === 0) break;
      parts.push(chunk);
    }
    const out = new Uint8Array(parts.reduce((a, c) => a + c.length, 0));
    let off = 0;
    for (const c of parts) { out.set(c, off); off += c.length; }
    return out;
  } finally {
    __wjs_fs_close(fd);
  }
}
export function readFileSync(p, opts) {
  p = __fsPath(p, "readFile");
  const enc = __fsEncoding(opts);
  const flag = opts && typeof opts === "object" ? opts.flag : undefined;
  return __fsCall("open", p, () => __fsDecode(__fsReadWhole(p, flag), enc, "readFile"));
}
export function writeFileSync(p, data, opts) {
  p = __fsPath(p, "writeFile");
  const flag = opts && typeof opts === "object" ? opts.flag : undefined;
  const bytes = __fsData(data, "writeFile");
  __fsCall("open", p, () => {
    if (flag === undefined || flag === "w") {
      __wjs_fs_write_file(p, bytes, __fsMode(opts));
      return;
    }
    const fd = __wjs_fs_open(p, __fsFlags(flag, "writeFile"));
    try { __wjs_fs_write_fd(fd, bytes, flag.startsWith("a") ? -1 : 0); }
    finally { __wjs_fs_close(fd); }
  });
  // mode 语义：仅新建文件时应用（存在性预判，记档近似）
  const mode = __fsMode(opts);
  if (mode > 0 && !existsSync(p)) chmodSync(p, mode);
}
export function appendFileSync(p, data, opts) {
  p = __fsPath(p, "appendFile");
  __fsCall("open", p, () => __wjs_fs_append_file(p, __fsData(data, "appendFile"), __fsMode(opts)));
}
export function statSync(p) {
  p = __fsPath(p, "stat");
  return new __Stats(JSON.parse(__fsCall("stat", p, () => __wjs_fs_stat(p, true))));
}
// 文件系统级状态（M5 vitest 牵引；unix 经 statvfs，type 取 filesystem_id 记档）。
export function statfsSync(p) {
  p = __fsPath(p, "statfs");
  return new __StatsFs(JSON.parse(__fsCall("statfs", p, () => __wjs_fs_statfs(p))));
}
export function lstatSync(p) {
  p = __fsPath(p, "lstat");
  return new __Stats(JSON.parse(__fsCall("stat", p, () => __wjs_fs_stat(p, false))));
}
export function existsSync(p) {
  try {
    return __wjs_fs_exists(__fsPath(p, "exists"));
  } catch {
    return false;
  }
}
export function mkdirSync(p, opts) {
  p = __fsPath(p, "mkdir");
  const recursive = !!(opts && (opts.recursive ?? false));
  __fsCall("mkdir", p, () => __wjs_fs_mkdir(p, recursive));
}
export function rmSync(p, opts) {
  p = __fsPath(p, "rm");
  const recursive = !!(opts && (opts.recursive ?? false));
  const force = !!(opts && (opts.force ?? false));
  __fsCall("rm", p, () => __wjs_fs_rm(p, recursive, force));
}
export function rmdirSync(p, opts) {
  p = __fsPath(p, "rmdir");
  const recursive = !!(opts && (opts.recursive ?? false));
  __fsCall("rmdir", p, () => __wjs_fs_rmdir(p, recursive));
}
export function unlinkSync(p) {
  p = __fsPath(p, "unlink");
  __fsCall("unlink", p, () => __wjs_fs_unlink(p));
}
export function readdirSync(p, opts) {
  p = __fsPath(p, "readdir");
  const withTypes = !!(opts && (opts.withFileTypes ?? false));
  const out = JSON.parse(__fsCall("scandir", p, () => __wjs_fs_readdir(p, withTypes)));
  if (!withTypes) return out;
  return out.map(([name, isDir, isFile, isLink]) => new __Dirent(name, isDir, isFile, isLink));
}
export function renameSync(a, b) {
  a = __fsPath(a, "rename");
  b = __fsPath(b, "rename");
  __fsCall("rename", a, () => __wjs_fs_rename(a, b));
}
export function copyFileSync(src, dst) {
  src = __fsPath(src, "copyFile");
  dst = __fsPath(dst, "copyFile");
  __fsCall("copyfile", src, () => __wjs_fs_copy_file(src, dst));
}
export function realpathSync(p) {
  p = __fsPath(p, "realpath");
  return __fsCall("lstat", p, () => __wjs_fs_realpath(p));
}
// Node 口径：.native = binding 级 realpath（无 JS 层缓存/规范化）。本仓两者
// 同底座（std canonicalize），直接自引用（vite 8 的 safeRealpathSync 取此面）。
realpathSync.native = realpathSync;
export function mkdtempSync(prefix) {
  return __fsCall("mkdir", String(prefix), () => __wjs_fs_mkdtemp(String(prefix)));
}
export const constants = {
  F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1,
  O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 64, O_EXCL: 128, O_TRUNC: 512, O_APPEND: 1024,
  S_IFMT: 61440, S_IFREG: 32768, S_IFDIR: 16384, S_IFLNK: 40960,
  COPYFILE_EXCL: 1, COPYFILE_FICLONE: 2, COPYFILE_FICLONE_FORCE: 4,
};
class __FSWatcher {
  #id;
  constructor(id) { this.#id = id; }
  close() { __wjs_watch_close(this.#id); }
  get closed() { return false; }
}
export function watch(p, opts, listener) {
  if (typeof opts === "function") { listener = opts; opts = {}; }
  if (typeof listener !== "function") throw new TypeError("watch: listener must be a function");
  p = __fsPath(p, "watch");
  const recursive = !!(opts && opts.recursive);
  const persistent = !(opts && opts.persistent === false);
  const id = __fsCall("watch", p, () => __wjs_watch_start(p, recursive, persistent, listener));
  return new __FSWatcher(id);
}
// stat 轮询表（watchFile 底座；interval 経 setInterval，statSync 取样）。
// 偏差记档：persistent:false 不实际 unref（定时器 keep-alive 由 Rust 表决定，
// 全局 Timeout 语义见 mod.rs）；stat 失败的 tick 跳过（不派发，缺失→出现视为
// 一次变化）；bigint 选项接受忽略（恒返回数字 Stats，chokidar 等调用方无影响）。
const __statWatchers = new Map();
function __statPoll(p) {
  const rec = __statWatchers.get(p);
  if (!rec) return;
  let curr = null;
  try { curr = statSync(p); } catch { curr = null; }
  const prev = rec.prev;
  rec.prev = curr;
  if (curr === null || prev === null) {
    if ((curr === null) !== (prev === null)) {
      for (const l of [...rec.listeners]) l(curr ?? prev, prev ?? curr);
      for (const l of [...rec.changeListeners]) l(curr ?? prev, prev ?? curr);
    }
    return;
  }
  if (curr.size !== prev.size || curr.mtimeMs !== prev.mtimeMs) {
    for (const l of [...rec.listeners]) l(curr, prev);
    for (const l of [...rec.changeListeners]) l(curr, prev);
  }
}
class __StatWatcher {
  #path;
  #listener;
  constructor(p, listener) { this.#path = p; this.#listener = listener; }
  stop() { unwatchFile(this.#path, this.#listener); return this; }
  close() { return this.stop(); }
  ref() { return this; }
  unref() { return this; }
  on(type, cb) {
    if (type === "change" && typeof cb === "function") {
      const rec = __statWatchers.get(this.#path);
      if (rec) rec.changeListeners.add(cb);
    }
    return this;
  }
  off(type, cb) {
    if (type === "change") {
      const rec = __statWatchers.get(this.#path);
      if (rec && cb) rec.changeListeners.delete(cb);
    }
    return this;
  }
}
export function watchFile(p, opts, listener) {
  if (typeof opts === "function") { listener = opts; opts = {}; }
  if (typeof listener !== "function") throw new TypeError("watchFile: listener must be a function");
  p = __fsPath(p, "watchFile");
  const interval = (opts && Number(opts.interval) > 0) ? Number(opts.interval) : 5007;
  let rec = __statWatchers.get(p);
  if (!rec) {
    rec = { listeners: new Set(), changeListeners: new Set(), prev: null, timer: null };
    try { rec.prev = statSync(p); } catch { rec.prev = null; }
    rec.timer = setInterval(() => __statPoll(p), interval);
    __statWatchers.set(p, rec);
  }
  rec.listeners.add(listener);
  return new __StatWatcher(p, listener);
}
export function unwatchFile(p, listener) {
  p = __fsPath(p, "unwatchFile");
  const rec = __statWatchers.get(p);
  if (!rec) return;
  if (typeof listener === "function") rec.listeners.delete(listener);
  else rec.listeners.clear();
  if (rec.listeners.size === 0 && rec.changeListeners.size === 0) {
    clearInterval(rec.timer);
    __statWatchers.delete(p);
  }
}
// ---- fs 流（同步底层 + Web 流外形；口径见头注）----
// 口径（文档记录）：createReadStream 返回 Web ReadableStream（整文件读入后按
// highWaterMark 切块；async 迭代/getReader 可用；Node 的 .on('data') 事件式
// 接口不在此列，用 for await 替代）；createWriteStream 返回 Web WritableStream
//（块先攒，close 时一次性落盘；flags `a` 表追加，其余覆盖）。
export function createReadStream(p, opts) {
  p = __fsPath(p, "createReadStream");
  const hwm = opts && opts.highWaterMark !== undefined ? Number(opts.highWaterMark) : 65536;
  const bytes = __fsCall("open", p, () => __wjs_fs_read_file(p));
  const size = Number.isFinite(hwm) && hwm > 0 ? Math.floor(hwm) : 65536;
  let off = 0;
  return new ReadableStream({
    pull(c) {
      if (off >= bytes.length) { c.close(); return; }
      const end = Math.min(bytes.length, off + size);
      c.enqueue(bytes.slice(off, end));
      off = end;
      if (off >= bytes.length) c.close();
    },
    cancel() {},
  });
}
export function createWriteStream(p, opts) {
  p = __fsPath(p, "createWriteStream");
  const append = !!(opts && (opts.flags === "a" || opts.flags === "a+"));
  const chunks = [];
  let total = 0;
  return new WritableStream({
    write(chunk) {
      const u8 = __fsData(chunk, "createWriteStream");
      chunks.push(u8);
      total += u8.length;
    },
    close() {
      const out = new Uint8Array(total);
      let off = 0;
      for (const c of chunks) { out.set(c, off); off += c.length; }
      if (append && __fsCall("stat", p, () => { try { __wjs_fs_stat(p, true); return true; } catch { return false; } })) {
        __fsCall("open", p, () => __wjs_fs_append_file(p, out, 0));
      } else {
        __fsCall("open", p, () => __wjs_fs_write_file(p, out, 0));
      }
    },
  });
}
// ---- Phase 9c：同步面增补（link 系/时间戳/权限/access/fd 系/cp/opendir）----
function __fsTimeMs(t, what) {
  if (t instanceof Date) return t.getTime();
  if (typeof t === "number") return t;
  if (typeof t === "string") { const n = Number(t); if (Number.isFinite(n)) return n; }
  throw new TypeError(`${what}: time must be a number, string or Date`);
}
function __fsModeNum(mode) {
  if (typeof mode === "string") {
    const n = parseInt(mode, 8);
    if (!Number.isInteger(n) || n < 0) throw new TypeError("chmod: invalid mode");
    return n;
  }
  const n = Number(mode);
  if (!Number.isInteger(n) || n < 0) throw new TypeError("chmod: mode must be an integer");
  return n;
}
export function accessSync(p, mode = 0) {
  p = __fsPath(p, "access");
  __fsCall("access", p, () => __wjs_fs_access(p, mode));
}
export function truncateSync(p, len) {
  p = __fsPath(p, "truncate");
  __fsCall("truncate", p, () => __wjs_fs_truncate(p, len ?? 0));
}
export function utimesSync(p, atime, mtime) {
  p = __fsPath(p, "utimes");
  __fsCall("utimes", p, () => __wjs_fs_utimes(p, __fsTimeMs(atime, "utimes"), __fsTimeMs(mtime, "utimes")));
}
export function chmodSync(p, mode) {
  p = __fsPath(p, "chmod");
  __fsCall("chmod", p, () => __wjs_fs_chmod(p, __fsModeNum(mode)));
}
export function linkSync(a, b) {
  a = __fsPath(a, "link");
  b = __fsPath(b, "link");
  __fsCall("link", a, () => __wjs_fs_link(a, b));
}
export function symlinkSync(target, p) {
  target = __fsPath(target, "symlink");
  p = __fsPath(p, "symlink");
  __fsCall("symlink", p, () => __wjs_fs_symlink(target, p));
}
export function readlinkSync(p) {
  p = __fsPath(p, "readlink");
  return __fsCall("readlink", p, () => __wjs_fs_read_link(p));
}
export function cpSync(src, dst, opts = {}) {
  src = __fsPath(src, "cp");
  dst = __fsPath(dst, "cp");
  const force = opts.force ?? true;
  const errorOnExist = opts.errorOnExist ?? false;
  const recursive = opts.recursive ?? false;
  const st = statSync(src);
  if (st.isDirectory()) {
    if (!recursive) throw new Error(`ERR_FS_EISDIR: cp '${src}': is a directory (recursive required)`);
    __fsCall("cp", dst, () => __wjs_fs_mkdir(dst, true));
    for (const e of readdirSync(src, { withFileTypes: true })) {
      cpSync(src.replace(/\/$/, "") + "/" + e.name, dst.replace(/\/$/, "") + "/" + e.name, opts);
    }
    return;
  }
  if (existsSync(dst)) {
    if (errorOnExist) __fsErr(new Error("EEXIST: file already exists"), "cp", dst);
    if (!force) return;
  }
  __fsCall("copyfile", src, () => __wjs_fs_copy_file(src, dst));
}
// fd 系（openSync 合成 fd，自 3 起单调，不复用最小号，记档）
export function openSync(p, flags, mode) {
  p = __fsPath(p, "open");
  return __fsCall("open", p, () => Number(__wjs_fs_open(p, __fsFlags(flags, "open"))));
}
export function closeSync(fd) {
  __fsCall("close", "", () => __wjs_fs_close(fd));
}
export function readSync(fd, buffer, offset, length, position) {
  offset ??= 0;
  length ??= buffer.length - offset;
  const chunk = __fsCall("read", "", () =>
    __wjs_fs_read_fd(fd, length, typeof position === "number" ? position : -1));
  buffer.set(chunk, offset);
  return chunk.length;
}
export function writeSync(fd, buffer, offset, length, position) {
  let data;
  let pos = -1;
  if (typeof buffer === "string") {
    pos = typeof offset === "number" ? offset : -1;
    data = __fsData(buffer, "write");
  } else {
    offset ??= 0;
    const u8 = __fsData(buffer, "write");
    length ??= u8.length - offset;
    data = u8.subarray(offset, offset + length);
    pos = typeof position === "number" ? position : -1;
  }
  return __fsCall("write", "", () => __wjs_fs_write_fd(fd, data, pos));
}
export function ftruncateSync(fd, len) {
  __fsCall("ftruncate", "", () => __wjs_fs_ftruncate(fd, len ?? 0));
}
export function fsyncSync(fd) {
  __fsCall("fsync", "", () => __wjs_fs_fsync(fd, false));
}
export function fdatasyncSync(fd) {
  __fsCall("fsync", "", () => __wjs_fs_fsync(fd, true));
}
export function fstatSync(fd) {
  return new __Stats(JSON.parse(__fsCall("fstat", "", () => __wjs_fs_fstat(fd))));
}
export function fchmodSync(fd, mode) {
  __fsCall("fchmod", "", () => __wjs_fs_fchmod(fd, __fsModeNum(mode)));
}
export function futimesSync(fd, atime, mtime) {
  __fsCall("futimes", "", () => __wjs_fs_futimes(fd, __fsTimeMs(atime, "futimes"), __fsTimeMs(mtime, "futimes")));
}
// ---- 9c：opendir / Dir（惰性游标，readdir 底座，记档非真流式）----
export class Dir {
  #entries;
  #cursor = 0;
  constructor(path) {
    this.path = path;
    this.#entries = readdirSync(path, { withFileTypes: true });
  }
  readSync() { return this.#cursor < this.#entries.length ? this.#entries[this.#cursor++] : null; }
  read() { return Promise.resolve(this.readSync()); }
  closeSync() {}
  close() { return Promise.resolve(); }
  [Symbol.iterator]() {
    const self = this;
    return {
      next() {
        const d = self.readSync();
        return d === null ? { done: true } : { value: d, done: false };
      },
    };
  }
  [Symbol.asyncIterator]() {
    const it = this[Symbol.iterator]();
    return { next: (v) => Promise.resolve(it.next(v)) };
  }
}
export function opendirSync(p) {
  p = __fsPath(p, "opendir");
  return __fsCall("opendir", p, () => new Dir(p));
}
// ---- 9c：FileHandle + fs.promises（promises 挂 node:fs 本体，fs/promises 反向
// re-export 免环；底层同步实现，文档口径不变）----
export class FileHandle {
  constructor(fd) { this.fd = fd; }
  close() {
    const fd = this.fd;
    return Promise.resolve().then(() => __fsCall("close", "", () => __wjs_fs_close(fd)));
  }
  read(buffer, offset, length, position) {
    return Promise.resolve().then(() => readSync(this.fd, buffer, offset, length, position));
  }
  write(buffer, offset, length, position) {
    return Promise.resolve().then(() => {
      if (typeof buffer === "string") {
        const n = writeSync(this.fd, buffer, offset);
        return { bytesWritten: n, buffer };
      }
      const n = writeSync(this.fd, buffer, offset, length, position);
      return { bytesWritten: n, buffer };
    });
  }
  stat() { return Promise.resolve().then(() => fstatSync(this.fd)); }
  truncate(len) { return Promise.resolve().then(() => ftruncateSync(this.fd, len ?? 0)); }
  chmod(mode) { return Promise.resolve().then(() => fchmodSync(this.fd, mode)); }
  utimes(atime, mtime) { return Promise.resolve().then(() => futimesSync(this.fd, atime, mtime)); }
  sync() { return Promise.resolve().then(() => fsyncSync(this.fd)); }
  datasync() { return Promise.resolve().then(() => fdatasyncSync(this.fd)); }
  readFile(opts) {
    return Promise.resolve().then(() => {
      const enc = __fsEncoding(opts);
      const parts = [];
      while (true) {
        const chunk = __fsCall("read", "", () => __wjs_fs_read_fd(this.fd, 1 << 20, -1));
        if (chunk.length === 0) break;
        parts.push(chunk);
      }
      const out = new Uint8Array(parts.reduce((a, c) => a + c.length, 0));
      let off = 0;
      for (const c of parts) { out.set(c, off); off += c.length; }
      return __fsDecode(out, enc, "readFile");
    });
  }
  writeFile(data) {
    const bytes = __fsData(data, "writeFile");
    return Promise.resolve().then(() => __fsCall("write", "", () => __wjs_fs_write_fd(this.fd, bytes, 0)));
  }
  appendFile(data) {
    const bytes = __fsData(data, "appendFile");
    return Promise.resolve().then(() => {
      const size = fstatSync(this.fd).size;
      __fsCall("write", "", () => __wjs_fs_write_fd(this.fd, bytes, size));
    });
  }
}
const __as = (fn) => function (...args) { return Promise.resolve().then(() => fn(...args)); };
export const promises = {
  access: __as(accessSync),
  appendFile: __as(appendFileSync),
  chmod: __as(chmodSync),
  close: __as(closeSync),
  constants,
  copyFile: __as(copyFileSync),
  cp: __as(cpSync),
  FileHandle,
  lstat: __as(lstatSync),
  link: __as(linkSync),
  mkdir: __as(mkdirSync),
  mkdtemp: __as(mkdtempSync),
  open: (...args) => Promise.resolve().then(() => new FileHandle(openSync(...args))),
  opendir: __as(opendirSync),
  readFile: __as(readFileSync),
  readdir: __as(readdirSync),
  readlink: __as(readlinkSync),
  realpath: __as(realpathSync),
  rename: __as(renameSync),
  rm: __as(rmSync),
  rmdir: __as(rmdirSync),
  stat: __as(statSync),
  statfs: __as(statfsSync),
  symlink: __as(symlinkSync),
  truncate: __as(truncateSync),
  unlink: __as(unlinkSync),
  utimes: __as(utimesSync),
  writeFile: __as(writeFileSync),
};
// ---- 9c：回调全家（err-first；promise 底座经 queueMicrotask 派发）----
function __nodeify(p, cb) {
  if (typeof cb !== "function") throw new TypeError("Callback must be a function");
  p.then(
    (v) => queueMicrotask(() => cb(null, v)),
    (e) => queueMicrotask(() => cb(e)),
  );
}
const __cb1 = (syncFn, name, before) => function (...args) {
  let cb = args[args.length - 1];
  if (typeof cb !== "function") throw new TypeError(`fs.${name}: callback must be a function`);
  const rest = args.slice(0, -1);
  __nodeify(Promise.resolve().then(() => syncFn(...before(rest))), cb);
};
const __id = (a) => a;
export const readFile = __cb1(readFileSync, "readFile", __id);
export const writeFile = __cb1(writeFileSync, "writeFile", __id);
export const appendFile = __cb1(appendFileSync, "appendFile", __id);
export const stat = __cb1(statSync, "stat", __id);
export const statfs = __cb1(statfsSync, "statfs", __id);
export const lstat = __cb1(lstatSync, "lstat", __id);
export const mkdir = __cb1(mkdirSync, "mkdir", __id);
export const rmdir = __cb1(rmdirSync, "rmdir", __id);
export const rm = __cb1(rmSync, "rm", __id);
export const unlink = __cb1(unlinkSync, "unlink", __id);
export const readdir = __cb1(readdirSync, "readdir", __id);
export const rename = __cb1(renameSync, "rename", __id);
export const copyFile = __cb1(copyFileSync, "copyFile", __id);
export const realpath = __cb1(realpathSync, "realpath", __id);
export const mkdtemp = __cb1(mkdtempSync, "mkdtemp", __id);
export const access = __cb1(accessSync, "access", __id);
export const truncate = __cb1(truncateSync, "truncate", __id);
export const utimes = __cb1(utimesSync, "utimes", __id);
export const chmod = __cb1(chmodSync, "chmod", __id);
export const link = __cb1(linkSync, "link", __id);
export const symlink = __cb1(symlinkSync, "symlink", __id);
export const readlink = __cb1(readlinkSync, "readlink", __id);
export const opendir = __cb1(opendirSync, "opendir", __id);
export const cp = __cb1(cpSync, "cp", __id);
export const open = __cb1(openSync, "open", __id);
export const close = __cb1(closeSync, "close", __id);
export function exists(p, cb) {
  if (typeof cb !== "function") throw new TypeError("fs.exists: callback must be a function");
  queueMicrotask(() => cb(existsSync(p)));
}
export function read(fd, buffer, offset, length, position, cb) {
  if (typeof position === "function") { cb = position; position = null; }
  Promise.resolve().then(() => readSync(fd, buffer, offset, length, position))
    .then(
      (n) => queueMicrotask(() => cb(null, n, buffer)),
      (e) => queueMicrotask(() => cb(e)),
    );
}
export function write(fd, buffer, offset, length, position, cb) {
  if (typeof buffer === "string") {
    if (typeof offset === "function") { cb = offset; offset = null; }
    else if (typeof length === "function") { cb = length; }
    else if (typeof position === "function") { cb = position; }
    const pos = typeof offset === "number" ? offset : null;
    Promise.resolve().then(() => writeSync(fd, buffer, pos))
      .then(
        (n) => queueMicrotask(() => cb(null, n, buffer)),
        (e) => queueMicrotask(() => cb(e)),
      );
    return;
  }
  if (typeof offset === "function") { cb = offset; offset = 0; length = undefined; position = null; }
  else if (typeof length === "function") { cb = length; length = undefined; }
  else if (typeof position === "function") { cb = position; position = null; }
  Promise.resolve().then(() => writeSync(fd, buffer, offset ?? 0, length, position))
    .then(
      (n) => queueMicrotask(() => cb(null, n, buffer)),
      (e) => queueMicrotask(() => cb(e)),
    );
}

const __api = {
  // 同步（Phase 4 基础面）
  readFileSync, writeFileSync, appendFileSync, statSync, lstatSync, existsSync,
  mkdirSync, rmSync, rmdirSync, unlinkSync, readdirSync, renameSync, copyFileSync,
  realpathSync, mkdtempSync, watch, watchFile, unwatchFile, constants, createReadStream, createWriteStream,
  // 同步（Phase 9c 增补）
  accessSync, truncateSync, utimesSync, chmodSync, linkSync, symlinkSync, readlinkSync,
  cpSync, opendirSync, openSync, closeSync, readSync, writeSync, ftruncateSync,
  fstatSync, fchmodSync, futimesSync, fsyncSync, fdatasyncSync, statfsSync,
  // 回调面（Phase 9c）
  readFile, writeFile, appendFile, stat, statfs, lstat, exists, mkdir, rmdir, rm, unlink,
  readdir, rename, copyFile, realpath, mkdtemp, access, truncate, utimes, chmod,
  link, symlink, readlink, open, close, read, write, opendir, cp,
  // 类 + promises
  Stats: __Stats, Dirent: __Dirent, StatsFs: __StatsFs, Dir, FileHandle, promises,
};
export default __api;
export { __Stats as Stats, __Dirent as Dirent, __StatsFs as StatsFs };
"#;

/// 内嵌 ESM 源（`node:fs/promises`；同步底层 async 包裹，见头注）。
pub const PROMISES_SOURCE: &str = r#"
// node:fs/promises——re-export node:fs 本体的 promises 面（9c 起 promises 挂
// node:fs，反向引用免环；default = promises 对象）。
import fs from "node:fs";
export const access = fs.promises.access;
export const appendFile = fs.promises.appendFile;
export const chmod = fs.promises.chmod;
export const close = fs.promises.close;
export const copyFile = fs.promises.copyFile;
export const cp = fs.promises.cp;
export const lstat = fs.promises.lstat;
export const link = fs.promises.link;
export const mkdir = fs.promises.mkdir;
export const mkdtemp = fs.promises.mkdtemp;
export const open = fs.promises.open;
export const opendir = fs.promises.opendir;
export const readFile = fs.promises.readFile;
export const readdir = fs.promises.readdir;
export const readlink = fs.promises.readlink;
export const realpath = fs.promises.realpath;
export const rename = fs.promises.rename;
export const rm = fs.promises.rm;
export const rmdir = fs.promises.rmdir;
export const stat = fs.promises.stat;
export const statfs = fs.promises.statfs;
export const symlink = fs.promises.symlink;
export const truncate = fs.promises.truncate;
export const unlink = fs.promises.unlink;
export const utimes = fs.promises.utimes;
export const writeFile = fs.promises.writeFile;
export const constants = fs.constants;
export const FileHandle = fs.FileHandle;
export default fs.promises;
"#;
