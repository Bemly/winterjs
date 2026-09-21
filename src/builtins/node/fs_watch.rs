//! fs watch 域（notify/防抖/dispatch/glob/流续命；对齐 fs.rs；纯搬移）。

use mozjs::jsapi::JSObject;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use mozjs::conversions::ToJSValConvertible as _;
use crate::state;
use super::fs::{arg_path_checked, PermClass};
use mozjs::context::JSContext;

// ── fs.watch（`notify` 线程 → 防抖线程 → 事件循环；300ms 静默窗，与 test --watch 同值）
// 防抖说明：`notify-debouncer-mini`（批准单内轮子）只给 `Any`/`AnyContinuous`
//（kind 丢失），`fs.watch` 的 `rename`/`change` 区分会被吃掉——此处用共享防抖线程
// 手写 coalesce（按 `(id, kind, file)` 键 300ms 静默窗，kind 原样保留；失败事件直通）。
// 线程模型与既有 `notify` 线程一致（Rust 侧多线程只经 channel 与 JS 线程通信，§6 合规）。

/// 防抖窗（与 `testrun.rs` 的 300ms 同值，用户感知一致）。
pub(crate) const WATCH_DEBOUNCE_MS: u64 = 300;

/// 防抖线程输入（生 kind 纯数据；终分类在分发侧做——notify 回调非 JS 线程，
/// TLS state 不可用，见 §4.154）。
struct RawWatch {
    id: u64,
    kind: WatchKind,
}

