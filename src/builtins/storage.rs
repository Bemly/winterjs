//! WinterCG 侧存储：全局 `storage` async KV + `localStorage` 同步垫片的 Rust 底座。
//!
//! 底座复用 `turso 0.6.1`（与 `bun:sqlite`/`node:sqlite` 同库，零新依赖）：
//! 单文件单表 `wjs_kv(k TEXT PK, v TEXT NOT NULL)`，值是 JS 侧编好的 JSON 文本
//!（含 `Uint8Array` 的 `{"$blob": b64}` 标记，与 sqlite 同桥），Rust 侧不解析值语义。
//!
//! 线程模型照 `bun:sqlite`：每 open 一条专用 worker 线程（自持 current-thread
//! tokio runtime），native 在 JS 线程经 crossbeam channel 阻塞往返。`storage.*`
//! 的 async 是 Web 形状（Promise 包装），阻塞体与 sqlite 同量级（本地文件 ms 内）；
//! `localStorage` 按 WHATWG 本就是同步语义，阻塞是合规实现（非 4.112 违规——
//! 4.112 禁的是"等回包"类网络投递路径走阻塞，本模块是本地文件短 IO）。
//!
//! 偏差记档：`localStorage` 无 5MB 上限（随文件走）；未 close 的库活到会话结束
//!（`init_session` 清表回收）；值仅支持 JSON 可序列化 + `Uint8Array`，`Blob/File`
//! 等传 `set` 即 `TypeError`（JS 层先拦，宁严勿默）。

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};

/// 默认库文件名（cwd 锚定，项目级隔离）。CLI `--storage-path` 覆盖，未设即此值。
pub const DEFAULT_FILE: &str = "winterjs2-storage.db";

/// 键规则（JS 层同判，native 侧再拦一道）：非空字符串，≤1024 字符。
pub const MAX_KEY_LEN: usize = 1024;

static DEFAULT_PATH: RwLock<Option<String>> = RwLock::new(None);

/// CLI 层安装默认库路径（`main` 在 `runtime::run` 之前调一次；进程级）。
pub fn set_default_path(p: Option<String>) {
    if let Ok(mut slot) = DEFAULT_PATH.write() {
        *slot = p;
    }
}

/// 当前默认库路径（未安装/未给即 `DEFAULT_FILE`）。
pub fn default_path() -> String {
    DEFAULT_PATH
        .read()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_else(|| DEFAULT_FILE.to_string())
}

/// 键校验（纯函数，单测覆盖；JS 层同规则先拦，native 侧兜底）。
pub fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("key must be a non-empty string".to_string());
    }
    if key.chars().count() > MAX_KEY_LEN {
        return Err(format!("key is too long (max {MAX_KEY_LEN} chars)"));
    }
    Ok(())
}

/// JS → worker 的一条操作（全 Send；值已是 JSON 文本，Rust 侧不解析）。
pub enum StorageOp {
    Get(String),
    Set(String, String),
    Delete(String),
    Keys(String),
    Clear,
    Close,
}

/// native 侧持住的 worker 端点（req/resp 均可 Clone；ops 严格顺序单飞）。
#[derive(Clone)]
pub struct StorageWorker {
    pub req_tx: crossbeam_channel::Sender<StorageOp>,
    pub resp_rx: crossbeam_channel::Receiver<Result<serde_json::Value, String>>,
}

/// worker 注册表（模块自有静态表，不进 `state::PlainState`——后者 999 行已顶
/// §0.9 上限；语义与 sqlite 表同：id 单调不复用，会话收尾 `reset_session` 全摘，
/// 线程在 channel 断开后自退；进程级共享，同项目多会话见同一库文件）。
static WORKERS: LazyLock<RwLock<HashMap<u64, StorageWorker>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 登记 worker 并分配 id。
pub fn storage_add(worker: StorageWorker) -> u64 {
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    if let Ok(mut t) = WORKERS.write() {
        t.insert(id, worker);
    }
    id
}

/// 取 worker 端点（Clone 出用，不持锁做阻塞 IO）。
pub fn storage_worker(id: u64) -> Option<StorageWorker> {
    WORKERS.read().ok().and_then(|t| t.get(&id).cloned())
}

/// 摘除 worker（close 调用；drop 掉的 req_tx 让线程自退）。
pub fn storage_remove(id: u64) {
    if let Ok(mut t) = WORKERS.write() {
        t.remove(&id);
    }
}

