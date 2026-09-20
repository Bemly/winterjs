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
            // EADDRNOTAVAIL：macOS 49 / Linux 99（非本机地址 bind；boundsocket 套件）
            49 | 99 => "EADDRNOTAVAIL",
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
            // ENOTSOCK：macOS 38（connect 非 socket 文件；pipe-connect-errors 套件面）
            38 => "ENOTSOCK",
            39 => "ENOTEMPTY",
            // 40 平台二义：macOS EMSGSIZE（UDP 超长报文，msgsize 套件）/
            // Linux ELOOP；62 相对（macOS ELOOP / Linux ETIME）——cfg 分流。
            #[cfg(target_os = "macos")]
            40 => "EMSGSIZE",
            #[cfg(target_os = "macos")]
            62 => "ELOOP",
            #[cfg(not(target_os = "macos"))]
            40 => "ELOOP",
            #[cfg(not(target_os = "macos"))]
            90 => "EMSGSIZE",
            _ => "UNKNOWN",
        };
    }
    match e.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        std::io::ErrorKind::IsADirectory => "EISDIR",
        // fs_err 包装丢 raw errno 的兜底（kind 重建件）
        std::io::ErrorKind::NotADirectory => "ENOTDIR",
        std::io::ErrorKind::DirectoryNotEmpty => "ENOTEMPTY",
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
/// errno → node/uv 错误消息（libuv uv_err_name 小节选；message 对拍用）。
fn uv_msg(e: &std::io::Error) -> &'static str {
    if let Some(errno) = e.raw_os_error() {
        return match errno {
            1 => "operation not permitted",
            2 => "no such file or directory",
            9 => "bad file descriptor",
            13 => "permission denied",
            17 => "file already exists",
            20 => "not a directory",
            21 => "illegal operation on a directory",
            22 => "invalid argument",
            24 => "too many open files",
            28 => "no space left on device",
            32 => "broken pipe",
            36 => "name too long",
            38 => "socket operation on non-socket",
            39 => "directory not empty",
            40 => "too many symbolic links encountered",
            _ => "unknown error",
        };
    }
    match e.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory",
        std::io::ErrorKind::PermissionDenied => "permission denied",
        std::io::ErrorKind::AlreadyExists => "file already exists",
        std::io::ErrorKind::IsADirectory => "illegal operation on a directory",
        std::io::ErrorKind::NotADirectory => "not a directory",
        std::io::ErrorKind::DirectoryNotEmpty => "directory not empty",
        std::io::ErrorKind::InvalidInput => "invalid argument",
        _ => "unknown error",
    }
}

fn report_io(cx: &mut mozjs::context::JSContext, syscall: &str, path: &str, e: std::io::Error) {
    // node lib/internal/errors 形状：`CODE: <uv msg>, <syscall> '<path>'`。
    report_error(
        cx,
        &format!("{}: {}, {syscall} '{path}'", io_code(&e), uv_msg(&e)),
    );
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
    // open/read 两阶段分开报（node 口径：目录 open 成功、read 失败——
    // syscall 分别 'open'/'read'，roundtrip 套件 EISDIR 形逐字点名）；
    // std 直用保 raw errno（fs_err 包装丢 errno 落 UNKNOWN）。
    use std::io::Read as _;
    match std::fs::File::open(&path) {
        Ok(mut f) => {
            let mut buf = Vec::new();
            match f.read_to_end(&mut buf) {
                Ok(_) => {
                    set_rval_bytes(&mut cx, &frame, &buf);
                    true
                }
                Err(e) => {
                    report_io(&mut cx, "read", &path, e);
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
    // mode 仅在 create 面生效（unix；0 保持 std 0o666 语义；append-file-sync
    // 套件 {mode} 建文件点名——旧形丢 mode 建出 666）。
    #[cfg(unix)]
    let r = (|| {
        let mut o = std::fs::OpenOptions::new();
        o.create(true).append(true);
        if frame.argc() > 2 && frame.arg(2).is_number() {
            let m = frame.arg(2).to_number() as u32;
            if m != 0 {
                use std::os::unix::fs::OpenOptionsExt as _;
                o.mode(m & 0o7777);
            }
        }
        o.open(&path)
    })();
    #[cfg(not(unix))]
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
                    "frsize": st.fragment_size(),
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
    // mode（mkdir-mode-mask 套件：0o644|0o10000 的高位由内核掩掉，原样传）；
    // recursive 建链 std 不带 mode——链上默认位、叶目录事后 set_permissions
    //（node recursive 口径：mode 生效于新建目录，链上目录取默认）。
    let mode = if frame.argc() > 2 && frame.arg(2).is_number() {
        frame.arg(2).to_number() as u32
    } else {
        0
    };
    // std 直用（fs_err 包装吞 raw errno → io_code 落 UNKNOWN，ENOTDIR 对拍现形）
    #[cfg(unix)]
    let r = (|| -> std::io::Result<()> {
        if recursive {
            std::fs::create_dir_all(&path)?;
            if mode != 0 {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o7777))?;
            }
            Ok(())
        } else {
            let mut b = std::fs::DirBuilder::new();
            if mode != 0 {
                use std::os::unix::fs::DirBuilderExt as _;
                b.mode(mode & 0o7777);
            }
            b.create(&path)
        }
    })();
    #[cfg(not(unix))]
    let r = if recursive { std::fs::create_dir_all(&path) } else { std::fs::create_dir(&path) };
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
    // §4.121：readdir native 禁 fs_err 包装（io_code/uv_msg 依赖 raw_os_error）——
    // std read_dir 直用（opendirSync(file) → ENOTDIR 口径由错误整形保证）。
    match std::fs::read_dir(&path) {
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

/// `__wjs_fs_mkdtemp(prefix)` → 唯一目录串（prefix + 6 随机 alnum；0700）。
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
        // node 口径（libuv mkdtemp）：prefix + 6 个随机 alnum 字符（test-fs-promises-
        // mkdtempDisposable 断言 basename 长度 == 'foo.XXXXXX'.length）。
        let mut rand = [0u8; 6];
        if getrandom::fill(&mut rand).is_err() {
            report_error(&mut cx, "OperationError: cannot get random values");
            return false;
        }
        const ALPHA: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let suffix: String = rand
            .iter()
            .map(|b| ALPHA[*b as usize % ALPHA.len()] as char)
            .collect();
        let name = format!("{prefix}{suffix}");
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
/// 首次访问惰性注册 0/1/2（dup 出自有句柄，try_clone_to_owned 全 safe）：
/// node 的 fd 族 API 对标准流可见（test-fs-stat `fs.fstat(-0)` 即 fstat(0)）；
/// 源流未打开（spawn stdin 关闭）时 dup 失败即跳过——保持 EBADF，与真机同。
/// 记档偏差：closeSync(0) 关的是 dup 不是真 stdin（node 关真流）。
fn fd_table() -> std::sync::MutexGuard<'static, std::collections::BTreeMap<i32, std::fs::File>> {
    static TABLE: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<i32, std::fs::File>>> =
        std::sync::OnceLock::new();
    let table = TABLE
        .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));
    let mut t = table.lock().unwrap();
    #[cfg(unix)]
    if t.is_empty() {
        use std::os::fd::AsFd;
        for (fd, src) in [
            (0, std::io::stdin().as_fd()),
            (1, std::io::stdout().as_fd()),
            (2, std::io::stderr().as_fd()),
        ] {
            if let Ok(owned) = src.try_clone_to_owned() {
                t.insert(fd, std::fs::File::from(owned));
            }
        }
    }
    t
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
    // utimensat 直调（旧 open(write)+set_times 对目录 EISDIR——utimes 目录
    // 是 node 合法面，utimes 套件 expect_ok 点名）。ms → sec+nsec。
    #[cfg(unix)]
    {
        let ts = |ms: f64| libc::timespec {
            tv_sec: (ms / 1000.0).floor() as i64,
            tv_nsec: ((ms % 1000.0).floor() * 1_000_000.0) as i64,
        };
        let times = [ts(atime), ts(mtime)];
        let Ok(c) = std::ffi::CString::new(path.as_str()) else {
            report_error(&mut cx, "EINVAL: invalid argument, utimes");
            return false;
        };
        // SAFETY: libc 调用（路径 CString 活跃；times 数组活跃）。
        let r = unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), 0) };
        if r == 0 {
            frame.set_rval(mozjs::jsval::UndefinedValue());
            true
        } else {
            let e = std::io::Error::last_os_error();
            report_io(&mut cx, "utimes", &path, e);
            false
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (atime, mtime);
        report_io(
            &mut cx,
            "utimes",
            &path,
            std::io::Error::new(std::io::ErrorKind::Unsupported, "utimes not implemented on this platform"),
        );
        false
    }
}

/// `__wjs_fs_lutimes(path, atimeMs, mtimeMs)` → utimensat AT_SYMLINK_NOFOLLOW
///（符号链接本身；lutimes 套件）。
#[cfg(unix)]
pub unsafe extern "C" fn fs_lutimes(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "lutimes", PermClass::Write) else {
        return false;
    };
    let atime = opt_f64(&frame, 1).unwrap_or(0.0);
    let mtime = opt_f64(&frame, 2).unwrap_or(atime);
    let ts = |ms: f64| libc::timespec {
        tv_sec: (ms / 1000.0).floor() as i64,
        tv_nsec: ((ms % 1000.0).floor() * 1_000_000.0) as i64,
    };
    let times = [ts(atime), ts(mtime)];
    let Ok(c) = std::ffi::CString::new(path.as_str()) else {
        report_error(&mut cx, "EINVAL: invalid argument, lutimes");
        return false;
    };
    // SAFETY: libc 调用（路径 CString 活跃；times 数组活跃）。
    let r = unsafe {
        libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), libc::AT_SYMLINK_NOFOLLOW)
    };
    if r == 0 {
        frame.set_rval(mozjs::jsval::UndefinedValue());
        true
    } else {
        let e = std::io::Error::last_os_error();
        report_io(&mut cx, "lutimes", &path, e);
        false
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
    #[serde(default)]
    mode: u32,
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
    let Ok(mut flags) = serde_json::from_str::<OpenFlags>(&flags_s) else {
        report_error(&mut cx, "TypeError: open: bad flags JSON");
        return false;
    };
    // mode（openSync 第 3 参经 JS 校验后透传；仅 unix create 面）。
    if frame.argc() > 2 && frame.arg(2).is_number() {
        let mv = frame.arg(2);
        flags.mode = mv.to_number() as u32;
    }
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
    // mode 仅在 create 面生效（unix；非 0 才设，缺省保持 std 0o666 语义）。
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if flags.mode != 0 {
            o.mode(flags.mode & 0o7777);
        }
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

/// `__wjs_fs_chown(path, uid, gid)` → std chown（safe；-1 = 不变更）。
pub unsafe extern "C" fn fs_chown(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "chown", PermClass::Write) else {
        return false;
    };
    let uid = opt_f64(&frame, 1).unwrap_or(-1.0) as i64;
    let gid = opt_f64(&frame, 2).unwrap_or(-1.0) as i64;
    match std::os::unix::fs::chown(
        &path,
        if uid < 0 { None } else { Some(uid as u32) },
        if gid < 0 { None } else { Some(gid as u32) },
    ) {
        Ok(()) => true,
        Err(e) => {
            report_io(&mut cx, "chown", &path, e);
            false
        }
    }
}

/// `__wjs_fs_lchown(path, uid, gid)` → libc lchown（符号链接本身；
/// -1 = 不变更，lchown-negative-one 套件点名）。
#[cfg(unix)]
pub unsafe extern "C" fn fs_lchown(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "lchown", PermClass::Write) else {
        return false;
    };
    let uid = opt_f64(&frame, 1).unwrap_or(-1.0) as i64;
    let gid = opt_f64(&frame, 2).unwrap_or(-1.0) as i64;
    let Ok(c) = std::ffi::CString::new(path.as_str()) else {
        report_error(&mut cx, "EINVAL: invalid argument, lchown");
        return false;
    };
    // SAFETY: libc 调用（路径 CString 活跃）；errno 经 from_raw_os_error 走 io_code。
    // uid_t/gid_t 为 u32；-1 哨兵（不变更）= 0xFFFFFFFF，与 chown(2) 语义同。
    let r = unsafe { libc::lchown(c.as_ptr(), uid as u32, gid as u32) };
    if r == 0 {
        frame.set_rval(mozjs::jsval::UndefinedValue());
        true
    } else {
        let e = std::io::Error::last_os_error();
        report_io(&mut cx, "lchown", &path, e);
        false
    }
}

/// `__wjs_fs_lchmod(path, mode)` → fchmodat(AT_FDCWD, …, AT_SYMLINK_NOFOLLOW)。
/// node 口径：fs.lchmod 仅 macOS 存在（Linux 下导出面 undefined）——
/// 本 native 仅 macOS 注册。
#[cfg(target_os = "macos")]
pub unsafe extern "C" fn fs_lchmod(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_path_checked(&mut cx, &frame, 0, "lchmod", PermClass::Write) else {
        return false;
    };
    let mode = opt_f64(&frame, 1).unwrap_or(0.0) as u32;
    let Ok(c) = std::ffi::CString::new(path.as_str()) else {
        report_error(&mut cx, "EINVAL: invalid argument, lchmod");
        return false;
    };
    // SAFETY: libc 调用（路径 CString 活跃；AT_FDCWD = 相对 cwd，与 chmod 同语义）。
    // macOS mode_t 为 u16。
    let r = unsafe {
        libc::fchmodat(libc::AT_FDCWD, c.as_ptr(), (mode & 0o7777) as libc::mode_t, libc::AT_SYMLINK_NOFOLLOW)
    };
    if r == 0 {
        frame.set_rval(mozjs::jsval::UndefinedValue());
        true
    } else {
        let e = std::io::Error::last_os_error();
        report_io(&mut cx, "lchmod", &path, e);
        false
    }
}

