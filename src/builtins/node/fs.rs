//! `node:fs` + `node:fs/promises`：同步核心（`fs-err` 增强报错，码经 `io_code`）。
//! 错误约定：native 报 `"{CODE}: {detail}"`，prelude 包装成带
//! `.code/.syscall/.path` 的 Error（Node 形状）；`fs/promises` 为同语义 async 包裹
//! （底层同步实现，文档记录；lint 脚本量级无感）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, view_bytes, wrap_cx, Frame};

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
            // ENAMETOOLONG：macOS 63 / Linux 36（超长文件名；cp filename-too-long 套件）。
            // Linux 63 系 ENOSR，不可合并且——cfg 分流。
            #[cfg(target_os = "macos")]
            63 => "ENAMETOOLONG",
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
pub(crate) enum PermClass {
    Read,
    Write,
}

/// 路径实参 + 权限检查（`--allow-*` 沙箱；拒绝即上报并返回 None）。
pub(crate) fn arg_path_checked(
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
pub(crate) fn arg_path(cx: &mut mozjs::context::JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
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
pub(crate) fn arg_bytes(
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
pub(crate) fn uv_msg(e: &std::io::Error) -> &'static str {
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

pub(crate) fn report_io(cx: &mut mozjs::context::JSContext, syscall: &str, path: &str, e: std::io::Error) {
    // node lib/internal/errors 形状：`CODE: <uv msg>, <syscall> '<path>'`。
    report_error(
        cx,
        &format!("{}: {}, {syscall} '{path}'", io_code(&e), uv_msg(&e)),
    );
}

/// Uint8Array 返回值（`crypto` 同款小 helper，不跨模块引）。
pub(crate) fn set_rval_bytes(cx: &mut mozjs::context::JSContext, frame: &Frame, out: &[u8]) -> bool {
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
pub(crate) fn set_rval_str(cx: &mut mozjs::context::JSContext, frame: &Frame, s: &str) {
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
pub(crate) fn stat_json(md: &std::fs::Metadata, path: &str) -> String {
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
    // std 直用保 raw errno（fs_err 包装丢 errno：超长文件名 ENAMETOOLONG 落
    // EINVAL/UNKNOWN，cp filename-too-long 套件点名；§4.121 同族）。
    let md = if follow { std::fs::metadata(&path) } else { std::fs::symlink_metadata(&path) };
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

pub use super::fs_fd::{
    fs_access, fs_chmod, fs_chown, fs_close, fs_fchmod, fs_fchown, fs_fstat, fs_fsync,
    fs_ftruncate, fs_futimes, fs_lchmod, fs_lchown, fs_link, fs_lutimes, fs_open,
    fs_read_fd, fs_read_link, fs_symlink, fs_truncate, fs_utimes, fs_write_fd,
};
pub use super::fs_watch::{
    WatchEvent, dispatch, fs_stream_ref, fs_stream_unref, glob_match, watch_close,
    watch_persistent, watch_start,
};


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
    use super::super::fs_watch::{glob_match_impl, is_fresh_birth, watch_display_name};

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
    fn fresh_birth_shapes() {
        // 新生文件（刚建）即 fresh；缺席路径回 true（旧行为 rename，不抛）。
        let dir = std::env::temp_dir().join("wjs-fresh-probe");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("new.txt");
        std::fs::write(&f, b"x").unwrap();
        assert!(is_fresh_birth(&f));
        assert!(is_fresh_birth(&dir.join("definitely-missing-xyz")));
        let _ = std::fs::remove_dir_all(&dir);
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

/// 内嵌 ESM 源（`node:fs`；§0.9 按域分块：`fs_base.js` 基础 + `fs_streams.js` 流 +
/// `fs_sync9c.js` 同步面增补 + `fs_fdsync.js` 校验/fd 同步 + `fs_async.js` promises/回调，concat 字节恒等；
/// 错误带 `.code/.syscall/.path`，偏差见头注）。
pub const SOURCE: &str = concat!(
    include_str!("fs_base.js"),
    include_str!("fs_streams.js"),
    include_str!("fs_sync9c.js"),
    include_str!("fs_fdsync.js"),
    include_str!("fs_async.js"),
);

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
export const watch = fs.promises.watch;
export default fs.promises;
"#;