type DebounceKey = (u64, String, String);

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
    // 前沿触发 + 同键抑制窗（node 无静默窗）：首事件立即刷（新文件 Create+
    // Modify 双事件时 rename 先到先赢，§4.27 诉求保留）；抑制窗内同键丢弃，
    // 窗后首事件再即刷。旧静默窗（到期才刷）在持续写下永不到，1ms 写循环
    // 套件饿死（test-fs-watch.js/encoding/promises-watch 现形）。
    // 事件量极小（人手/测试级），O(n) 扫描可接受。
    let mut suppressed: Vec<(DebounceKey, Instant)> = Vec::new();
    loop {
        match rx.recv() {
            Ok(raw) => {
                let id = raw.id;
                match raw.kind {
                    // 失败直通（不抑制，尽早报错）
                    WatchKind::Failed(msg) => {
                        if js_tx.send(WatchEvent { id, kind: WatchKind::Failed(msg) }).is_err() {
                            return;
                        }
                    }
                    WatchKind::Fired { raw, file, full } => {
                        let now = Instant::now();
                        suppressed.retain(|(_, until)| *until > now);
                        // 抑制键走生 kind + 全路径（终分类在分发侧，同键抑制不丢语义）。
                        let key = (id, raw.clone(), full.clone());
                        if suppressed.iter().any(|(k, _)| *k == key) {
                            continue;
                        }
                        suppressed.push((key, now + window));
                        if js_tx
                            .send(WatchEvent { id, kind: WatchKind::Fired { raw, file, full } })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            Err(std::sync::mpsc::RecvError) => return,
        }
    }
}

/// notify 线程 → 事件循环（纯数据；`kind` 为 `rename`/`change`）。
pub struct WatchEvent {
    pub id: u64,
    pub kind: WatchKind,
}

pub enum WatchKind {
    /// 生 kind（"create"/"remove"/"modify"）+ 展示名 + 全路径键；终分类
    /// （rename/change）在分发侧（JS 线程，TLS 可用）做。
    Fired { raw: String, file: Option<String>, full: String },
    Failed(String),
}

/// node 口径：递归 watch 的 filename 含相对路径（ignore-recursive 套件
// `endsWith/includes` 断言）；单文件/目录回落 basename。
// 根形态不定（相对/绝对/经 symlink）而 notify 事件恒绝对——规范根 + 绝对原根
// 双试 strip（§4.12 同源：/var↔/private/var），皆失才回落 basename。
pub(crate) fn watch_display_name(canon_root: &std::path::Path, raw_root: &std::path::Path, p: &std::path::Path) -> Option<String> {
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

/// Create 二判据之起点种子：watch 起始已存在文件（相对根列出），重写首事件
/// 即 change。符号链接不跟（防环）；漏网（竞态新建）由 birthtime 规则兜。
fn seed_rels(dir: &std::path::Path, out: &mut Vec<String>, recursive: bool) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(|e| e.ok()) {
        let Ok(ft) = e.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if ft.is_file() {
            out.push(name);
        } else if recursive && ft.is_dir() {
            let mut sub = Vec::new();
            seed_rels(&e.path(), &mut sub, true);
            for s in sub {
                out.push(format!("{name}/{s}"));
            }
        }
    }
}
/// 新生文件判定（Create 首见二判据之下半）：birthtime 距此刻 ≤10s 即新生→
/// rename，否则旧文件重写 artifact→change。取不到 birthtime（fs 不支持/
 /// 路径已失）回 rename（旧行为）。重写的新生文件由 seen 表兜（创建 Create
/// 已标记），阈值只裁"慢投递的新生"极端。
pub(crate) fn is_fresh_birth(p: &std::path::Path) -> bool {
    const FRESH_SECS: u64 = 10;
    let Ok(md) = std::fs::metadata(p) else {
        return true;
    };
    let Ok(born) = md.created() else {
        return true;
    };
    std::time::SystemTime::now()
        .duration_since(born)
        .is_ok_and(|d| d.as_secs() <= FRESH_SECS)
}
/// minimatch 近似（fs.watch `ignore` 字符串面）：`**` 递归 + 无斜杠模式配
/// basename（matchBase）+ win/mac 不分大小写；模式非法回字面相等。
pub(crate) fn glob_match_impl(pat: &str, name: &str, base: &str, nocase: bool) -> bool {
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
    // Create 二判据之起点种子：起始已存在文件全标记（双根全路径形，与事件
    // full 键同形）；之后新建/删除走 mark/forget。
    {
        let root = std::path::Path::new(&watched);
        if root.is_file() {
            state::watch_seen_mark(id, &canon_root);
            state::watch_seen_mark(id, &abs_root);
        } else if root.is_dir() {
            let mut rels = Vec::new();
            seed_rels(root, &mut rels, recursive);
            for r in rels {
                let rel = std::path::Path::new(&r);
                let a = std::path::Path::new(&canon_root).join(rel).to_string_lossy().into_owned();
                let b = std::path::Path::new(&abs_root).join(rel).to_string_lossy().into_owned();
                state::watch_seen_mark(id, &a);
                state::watch_seen_mark(id, &b);
            }
        }
    }
    let build: Result<notify::RecommendedWatcher, String> = (|| {
        use notify::Watcher as _;
        // notify 回调只做分类（纯数据），派发由防抖线程做（前沿即刷 + 同键
        // 抑制窗，kind 保留）。
        let mut watcher =
            notify::RecommendedWatcher::new(move |res: Result<notify::Event, notify::Error>| {
                // 纯数据搬运（禁 TLS state：本回调不在 JS 线程，见 §4.154）。
                match res {
                    Ok(ev) => {
                        let raw: Option<&str> = match &ev.kind {
                            notify::EventKind::Create(_) => Some("create"),
                            notify::EventKind::Remove(_) => Some("remove"),
                            notify::EventKind::Modify(_) => Some("modify"),
                            _ => None,
                        };
                        let Some(raw) = raw else {
                            return;
                        };
                        // 全路径键（seen 表跨相对/绝对稳定）与展示名（相对路径优先）。
                        let full = ev.paths.first().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                        let file = ev.paths.first().and_then(|p| {
                            watch_display_name(
                                std::path::Path::new(&rel_canon),
                                std::path::Path::new(&rel_abs),
                                std::path::Path::new(p),
                            )
                        });
                        let _ = dtx.send(RawWatch {
                            id,
                            kind: WatchKind::Fired { raw: raw.to_string(), file, full },
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

/// `__wjs_fs_stream_ref()`（fs 流续命 +1；构造期调用）。
///
/// UNSAFE-BOUNDARY(fs_stream_ref)：前置——realm 内同步 native 调用（引擎回调
/// 上下文）；无 JS 值出入、无 GC 触点（纯 Rust 计数器），不可 panic（usize 加法、
/// 进程级流数恒远小于上限）；覆盖：tests/node/fs.rs phase10f_fs_stream_lifetime。
pub unsafe extern "C" fn fs_stream_ref(
    cx_raw: *mut mozjs::jsapi::JSContext,
    _argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；仅调 wrap_cx + Frame::from_raw（入口
    // 固定两边界块），其后纯 Rust 计数，无堆/GC 触点。
    let _cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, _argc) };
    state::fs_stream_ref();
    frame.set_rval(UndefinedValue());
    true
}

/// `__wjs_fs_stream_unref()`（fs 流摘除 -1，饱和减；close/终结期调用）。
///
/// UNSAFE-BOUNDARY(fs_stream_unref)：前置同上；saturating_sub 不可 panic；
/// 覆盖：同上（double-close 路径断言计数归零进程退出）。
pub unsafe extern "C" fn fs_stream_unref(
    cx_raw: *mut mozjs::jsapi::JSContext,
    _argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上。
    let _cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, _argc) };
    state::fs_stream_unref();
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
        WatchKind::Fired { raw, file, full } => {
            let Some(listener) = state::watch_listener(ev.id) else {
                return Ok(());
            };
            // 终分类（Create→rename/change 二判据，recursive-watch-file 套件：
            // 截断重写在 macOS 报 Create artifact——见过即重写→change；首见看
            // birthtime（新生→rename，旧文件重写→change）；Remove 即 rename +
            // 遗忘（重建即新）；Modify 即 change）。此处 JS 线程，TLS 可用。
            let event: &str = match raw.as_str() {
                "remove" => {
                    state::watch_seen_forget(ev.id, &full);
                    "rename"
                }
                "modify" => {
                    state::watch_seen_mark(ev.id, &full);
                    "change"
                }
                _ => {
                    if state::watch_seen_has(ev.id, &full) {
                        "change"
                    } else {
                        state::watch_seen_mark(ev.id, &full);
                        let fresh = full.is_empty() || is_fresh_birth(std::path::Path::new(&full));
                        if fresh { "rename" } else { "change" }
                    }
                }
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