/// `__wjs_fs_fchown(fd, uid, gid)` → libc fchown（UNSAFE-BOUNDARY：fd 来自
/// fd_table 的真实 fd，as_raw_fd 取裸号后立刻调用，不跨 GC/线程存活；
/// 覆盖测试：tests/node/fs.rs phase10f fs chown/fchown 族 + 真机对拍）。
pub unsafe extern "C" fn fs_fchown(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(fd) = fd_num(&mut cx, &frame, 0, "fchown") else {
        return false;
    };
    let uid = opt_f64(&frame, 1).unwrap_or(-1.0) as i64;
    let gid = opt_f64(&frame, 2).unwrap_or(-1.0) as i64;
    let raw = {
        let table = fd_table();
        match table.get(&fd) {
            Some(f) => std::os::unix::io::AsRawFd::as_raw_fd(f),
            None => {
                bad_fd(&mut cx, "fchown");
                return false;
            }
        }
    };
    // SAFETY: libc 系统调用；raw fd 在上块作用域内取出后立即使用，
    // uid/gid 已由 JS 侧校验为整数；-1 表示不变更（POSIX 口径）。
    let rc = unsafe { libc::fchown(raw, uid as libc::uid_t, gid as libc::gid_t) };
    if rc == 0 {
        true
    } else {
        let err = std::io::Error::last_os_error();
        report_io(&mut cx, "fchown", "", err);
        false
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

/// node 口径：递归 watch 的 filename 含相对路径（ignore-recursive 套件
// `endsWith/includes` 断言）；单文件/目录回落 basename。
// 根形态不定（相对/绝对/经 symlink）而 notify 事件恒绝对——规范根 + 绝对原根
// 双试 strip（§4.12 同源：/var↔/private/var），皆失才回落 basename。
fn watch_display_name(canon_root: &std::path::Path, raw_root: &std::path::Path, p: &std::path::Path) -> Option<String> {
    for root in [canon_root, raw_root] {
        if let Ok(rel) = p.strip_prefix(root) {
            let s = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

/// minimatch 近似（fs.watch `ignore` 字符串面）：`**` 递归 + 无斜杠模式配
/// basename（matchBase）+ win/mac 不分大小写；模式非法回字面相等。
fn glob_match_impl(pat: &str, name: &str, base: &str, nocase: bool) -> bool {
    let opts = glob::MatchOptions {
        case_sensitive: !nocase,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    };
    match glob::Pattern::new(pat) {
        Ok(p) => {
            p.matches_with(name, opts) || (!pat.contains('/') && p.matches_with(base, opts))
        }
        Err(_) => name == pat || base == pat,
    }
}
// notify 事件 → Node `rename`/`change`（Access/Other 忽略，返回 None）。
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
    // 相对根 vs 绝对事件路径：规范根（存在性已校验，canonicalize 必成）+
    // 绝对原根，双试 strip。
    let canon_root = std::fs::canonicalize(&watched)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| watched.clone());
    let abs_root = if std::path::Path::new(&watched).is_absolute() {
        watched.clone()
    } else {
        std::env::current_dir()
            .map(|c| c.join(&watched).to_string_lossy().into_owned())
            .unwrap_or_else(|_| watched.clone())
    };
    let rel_canon = canon_root.clone();
    let rel_abs = abs_root.clone();
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
                            watch_display_name(
                                std::path::Path::new(&rel_canon),
                                std::path::Path::new(&rel_abs),
                                std::path::Path::new(p),
                            )
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

/// `__wjs_glob_match(pattern, filename, basename, nocaseBool)` → bool。
/// fs.watch `ignore` 字符串面（minimatch 近似，见 `glob_match_impl`）。
pub unsafe extern "C" fn glob_match(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同 watch_start
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 3 {
        report_error(&mut cx, "TypeError: glob match needs pattern, filename and basename");
        return false;
    }
    let pat = value_to_string(&mut cx, frame.arg(0));
    let name = value_to_string(&mut cx, frame.arg(1));
    let base = value_to_string(&mut cx, frame.arg(2));
    let nocase = frame.argc() > 3 && frame.arg(3) == mozjs::jsval::BooleanValue(true);
    frame.set_rval(mozjs::jsval::BooleanValue(glob_match_impl(&pat, &name, &base, nocase)));
    true
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

/// `__wjs_watch_persistent(id, bool)`（ref/unref 续命位；幂等，不存在即 noop）。
pub unsafe extern "C" fn watch_persistent(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 || !frame.arg(0).is_number() {
        report_error(&mut cx, "TypeError: watch persistent needs a numeric id and flag");
        return false;
    }
    let on = frame.arg(1) == mozjs::jsval::BooleanValue(true);
    state::watch_set_persistent(frame.arg(0).to_number() as u64, on);
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
    // std 直用（fs_err 包装只留 kind 丢 raw errno → ENOTDIR 落 UNKNOWN；
    // rmdir-throws-on-file 套件对拍现形，mkdir 已同批切 std）
    let r = if recursive { std::fs::remove_dir_all(&path) } else { std::fs::remove_dir(&path) };
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
    fn watch_display_name_shapes() {
        use std::path::Path;
        let canon = Path::new("/tmp/x");
        let raw = Path::new("/tmp/x");
        // 递归：相对路径（含斜杠）；目录：basename；单文件：自身回落 basename。
        assert_eq!(
            watch_display_name(canon, raw, Path::new("/tmp/x/subdir/file.txt")).as_deref(),
            Some("subdir/file.txt")
        );
        assert_eq!(
            watch_display_name(canon, raw, Path::new("/tmp/x/n.txt")).as_deref(),
            Some("n.txt")
        );
        assert_eq!(
            watch_display_name(canon, raw, Path::new("/tmp/x/foo")).as_deref(),
            Some("foo")
        );
        // 相对根 + 绝对事件（黑盒常形）：规范根命中；symlink 变体经绝对原根。
        assert_eq!(
            watch_display_name(
                Path::new("/private/tmp/x/tree"),
                Path::new("/var/tmp/x/tree"),
                Path::new("/private/tmp/x/tree/src/app.js"),
            )
            .as_deref(),
            Some("src/app.js")
        );
        assert_eq!(
            watch_display_name(
                Path::new("/private/tmp/x/tree"),
                Path::new("/var/tmp/x/tree"),
                Path::new("/var/tmp/x/tree/src/app.js"),
            )
            .as_deref(),
            Some("src/app.js")
        );
        // 根外路径：回落 basename，不抛。
        assert_eq!(
            watch_display_name(canon, raw, Path::new("/other/y.txt")).as_deref(),
            Some("y.txt")
        );
    }

    #[test]
    fn ignore_glob_match_shapes() {
        // matchBase：无斜杠模式配 basename。
        assert!(glob_match_impl("*.log", "subdir/file.log", "file.log", true));
        assert!(!glob_match_impl("*.log", "subdir/file.txt", "file.txt", true));
        // `**` 递归跨段。
        assert!(glob_match_impl("**/node_modules/**", "node_modules/package.json", "package.json", true));
        assert!(!glob_match_impl("**/node_modules/**", "src/app.js", "app.js", true));
        // `*` 不跨分隔符（全路径失配）但 matchBase 兜 basename；首点不吞（dot:false）。
        assert!(glob_match_impl("*.log", "a/b.log", "b.log", true));
        assert!(!glob_match_impl("*", ".secret", ".secret", true));
        // 非法模式回字面相等，不抛。
        assert!(glob_match_impl("[unclosed", "[unclosed", "[unclosed", true));
        assert!(!glob_match_impl("[unclosed", "other", "other", true));
    }

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
        // 10f：非本机地址 bind（EADDRNOTAVAIL，macOS 49 / Linux 99 双码同列）
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(49)), "EADDRNOTAVAIL");
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(99)), "EADDRNOTAVAIL");
        // 10f：priority 面（ESRCH）+ UDS 非 socket（ENOTSOCK=38 实测）
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(3)), "ESRCH");
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(38)), "ENOTSOCK");
        assert_eq!(io_code(&std::io::Error::from_raw_os_error(9999)), "UNKNOWN");
    }
}

/// 内嵌 ESM 源（`node:fs`；错误带 `.code/.syscall/.path`；偏差见头注）。
pub const SOURCE: &str = r#"
import { Readable, Writable } from 'node:stream';
import { EventEmitter } from 'node:events';
import { inspect } from 'node:util';

