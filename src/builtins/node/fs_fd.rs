//! fs fd 域（fd 表/open/close/读写/f 系 utimes/chmod/chown/fsync；对齐 fs.rs；纯搬移）。

use mozjs::jsval::JSVal;
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use super::fs::{arg_bytes, arg_path, arg_path_checked, report_io, set_rval_bytes, set_rval_str, stat_json, PermClass};

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