/// 会话重置（`init_session` 调用；见模块注释）。
pub fn reset_session() {
    if let Ok(mut t) = WORKERS.write() {
        t.clear();
    }
}

fn err_string(e: turso::Error) -> String {
    e.to_string()
}

/// 发一条操作并阻塞取回结果（JS 线程同步语义的落点，与 sqlite 同哲学）。
fn roundtrip(id: u64, op: StorageOp) -> Result<serde_json::Value, String> {
    let StorageWorker { req_tx: tx, resp_rx: rx } =
        storage_worker(id).ok_or_else(|| "storage is not open".to_string())?;
    tx.send(op).map_err(|_| "storage worker died".to_string())?;
    rx.recv().map_err(|_| "storage worker died".to_string())?
}

fn open_worker(path: String) -> Result<StorageWorker, String> {
    let (req_tx, req_rx) = crossbeam_channel::unbounded::<StorageOp>();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded::<Result<serde_json::Value, String>>();
    let spawned = std::thread::Builder::new()
        .name("winterjs2-storage".into())
        .spawn(move || worker_main(path, req_rx, resp_tx));
    spawned.map_err(|e| format!("failed to spawn storage worker: {e}"))?;
    match resp_rx.recv() {
        Ok(Ok(_)) => Ok(StorageWorker { req_tx, resp_rx }),
        Ok(Err(msg)) => Err(msg),
        Err(_) => Err("storage worker died".to_string()),
    }
}

fn worker_main(
    path: String,
    req_rx: crossbeam_channel::Receiver<StorageOp>,
    resp: crossbeam_channel::Sender<Result<serde_json::Value, String>>,
) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = resp.send(Err(format!("failed to start storage runtime: {e}")));
            return;
        }
    };
    let db = match rt.block_on(turso::Builder::new_local(&path).build()) {
        Ok(db) => db,
        Err(e) => {
            let _ = resp.send(Err(err_string(e)));
            return;
        }
    };
    let conn = match db.connect() {
        Ok(conn) => conn,
        Err(e) => {
            let _ = resp.send(Err(err_string(e)));
            return;
        }
    };
    if let Err(e) = rt.block_on(conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS wjs_kv(k TEXT PRIMARY KEY, v TEXT NOT NULL)",
    )) {
        let _ = resp.send(Err(err_string(e)));
        return;
    }
    let _ = resp.send(Ok(serde_json::json!({ "open": path })));
    for op in req_rx.iter() {
        match op {
            StorageOp::Close => {
                let _ = resp.send(Ok(serde_json::json!({})));
                break; // conn/db 就地 drop，线程退出
            }
            other => {
                let _ = resp.send(run_op(&rt, &conn, other));
            }
        }
    }
}

/// 执行一条操作（worker 线程内；block_on 到自持 runtime）。
fn run_op(
    rt: &tokio::runtime::Runtime,
    conn: &turso::Connection,
    op: StorageOp,
) -> Result<serde_json::Value, String> {
    rt.block_on(async {
        match op {
            StorageOp::Close => Ok(serde_json::json!({})),
            StorageOp::Get(key) => {
                let mut rows = conn
                    .query(
                        "SELECT v FROM wjs_kv WHERE k = ?",
                        turso::params::Params::Positional(vec![turso::Value::Text(key)]),
                    )
                    .await
                    .map_err(err_string)?;
                match rows.next().await.map_err(err_string)? {
                    None => Ok(serde_json::Value::Null),
                    Some(row) => match row.get_value(0).map_err(err_string)? {
                        turso::Value::Text(s) => {
                            serde_json::from_str(&s).map_err(|e| format!("bad stored value: {e}"))
                        }
                        _ => Err("bad stored value: expected TEXT".to_string()),
                    },
                }
            }
            StorageOp::Set(key, json) => {
                // 先验值是合法 JSON（坏值不落地）。
                serde_json::from_str::<serde_json::Value>(&json)
                    .map_err(|e| format!("bad storage value: {e}"))?;
                conn.execute(
                    "INSERT INTO wjs_kv(k, v) VALUES(?, ?) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
                    turso::params::Params::Positional(vec![
                        turso::Value::Text(key),
                        turso::Value::Text(json),
                    ]),
                )
                .await
                .map_err(err_string)?;
                Ok(serde_json::json!({ "ok": true }))
            }
            StorageOp::Delete(key) => {
                let n = conn
                    .execute(
                        "DELETE FROM wjs_kv WHERE k = ?",
                        turso::params::Params::Positional(vec![turso::Value::Text(key)]),
                    )
                    .await
                    .map_err(err_string)?;
                Ok(serde_json::json!({ "deleted": n > 0 }))
            }
            StorageOp::Keys(prefix) => {
                let mut rows = conn
                    .query("SELECT k FROM wjs_kv ORDER BY k", turso::params::Params::Positional(vec![]))
                    .await
                    .map_err(err_string)?;
                let mut out = Vec::new();
                while let Some(row) = rows.next().await.map_err(err_string)? {
                    match row.get_value(0).map_err(err_string)? {
                        turso::Value::Text(k) => {
                            if k.starts_with(&prefix) {
                                out.push(serde_json::Value::String(k));
                            }
                        }
                        _ => return Err("bad stored key: expected TEXT".to_string()),
                    }
                }
                Ok(serde_json::Value::Array(out))
            }
            StorageOp::Clear => {
                let n = conn
                    .execute("DELETE FROM wjs_kv", turso::params::Params::Positional(vec![]))
                    .await
                    .map_err(err_string)?;
                Ok(serde_json::json!({ "cleared": n }))
            }
        }
    })
}