function __fsErr(e, syscall, path) {
  const m = String((e && e.message) || e);
  // JS 侧已带 code 的类型/范围错误直通（__fsCall 闭包内抛的校验错误
  // 不得被重包成 UNKNOWN——fchmod '123x' 套件现形）。
  if (e && typeof e.code === "string" && e.code !== "" && e.code !== "UNKNOWN") {
    if (e.path === undefined) e.path = path;
    throw e;
  }
  // 权限拒绝直通（不套 io 形状；Deno NotCapable 同款可读错）
  if (m.startsWith("PermissionError:")) {
    const perr = new Error(m.slice("PermissionError: ".length));
    perr.name = "PermissionError";
    perr.path = path;
    throw perr;
  }
  const code = (m.match(/^([A-Z_]+): /) || [])[1] || "UNKNOWN";
  // native report_io 已产出 node 形状（`CODE: msg, syscall 'path'`）→ 直通，
  // 内层 syscall 为准（truncate 的 open 失败 node 口径 syscall='open'，
  // truncate 套件 message 逐字点名）。
  const ioShape = m.match(/^([A-Z_]+): (.*), ([a-z_]+) '(.*)'$/);
  if (ioShape) {
    const err = new Error(m);
    err.code = ioShape[1];
    err.syscall = ioShape[3];
    err.path = ioShape[4];
    throw err;
  }
  const rest = m.replace(/^[A-Z_]+: /, "");
  const err = new Error(`${code}: ${rest}, ${syscall} '${path}'`);
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
function __fsPath(p, what, argName) {
  if (typeof p === "string") {
    // node getValidatedPath：\0 即 ERR_INVALID_ARG_VALUE（null-bytes 套件全 API 面）。
    if (p.includes("\0")) {
      const e = new TypeError(`The argument 'path' must be a string, Uint8Array, or URL without null bytes. Received '${p}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    return p;
  }
  if (p instanceof URL) {
    if (p.protocol !== "file:") throw new TypeError(`${what}: only file: URLs supported`);
    // URL 形同样禁 \0（null-bytes 套件 fileUrl/fileUrl2 两形 + 真机 26.8.2
    // 逐字对拍：ARG_VALUE 'without null bytes'）。pathname 解码后判。
    let dec = p.pathname;
    try { dec = decodeURIComponent(p.pathname); } catch { }
    if (dec.includes("\0")) {
      const e = new TypeError(`The argument 'path' must be a string, Uint8Array, or URL without null bytes. Received '${p.href}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    return p;
  }
  if (ArrayBuffer.isView(p)) {
    const s = Buffer.from(p).toString("utf8");
    if (s.includes("\0")) {
      const e = new TypeError(`The argument 'path' must be a string, Uint8Array, or URL without null bytes. Received '${s}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    return s;
  }
  // node getValidatedPath 口径（test-fs-buffer 点名 message 逐字）。
  __vErrType(argName ?? "path", "string or an instance of Buffer or URL", p);
}
function __fsAbortErr(reason) {
  const e = new Error("The operation was aborted");
  e.name = "AbortError";
  e.code = "ABORT_ERR";
  if (reason !== undefined) e.cause = reason;
  return e;
}
// options.signal 面：键存在（≠undefined）即必须是 AbortSignal 形
//（aborted:boolean + addEventListener 函数；跨域 instanceof 不可靠，§4.57
// 结构判），非法即同步 TypeError ERR_INVALID_ARG_TYPE（readfile 套件
// signal:'hello' 点名）；返回 signal 供 aborted 检查。
function __fsSignalCheck(opts) {
  if (opts && typeof opts === "object" && opts.signal !== undefined) {
    const s = opts.signal;
    if (!s || typeof s !== "object" ||
        typeof s.aborted !== "boolean" || typeof s.addEventListener !== "function") {
      const e = new TypeError('The "options.signal" property must be of type AbortSignal. Received ' + String(s));
      e.code = "ERR_INVALID_ARG_TYPE";
      throw e;
    }
    return s;
  }
  return null;
}
function __fsData(d, what) {
  if (typeof d === "string") return new TextEncoder().encode(d);
  if (d instanceof Uint8Array) return d;
  if (d instanceof ArrayBuffer) return new Uint8Array(d);
  if (ArrayBuffer.isView(d)) return new Uint8Array(d.buffer, d.byteOffset, d.byteLength);
  // node validateBufferData 口径：fs.write/writeSync 参数名 "buffer"，
  // writeFile/appendFile 系 "data"（append-file/buffertype 套件 message 断言）。
  __vErrType(what === "write" ? "buffer" : "data",
             "string or an instance of Buffer, TypedArray, or DataView", d);
}
// 顶层 string data 的 encoding 面（'hex'/'base64'/'latin1' 等解码为字节；
// write-file-sync 套件 hex roundtrip 点名——旧形按 utf8 直写原串）。
function __fsDataEnc(d, what, enc) {
  if (typeof d === "string" && enc && enc !== "utf8" && enc !== "utf-8") {
    return Buffer.from(d, enc);
  }
  return __fsData(d, what);
}
const __fsEncodings = new Set([
  "utf8", "utf-8", "utf16le", "utf-16le", "ucs2", "ucs-2", "ascii", "latin1",
  "binary", "base64", "base64url", "hex", "buffer",
]);
// node validateOffset（lib/internal/fs/streams.js）：非 number/非整数/负数
// 一律 ARG_TYPE（'4' 字符串形套件点名）。
function __fsValidateOffset(v, name) {
  if (v !== undefined && (typeof v !== "number" || !Number.isInteger(v) || v < 0)) {
    __vErrType(name, "number", v);
  }
}
function __fsEncoding(opts) {
  const check = (enc) => {
    if (typeof enc === "string" && !__fsEncodings.has(enc.toLowerCase())) {
      // node validateEncoding：`ERR_INVALID_ARG_VALUE` + TypeError。
      const e = new TypeError(`The argument 'encoding' is invalid encoding. Received '${enc}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    return enc;
  };
  if (opts === undefined || opts === null) return null;
  if (typeof opts === "string") return check(opts);
  if (opts.encoding !== undefined && opts.encoding !== null) check(opts.encoding);
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
  // 10f crypto二轮：hex/base64 系经 Buffer（TextDecoder 无此表，读 hex 固件点名）。
  const enc = String(encoding).toLowerCase();
  if (enc === "hex" || enc === "base64" || enc === "base64url") {
    return Buffer.from(bytes).toString(enc);
  }
  // latin1/binary：字节直映码点（node 'latin1' = binary）。TextDecoder 的
  // latin1 标签是 windows-1252——0x80-0x9F 段与 latin1 不同，roundtrip
  // 套件 ✓/😀 文本现形。
  if (enc === "latin1" || enc === "binary") {
    return Buffer.from(bytes).toString("latin1");
  }
  return new TextDecoder(String(encoding)).decode(bytes);
}
// node 口径：Stats 可无 new 调用（DEP0180 弃用警告 + 位置参数形；
// stat 套件 `fs.Stats(dev, mode, ...)` 直调 + instanceof 断言）。
function __Stats(jOrDev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks, atime, mtime, ctime, birthtime) {
  if (!(this instanceof __Stats)) {
    process.emitWarning("fs.Stats constructor is deprecated.", "DeprecationWarning", "DEP0180");
    return new __Stats(jOrDev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks, atime, mtime, ctime, birthtime);
  }
  const isPos = typeof jOrDev === "number";
  if (isPos) {
    jOrDev = {
      dev: jOrDev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks,
      atimeMs: +atime, mtimeMs: +mtime, ctimeMs: +ctime, birthtimeMs: +birthtime,
    };
    // isX 由 mode 类型位推导（S_IFMT 族，constants 同值）。
    const t = mode & 61440;
    jOrDev.isFile = t === 32768;
    jOrDev.isDirectory = t === 16384;
    jOrDev.isSymlink = t === 40960;
    jOrDev.isFifo = t === 4096;
    jOrDev.isSocket = t === 49152;
    jOrDev.isBlock = t === 24576;
    jOrDev.isChar = t === 8192;
  }
  const bigint = !isPos && mode === true;
  {
    const j = jOrDev;
    // bigint 选项（stat-bigint 套件）：数值字段 BigInt 包装（同源 JSON，与
    // BigInt(numStats[key]) 逐键 strictEqual）；日期恒 Date（getTime 比对）。
    const B = bigint ? (v) => BigInt(Math.round(v)) : (v) => v;
    this.dev = B(j.dev ?? 0);
    this.ino = B(j.ino ?? 0);
    this.mode = B(j.mode);
    this.nlink = B(j.nlink ?? 1);
    this.uid = B(j.uid ?? 0);
    this.gid = B(j.gid ?? 0);
    this.rdev = B(j.rdev ?? 0);
    this.size = B(j.size);
    this.blksize = B(j.blksize ?? 4096);
    this.blocks = B(j.blocks ?? 0);
    this.atimeMs = B(j.atimeMs);
    this.mtimeMs = B(j.mtimeMs);
    this.ctimeMs = B(j.mtimeMs);
    this.birthtimeMs = B(j.birthtimeMs);
    if (bigint) {
      // Ns 形（stat-bigint 套件：Ms/Ns 双键，Ns = ms×1e6 取整）。
      this.atimeNs = BigInt(Math.round(j.atimeMs * 1e6));
      this.mtimeNs = BigInt(Math.round(j.mtimeMs * 1e6));
      this.ctimeNs = BigInt(Math.round(j.mtimeMs * 1e6));
      this.birthtimeNs = BigInt(Math.round(j.birthtimeMs * 1e6));
    }
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
}
__Stats.prototype.isFile = function () { return this.__f; };
__Stats.prototype.isDirectory = function () { return this.__d; };
__Stats.prototype.isSymbolicLink = function () { return this.__l; };
__Stats.prototype.isFIFO = function () { return !!this.__fifo; };
__Stats.prototype.isSocket = function () { return !!this.__sock; };
__Stats.prototype.isBlockDevice = function () { return !!this.__blk; };
__Stats.prototype.isCharacterDevice = function () { return !!this.__chr; };
class __Dirent {
  constructor(name, isDir, isFile, isLink, parentPath) {
    this.name = name;
    this.__d = isDir;
    this.__f = isFile;
    this.__l = isLink;
    // node getDirent：parentPath（26 线新名）+ path（旧别名，同值）。
    this.parentPath = parentPath;
    this.path = parentPath;
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
  constructor(j, bigint = false) {
    // bigint 选项：数值字段 BigInt 包装（statfs 套件 verifyStatFsObject 逐键 typeof）。
    const B = bigint ? (v) => BigInt(Math.round(v)) : (v) => v;
    this.type = B(j.type ?? 0);
    this.bsize = B(j.bsize ?? 4096);
    this.frsize = B(j.frsize ?? j.bsize ?? 4096);
    this.blocks = B(j.blocks ?? 0);
    this.bfree = B(j.bfree ?? 0);
    this.bavail = B(j.bavail ?? 0);
    this.files = B(j.files ?? 0);
    this.ffree = B(j.ffree ?? 0);
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
// node readFile 大文件帽：>2^31-1 即 ERR_FS_FILE_TOO_LARGE（真机 26.8.2 实测
// RangeError，message 带 size：`File size (2147483648) is greater than 2 GiB`；
// readfile 套件造 2GiB 稀疏文件点名——无帽时 native 真 2GiB 分配 abort）。
const __kIoMaxLength = 2 ** 31 - 1;
function __fsFileTooLarge(size) {
  const e = new RangeError(`File size (${size}) is greater than 2 GiB`);
  e.code = "ERR_FS_FILE_TOO_LARGE";
  return e;
}
function __fsReadWhole(p, flag) {
  if (flag === undefined || flag === "r") {
    // 帽先于 native 读：stat 跟随符号链接，与 readFile 语义同；ENOENT 落到
    // native open 再报（不在此吞）。
    const st = statSync(p, { throwIfNoEntry: false });
    if (st && st.size > __kIoMaxLength) throw __fsFileTooLarge(st.size);
    return __wjs_fs_read_file(p);
  }
  const fd = Number(__wjs_fs_open(p, __fsFlags(flag, "readFile")));
  try {
    const parts = [];
    let total = 0;
    while (true) {
      const chunk = __wjs_fs_read_fd(fd, 1 << 20, -1);
      if (chunk.length === 0) break;
      total += chunk.length;
      if (total > __kIoMaxLength) throw __fsFileTooLarge(total);
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
function __fsFdOf(p) {
  // node readFile/writeFile 系：fd number 或带 .fd 的句柄 → fd；否则 null。
  if (typeof p === "number" && Number.isInteger(p)) return __vFd(p);
  if (p && typeof p === "object" && typeof p.fd === "number") return p.fd;
  return null;
}
export function readFileSync(p, opts) {
  const enc = __fsEncoding(opts);
  // signal 面：已 abort 即 AbortError（node readFileSync 同步检查同口径）。
  const sig = __fsSignalCheck(opts);
  if (sig && sig.aborted) throw __fsAbortErr(sig.reason);
  const fd = __fsFdOf(p);
  if (fd !== null) {
    // fd 形：从当前位读到 EOF（node readFileHandle 同口径）。
    const parts = [];
    let total = 0;
    for (;;) {
      const chunk = __fsCall("read", "", () => __wjs_fs_read_fd(fd, 1 << 20, -1));
      if (chunk.length === 0) break;
      total += chunk.length;
      if (total > __kIoMaxLength) throw __fsFileTooLarge(total);
      parts.push(chunk);
    }
    const out = new Uint8Array(parts.reduce((a, c) => a + c.length, 0));
    let off = 0;
    for (const c of parts) { out.set(c, off); off += c.length; }
    return __fsDecode(out, enc, "readFile");
  }
  p = __fsPath(p, "readFile");
  const flag = opts && typeof opts === "object" ? opts.flag : undefined;
  return __fsCall("open", p, () => __fsDecode(__fsReadWhole(p, flag), enc, "readFile"));
}
export function writeFileSync(p, data, opts) {
  const enc = __fsEncoding(opts);
  // node 口径：signal 面校验（'hello' 即同步 ERR_INVALID_ARG_TYPE）+
  // signal.aborted 即 AbortError（走回调拒绝路径，writefile-with-fd 点名）。
  const wsig = __fsSignalCheck(opts);
  if (wsig && wsig.aborted) throw __fsAbortErr(wsig.reason);
  const fd = __fsFdOf(p);
  if (fd !== null) {
    // fd 形：写现位（node writeFileHandle 同口径）。
    const bytes = __fsDataEnc(data, "writeFile", enc);
    writeSync(fd, bytes, 0, bytes.byteLength, null);
    return;
  }
  p = __fsPath(p, "writeFile");
  const flag = opts && typeof opts === "object" ? opts.flag : undefined;
  const bytes = __fsDataEnc(data, "writeFile", enc);
  __fsCall("open", p, () => {
    if (flag === undefined || flag === "w") {
      __wjs_fs_write_file(p, bytes, __fsMode(opts));
      return;
    }
    const fd = Number(__wjs_fs_open(p, __fsFlags(flag, "writeFile")));
    try { __wjs_fs_write_fd(fd, bytes, flag.startsWith("a") ? -1 : 0); }
    finally { __wjs_fs_close(fd); }
  });
  // mode 语义：仅新建文件时应用（存在性预判，记档近似）
  const mode = __fsMode(opts);
  if (mode > 0 && !existsSync(p)) chmodSync(p, mode);
}
// node writeFile/appendFile data 面（10f）：string/Buffer/TypedArray/DataView +
// 同步可迭代逐块收；chunk 非法即 'chunk' ARG_TYPE；顶层非法 data 为 'data' ARG_TYPE。
function __chunkArgErr(c) {
  const e = new TypeError(`The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${c === null ? "null" : typeof c}`);
  e.code = "ERR_INVALID_ARG_TYPE"; throw e;
}
function __fsDataSync(data, what, opts) {
  const enc = typeof opts === "string" ? opts : (opts && opts.encoding);
  const isChunk = (c) => typeof c === "string" || ArrayBuffer.isView(c);
  const encChunk = (c) => {
    const u8 = typeof c === "string" && enc && enc !== "utf8" ? Buffer.from(c, enc) : __fsData(c, what);
    return new Uint8Array(u8.buffer, u8.byteOffset, u8.byteLength);
  };
  // node 口径：Array 实例整体拒收（真机 appendFileSync(file, []) →
  // ERR_INVALID_ARG_TYPE /data/；可迭代仅限非数组形态，append-file-sync 套件）。
  if (Array.isArray(data)) __vErrType(what, "string or an instance of Buffer, TypedArray, or DataView", data);
  if (data && typeof data === "object" && !ArrayBuffer.isView(data) && typeof data[Symbol.iterator] === "function") {
    const parts = [];
    for (const c of data) {
      if (!isChunk(c)) __chunkArgErr(c);
      parts.push(c);
    }
    const out = new Uint8Array(parts.reduce((a, c) => a + encChunk(c).length, 0));
    let off = 0;
    for (const c of parts) { const u8 = encChunk(c); out.set(u8, off); off += u8.length; }
    return out;
  }
  return __fsData(data, what);
}
// promises.appendFile 异步面：asyncIterable 全量收 + signal 两点 abort
//（收集前预检 + 收集后复查；doAppendStreamWithCancel 套件 AbortError 契约）。
async function __fsAppendFileAsync(p, data, opts) {
  const signal = opts && typeof opts === "object" ? opts.signal : undefined;
  if (signal) {
    if (typeof signal.addEventListener !== "function") {
      const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (signal.aborted) throw __fsAbortErr(signal.reason);
  }
  let bytes;
  if (data && typeof data === "object" && !ArrayBuffer.isView(data) && typeof data[Symbol.asyncIterator] === "function") {
    const enc = typeof opts === "string" ? opts : (opts && opts.encoding);
    const isChunk = (c) => typeof c === "string" || ArrayBuffer.isView(c);
    const encChunk = (c) => {
      const u8 = typeof c === "string" && enc && enc !== "utf8" ? Buffer.from(c, enc) : __fsData(c, "appendFile");
      return new Uint8Array(u8.buffer, u8.byteOffset, u8.byteLength);
    };
    const parts = [];
    for await (const c of data) {
      if (signal && signal.aborted) throw __fsAbortErr(signal.reason);
      if (!isChunk(c)) __chunkArgErr(c);
      parts.push(c);
    }
    const out = new Uint8Array(parts.reduce((a, c) => a + encChunk(c).length, 0));
    let off = 0;
    for (const c of parts) { const u8 = encChunk(c); out.set(u8, off); off += u8.length; }
    bytes = out;
  } else {
    bytes = __fsDataSync(data, "data", opts);
  }
  if (signal && signal.aborted) throw __fsAbortErr(signal.reason);
  if (signal) {
    // node 写为线程池异步——abort 竞速即拒；本仓写为同步，落宏任务（setTimeout 0）
    // 使 abort 的 nextTick 检查点先于写（FileHandle.write signal 路径同构）。
    await Promise.race([
      new Promise((resolve, reject) => {
        setTimeout(() => {
          try {
            if (signal.aborted) { reject(__fsAbortErr(signal.reason)); return; }
            resolve(appendFileSync(p, bytes, opts));
          } catch (e) { reject(e); }
        }, 0);
      }),
      new Promise((_, reject) =>
        signal.addEventListener("abort", () => reject(__fsAbortErr(signal.reason)), { once: true })),
    ]);
    return;
  }
  return appendFileSync(p, bytes, opts);
}
export function appendFileSync(p, data, opts) {
  __fsEncoding(opts);
  // node 口径：signal 面校验（非法 signal 同步 ARG_TYPE）+ aborted 即
  // AbortError（promises-appendfile cancel 套件）。
  const asig = __fsSignalCheck(opts);
  if (asig && asig.aborted) throw __fsAbortErr(asig.reason);
  // node 口径：data 校验先于 open（非法 data 不得留下已创建的文件）；
  // 同步可迭代逐块收（promises-appendfile doAppendStream 族）。
  const bytes = __fsDataSync(data, "data", opts);
  if (typeof p === "number" && Number.isInteger(p)) {
    // fd 形：写现位（fd 'a+' 打开即尾）。
    __vFd(p);
    return writeSync(p, bytes, 0, bytes.byteLength, null) && undefined;
  }
  if (p && typeof p === "object" && typeof p.fd === "number") {
    return writeSync(p.fd, bytes, 0, bytes.byteLength, null) && undefined;
  }
  p = __fsPath(p, "appendFile");
  __fsCall("open", p, () => __wjs_fs_append_file(p, bytes, __fsMode(opts)));
}
export function statSync(p) {
  p = __fsPath(p, "stat");
  // options：bigint（BigInt Stats）+ throwIfNoEntry:false（缺失回 undefined，node 口径）。
  const __o = arguments[1];
  if (__o && __o.throwIfNoEntry === false) {
    try {
      return new __Stats(JSON.parse(__wjs_fs_stat(p, true)), __o.bigint === true);
    } catch (e) {
      // 裸 native 错误无 code——先过 __fsErr 归一化（ENOENT 豁免，其余照抛）
      try { __fsErr(e, "stat", p); } catch (e2) { if (e2 && e2.code === "ENOENT") return undefined; throw e2; }
    }
  }
  return new __Stats(JSON.parse(__fsCall("stat", p, () => __wjs_fs_stat(p, true))), __o?.bigint === true);
}
// 文件系统级状态（M5 vitest 牵引；unix 经 statvfs，type 取 filesystem_id 记档）。
export function statfsSync(p) {
  if (typeof p !== "string" && !(p instanceof URL)) {
    const e = new TypeError(`The "path" argument must be of type string or an instance of Buffer or URL. Received ${p === null ? "null" : typeof p}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  p = __fsPath(p, "statfs");
  const __o = arguments[1];
  return new __StatsFs(JSON.parse(__fsCall("statfs", p, () => __wjs_fs_statfs(p))), __o?.bigint === true);
}
export function lstatSync(p) {
  p = __fsPath(p, "lstat");
  const __o = arguments[1];
  if (__o && __o.throwIfNoEntry === false) {
    try {
      return new __Stats(JSON.parse(__wjs_fs_stat(p, false)), __o.bigint === true);
    } catch (e) {
      try { __fsErr(e, "lstat", p); } catch (e2) { if (e2 && e2.code === "ENOENT") return undefined; throw e2; }
    }
  }
  return new __Stats(JSON.parse(__fsCall("stat", p, () => __wjs_fs_stat(p, false))), __o?.bigint === true);
}
export function existsSync(p) {
  try {
    return __wjs_fs_exists(__fsPath(p, "exists"));
  } catch {
    return false;
  }
}
function __firstMissing(p) {
  // node 递归 mkdir 返回首个**新建**路径 = 最浅的不存在祖先。
  const abs = p.startsWith("/");
  const segs = p.split("/").filter((x) => x !== "");
  let acc = abs ? "/" : "";
  for (const seg of segs) {
    acc = acc === "/" ? "/" + seg : acc + "/" + seg;
    if (!existsSync(acc)) return acc;
  }
  return null;
}
export function mkdirSync(p, opts) {
  p = __fsPath(p, "mkdir");
  // node 口径：opts 可为 number/string（mode 简写，mkdir-mode-mask 套件
  // 0o10644/"10644" 点名）或 object{recursive,mode}。
  let recursive = false;
  let mode;
  if (typeof opts === "number" || typeof opts === "string") {
    mode = __fsModeNum(opts);
  } else if (opts && typeof opts === "object") {
    recursive = opts.recursive !== undefined
      ? __vBooleanProp(opts.recursive, "options.recursive")
      : false;
    if (opts.mode !== undefined) mode = __fsModeNum(opts.mode);
  }
  // node 口径：recursive 只容忍已存在的**目录**；路径是文件即 EEXIST
  //（syscall 'mkdir'，test-fs-mkdir 点名）。
  if (recursive && existsSync(p) && !statSync(p).isDirectory()) {
    const e = new Error(`EEXIST: file already exists, mkdir '${p}'`);
    e.code = "EEXIST"; e.errno = -17; e.syscall = "mkdir"; e.path = p;
    throw e;
  }
  const firstCreated = recursive ? __firstMissing(p) : null;
  __fsCall("mkdir", p, () => __wjs_fs_mkdir(p, recursive, mode ?? 0o777));
  return firstCreated ?? undefined;
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
  const enc = __fsEncoding(opts);
  const asBuf = enc === "buffer";
  const out = JSON.parse(__fsCall("scandir", p, () => __wjs_fs_readdir(p, withTypes)));
  if (!withTypes) return asBuf ? out.map((n) => Buffer.from(n)) : out;
  // node getDirent：dirent.parentPath = 目录路径（Dirent 亦挂 path 别名）。
  return out.map(([name, isDir, isFile, isLink]) => new __Dirent(asBuf ? Buffer.from(name) : name, isDir, isFile, isLink, p));
}
export function renameSync(a, b) {
  // node 口径：位置参数名 oldPath/newPath（rename-type-check 套件 message 逐字）。
  a = __fsPath(a, "rename", "oldPath");
  b = __fsPath(b, "rename", "newPath");
  __fsCall("rename", a, () => __wjs_fs_rename(a, b));
}
export function copyFileSync(src, dst, mode) {
  if (mode !== undefined && typeof mode !== "number") {
    const e = new TypeError(`The "mode" argument must be of type number. Received ${typeof mode} (${JSON.stringify(String(mode))})`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  src = __fsPath(src, "copyFile");
  dst = __fsPath(dst, "copyFile");
  __fsCall("copyfile", src, () => __wjs_fs_copy_file(src, dst));
}
export function realpathSync(p, opts) {
  __fsEncoding(opts);
  p = __fsPath(p, "realpath");
  return __fsCall("lstat", p, () => __wjs_fs_realpath(p));
}
// Node 口径：.native = binding 级 realpath（无 JS 层缓存/规范化）。本仓两者
// 同底座（std canonicalize），直接自引用（vite 8 的 safeRealpathSync 取此面）。
realpathSync.native = realpathSync;
export function mkdtempSync(prefix, opts) {
  // node：prefix 经 getValidatedPath(prefix, 'prefix')（undefined/null/数字/布尔
  // 全 ARG_TYPE；mkdtemp-prefix-check 套件）。
  prefix = __fsPath(prefix, "prefix");
  __fsEncoding(opts);
  return __fsCall("mkdir", String(prefix), () => __wjs_fs_mkdtemp(String(prefix)));
}
export const constants = {
  F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1,
  O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 512, O_EXCL: 2048, O_NOCTTY: 131072,
  O_TRUNC: 1024, O_APPEND: 8, O_DIRECTORY: 1048576, O_NOFOLLOW: 256, O_SYNC: 128,
  O_DSYNC: 4194304, O_SYMLINK: 2097152, O_NONBLOCK: 4,
  S_IFMT: 61440, S_IFREG: 32768, S_IFDIR: 16384, S_IFCHR: 8192, S_IFBLK: 24576,
  S_IFIFO: 4096, S_IFLNK: 40960, S_IFSOCK: 49152,
  S_IRWXU: 448, S_IRUSR: 256, S_IWUSR: 128, S_IXUSR: 64,
  S_IRWXG: 56, S_IRGRP: 32, S_IWGRP: 16, S_IXGRP: 8,
  S_IRWXO: 7, S_IROTH: 4, S_IWOTH: 2, S_IXOTH: 1,
  COPYFILE_EXCL: 1, COPYFILE_FICLONE: 2, COPYFILE_FICLONE_FORCE: 4,
  UV_FS_COPYFILE_EXCL: 1, UV_FS_COPYFILE_FICLONE: 2, UV_FS_COPYFILE_FICLONE_FORCE: 4,
  UV_DIRENT_UNKNOWN: 0, UV_DIRENT_FILE: 1, UV_DIRENT_DIR: 2, UV_DIRENT_LINK: 3,
  UV_DIRENT_FIFO: 4, UV_DIRENT_SOCKET: 5, UV_DIRENT_CHAR: 6, UV_DIRENT_BLOCK: 7,
  // 平台相关 uv 开文件旗（真机 26.8.2 macOS 全 0，逐项导出对拍）
  UV_FS_O_FILEMAP: 0, UV_FS_O_RANDOM: 0, UV_FS_O_SEQUENTIAL: 0,
  UV_FS_O_SHORT_LIVED: 0, UV_FS_O_TEMPORARY: 0,
  UV_FS_SYMLINK_DIR: 1, UV_FS_SYMLINK_JUNCTION: 2,
};
// node 口径：constants 无原型（stat-constants 套件 getPrototypeOf === null）。
Object.setPrototypeOf(constants, null);
// node 口径（lib/internal/validators.js validateIgnoreOption 逐字）：
// null/undefined 过；数组逐元校验（名 `options.ignore[i]`）；单元素 string
// 非空（空串 ARG_VALUE 'must be a non-empty string'）/RegExp/Function 过，
// 余下 ARG_TYPE ['string','RegExp','Function']。
function __validateIgnoreOption(v, name) {
  if (v === undefined || v === null) return;
  if (Array.isArray(v)) {
    for (let i = 0; i < v.length; i++) __validateIgnoreElement(v[i], `${name}[${i}]`);
    return;
  }
  __validateIgnoreElement(v, name);
}
function __validateIgnoreElement(m, name) {
  if (typeof m === "string") {
    if (m.length === 0) {
      const e = new TypeError(`The argument '${name}' must be a non-empty string. Received ''`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    return;
  }
  if (Object.prototype.toString.call(m) === "[object RegExp]") return;
  if (typeof m === "function") return;
  const e = new TypeError(`The "${name}" argument must be one of type string, RegExp, or Function. Received ${Object.prototype.toString.call(m)}`);
  e.code = "ERR_INVALID_ARG_TYPE"; throw e;
}
// node 口径（lib/internal/fs/watchers.js createIgnoreMatcher）：string 经
// minimatch（matchBase + win/mac nocase，本仓 `__wjs_glob_match` 近似）/
// RegExp 经 exec 判空/Function 直透；filename 为 null 时不滤（调用方守卫）。
const __globNocase = process.platform === "win32" || process.platform === "darwin";
function __ignoreMatcher(ignore) {
  if (ignore === undefined || ignore === null) return null;
  const list = Array.isArray(ignore) ? ignore : [ignore];
  const compiled = list.map((m) => {
    if (typeof m === "string") {
      return (fn) => __wjs_glob_match(m, fn, fn.split("/").pop(), __globNocase);
    }
    if (Object.prototype.toString.call(m) === "[object RegExp]") {
      return (fn) => m.exec(fn) !== null;
    }
    return m;
  });
  return (fn) => {
    for (const c of compiled) {
      if (c(fn)) return true;
    }
    return false;
  };
}
// FSWatcher（10f，node 口径）：EventEmitter 形（'change'/'close' 事件面 +
// on/once/off），options.listener 可选、{ signal } abort 即 close。
class __FSWatcher extends EventEmitter {
  #id;
  constructor() { super(); this.#id = 0; }
  __attach(id) { this.#id = id; return this; }
  close() {
    // node 口径（lib/internal/fs/watchers.js FSWatcher.close）：已关即 noop；
    // 'close' 经 nextTick 异步发（handler 内自调 close 安全）。
    if (this.#id !== 0) {
      __wjs_watch_close(this.#id); this.#id = 0;
      process.nextTick(() => this.emit("close"));
    }
  }
  // node 口径：ref/unref 取/释底层句柄引用（watch-ref-unref 套件：unref 后
  // 进程可退；Rust 侧 watch_open 计数联动，幂等）。
  ref() { if (this.#id !== 0) __wjs_watch_persistent(this.#id, true); return this; }
  unref() { if (this.#id !== 0) __wjs_watch_persistent(this.#id, false); return this; }
  get closed() { return this.#id === 0; }
}
export function watch(p, opts, listener) {
  if (typeof opts === "function") { listener = opts; opts = {}; }
  if (listener !== undefined && typeof listener !== "function") throw new TypeError("watch: listener must be a function");
  if (opts !== undefined && opts !== null && typeof opts !== "function") __fsEncoding(opts);
  p = __fsPath(p, "watch");
  const recursive = !!(opts && opts.recursive);
  const persistent = !(opts && opts.persistent === false);
  const watcher = new __FSWatcher();
  if (typeof listener === "function") watcher.on("change", listener);
  // node 26 口径（lib/internal/validators.js validateIgnoreOption +
  // lib/internal/fs/watchers.js createIgnoreMatcher）：string（含 glob，
  // matchBase）/RegExp/Function/数组混排；非法即 ARG_TYPE（空串 ARG_VALUE）。
  const ignoreOpt = opts ? opts.ignore : undefined;
  __validateIgnoreOption(ignoreOpt, "options.ignore");
  const ignoreFn = __ignoreMatcher(ignoreOpt);
  const id = __fsCall("watch", p, () => __wjs_watch_start(p, recursive, persistent, (ev, fn) => {
    if (fn != null && ignoreFn && ignoreFn(fn)) return;
    watcher.emit("change", ev, fn);
  }));
  watcher.__attach(id);
  if (opts && opts.signal) {
    if (opts.signal.aborted) watcher.close();
    else opts.signal.addEventListener("abort", () => watcher.close(), { once: true });
  }
  return watcher;
}
// stat 轮询（watchFile 底座；interval 経 setInterval，statSync 取样）。
// node 口径（lib/internal/fs/watchers.js StatWatcher）：EventEmitter 形
// （'change'/'stop' + listenerCount）；stop() 经 nextTick 发 'stop'（已停即
// noop，不重发）；ref/unref 链式（定时器 keep-alive 由 Rust 表决定，记档）；
// 缺席侧零 Stats 派发（watchfile 套件：缺席首轮 (zero,zero)，出现轮 prev.ino<=0）。
const __statWatchers = new Map();
function __zeroStats() {
  return new __Stats(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
}
function __statPoll(p) {
  const rec = __statWatchers.get(p);
  if (!rec) return;
  let curr = null;
  try { curr = statSync(p); } catch { curr = null; }
  const prev = rec.prev;
  rec.prev = curr;
  // 缺席文件首轮即发 (zero,zero)（真机实测；seen 旗区分"未轮询"与"轮询过缺席"）。
  if (!rec.seen) {
    rec.seen = true;
    if (curr === null && prev === null) {
      const z = __zeroStats();
      rec.watcher.emit("change", z, __zeroStats());
    }
    return;
  }
  if (curr === null && prev === null) return;
  if (curr !== null && prev !== null && curr.size === prev.size && curr.mtimeMs === prev.mtimeMs) return;
  const c = curr ?? __zeroStats();
  const v = prev ?? __zeroStats();
  rec.watcher.emit("change", c, v);
}
class __StatWatcher extends EventEmitter {
  #path;
  #stopped;
  constructor(p) { super(); this.#path = p; this.#stopped = false; }
  stop() {
    // 已停即 noop（不重发 stop，watchfile 套件点名）；同路径单例：停即全停
    //（真机 w2.stop 关共享句柄口径），摘表停 timer + nextTick 发 stop。
    if (this.#stopped) return this;
    this.#stopped = true;
    const rec = __statWatchers.get(this.#path);
    if (rec && rec.watcher === this) {
      clearInterval(rec.timer);
      __statWatchers.delete(this.#path);
    }
    process.nextTick(() => this.emit("stop"));
    return this;
  }
  close() { return this.stop(); }
  ref() { return this; }
  unref() { return this; }
}
export function watchFile(p, opts, listener) {
  if (typeof opts === "function") { listener = opts; opts = {}; }
  if (typeof listener !== "function") {
    const e = new TypeError(`The "listener" argument must be of type function. Received ${listener}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  p = __fsPath(p, "watchFile");
  const interval = (opts && Number(opts.interval) > 0) ? Number(opts.interval) : 5007;
  // node 口径：同路径单例 watcher（多次 watchFile 同一对象，监听累积；
  // watchfile-ref-unref 套件 listenerCount 点名）。
  let rec = __statWatchers.get(p);
  if (!rec) {
    rec = { watcher: new __StatWatcher(p), prev: null, seen: false, timer: null };
    try { rec.prev = statSync(p); } catch { rec.prev = null; }
    rec.timer = setInterval(() => __statPoll(p), interval);
    __statWatchers.set(p, rec);
  }
  rec.watcher.on("change", listener);
  return rec.watcher;
}
export function unwatchFile(p, listener) {
  p = __fsPath(p, "unwatchFile");
  const rec = __statWatchers.get(p);
  if (!rec) return;
  // node 口径：摘指定监听（缺省全摘）；归零即 stop（恰发一次 stop）。
  if (typeof listener === "function") rec.watcher.removeListener("change", listener);
  else rec.watcher.removeAllListeners("change");
  if (rec.watcher.listenerCount("change") === 0) rec.watcher.stop();
}
// ---- fs 流（10f：createReadStream 换 node ReadStream——真机 26 口径
// open(fd)/ready/data(Buffer)/end/close 事件序 + path/flags/autoClose/
// start/end 选项；chunk 为 Buffer（§4.83）。sync 底座偏差记档：'open' 的
// fd 恒 null（无真异步 fd 生命周期），sync 读错误在构造期抛而非 'error' 事件。
// createWriteStream 维持 Web 流外形（口径见下注）。----
class __ReadStream extends Readable {
  constructor(p, opts) {
    opts = opts ?? {};
    __fsEncoding(opts);
    // node validateOffset（non-number-arguments-throw 套件）：start/end 非 number
    // （含 '4' 字符串形）即 ARG_TYPE；node 校验点在构造器（fd 形同样适用）。
    __fsValidateOffset(opts.start, "start");
    __fsValidateOffset(opts.end, "end");
    const hwm = opts.highWaterMark !== undefined ? Number(opts.highWaterMark) : 65536;
    const size = Number.isFinite(hwm) && hwm > 0 ? Math.floor(hwm) : 65536;
    super({ highWaterMark: size, autoDestroy: true, emitClose: true });
    this.path = p;
    this.flags = opts.flags ?? "r";
    this.mode = opts.mode ?? 0o666;
    this.autoClose = opts.autoClose !== false;
    this.bytesRead = 0;
    this.fd = null;
    if (opts.fd !== undefined && opts.fd !== null) {
      // 10f：fd 形（FileHandle.createReadStream / { fd }）——不 open，增量读；
      // start/end 不适用（偏差记档）。fd 为 FileHandle 时读/关走 handle 方法
      //（node streams.js FileHandleOperations 同构；write-stream-2 以 spy 断言）。
      if (opts.fs) {
        const e = new Error("The FileHandle with fs method is not implemented");
        e.code = "ERR_METHOD_NOT_IMPLEMENTED"; throw e;
      }
      this.__fh = typeof opts.fd === "object" ? opts.fd : null;
      this.fd = this.__fh ? this.__fh.fd : opts.fd;
      this.__fdMode = true;
      this.__opened = false;
      this.__hwm = size;
      if (opts.signal !== undefined) {
        // node validateAbortSignal：undefined 跳过，null/非 signal 即抛。
        if (opts.signal === null || typeof opts.signal.addEventListener !== "function") {
          const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        if (opts.signal.aborted) {
          queueMicrotask(() => { if (!this.destroyed) this.destroy(__fsAbortErr(opts.signal.reason)); });
          return;
        }
        opts.signal.addEventListener("abort", () => this.destroy(__fsAbortErr(opts.signal.reason)), { once: true });
      }
      // node 口径：FileHandle 被（用户/他人）close 即毁流——handle 关后再读
      // 是 EBADF，不是可恢复错误（read-stream-file-handle data→close 用例）。
      // 位置：signal 校验之后——无效 signal 的构造器抛错不得积累监听。
      if (this.__fh && typeof this.__fh.on === "function") {
        this.__fh.on("close", () => { if (!this.destroyed) this.destroy(); });
      }
      return;
    }
    const bytes = __fsCall("open", p, () => __wjs_fs_read_file(p));
    let start = opts.start !== undefined ? Math.max(0, Math.floor(Number(opts.start) || 0)) : 0;
    let end = opts.end !== undefined ? Math.floor(Number(opts.end)) : bytes.length - 1;
    if (!Number.isFinite(start) || start < 0) start = 0;
    if (!Number.isFinite(end) || end >= bytes.length) end = bytes.length - 1;
    this.__bytes = bytes.subarray(start, end + 1);
    this.__off = 0;
    this.__hwm = size;
    this.__opened = false;
  }
  close(cb) {
    // node ReadStream.close：毁流 → 'close'；裸 fd 在此收口（destroy 不带钩）。
    if (this.__fdMode && !this.__fh && this.autoClose && this.fd != null && this.fd !== -1) {
      try { __wjs_fs_close(this.fd); } catch { }
      this.fd = -1;
    }
    this.destroy();
    if (typeof cb === "function") this.once("close", cb);
    return this;
  }
  _read() {
    // node 口径事件序 open → ready → data …：首次 _read 前派发（sync 底座下
    // 若走 microtask，流在监听器挂载的同一同步链上已流到 close，事件被
    // destroyed 早退吞掉）。
    if (!this.__opened) {
      this.__opened = true;
      this.emit("open", null);
      this.emit("ready");
    }
    if (this.__fdMode) {
      const buf = Buffer.alloc(this.__hwm);
      if (this.__fh) {
        this.__fh.read(buf, 0, this.__hwm, null).then(
          (r) => {
            if (this.destroyed) return;
            if (!r || r.bytesRead <= 0) { this.push(null); return; }
            this.bytesRead += r.bytesRead;
            this.push(r.bytesRead === r.buffer.byteLength ? r.buffer : Buffer.from(r.buffer.subarray(0, r.bytesRead)));
          },
          (e) => this.destroy(e),
        );
        return;
      }
      let n;
      try { n = readSync(this.fd, buf, 0, this.__hwm, null); } catch (e) { this.destroy(e); return; }
      if (n <= 0) {
        if (this.autoClose) { try { __wjs_fs_close(this.fd); } catch { } this.fd = -1; }
        this.push(null);
        return;
      }
      this.bytesRead += n;
      this.push(n === this.__hwm ? buf : Buffer.from(buf.subarray(0, n)));
      return;
    }
    if (this.__off >= this.__bytes.length) {
      this.push(null);
      return;
    }
    const end = Math.min(this.__bytes.length, this.__off + this.__hwm);
    this.push(Buffer.from(this.__bytes.subarray(this.__off, end)));
    this.bytesRead = end;
    this.__off = end;
  }
}

// node legacy 形：fs.ReadStream(file) 无 new 可调（自 new）+ instanceof 成立——
// Proxy apply 转 construct。
export const ReadStream = new Proxy(__ReadStream, {
  apply(_t, _this, args) { return new __ReadStream(...args); },
});

export function createReadStream(p, opts) {
  // node 口径：{ fd } 形下 path 可 null（test-fs-promises-file-handle-read 点名）。
  if (!(opts && opts.fd != null)) p = __fsPath(p, "createReadStream");
  return new ReadStream(p, opts);
}
// WriteStream（10f，node 口径镜像 ReadStream）：open(fd)/ready/finish/close 事件序
// + path/flags/autoClose/bytesWritten；sync 底座偏差记档：fd 恒 null、块在内存
// 攒至 _final 一次性落盘（无增量 flush）、open/ready 于首个 _write/_final 前派发。
class __WriteStream extends Writable {
  constructor(p, opts) {
    opts = opts ?? {};
    __fsEncoding(opts);
    __fsValidateOffset(opts.start, "start");
    __fsValidateOffset(opts.end, "end");
    super({ autoDestroy: true, emitClose: true });
    this.path = p;
    this.flags = opts.flags ?? "w";
    this.mode = opts.mode ?? 0o666;
    this.autoClose = opts.autoClose !== false;
    this.bytesWritten = 0;
    this.fd = null;
    this.__chunks = [];
    this.__opened = false;
    if (opts.fd !== undefined && opts.fd !== null) {
      // 10f：fd 形（FileHandle.createWriteStream）——增量直写，非 path 攒块；
      // fd 为 FileHandle 时写/关走 handle 方法（FileHandleOperations 同构）。
      if (opts.fs) {
        const e = new Error("The FileHandle with fs method is not implemented");
        e.code = "ERR_METHOD_NOT_IMPLEMENTED"; throw e;
      }
      this.__fh = typeof opts.fd === "object" ? opts.fd : null;
      this.fd = this.__fh ? this.__fh.fd : opts.fd;
      this.__fdMode = true;
      if (this.__fh && typeof this.__fh.on === "function") {
        this.__fh.on("close", () => { if (!this.destroyed) this.destroy(); });
      }
    }
  }
  __emitOpen() {
    if (this.__opened) return;
    // node 口径：open 事件触发时文件已真实打开（options-immutable 套件：
    // 'open' 回调里 ReadStream 必须读得到该文件——旧假 open 不建文件即 ENOENT）。
    if (!this.__fdMode && this.fd === null) {
      this.fd = Number(__fsCall("open", this.path, () =>
        __wjs_fs_open(this.path, __fsFlags(this.flags, "createWriteStream"), this.mode)));
    }
    this.__opened = true;
    this.emit("open", null);
    this.emit("ready");
  }
  _write(chunk, enc, cb) {
    this.__emitOpen();
    const u8 = __fsData(chunk, "createWriteStream");
    if (this.__fdMode) {
      if (this.__fh) {
        this.__fh.write(u8).then(
          (r) => { this.bytesWritten += r ? r.bytesWritten : u8.length; cb(); },
          (e) => cb(e),
        );
        return;
      }
      try { this.bytesWritten += writeSync(this.fd, u8); cb(); }
      catch (e) { cb(e); }
      return;
    }
    this.__chunks.push(u8);
    this.bytesWritten += u8.length;
    cb();
  }
  _final(cb) {
    this.__emitOpen();
    if (this.__fdMode) {
      if (this.autoClose) {
        if (this.__fh) { this.__fh.close().catch(() => { }); }
        else { try { __wjs_fs_close(this.fd); } catch { } this.fd = -1; }
      }
      cb();
      return;
    }
    // fd 已在 __emitOpen 真实打开（'w' 截断建/a 追加）——攒块经 fd 落盘，
    // 位置 0（'w' 开头）或 -1（追加尾），node close-on-finish 同口径。
    const total = this.__chunks.reduce((n, c) => n + c.length, 0);
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of this.__chunks) { out.set(c, off); off += c.length; }
    const append = this.flags === "a" || this.flags === "a+";
    try {
      __fsCall("write", this.path, () => __wjs_fs_write_fd(this.fd, out, append ? -1 : 0));
    } finally {
      this.__chunks.length = 0;
      if (this.autoClose) { try { __wjs_fs_close(this.fd); } catch { } this.fd = -1; }
    }
    cb();
  }
  _destroy(err, cb) {
    if (!this.__fdMode && typeof this.fd === "number" && this.fd >= 0) {
      try { __wjs_fs_close(this.fd); } catch { }
    }
    this.fd = -1;
    cb(err);
  }
}

export const WriteStream = new Proxy(__WriteStream, {
  apply(_t, _this, args) { return new __WriteStream(...args); },
});

export function createWriteStream(p, opts) {
  if (!(opts && opts.fd != null)) p = __fsPath(p, "createWriteStream");
  return new WriteStream(p, opts);
}
// ---- Phase 9c：同步面增补（link 系/时间戳/权限/access/fd 系/cp/opendir）----
function __fsTimeMs(t, what) {
  if (t instanceof Date) return t.getTime();
  if (typeof t === "number") return t;
  if (typeof t === "string") { const n = Number(t); if (Number.isFinite(n)) return n; }
  throw new TypeError(`${what}: time must be a number, string or Date`);
}
// truncate/ftruncate 的 len（truncate 套件逐字）：undefined → 0；非 number →
// ARG_TYPE 'len'；非整数（±1.5）→ OUT_OF_RANGE 'It must be an integer'。
function __fsLenArg(len) {
  if (len === undefined) return 0;
  if (typeof len !== "number") __vErrType("len", "number", len);
  if (!Number.isInteger(len)) {
    const e = new RangeError(`The value of "len" is out of range. It must be an integer. Received ${len}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  return len;
}
// node lib/internal/fs/utils.js toUnixTimestamp——真机 26.8.2 实测口径：
// 数字串（含 '-1'）→ 数值原样；number 非有限 → ARG_TYPE；number 负 → 当前秒
//（实测 -1 → Date.now()/1000，非 throw）；Date → 秒浮点；日期串可 parse → 秒；
// 其余 ARG_TYPE（timestamp-parsing/utimes 套件）。
function __toUnixTimestamp(time, name = "time") {
  if (typeof time === "number") {
    if (!Number.isFinite(time)) __vErrType(name, "number or Date", time);
    return time < 0 ? Date.now() / 1000 : time;
  }
  if (typeof time === "string" && +time === time) return +time;
  if (time instanceof Date) return time.getTime() / 1000;
  if (typeof time === "string") {
    const d = new Date(time);
    if (!Number.isNaN(d.getTime())) return d.getTime() / 1000;
  }
  __vErrType(name, "number or Date", time);
}
// utimes 族的时间实参（秒口径，y2K38 套件）：Date → ms 直传（精度全保）；
// 其余经 toUnixTimestamp（秒）× 1000。
function __fsUtimeMs(t, what) {
  if (t instanceof Date) return t.getTime();
  return __toUnixTimestamp(t, what) * 1000;
}
function __fsModeNum(mode) {
  // node chmod 族：parseFileMode(mode, 'mode')（无 def；undefined → ARG_TYPE）。
  return __fsParseMode(mode);
}
export function accessSync(p, mode = 0) {
  if (mode !== undefined && typeof mode !== "number") {
    const e = new TypeError(`The "mode" argument must be of type number. Received ${typeof mode} (${JSON.stringify(String(mode))})`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  p = __fsPath(p, "access");
  __fsCall("access", p, () => __wjs_fs_access(p, mode));
}
export function truncateSync(p, len) {
  p = __fsPath(p, "truncate");
  const n = __fsLenArg(len);
  __fsCall("truncate", p, () => __wjs_fs_truncate(p, n));
}
export function utimesSync(p, atime, mtime) {
  p = __fsPath(p, "utimes");
  __fsCall("utimes", p, () => __wjs_fs_utimes(p, __fsUtimeMs(atime, "atime"), __fsUtimeMs(mtime, "mtime")));
}
export function lutimesSync(p, atime, mtime) {
  p = __fsPath(p, "lutimes");
  __fsCall("lutimes", p, () => __wjs_fs_lutimes(p, __fsUtimeMs(atime, "atime"), __fsUtimeMs(mtime, "mtime")));
}
export function chmodSync(p, mode) {
  p = __fsPath(p, "chmod");
  __vModeArg(mode);
  __fsCall("chmod", p, () => __wjs_fs_chmod(p, __fsModeNum(mode)));
}
export function chownSync(p, uid, gid) {
  p = __fsPath(p, "chown");
  // node validateInt32（fchown 套件）：非 number → ARG_TYPE /uid|gid/；
  // 非整数（Infinity/NaN）→ OUT_OF_RANGE 'It must be an integer'；-1 = 不变更。
  __vIntRange(uid, "uid", -1, 4294967295);
  __vIntRange(gid, "gid", -1, 4294967295);
  __fsCall("chown", p, () => __wjs_fs_chown(p, uid, gid));
}
export function fchownSync(fd, uid, gid) {
  __vFd(fd);
  __vIntRange(uid, "uid", -1, 4294967295);
  __vIntRange(gid, "gid", -1, 4294967295);
  __fsCall("fchown", "", () => __wjs_fs_fchown(fd, uid, gid));
}
export function lchownSync(p, uid, gid) {
  p = __fsPath(p, "lchown");
  __vIntRange(uid, "uid", -1, 4294967295);
  __vIntRange(gid, "gid", -1, 4294967295);
  __fsCall("lchown", p, () => __wjs_fs_lchown(p, uid, gid));
}
// node：fs.lchmod 仅 macOS 存在（native 仅 macOS 注册，非 macOS 导出 undefined）；
// mode 走 parseFileMode（lchmod 套件校验矩阵）。
function lchmodSyncImpl(p, mode) {
  p = __fsPath(p, "lchmod");
  const n = __fsModeNum(mode);
  __fsCall("lchmod", p, () => __wjs_fs_lchmod(p, n));
}
export const lchmodSync = typeof __wjs_fs_lchmod === "function" ? lchmodSyncImpl : undefined;

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
export function readlinkSync(p, opts) {
  p = __fsPath(p, "readlink");
  const enc = __fsEncoding(opts);
  const link = __fsCall("readlink", p, () => __wjs_fs_read_link(p));
  if (enc === "buffer") return Buffer.from(link);
  return link;
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
  // node 顺序：stringToFlags → parseFileMode(mode, 'mode', 0o666) → path 校验。
  const f = __fsFlags(flags, "open");
  const m = __fsParseMode(mode, 0o666);
  p = __fsPath(p, "open");
  return __fsCall("open", p, () => Number(__wjs_fs_open(p, f, m)));
}
export function closeSync(fd) {
  __vFd(fd);
  __fsCall("close", "", () => __wjs_fs_close(fd));
}
// ── 10f：参数校验族（node lib/internal/validators 口径；code+message 对拍）──
function __vReceived(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  const t = typeof v;
  if (t === "string") return `type string ('${v}')`;
  if (t === "boolean") return `type boolean (${v})`;
  if (t === "number") return `type number (${v})`;
  if (t === "object") {
    if (Array.isArray(v)) return "an instance of Array";
    const n = v.constructor && v.constructor.name ? v.constructor.name : "Object";
    return `an instance of ${n}`;
  }
  if (t === "function") return `function ${v.name}`;
  return `type ${t} (${String(v)})`;
}
function __vErrType(name, expected, v) {
  const e = new TypeError(`The "${name}" argument must be of type ${expected}. Received ${__vReceived(v)}`);
  e.code = "ERR_INVALID_ARG_TYPE"; throw e;
}
function __vIntRange(v, name, min, max) {
  if (typeof v !== "number") __vErrType(name, "number", v);
  if (!Number.isInteger(v)) {
    const e = new RangeError(`The value of "${name}" is out of range. It must be an integer. Received ${v}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  if (v < min || v > max) {
    const e = new RangeError(`The value of "${name}" is out of range. It must be >= ${min} && <= ${max}. Received ${v}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  return v;
}
function __vFd(fd) {
  // node getValidatedFd：int32 正数域（fchmod 套件点名 -1 / 2**32）。
  return __vIntRange(fd, "fd", 0, 2147483647);
}
function __fsParseMode(mode, def) {
  // node parseFileMode（validators.js 逐字；open-mode-mask/fchmod 套件 + 真机对拍）：
  // null/undefined → def；string 须全八进制位 [0-7]+（非八进制串 ARG_VALUE
  // 'must be a 32-bit unsigned integer or an octal string'，TypeError 类）；
  // 其余非 number → ARG_TYPE 'number'；整数域 [0, 2**32-1] OUT_OF_RANGE。
  mode ??= def;
  if (typeof mode === "string") {
    if (!/^[0-7]+$/.test(mode)) {
      const e = new TypeError(`The argument 'mode' must be a 32-bit unsigned integer or an octal string. Received '${mode}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    mode = parseInt(mode, 8);
  }
  if (typeof mode !== "number") __vErrType("mode", "number", mode);
  if (!Number.isInteger(mode)) {
    const e = new RangeError(`The value of "mode" is out of range. It must be an integer. Received ${mode}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  if (mode < 0 || mode > 4294967295) {
    const e = new RangeError(`The value of "mode" is out of range. It must be >= 0 && <= 4294967295. Received ${mode}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  return mode + 0;
}
function __vModeArg(mode) {
  if (typeof mode !== "number" && typeof mode !== "string") __vErrType("mode", "number or string", mode);
  return mode;
}
function __vIdNum(v, name) {
  if (typeof v !== "number" || !Number.isInteger(v)) __vErrType(name, "number", v);
  return v;
}
function __vCbArg(cb) {
  if (typeof cb !== "function") __vErrType("callback", "function", cb);
  return cb;
}
function __vBooleanProp(v, name) {
  // node validateBoolean：属性校验文案用 "property"（test-fs-mkdir 逐字断言）。
  if (typeof v !== "boolean") {
    const e = new TypeError(`The "${name}" property must be of type boolean. Received ${__vReceived(v)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  return v;
}
function __vInteger(v, name, min) {
  // node validateInteger 逐字：非 number ARG_TYPE；非整数 OOR 'an integer'。
  if (typeof v !== "number") __vErrType(name, "number", v);
  if (!Number.isInteger(v)) {
    const e = new RangeError(`The value of "${name}" is out of range. It must be an integer. Received ${v}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  if (min !== undefined && v < min) {
    const e = new RangeError(`The value of "${name}" is out of range. It must be >= ${min}. Received ${v}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  return v;
}
function __vOffsetLength(offset, length, byteLength) {
  if (offset < 0 || length < 0 || byteLength - offset < length) {
    const bad = offset < 0 ? "offset" : "length";
    const e = new RangeError(`The value of "${bad}" is out of range. It must be >= 0. Received ${bad === "offset" ? offset : length}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
}
function __vBuffer(buffer) {
  // node validateBuffer 逐字（test-fs-read 'options.buffer is null' 点名 message）。
  if (!ArrayBuffer.isView(buffer)) {
    const e = new TypeError(`The "buffer" argument must be an instance of Buffer, TypedArray, or DataView. Received ${__vReceived(buffer)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
}
function __vEmptyBuffer(buffer) {
  // node ERR_INVALID_ARG_VALUE('buffer', buffer, 'is empty and cannot be written')。
  if (buffer.byteLength === 0) __fsEmptyBufferErr(buffer);
}
export function readSync(fd, buffer, offsetOrOptions, length, position) {
  __vFd(fd);
  __vBuffer(buffer);
  // node：3 参即 options 形（{offset,length,position}；validateObject 允 null）。
  let offset = offsetOrOptions;
  if (arguments.length <= 3 || (offsetOrOptions !== null && typeof offsetOrOptions === "object")) {
    if (offsetOrOptions !== undefined && offsetOrOptions !== null &&
        (Array.isArray(offsetOrOptions) || typeof offsetOrOptions !== "object")) {
      __vErrType("options", "object", offsetOrOptions);
    }
    ({ offset = 0, length = buffer.byteLength - offset, position = null } = offsetOrOptions ?? {});
  }
  if (offset === undefined) offset = 0;
  else __vInteger(offset, "offset", 0);
  length |= 0;
  if (position == null) position = -1;
  else __fsValidatePosition(position, "position", length);
  if (length === 0) return 0;
  if (buffer.byteLength === 0) __fsEmptyBufferErr(buffer);
  __vOffsetLength(offset, length, buffer.byteLength);
  const chunk = __fsCall("read", "", () =>
    __wjs_fs_read_fd(fd, length, typeof position === "bigint" ? Number(position) : position));
  buffer.set(chunk, offset);
  return chunk.length;
}
// node：ERR_INVALID_ARG_VALUE('buffer', buffer, 'is empty and cannot be written')
// （read-empty-buffer 套件逐字；inspect 形 Received Uint8Array(0) []）。
function __fsEmptyBufferErr(buffer) {
  // node inspect 口径：空 TypedArray 即 `Uint8Array(0) []`（套件仅空视图到达此点）。
  const ctor = (buffer && buffer.constructor && buffer.constructor.name) || "Uint8Array";
  const e = new TypeError(`The argument 'buffer' is empty and cannot be written. Received ${ctor}(0) []`);
  e.code = "ERR_INVALID_ARG_VALUE"; throw e;
}
// node validatePosition：number 整数 >= -1；bigint 域 [ -1, 2**63-1-length ]；余 ARG_TYPE。
function __fsValidatePosition(position, name, length) {
  if (typeof position === "number") {
    __vInteger(position, name, -1);
  } else if (typeof position === "bigint") {
    const maxPosition = 2n ** 63n - 1n - BigInt(length);
    if (!(position >= -1n && position <= maxPosition)) {
      const e = new RangeError(`The value of "${name}" is out of range. It must be >= -1 && <= ${maxPosition}. Received ${position}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
  } else {
    __vErrType(name, "integer or bigint", position);
  }
}
export function writeSync(fd, buffer, offsetOrOptions, length, position) {
  __vFd(fd);
  let data;
  let pos = -1;
  if (ArrayBuffer.isView(buffer)) {
    let offset = offsetOrOptions;
    // node：view + options 对象形（{offset,length,position}）。
    if (offsetOrOptions !== null && typeof offsetOrOptions === "object" && !ArrayBuffer.isView(offsetOrOptions)) {
      ({ offset = 0, length = buffer.byteLength - offset, position = null } = offsetOrOptions ?? {});
    }
    if (offset == null) offset = 0;
    else __vInteger(offset, "offset", 0);
    if (typeof length !== "number") length = buffer.byteLength - offset;
    length |= 0;
    const u8 = __fsData(buffer, "write");
    __fsValidateOffsetLengthWrite(offset, length, u8.length);
    data = u8.subarray(offset, offset + length);
    if (position != null) __fsValidatePosition(position, "position", length);
    pos = typeof position === "bigint" ? Number(position) : (position ?? -1);
  } else if (typeof buffer === "string") {
    pos = typeof offsetOrOptions === "number" ? offsetOrOptions : -1;
    data = __fsData(buffer, "write");
  } else {
    __vBuffer(buffer);
  }
  return Number(__fsCall("write", "", () => __wjs_fs_write_fd(fd, data, pos)));
}
// node validateOffsetLengthWrite 逐字：length > byteLength - offset 即 OOR。
function __fsValidateOffsetLengthWrite(offset, length, byteLength) {
  if (length > byteLength - offset) {
    const e = new RangeError(`The value of "length" is out of range. It must be <= ${byteLength - offset}. Received ${length}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  if (offset < 0 || length < 0) {
    const bad = offset < 0 ? "offset" : "length";
    const e = new RangeError(`The value of "${bad}" is out of range. It must be >= 0. Received ${bad === "offset" ? offset : length}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
}
// node validateBufferArray（validators 逐字口径）：数组 + 每项 ArrayBufferView。
function __fsValidateBufferArray(buffers) {
  if (!Array.isArray(buffers)) __vErrType("buffers", "ArrayBufferView[]", buffers);
  for (const b of buffers) {
    if (!ArrayBuffer.isView(b)) __vErrType("buffers", "ArrayBufferView[]", buffers);
  }
  return buffers;
}
export function ftruncateSync(fd, len) {
  __vFd(fd);
  const n = __fsLenArg(len);
  __fsCall("ftruncate", "", () => __wjs_fs_ftruncate(fd, n));
}
export function fsyncSync(fd) {
  __vFd(fd);
  __fsCall("fsync", "", () => __wjs_fs_fsync(fd, false));
}
export function fdatasyncSync(fd) {
  __vFd(fd);
  __fsCall("fsync", "", () => __wjs_fs_fsync(fd, true));
}
export function fstatSync(fd) {
  __vFd(fd);
  // throwIfNoEntry 只豁免路径 ENOENT——fd EBADF 恒抛（stat-bigint 套件逐项）。
  const __o = arguments[1];
  return new __Stats(JSON.parse(__fsCall("fstat", "", () => __wjs_fs_fstat(fd))), __o?.bigint === true);
}
export function fchmodSync(fd, mode) {
  __vFd(fd);
  __vModeArg(mode);
  __fsCall("fchmod", "", () => __wjs_fs_fchmod(fd, __fsModeNum(mode)));
}
export function futimesSync(fd, atime, mtime) {
  __vFd(fd);
  __fsCall("futimes", "", () => __wjs_fs_futimes(fd, __fsUtimeMs(atime, "atime"), __fsUtimeMs(mtime, "mtime")));
}
// ---- 9c：opendir / Dir（惰性游标，readdir 底座，记档非真流式）----
// node lib/internal/fs/dir.js 同构（10f 对拍）：path 原型 getter 带 brand 门
// （ERR_INVALID_THIS）、close 后 ERR_DIR_CLOSED、async 在途时同步操作
// ERR_DIR_CONCURRENT_OPERATION（async 互排队）、close(cb) 错误走回调。
export class Dir {
  #entries;
  #cursor = 0;
  #closed = false;
  #path;
  #opQueue = null;
  constructor(path) {
    // node：new fs.Dir() 无 handle → ERR_MISSING_ARGS（test-fs-read 点名）。
    if (path === undefined || path === null) {
      const e = new TypeError('The "handle" argument must be specified');
      e.code = "ERR_MISSING_ARGS"; throw e;
    }
    this.#path = path;
    this.#entries = readdirSync(path, { withFileTypes: true });
  }
  get path() {
    if (!(#path in this)) {
      const e = new TypeError('Value of "this" must be of type Dir');
      e.code = "ERR_INVALID_THIS"; throw e;
    }
    return this.#path;
  }
  #dirClosedErr() {
    const err = new Error("Directory handle was closed");
    err.code = "ERR_DIR_CLOSED";
    return err;
  }
  #dirClosed() { throw this.#dirClosedErr(); }
  #readOne() {
    return this.#cursor < this.#entries.length ? this.#entries[this.#cursor++] : null;
  }
  readSync() {
    if (this.#closed) this.#dirClosed();
    if (this.#opQueue !== null) {
      const err = new Error("Cannot do synchronous work on directory handle with concurrent asynchronous operations");
      err.code = "ERR_DIR_CONCURRENT_OPERATION"; throw err;
    }
    return this.#readOne();
  }
  // node 口径：目录项立即读（同步）再包 promise——延迟读会在 close() 后才
  // 执行抛 ERR_DIR_CLOSED（phase9c 现形）。promise 形在途时，后到的 async
  // read/close 进 #opQueue 排队（node operationQueue 同构）。
  read() {
    if (arguments.length > 0) {
      // node #readImpl 顺序：closed 先于 callback 校验。
      if (this.#closed) this.#dirClosed();
      const cb = arguments[0];
      if (typeof cb !== "function") __vErrType("callback", "function", cb);
      const self = this;
      queueMicrotask(() => { let d; try { d = self.#readOne(); } catch (e) { cb(e); return; } cb(null, d); });
      return;
    }
    if (this.#closed) return Promise.reject(this.#dirClosedErr());
    const self = this;
    if (this.#opQueue !== null) {
      return new Promise((res, rej) => this.#opQueue.push(() => {
        try { res(self.#readOne()); } catch (e) { rej(e); }
      }));
    }
    this.#opQueue = [];
    return Promise.resolve().then(() => {
      const d = self.#readOne();
      const q = self.#opQueue ?? []; self.#opQueue = null;
      for (const op of q) queueMicrotask(op);
      return d;
    });
  }
  closeSync() {
    if (this.#closed) this.#dirClosed();
    if (this.#opQueue !== null) {
      const err = new Error("Cannot do synchronous work on directory handle with concurrent asynchronous operations");
      err.code = "ERR_DIR_CONCURRENT_OPERATION"; throw err;
    }
    this.#closed = true;
  }
  close(cb) {
    if (cb === undefined) {
      if (this.#closed) return Promise.reject(this.#dirClosedErr());
      if (this.#opQueue !== null) {
        return new Promise((res, rej) => this.#opQueue.push(() => {
          try { this.#closed = true; res(); } catch (e) { rej(e); }
        }));
      }
      const self = this;
      return Promise.resolve().then(() => {
        self.#closed = true;
        const q = self.#opQueue ?? []; self.#opQueue = null;
        for (const op of q) queueMicrotask(op);
      });
    }
    if (typeof cb !== "function") __vErrType("callback", "function", cb);
    if (this.#closed) {
      const err = this.#dirClosedErr();
      queueMicrotask(() => cb(err));
      return;
    }
    const self = this;
    queueMicrotask(() => { self.#closed = true; cb(null); });
  }
  // node dir.js 逐字：dispose 幂等（已关即静默返回，不重复拒绝/抛出）。
  [Symbol.dispose]() { if (this.#closed) return; this.closeSync(); }
  async [Symbol.asyncDispose]() {
    if (this.#closed) return;
    await this.close();
  }
  [Symbol.iterator]() {
    const self = this;
    return {
      next() {
        const d = self.readSync();
        return d === null ? { done: true } : { value: d, done: false };
      },
    };
  }
  // node：for-await 用 async generator（entries()）；break/return/throw 即自动
  // close（AsyncIterBreak/Return/Throw 三套件）。
  [Symbol.asyncIterator]() {
    const self = this;
    return {
      async next() {
        const d = await self.read();
        return d === null ? { done: true } : { value: d, done: false };
      },
      async return() {
        try { await self.close(); } catch {}
        return { done: true };
      },
    };
  }
}
export function opendirSync(p, opts) {
  __fsEncoding(opts);
  // node Dir 构造：validateUint32(bufferSize, 'options.bufferSize', true)——
  // 非 number ARG_TYPE；0/负/非整 OOR（>= 1 && <= 4294967295）。
  const bs = (opts && typeof opts === "object" && !Array.isArray(opts)) ? opts.bufferSize : undefined;
  if (bs !== undefined) {
    if (typeof bs !== "number") {
      const e = new TypeError(`The "options.bufferSize" property must be of type number. Received ${__vReceived(bs)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (!Number.isInteger(bs)) {
      const e = new RangeError(`The value of "options.bufferSize" is out of range. It must be an integer. Received ${bs}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    if (bs < 1 || bs > 4294967295) {
      const e = new RangeError(`The value of "options.bufferSize" is out of range. It must be >= 1 && <= 4294967295. Received ${bs}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
  }
  p = __fsPath(p, "opendir");
  return __fsCall("opendir", p, () => new Dir(p));
}
// ---- 9c：FileHandle + fs.promises（promises 挂 node:fs 本体，fs/promises 反向
// re-export 免环；底层同步实现，文档口径不变）----
// node 口径：FileHandle 即 EventEmitter（'close' 事件）+ [Symbol.dispose]
// （触发 close，不 await——node 26 explicit resource management 面）。
export class FileHandle extends EventEmitter {
  constructor(fd) {
    super();
    this.fd = fd;
  }
  [Symbol.dispose]() {
    this.close();
  }
  async [Symbol.asyncDispose]() {
    await this.close();
  }
  close() {
    // node 口径：close 幂等——缓存 close promise，二次调用返回同一 promise
    //（流收尾会代调 handle.close()，test-fs-promises-file-handle-read 显式
    // 二次 close 不炸不重发）。
    if (this.__closePromise) return this.__closePromise;
    const fd = this.fd;
    this.__closePromise = Promise.resolve()
      .then(() => __fsCall("close", "", () => __wjs_fs_close(fd)))
      .then(() => {
        this.fd = -1;
        this.emit("close");
      });
    return this.__closePromise;
  }
  // node internal/fs/promises.js read()/write() 同构（10f）：params 形 /
  // (buffer, options) 形 / null 位置归 -1，返回 { bytesRead, buffer }。
  read(buffer, offset, length, position) {
    return Promise.resolve().then(() => {
      let b = buffer;
      if (!ArrayBuffer.isView(b)) {
        // fh.read(params)
        ({ buffer: b = Buffer.alloc(16384), offset = 0, length = b.byteLength - offset, position = null } = buffer ?? {});
      }
      if (offset !== null && typeof offset === "object") {
        // fh.read(buffer, options)
        ({ offset = 0, length = b.byteLength - offset, position = null } = offset);
      }
      if (offset == null) offset = 0;
      length ??= b.byteLength - offset;
      if (position == null) position = -1;
      if (b.byteLength === 0) __fsEmptyBufferErr(b);
      if (length === 0) return { bytesRead: 0, buffer: b };
      const n = readSync(this.fd, b, offset, length, position);
      return { bytesRead: n || 0, buffer: b };
    });
  }
  write(buffer, offsetOrOptions, length, position) {
    return Promise.resolve().then(() => {
      if (typeof buffer !== "string" && !ArrayBuffer.isView(buffer)) {
        const e = new TypeError(`The "buffer" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${typeof buffer}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (buffer?.byteLength === 0) return { bytesWritten: 0, buffer };
      let offset = offsetOrOptions;
      if (ArrayBuffer.isView(buffer)) {
        if (typeof offset === "object" && offset !== null) {
          ({ offset = 0, length = buffer.byteLength - offset, position = null } = offsetOrOptions ?? {});
        }
        if (offset == null) offset = 0;
        if (typeof length !== "number") length = buffer.byteLength - offset;
        if (typeof position !== "number") position = null;
        const n = writeSync(this.fd, buffer, offset, length, position);
        return { bytesWritten: n || 0, buffer };
      }
      // 字符串形态：(str[, position[, encoding]])
      const pos = typeof offset === "number" ? offset : -1;
      const enc = typeof length === "string" ? length : "utf8";
      const bytes = enc === "utf8" ? __fsData(buffer, "write") : Buffer.from(buffer, enc);
      const n = __fsCall("write", "", () => __wjs_fs_write_fd(this.fd, bytes, pos));
      return { bytesWritten: n || 0, buffer };
    });
  }
  chown(uid, gid) { return Promise.resolve().then(() => fchownSync(this.fd, uid, gid)); }
  readv(buffers, position) {
    return Promise.resolve().then(() => {
      const n = readvSync(this.fd, buffers, typeof position === "number" ? position : null);
      return { bytesRead: n || 0, buffers };
    });
  }
  writev(buffers, position) {
    return Promise.resolve().then(() => {
      const n = writevSync(this.fd, buffers, typeof position === "number" ? position : null);
      return { bytesWritten: n || 0, buffers };
    });
  }
  createReadStream(options) {
    // node 口径：fs 流 + { ...options, fd: this }（fd 形流，非 path）。
    return new ReadStream(undefined, { ...options, fd: this });
  }
  createWriteStream(options) {
    return new WriteStream(undefined, { ...options, fd: this });
  }
  stat(options) {
    // node 口径：close 后 fd=-1 → binding 层 EBADF（非范围校验错误）。
    return Promise.resolve().then(() => {
      if (this.fd === -1) {
        const e = new Error("EBADF: bad file descriptor, fstat");
        e.code = "EBADF"; e.errno = -9; e.syscall = "fstat";
        throw e;
      }
      return fstatSync(this.fd, options);
    });
  }
  truncate(len) { return Promise.resolve().then(() => ftruncateSync(this.fd, len ?? 0)); }
  chmod(mode) { return Promise.resolve().then(() => fchmodSync(this.fd, mode)); }
  utimes(atime, mtime) { return Promise.resolve().then(() => futimesSync(this.fd, atime, mtime)); }
  sync() { return Promise.resolve().then(() => fsyncSync(this.fd)); }
  datasync() { return Promise.resolve().then(() => fdatasyncSync(this.fd)); }
  readFile(opts) {
    const enc = __fsEncoding(opts);
    const signal = opts && typeof opts === "object" ? opts.signal : undefined;
    if (signal) {
      if (typeof signal.addEventListener !== "function") {
        const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (signal.aborted) throw __fsAbortErr(signal.reason);
    }
    const doRead = () => {
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
    };
    if (signal) {
      // node 读为线程池异步——abort 竞速即拒（file-handle-readFile cancel 套件）。
      return Promise.race([
        new Promise((resolve, reject) => {
          setTimeout(() => {
            try {
              if (signal.aborted) { reject(__fsAbortErr(signal.reason)); return; }
              resolve(doRead());
            } catch (e) { reject(e); }
          }, 0);
        }),
        new Promise((_, reject) =>
          signal.addEventListener("abort", () => reject(__fsAbortErr(signal.reason)), { once: true })),
      ]);
    }
    return Promise.resolve().then(doRead);
  }
  async writeFile(data, opts) {
    // node 口径：流/同步+异步可迭代逐块收；signal abort 即 AbortError
    //（cause=reason）；第二参可 string（encoding）或 { encoding, signal }。
    if (typeof data !== "string" && !ArrayBuffer.isView(data) &&
        !(data && typeof data[Symbol.asyncIterator] === "function") &&
        !(data && typeof data[Symbol.iterator] === "function")) {
      const e = new TypeError(`The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${data === null ? "null" : typeof data}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    const enc = typeof opts === "string" ? opts : (opts && opts.encoding);
    const isChunk = (c) => typeof c === "string" || ArrayBuffer.isView(c);
    let parts = null;
    if (data && typeof data[Symbol.asyncIterator] === "function") {
      parts = [];
      for await (const c of data) {
        if (!isChunk(c)) { const e = new TypeError(`The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${c === null ? "null" : typeof c}`); e.code = "ERR_INVALID_ARG_TYPE"; throw e; }
        parts.push(c);
      }
    } else if (data && !ArrayBuffer.isView(data) && typeof data[Symbol.iterator] === "function") {
      parts = [];
      for (const c of data) {
        if (!isChunk(c)) { const e = new TypeError(`The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${c === null ? "null" : typeof c}`); e.code = "ERR_INVALID_ARG_TYPE"; throw e; }
        parts.push(c);
      }
    }
    const encChunk = (c) => {
      const u8 = typeof c === "string" && enc && enc !== "utf8" ? Buffer.from(c, enc) : __fsData(c, "writeFile");
      return new Uint8Array(u8.buffer, u8.byteOffset, u8.byteLength);
    };
    const bytes = parts
      ? (() => { const out = new Uint8Array(parts.reduce((a, c) => a + encChunk(c).length, 0)); let off = 0; for (const c of parts) { const u8 = encChunk(c); out.set(u8, off); off += u8.length; } return out; })()
      : __fsData(data, "writeFile");
    const signal = opts && typeof opts === "object" ? opts.signal : undefined;
    if (signal) {
      if (typeof signal.addEventListener !== "function") {
        const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (signal.aborted) throw __fsAbortErr(signal.reason);
    }
    if (signal) {
      await Promise.race([
        new Promise((resolve, reject) => {
          // node 写经线程池异步完成；本仓为同步写——落宏任务使 abort 的
          // nextTick 检查点先于写触发（doWriteBufferAndCancel 点名）。
          setTimeout(() => {
            try {
              if (signal.aborted) { reject(__fsAbortErr(signal.reason)); return; }
              resolve(__fsCall("write", "", () => __wjs_fs_write_fd(this.fd, bytes, 0)));
            } catch (e) { reject(e); }
          }, 0);
        }),
        new Promise((_, reject) =>
          signal.addEventListener("abort", () => reject(__fsAbortErr(signal.reason)), { once: true })),
      ]);
      return;
    }
    // position -1 = 当前位（writeFile 无 position；O_APPEND fd 落尾，
    // doWriteFileAndAppend 'HelloWorld' 点名）。
    await Promise.resolve().then(() => __fsCall("write", "", () => __wjs_fs_write_fd(this.fd, bytes, -1)));
  }
  // node：appendFile data 面 = writeFile 同族（iterable/asyncIterable + signal
  // abort 契约；promises-file-handle-append-file 套件逐项）。
  async appendFile(data, opts) {
    const signal = opts && typeof opts === "object" ? opts.signal : undefined;
    if (signal) {
      if (typeof signal.addEventListener !== "function") {
        const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (signal.aborted) throw __fsAbortErr(signal.reason);
    }
    let bytes;
    if (data && typeof data === "object" && !ArrayBuffer.isView(data) && typeof data[Symbol.asyncIterator] === "function") {
      const enc = typeof opts === "string" ? opts : (opts && opts.encoding);
      const isChunk = (c) => typeof c === "string" || ArrayBuffer.isView(c);
      const encChunk = (c) => {
        const u8 = typeof c === "string" && enc && enc !== "utf8" ? Buffer.from(c, enc) : __fsData(c, "appendFile");
        return new Uint8Array(u8.buffer, u8.byteOffset, u8.byteLength);
      };
      const parts = [];
      for await (const c of data) {
        if (signal && signal.aborted) throw __fsAbortErr(signal.reason);
        if (!isChunk(c)) __chunkArgErr(c);
        parts.push(c);
      }
      const out = new Uint8Array(parts.reduce((a, c) => a + encChunk(c).length, 0));
      let off = 0;
      for (const c of parts) { const u8 = encChunk(c); out.set(u8, off); off += u8.length; }
      bytes = out;
    } else {
      bytes = __fsDataSync(data, "data", opts);
    }
    if (signal && signal.aborted) throw __fsAbortErr(signal.reason);
    const doAppend = () => {
      const size = fstatSync(this.fd).size;
      __fsCall("write", "", () => __wjs_fs_write_fd(this.fd, bytes, size));
    };
    if (signal) {
      // 写落宏任务 + abort 竞速（FileHandle.write 同构；bufferAndCancel 契约）。
      await Promise.race([
        new Promise((resolve, reject) => {
          setTimeout(() => {
            try {
              if (signal.aborted) { reject(__fsAbortErr(signal.reason)); return; }
              resolve(doAppend());
            } catch (e) { reject(e); }
          }, 0);
        }),
        new Promise((_, reject) =>
          signal.addEventListener("abort", () => reject(__fsAbortErr(signal.reason)), { once: true })),
      ]);
      return;
    }
    return Promise.resolve().then(doAppend);
  }
}
const __as = (fn) => function (...args) { return Promise.resolve().then(() => fn(...args)); };
export const promises = {
  access: __as(accessSync),
  appendFile: (p, data, opts) => Promise.resolve().then(() => __fsAppendFileAsync(p, data, opts)),
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
  mkdtempDisposable: mkdtempDisposableProm,
  readv: (fd, buffers, position) => Promise.resolve().then(() => ({ bytesRead: readvSync(fd, buffers, position) || 0, buffers })),
  writev: (fd, buffers, position) => Promise.resolve().then(() => ({ bytesWritten: writevSync(fd, buffers, position) || 0, buffers })),
  stat: __as(statSync),
  statfs: __as(statfsSync),
  symlink: __as(symlinkSync),
  truncate: __as(truncateSync),
  unlink: __as(unlinkSync),
  utimes: __as(utimesSync),
  lutimes: __as(lutimesSync),
  writeFile: __as(writeFileSync),
  chown: __as(chownSync),
  lchown: __as(lchownSync),
  fchown: __as(fchownSync),
  futimes: __as(futimesSync),
  ...(typeof lchmodSync === "function" ? { lchmod: __as(lchmodSync) } : {}),
};
// ---- 9c：回调全家（err-first；promise 底座经 queueMicrotask 派发）----
function __nodeify(p, cb) {
  __vCbArg(cb);
  p.then(
    // node 口径：无结果 API（close/access 等）回调只带 (err)，不补 undefined
    //（test-fs-close：deepStrictEqual(args, [null]) 点名）。
    (v) => queueMicrotask(() => v === undefined ? cb(null) : cb(null, v)),
    (e) => queueMicrotask(() => cb(e)),
  );
}
const __cb1 = (syncFn, name, before) => function (...args) {
  let cb = args[args.length - 1];
  __vCbArg(cb);
  const rest = args.slice(0, -1);
  // node 口径：参数校验错误（ERR_INVALID_ARG_* / ERR_OUT_OF_RANGE）同步抛，
  // 操作错误（ENOENT 等）走回调（syncFn 立即执行，回调仍经 queueMicrotask）。
  let p;
  try {
    p = Promise.resolve(syncFn(...before(rest)));
  } catch (e) {
    if (e && typeof e.code === "string" &&
        (e.code === "ERR_INVALID_ARG_TYPE" || e.code === "ERR_INVALID_ARG_VALUE" || e.code === "ERR_OUT_OF_RANGE")) throw e;
    p = Promise.reject(e);
  }
  __nodeify(p, cb);
};
const __fdCb = (syncFn, name) => function (...args) {
  let cb = args[args.length - 1];
  if (typeof cb !== "function") {
    // node 口径：值校验错误优先抛（/fd|uid|gid/），操作错误让位于
    // callback 校验（lchown 套件：值错误 ARG_TYPE / 操作错误 + 无效 cb
    // → ARG_TYPE /callback/；不得造孤儿 rejected promise → unhandled）。
    try { syncFn(...args); } catch (e) {
      if (e && typeof e.code === "string" &&
          (e.code === "ERR_INVALID_ARG_TYPE" || e.code === "ERR_INVALID_ARG_VALUE" || e.code === "ERR_OUT_OF_RANGE")) throw e;
    }
    __vCbArg(cb);
    return;
  }
  const rest = args.slice(0, -1);
  let p;
  try {
    p = Promise.resolve(syncFn(...rest));
  } catch (e) {
    if (e && typeof e.code === "string" &&
        (e.code === "ERR_INVALID_ARG_TYPE" || e.code === "ERR_INVALID_ARG_VALUE" || e.code === "ERR_OUT_OF_RANGE")) throw e;
    p = Promise.reject(e);
  }
  __nodeify(p, cb);
};

const __id = (a) => a;
// readFile 回调面定制（非泛型 __cb1）：node 读在线程池异步执行，abort 可落
// "读期间"；本仓读同步完成，等价窗口 = 同步读之后、交付之前——在 promise
// then（microtask，先于 __nodeify 的交付）里复查 signal.aborted，成功转
// AbortError（readfile 套件 "cancellation, during read" 点名；nextTick abort
// 先于 microtask 交付，纯同步复查抓不到）。校验/已abort 同步抛路径不变。
export const readFile = function (...args) {
  let cb = args[args.length - 1];
  __vCbArg(cb);
  const rest = args.slice(0, -1);
  const sig = (() => {
    const opts = rest[1];
    return opts && typeof opts === "object" && typeof opts.signal === "object" && opts.signal !== null && typeof opts.signal.addEventListener === "function" ? opts.signal : null;
  })();
  let p;
  try {
    p = Promise.resolve(readFileSync(...rest));
  } catch (e) {
    if (e && typeof e.code === "string" &&
        (e.code === "ERR_INVALID_ARG_TYPE" || e.code === "ERR_INVALID_ARG_VALUE" || e.code === "ERR_OUT_OF_RANGE")) throw e;
    p = Promise.reject(e);
  }
  if (sig && !sig.aborted) {
    p = p.then((v) => (sig.aborted ? Promise.reject(__fsAbortErr(sig.reason)) : v));
  }
  __nodeify(p, cb);
};
// writeFile 回调面定制（镜像 readFile）：本仓写同步完成，abort 只能落在
// "同步写之后、交付之前"——then（microtask，先于 __nodeify 交付）里复查
// signal.aborted，成功转 AbortError（write-file 套件 cancellable 点名）。
export const writeFile = function (...args) {
  let cb = args[args.length - 1];
  __vCbArg(cb);
  const rest = args.slice(0, -1);
  // node writeFile 形：(path, data, options, callback)——opts 在 rest[2]
  //（readFile 是 (path, options, cb)，opts 在 rest[1]，两者勿混）。
  const sig = (() => {
    const opts = rest[2];
    return opts && typeof opts === "object" && typeof opts.signal === "object" && opts.signal !== null && typeof opts.signal.addEventListener === "function" ? opts.signal : null;
  })();
  let p;
  try {
    p = Promise.resolve(writeFileSync(...rest));
  } catch (e) {
    if (e && typeof e.code === "string" &&
        (e.code === "ERR_INVALID_ARG_TYPE" || e.code === "ERR_INVALID_ARG_VALUE" || e.code === "ERR_OUT_OF_RANGE")) throw e;
    p = Promise.reject(e);
  }
  if (sig && !sig.aborted) {
    p = p.then((v) => (sig.aborted ? Promise.reject(__fsAbortErr(sig.reason)) : v));
  }
  __nodeify(p, cb);
};
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
export const truncate = __fdCb(truncateSync, "truncate");
export const utimes = __fdCb(utimesSync, "utimes");
export const lutimes = __fdCb(lutimesSync, "lutimes");
export const chmod = __cb1(chmodSync, "chmod", __id);
export const link = __cb1(linkSync, "link", __id);
export const symlink = __cb1(symlinkSync, "symlink", __id);
export const readlink = __cb1(readlinkSync, "readlink", __id);
export const opendir = __cb1(opendirSync, "opendir", __id);
export const chown = __cb1(chownSync, "chown", __id);
// f 系回调包装：node 口径 fd/mode 等实参校验先于 cb（fchmod(1,'123x') →
// ARG_VALUE 而非 cb 错误），cb 缺省仍抛 callback 类型错。
export const fchown = __fdCb(fchownSync, "fchown");
export const lchown = __fdCb(lchownSync, "lchown");
export const lchmod = lchmodSync ? __fdCb(lchmodSync, "lchmod") : undefined;
// node 内部门（lib/fs.js 导出；utimes/timestamp-parsing 套件直用）。
export const _toUnixTimestamp = __toUnixTimestamp;
export const fchmod = __fdCb(fchmodSync, "fchmod");
export const fstat = __fdCb(fstatSync, "fstat");
export const ftruncate = __fdCb(ftruncateSync, "ftruncate");
export const fsync = __fdCb(fsyncSync, "fsync");
export const fdatasync = __fdCb(fdatasyncSync, "fdatasync");
export const futimes = __fdCb(futimesSync, "futimes");
// mkdtempDisposable（10f，node 26 口径）：{ path, remove, [Symbol.dispose /
// asyncDispose] }。remove 锁创建期绝对路径（"Stash the full path in case of
// process.chdir()"）；promises 版 remove 为 async（assert.rejects 契约）。
function __pathResolve(p) {
  if (typeof p === "string" && p.startsWith("/")) return p;
  const cwd = process.cwd();
  return cwd.endsWith("/") ? cwd + p : cwd + "/" + p;
}
export function mkdtempDisposableSync(prefix, opts) {
  const p = mkdtempSync(prefix, opts);
  const fullPath = __pathResolve(p);
  return {
    path: p,
    remove() { rmSync(fullPath, { recursive: true, force: true }); },
    [Symbol.dispose]() { this.remove(); },
    async [Symbol.asyncDispose]() { this.remove(); },
  };
}
async function mkdtempDisposableProm(prefix, opts) {
  const p = mkdtempSync(prefix, opts);
  const fullPath = __pathResolve(p);
  return {
    path: p,
    async remove() {
      // node rimraf：缺失路径静默（幂等）；EACCES/EPERM 等真实错误必须透传
      //（只读父目录用例断言 rejects /EACCES|EPERM/）。
      try { rmSync(fullPath, { recursive: true, force: true }); }
      catch (e) { if (e && e.code === "ENOENT") return; throw e; }
    },
    async [Symbol.asyncDispose]() { await this.remove(); },
  };
}
export const cp = __cb1(cpSync, "cp", __id);
export const open = __cb1(openSync, "open", __id);
export function close(fd, cb) {
  __vFd(fd);
  if (cb === undefined) cb = __nop;
  __vCbArg(cb);
  __nodeify(Promise.resolve().then(() => closeSync(fd)), cb);
}
function __nop() {}
export function exists(p, cb) {
  __vCbArg(cb);
  queueMicrotask(() => cb(existsSync(p)));
}
// promisify(fs.exists) → boolean（node：回调非 err-first，走 custom promisified）。
exists[Symbol.for("nodejs.util.promisify.custom")] = function (path) {
  // 内层引外层导出（不得命名内函数——遮蔽后自递归，promisified 套件现形）。
  return new Promise((resolve) => exists(path, resolve));
};
// 回调 read/write 全形态（10f，node lib/fs.js read()/write() 同构）：
// read(fd, cb) / read(fd, params, cb) / read(fd, buffer, options, cb) /
// read(fd, buffer, offset, length, position, cb)；write 同族 + 字符串形态。
export function read(fd, buffer, offsetOrOptions, length, position, callback) {
  __vFd(fd);
  let cb = callback;
  let offset = offsetOrOptions;
  let params = null;
  if (arguments.length <= 4) {
    if (arguments.length === 4) {
      // fs.read(fd, buffer, options, cb)
      cb = length;
      params = offsetOrOptions;
    } else if (arguments.length === 3) {
      // fs.read(fd, bufferOrParams, cb)
      if (!ArrayBuffer.isView(buffer)) {
        // node 同构：({ buffer = Buffer.alloc(16384) } = params ?? {})——
        // params.buffer 为 null 时 buffer 恒 null（不得落默认；read 套件点名）。
        params = buffer;
        ({ buffer = Buffer.alloc(16384) } = params ?? {});
      }
      cb = offsetOrOptions;
    } else {
      // fs.read(fd, cb)
      cb = buffer;
      buffer = Buffer.alloc(16384);
    }
    // node 原文：buffer?.byteLength（params.buffer 为 null 时由 validateBuffer 报）。
    ({ offset = 0, length = buffer?.byteLength - offset, position = null } = params ?? {});
  }
  if (typeof cb !== "function") {
    const e = new TypeError(`The "cb" argument must be of type function. Received ${cb === null ? "null" : typeof cb}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  __vBuffer(buffer);
  if (offset == null) offset = 0;
  else __vInteger(offset, "offset", 0);
  length |= 0;
  if (position == null) position = -1;
  if (length === 0) { queueMicrotask(() => cb(null, 0, buffer)); return; }
  __vEmptyBuffer(buffer);
  __vOffsetLength(offset, length, buffer.byteLength);
  Promise.resolve().then(() => readSync(fd, buffer, offset, length, position))
    .then(
      (n) => queueMicrotask(() => cb(null, n || 0, buffer)),
      (e) => queueMicrotask(() => cb(e)),
    );
}
// util.promisify(fs.read) → { bytesRead, buffer }（test-fs-promisified 点名）。
read[Symbol.for("nodejs.util.promisify.customArgs")] = ["bytesRead", "buffer"];

export function write(fd, buffer, offsetOrOptions, length, position, callback) {
  __vFd(fd);
  let offset = offsetOrOptions;
  if (ArrayBuffer.isView(buffer)) {
    callback ||= position || length || offset;
    if (typeof callback !== "function") {
      const e = new TypeError(`The "cb" argument must be of type function. Received ${callback === null ? "null" : typeof callback}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    let cb = callback;
    if (typeof offset === "object" && offset !== null) {
      // fs.write(fd, buffer, options, cb)
      ({ offset = 0, length = buffer.byteLength - offset, position = null } = offsetOrOptions ?? {});
    }
    if (offset == null || typeof offset === "function") offset = 0;
    else __vInteger(offset, "offset", 0);
    if (typeof length !== "number") length = buffer.byteLength - offset;
    if (typeof position !== "number") position = null;
    __fsValidateOffsetLengthWrite(offset, length, buffer.byteLength);
    Promise.resolve().then(() => writeSync(fd, buffer, offset, length, position))
      .then(
        (n) => queueMicrotask(() => cb(null, n || 0, buffer)),
        (e) => queueMicrotask(() => cb(e)),
      );
    return;
  }
  // node：非 view 非串（含 {} / Date / Promise / function / primitive）一律
  // validateBuffer ARG_TYPE（write-optional-params 'first argument not wrongly
  // interpreted' 族；不得走字符串分支静默成功）。
  if (typeof buffer !== "string") {
    __vBuffer(buffer);
  }
  // 字符串形态：(fd, str, cb) / (fd, str, position, cb) / (fd, str, position, encoding, cb)
  if (typeof position !== "function") {
    if (typeof offset === "function") { position = offset; offset = null; }
    else position = length;
    length = "utf8";
  }
  const cb = position;
  if (typeof cb !== "function") {
    const e = new TypeError(`The "cb" argument must be of type function. Received ${cb === null ? "null" : typeof cb}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  const pos = typeof offset === "number" ? offset : -1;
  Promise.resolve().then(() => writeSync(fd, buffer, pos))
    .then(
      (n) => queueMicrotask(() => cb(null, n || 0, buffer)),
      (e) => queueMicrotask(() => cb(e)),
    );
}
write[Symbol.for("nodejs.util.promisify.customArgs")] = ["bytesWritten", "buffer"];

// readv/writev（10f：JS 顺序合成，非原子——测试可见面 {bytesRead, buffers} 同构；
// 校验同步先抛——node getValidatedFd/validateBufferArray/validateFunction 同序）。
export function readvSync(fd, buffers, position) {
  __vFd(fd);
  __fsValidateBufferArray(buffers);
  let total = 0;
  for (const b of buffers) {
    const n = readSync(fd, b, 0, b.byteLength, typeof position === "bigint" ? Number(position) + total : (typeof position === "number" ? position + total : null));
    total += n;
    if (n < b.byteLength) break;
  }
  return total;
}
export function writevSync(fd, buffers, position) {
  __vFd(fd);
  __fsValidateBufferArray(buffers);
  let total = 0;
  for (const b of buffers) {
    const n = writeSync(fd, b, 0, b.byteLength, typeof position === "bigint" ? Number(position) + total : (typeof position === "number" ? position + total : null));
    total += n;
    if (n < b.byteLength) break;
  }
  return total;
}
export function readv(fd, buffers, position, cb) {
  if (typeof position === "function") { cb = position; position = null; }
  __vFd(fd);
  __fsValidateBufferArray(buffers);
  if (typeof cb !== "function") __vErrType("cb", "function", cb);
  Promise.resolve().then(() => readvSync(fd, buffers, position))
    .then(
      (n) => queueMicrotask(() => cb(null, n || 0, buffers)),
      (e) => queueMicrotask(() => cb(e)),
    );
}
readv[Symbol.for("nodejs.util.promisify.customArgs")] = ["bytesRead", "buffers"];
export function writev(fd, buffers, position, cb) {
  if (typeof position === "function") { cb = position; position = null; }
  __vFd(fd);
  __fsValidateBufferArray(buffers);
  if (typeof cb !== "function") __vErrType("cb", "function", cb);
  Promise.resolve().then(() => writevSync(fd, buffers, position))
    .then(
      (n) => queueMicrotask(() => cb(null, n || 0, buffers)),
      (e) => queueMicrotask(() => cb(e)),
    );
}
writev[Symbol.for("nodejs.util.promisify.customArgs")] = ["bytesWritten", "buffers"];

const __api = {
  // 同步（Phase 4 基础面）
  readFileSync, writeFileSync, appendFileSync, statSync, lstatSync, existsSync,
  mkdirSync, rmSync, rmdirSync, unlinkSync, readdirSync, renameSync, copyFileSync,
  realpathSync, mkdtempSync, watch, watchFile, unwatchFile, constants, createReadStream, createWriteStream, ReadStream, WriteStream,
  // 同步（Phase 9c 增补）
  accessSync, truncateSync, utimesSync, chmodSync, chownSync, fchownSync, linkSync, symlinkSync, readlinkSync,
  cpSync, opendirSync, openSync, closeSync, readSync, writeSync, ftruncateSync,
  fstatSync, fchmodSync, futimesSync, fsyncSync, fdatasyncSync, statfsSync,
  readvSync, writevSync,
  // 回调面（Phase 9c）
  readFile, writeFile, appendFile, stat, statfs, lstat, exists, mkdir, rmdir, rm, unlink,
  readdir, rename, copyFile, realpath, mkdtemp, access, truncate, utimes, chmod,
  link, symlink, readlink, open, close, read, write, readv, writev, chown, fchown, fchmod, fstat, ftruncate, fsync, fdatasync, futimes, opendir, cp,
  lchown, lchownSync, _toUnixTimestamp, lutimes, lutimesSync,
  ...(lchmod ? { lchmod, lchmodSync } : {}),
  mkdtempDisposable: mkdtempDisposableSync,
  // node 26 两条名都在（mkdtempDisposableSync 套件 `require('fs')` 点名）。
  mkdtempDisposableSync,
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
