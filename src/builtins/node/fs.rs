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
    serde_json::json!({
        "size": md.len(),
        "mtimeMs": ms(md.modified()),
        "atimeMs": ms(md.accessed()),
        "birthtimeMs": ms(md.created()),
        "isFile": md.is_file(),
        "isDirectory": md.is_dir(),
        "isSymlink": md.is_symlink(),
        "mode": mode,
    })
    .to_string()
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
    let mut pending: std::collections::HashMap<DebounceKey, (Instant, WatchEvent)> =
        std::collections::HashMap::new();
    loop {
        let now = Instant::now();
        // 到期即刷
        let due: Vec<DebounceKey> = pending
            .iter()
            .filter(|(_, (d, _))| *d <= now)
            .map(|(k, _)| k.clone())
            .collect();
        for k in due {
            if let Some((_, ev)) = pending.remove(&k) {
                if js_tx.send(ev).is_err() {
                    return;
                }
            }
        }
        let wait = pending
            .values()
            .map(|(d, _)| d.checked_duration_since(Instant::now()).unwrap_or(Duration::ZERO))
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
                        pending.insert(
                            key,
                            (Instant::now() + window, WatchEvent { id, kind: WatchKind::Fired { event, file } }),
                        );
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
  if (encoding === null) return bytes;
  return new TextDecoder(String(encoding)).decode(bytes);
}
class __Stats {
  constructor(j) {
    this.size = j.size;
    this.mtimeMs = j.mtimeMs;
    this.atimeMs = j.atimeMs;
    this.birthtimeMs = j.birthtimeMs;
    this.mode = j.mode;
    this.__f = j.isFile;
    this.__d = j.isDirectory;
    this.__l = j.isSymlink;
    this.mtime = new Date(j.mtimeMs);
    this.atime = new Date(j.atimeMs);
    this.birthtime = new Date(j.birthtimeMs);
  }
  isFile() { return this.__f; }
  isDirectory() { return this.__d; }
  isSymbolicLink() { return this.__l; }
  isBlockDevice() { return false; }
  isCharacterDevice() { return false; }
  isFIFO() { return false; }
  isSocket() { return false; }
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
export function readFileSync(p, opts) {
  p = __fsPath(p, "readFile");
  const enc = __fsEncoding(opts);
  return __fsCall("open", p, () => __fsDecode(__wjs_fs_read_file(p), enc, "readFile"));
}
export function writeFileSync(p, data, opts) {
  p = __fsPath(p, "writeFile");
  __fsCall("open", p, () => __wjs_fs_write_file(p, __fsData(data, "writeFile"), __fsMode(opts)));
}
export function appendFileSync(p, data, opts) {
  p = __fsPath(p, "appendFile");
  __fsCall("open", p, () => __wjs_fs_append_file(p, __fsData(data, "appendFile"), __fsMode(opts)));
}
export function statSync(p) {
  p = __fsPath(p, "stat");
  return new __Stats(JSON.parse(__fsCall("stat", p, () => __wjs_fs_stat(p, true))));
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
export function mkdtempSync(prefix) {
  return __fsCall("mkdir", String(prefix), () => __wjs_fs_mkdtemp(String(prefix)));
}
export const constants = {
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
const __api = { readFileSync, writeFileSync, appendFileSync, statSync, lstatSync, existsSync, mkdirSync, rmSync, rmdirSync, unlinkSync, readdirSync, renameSync, copyFileSync, realpathSync, mkdtempSync, watch, constants, createReadStream, createWriteStream };
export default __api;
"#;

/// 内嵌 ESM 源（`node:fs/promises`；同步底层 async 包裹，见头注）。
pub const PROMISES_SOURCE: &str = r#"
import { readFileSync, writeFileSync, appendFileSync, statSync, lstatSync, mkdirSync, rmSync, rmdirSync, unlinkSync, readdirSync, renameSync, copyFileSync, realpathSync, mkdtempSync, constants } from "node:fs";
export async function readFile(p, opts) { return readFileSync(p, opts); }
export async function writeFile(p, data, opts) { return writeFileSync(p, data, opts); }
export async function appendFile(p, data, opts) { return appendFileSync(p, data, opts); }
export async function stat(p) { return statSync(p); }
export async function lstat(p) { return lstatSync(p); }
export async function mkdir(p, opts) { return mkdirSync(p, opts); }
export async function rm(p, opts) { return rmSync(p, opts); }
export async function rmdir(p, opts) { return rmdirSync(p, opts); }
export async function unlink(p) { return unlinkSync(p); }
export async function readdir(p, opts) { return readdirSync(p, opts); }
export async function rename(a, b) { return renameSync(a, b); }
export async function copyFile(src, dst) { return copyFileSync(src, dst); }
export async function realpath(p) { return realpathSync(p); }
export async function mkdtemp(prefix) { return mkdtempSync(prefix); }
export { constants };
export default { readFile, writeFile, appendFile, stat, lstat, mkdir, rm, rmdir, unlink, readdir, rename, copyFile, realpath, mkdtemp, constants };
"#;