// ── natives（全同步；参数校验失败 report_error + false）──────────────────────
// UNSAFE-BOUNDARY：全部 JSNative 入口经 `wrap_cx` + `Frame::from_raw`（结构性边界块，
// 随 native 数线性增长）；前置：调用方 realm 内 + 参数槽 rooted 后才分配；
// 覆盖：`tests/storage.rs`（正常/报错/边界）+ 黑盒 panic 路径用例。

fn arg_string(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    Some(value_to_string(cx, frame.arg(i)))
}

fn arg_id(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<u64> {
    if frame.argc() <= i || !frame.arg(i).is_number() {
        report_error(cx, &format!("TypeError: {what} requires a database id"));
        return None;
    }
    Some(frame.arg(i).to_number() as u64)
}

fn arg_key(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    let key = arg_string(cx, frame, i, what)?;
    if let Err(msg) = validate_key(&key) {
        report_error(cx, &format!("TypeError: {msg}"));
        return None;
    }
    Some(key)
}

/// serde Value → JS 值（JSON.parse 语义；前置 realm 内）。
fn json_string(cx: &mut JSContext, v: &serde_json::Value) -> JSVal {
    rooted!(&in(cx) let mut out = UndefinedValue());
    let text = v.to_string();
    text.to_jsval(cx, out.handle_mut());
    out.get()
}

/// `__wjs2_storage_default_path()` → 默认库路径（CLI `--storage-path` 未给即 `winterjs2-storage.db`）。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/storage.rs::storage_default_path_and_isolation`。
pub unsafe extern "C" fn storage_default_path(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let path = default_path();
    tracing::debug!(target: "winterjs2::storage", path_len = path.len(), "default path");
    frame.set_rval({
        rooted!(&in(cx) let mut out = UndefinedValue());
        path.to_jsval(&mut cx, out.handle_mut());
        out.get()
    });
    true
}

/// `__wjs2_storage_open(path)` → id。阻塞到 worker open 握手完成（含建表）。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/storage.rs::storage_crud_and_persist`。
pub unsafe extern "C" fn storage_open(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_string(&mut cx, &frame, 0, "storage open") else {
        return false;
    };
    if path.is_empty() {
        report_error(&mut cx, "TypeError: storage open requires a non-empty path");
        return false;
    }
    if let Err(msg) = crate::permissions::check_read(&path) {
        report_error(&mut cx, &msg);
        return false;
    }
    if let Err(msg) = crate::permissions::check_write(&path) {
        report_error(&mut cx, &msg);
        return false;
    }
    tracing::debug!(target: "winterjs2::storage", path_len = path.len(), "open");
    match open_worker(path) {
        Ok(worker) => {
            let id = storage_add(worker);
            frame.set_rval(mozjs::jsval::Int32Value(id as i32));
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("StorageError: {msg}"));
            false
        }
    }
}

/// `__wjs2_storage_get(id, key)` → 值 JSON 串（缺失即 `"null"`）。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/storage.rs::storage_crud_and_persist`。
pub unsafe extern "C" fn storage_get(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(id), Some(key)) = (
        arg_id(&mut cx, &frame, 0, "storage get"),
        arg_key(&mut cx, &frame, 1, "storage get"),
    ) else {
        return false;
    };
    match roundtrip(id, StorageOp::Get(key)) {
        Ok(v) => {
            frame.set_rval(json_string(&mut cx, &v));
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("StorageError: {msg}"));
            false
        }
    }
}

/// `__wjs2_storage_set(id, key, valueJson)`。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/storage.rs::storage_crud_and_persist`。
pub unsafe extern "C" fn storage_set(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(id), Some(key), Some(json)) = (
        arg_id(&mut cx, &frame, 0, "storage set"),
        arg_key(&mut cx, &frame, 1, "storage set"),
        arg_string(&mut cx, &frame, 2, "storage set"),
    ) else {
        return false;
    };
    tracing::debug!(
        target: "winterjs2::storage",
        key_len = key.len(),
        json_len = json.len(),
        "set"
    );
    match roundtrip(id, StorageOp::Set(key, json)) {
        Ok(_) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("StorageError: {msg}"));
            false
        }
    }
}

/// `__wjs2_storage_delete(id, key)` → `"true"`/`"false"`（是否删到）。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/storage.rs::storage_crud_and_persist`。
pub unsafe extern "C" fn storage_delete(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(id), Some(key)) = (
        arg_id(&mut cx, &frame, 0, "storage delete"),
        arg_key(&mut cx, &frame, 1, "storage delete"),
    ) else {
        return false;
    };
    match roundtrip(id, StorageOp::Delete(key)) {
        Ok(v) => {
            let hit = v.get("deleted").and_then(|b| b.as_bool()).unwrap_or(false);
            frame.set_rval(mozjs::jsval::BooleanValue(hit));
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("StorageError: {msg}"));
            false
        }
    }
}

/// `__wjs2_storage_keys(id, prefix)` → 键数组 JSON 串（有序）。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/storage.rs::storage_keys_prefix_and_clear`。
pub unsafe extern "C" fn storage_keys(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(id), Some(prefix)) = (
        arg_id(&mut cx, &frame, 0, "storage keys"),
        arg_string(&mut cx, &frame, 1, "storage keys"),
    ) else {
        return false;
    };
    match roundtrip(id, StorageOp::Keys(prefix)) {
        Ok(v) => {
            frame.set_rval(json_string(&mut cx, &v));
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("StorageError: {msg}"));
            false
        }
    }
}

/// `__wjs2_storage_clear(id)` → 清掉的条数。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/storage.rs::storage_keys_prefix_and_clear`。
pub unsafe extern "C" fn storage_clear(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&mut cx, &frame, 0, "storage clear") else {
        return false;
    };
    match roundtrip(id, StorageOp::Clear) {
        Ok(v) => {
            let n = v.get("cleared").and_then(|b| b.as_u64()).unwrap_or(0);
            frame.set_rval(mozjs::jsval::Int32Value(n as i32));
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("StorageError: {msg}"));
            false
        }
    }
}

/// `__wjs2_storage_close(id)`：worker 收尾退出；state 表摘除（幂等）。
/// UNSAFE-BOUNDARY：见本文件 natives 头注；覆盖 `tests/storage.rs::storage_errors_boundary`。
pub unsafe extern "C" fn storage_close(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&mut cx, &frame, 0, "storage close") else {
        return false;
    };
    if storage_worker(id).is_none() {
        frame.set_rval(UndefinedValue());
        return true;
    }
    match roundtrip(id, StorageOp::Close) {
        Ok(_) => {
            storage_remove(id);
            frame.set_rval(UndefinedValue());
            true
        }
        Err(_) => {
            storage_remove(id);
            frame.set_rval(UndefinedValue());
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_rules() {
        assert!(validate_key("a").is_ok());
        assert!(validate_key("user:1").is_ok());
        assert!(validate_key("").is_err());
        assert!(validate_key(&"k".repeat(MAX_KEY_LEN)).is_ok());
        assert!(validate_key(&"k".repeat(MAX_KEY_LEN + 1)).is_err());
    }

    #[test]
    fn default_path_fallback_and_set() {
        // 读全局态先复位（§4.41），复位回缺省。
        set_default_path(None);
        assert_eq!(default_path(), DEFAULT_FILE);
        set_default_path(Some("x.db".to_string()));
        assert_eq!(default_path(), "x.db");
        set_default_path(None);
    }
}
