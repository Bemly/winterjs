//! `bun:sqlite`：Bun SQLite 兼容层（turso 0.6.1 之上，plan Phase 7-e4）。
//!
//! 同步语义的落法：Bun 的 API 全同步，turso 全 async —— 每个 Database 开一条
//! 专用 worker 线程（自带 current-thread tokio runtime），native 在 JS 线程经
//! crossbeam channel 阻塞往返（与 node:fs 同步底层同哲学；JS 线程独占不破坏，
//! worker 自包含不回调用 JS）。参数/行值经 JSON 桥，blob 用 `{"$blob": b64}`
//! 标记（JS 侧 revive 成 Uint8Array）。
//!
//! 偏差（文档记录）：整数超 2^53 有精度损失（同 Bun 默认 safeIntegers=false）；
//! non-finite 数字拒绝绑定（JSON 桥无法承载 Infinity；NaN 在 SQLite 本存 NULL，
//! 此处统一报错，宁严勿默）；`Database(options)` 的 readonly/create 忽略
//! （turso 0.6.1 Builder 无对应项）；未 close 的 Database 其 worker 线程活到
//! 进程/会话结束（`init_session` 清表回收上一会话的）；stmt.run 返回
//! `{changes, lastInsertRowid}`（better-sqlite3 同形）；db.close 幂等。

use std::borrow::Cow;

use base64::Engine as _;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;

/// JS → worker 的一条操作（全 Send；turso `Params` 为公开枚举，钉 =0.6.1 可直用）。
pub enum SqliteOp {
    Exec(String),
    Run { sql: String, params: turso::params::Params },
    Rows { sql: String, params: turso::params::Params },
    Txn,
    Close,
}

/// native 侧持住的 worker 端点（req/resp 均可 Clone；ops 严格顺序单飞）。
pub struct SqliteWorker {
    pub req_tx: crossbeam_channel::Sender<SqliteOp>,
    pub resp_rx: crossbeam_channel::Receiver<Result<serde_json::Value, String>>,
}

fn err_string(e: turso::Error) -> String {
    e.to_string()
}

/// 发一条操作并阻塞取回结果（JS 线程同步语义的落点）。
fn roundtrip(id: u64, op: SqliteOp) -> Result<serde_json::Value, String> {
    let SqliteWorker { req_tx: tx, resp_rx: rx } =
        state::sqlite_worker(id).ok_or_else(|| "database is not open".to_string())?;
    tx.send(op).map_err(|_| "sqlite worker died".to_string())?;
    rx.recv().map_err(|_| "sqlite worker died".to_string())?
}

/// 打开 worker（Bun 语义：路径打不开在构造时即报错 —— worker 先握手 open 结果，
/// 失败则线程自退）。成功返回 native 侧端点。
fn open_worker(path: String) -> Result<SqliteWorker, String> {
    let (req_tx, req_rx) = crossbeam_channel::unbounded::<SqliteOp>();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded::<Result<serde_json::Value, String>>();
    let spawned = std::thread::Builder::new()
        .name("winterjs-sqlite".into())
        .spawn(move || worker_main(path, req_rx, resp_tx));
    spawned.map_err(|e| format!("failed to spawn sqlite worker: {e}"))?;
    match resp_rx.recv() {
        Ok(Ok(_)) => Ok(SqliteWorker { req_tx, resp_rx }),
        Ok(Err(msg)) => Err(msg),
        Err(_) => Err("sqlite worker died".to_string()),
    }
}

/// worker 线程主体：自带 tokio runtime（turso 的 IO 需要异步上下文），
/// 循环收 op → block_on 执行 → 回发结果；Close 或 channel 断开即退。
fn worker_main(
    path: String,
    req_rx: crossbeam_channel::Receiver<SqliteOp>,
    resp: crossbeam_channel::Sender<Result<serde_json::Value, String>>,
) {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = resp.send(Err(format!("failed to start sqlite runtime: {e}")));
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
    let _ = resp.send(Ok(serde_json::json!({ "open": path })));
    for op in req_rx.iter() {
        match op {
            SqliteOp::Close => {
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
    op: SqliteOp,
) -> Result<serde_json::Value, String> {
    rt.block_on(async {
        match op {
            SqliteOp::Close => Ok(serde_json::json!({})),
            SqliteOp::Exec(sql) => conn
                .execute_batch(&sql)
                .await
                .map(|_| serde_json::json!({}))
                .map_err(err_string),
            SqliteOp::Run { sql, params } => {
                let changes = conn.execute(&sql, params).await.map_err(err_string)?;
                let last = conn.last_insert_rowid();
                Ok(serde_json::json!({ "changes": changes, "lastInsertRowid": last }))
            }
            SqliteOp::Rows { sql, params } => {
                let mut rows = conn.query(&sql, params).await.map_err(err_string)?;
                let columns = rows.column_names();
                let mut out = Vec::new();
                while let Some(row) = rows.next().await.map_err(err_string)? {
                    let mut vals = Vec::with_capacity(row.column_count());
                    for i in 0..row.column_count() {
                        vals.push(value_json(row.get_value(i).map_err(err_string)?));
                    }
                    out.push(serde_json::Value::Array(vals));
                }
                Ok(serde_json::json!({ "columns": columns, "rows": out }))
            }
            SqliteOp::Txn => {
                let autocommit = conn.is_autocommit().map_err(err_string)?;
                Ok(serde_json::json!(!autocommit))
            }
        }
    })
}

/// JSON 编码值 → turso Value（`$blob` 标记 → Blob）。
fn json_value(v: serde_json::Value) -> Result<turso::Value, String> {
    match v {
        serde_json::Value::Null => Ok(turso::Value::Null),
        serde_json::Value::Bool(_) => Err("boolean parameters are not supported".into()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(turso::Value::Integer(i))
            } else {
                Ok(turso::Value::Real(n.as_f64().unwrap_or_default()))
            }
        }
        serde_json::Value::String(s) => Ok(turso::Value::Text(s)),
        serde_json::Value::Object(o) => match o.get("$blob") {
            Some(serde_json::Value::String(b64)) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| format!("bad blob parameter: {e}"))?;
                Ok(turso::Value::Blob(bytes))
            }
            _ => Err("unsupported parameter object".into()),
        },
        serde_json::Value::Array(_) => Err("unsupported parameter: nested array".into()),
    }
}

/// 参数 JSON（positional 值数组 / named `[键, 值]` 对数组）→ turso Params。
fn parse_params(json: &str, named: bool) -> Result<turso::params::Params, String> {
    let raw: Vec<serde_json::Value> =
        serde_json::from_str(json).map_err(|e| format!("bad sqlite parameters: {e}"))?;
    if !named {
        let vals = raw
            .into_iter()
            .map(json_value)
            .collect::<Result<Vec<_>, String>>()?;
        return Ok(turso::params::Params::Positional(vals));
    }
    let mut out = Vec::with_capacity(raw.len());
    for item in raw {
        let serde_json::Value::Array(pair) = item else {
            return Err("bad named parameter entry".into());
        };
        let mut it = pair.into_iter();
        let serde_json::Value::String(key) = it.next().unwrap_or(serde_json::Value::Null) else {
            return Err("bad named parameter key".into());
        };
        let Some(val) = it.next() else {
            return Err("bad named parameter entry".into());
        };
        if it.next().is_some() {
            return Err("bad named parameter entry".into());
        }
        out.push((Cow::Owned(key), json_value(val)?));
    }
    Ok(turso::params::Params::Named(out))
}

/// turso Value → JSON（Blob 走 `$blob` 标记）。
fn value_json(v: turso::Value) -> serde_json::Value {
    match v {
        turso::Value::Null => serde_json::Value::Null,
        turso::Value::Integer(i) => serde_json::json!(i),
        turso::Value::Real(f) => serde_json::json!(f),
        turso::Value::Text(s) => serde_json::json!(s),
        turso::Value::Blob(b) => {
            serde_json::json!({ "$blob": base64::engine::general_purpose::STANDARD.encode(b) })
        }
    }
}

// ── natives（全同步；参数校验失败 report_error + false）─────────────────────

fn arg_string(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<String> {
    if frame.argc() <= i {
        report_error(cx, &format!("TypeError: {what} requires an argument"));
        return None;
    }
    Some(value_to_string(cx, frame.arg(i)))
}

/// `__wjs_sqlite_open(path)` → id。阻塞到 worker open 握手完成。
pub unsafe extern "C" fn sqlite_open(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_string(&mut cx, &frame, 0, "sqlite open") else {
        return false;
    };
    if path != ":memory:" {
        // 文件库要读 + 写（WAL/journal）；只授读不足以安全打开
        if let Err(msg) = crate::permissions::check_read(&path).and_then(|_| crate::permissions::check_write(&path)) {
            report_error(&mut cx, &msg);
            return false;
        }
    }
    match open_worker(path) {
        Ok(worker) => {
            let id = state::sqlite_add(worker);
            frame.set_rval(mozjs::jsval::Int32Value(id as i32));
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("SqliteError: {msg}"));
            false
        }
    }
}

/// `__wjs_sqlite_exec(id, sql)`：多语句批量（无参数）。
pub unsafe extern "C" fn sqlite_exec(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(id), Some(sql)) = (
        arg_id(&mut cx, &frame, 0, "sqlite exec"),
        arg_string(&mut cx, &frame, 1, "sqlite exec"),
    ) else {
        return false;
    };
    match roundtrip(id, SqliteOp::Exec(sql)) {
        Ok(_) => {
            frame.set_rval(UndefinedValue());
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("SqliteError: {msg}"));
            false
        }
    }
}

/// `__wjs_sqlite_run(id, sql, paramsJson, named)` → `{changes, lastInsertRowid}` JSON。
pub unsafe extern "C" fn sqlite_run(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some((id, sql, params)) = arg_sql_params(&mut cx, &frame, "sqlite run") else {
        return false;
    };
    match roundtrip(id, SqliteOp::Run { sql, params }) {
        Ok(v) => {
            frame.set_rval(json_string(&mut cx, &v));
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("SqliteError: {msg}"));
            false
        }
    }
}

/// `__wjs_sqlite_rows(id, sql, paramsJson, named)` → `{columns, rows}` JSON。
pub unsafe extern "C" fn sqlite_rows(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some((id, sql, params)) = arg_sql_params(&mut cx, &frame, "sqlite query") else {
        return false;
    };
    match roundtrip(id, SqliteOp::Rows { sql, params }) {
        Ok(v) => {
            frame.set_rval(json_string(&mut cx, &v));
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("SqliteError: {msg}"));
            false
        }
    }
}

/// `__wjs_sqlite_txn(id)` → 1/0（是否在事务内；即 SQLite 非 autocommit）。
pub unsafe extern "C" fn sqlite_txn(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&mut cx, &frame, 0, "sqlite txn") else {
        return false;
    };
    match roundtrip(id, SqliteOp::Txn) {
        Ok(v) => {
            let in_txn = v.as_bool().unwrap_or(false);
            frame.set_rval(mozjs::jsval::Int32Value(if in_txn { 1 } else { 0 }));
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("SqliteError: {msg}"));
            false
        }
    }
}

/// `__wjs_sqlite_close(id)`：worker 收尾退出；state 表摘除（幂等）。
pub unsafe extern "C" fn sqlite_close(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&mut cx, &frame, 0, "sqlite close") else {
        return false;
    };
    // 已不在表内（重复 close）→ 静默成功（幂等语义由 prelude 保证不重入）。
    if state::sqlite_worker(id).is_none() {
        frame.set_rval(UndefinedValue());
        return true;
    }
    match roundtrip(id, SqliteOp::Close) {
        Ok(_) => {
            state::sqlite_remove(id);
            frame.set_rval(UndefinedValue());
            true
        }
        Err(msg) => {
            state::sqlite_remove(id);
            report_error(&mut cx, &format!("SqliteError: {msg}"));
            false
        }
    }
}

fn arg_id(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<u64> {
    if frame.argc() <= i || !frame.arg(i).is_number() {
        report_error(cx, &format!("TypeError: {what} requires a database id"));
        return None;
    }
    Some(frame.arg(i).to_number() as u64)
}

/// `(id, sql, paramsJson, namedFlag)` 三元组解析（run/rows 共用）。
fn arg_sql_params(
    cx: &mut JSContext,
    frame: &Frame,
    what: &str,
) -> Option<(u64, String, turso::params::Params)> {
    let id = arg_id(cx, frame, 0, what)?;
    let sql = arg_string(cx, frame, 1, what)?;
    let json = arg_string(cx, frame, 2, what)?;
    if frame.argc() <= 3 || !frame.arg(3).is_number() {
        report_error(cx, &format!("TypeError: {what} requires a named flag"));
        return None;
    }
    let named = frame.arg(3).to_number() != 0.0;
    match parse_params(&json, named) {
        Ok(params) => Some((id, sql, params)),
        Err(msg) => {
            report_error(cx, &format!("TypeError: {msg}"));
            None
        }
    }
}

/// serde Value → JS 值（JSON.parse 语义；前置 realm 内）。
fn json_string(cx: &mut JSContext, v: &serde_json::Value) -> JSVal {
    rooted!(&in(cx) let mut out = UndefinedValue());
    let text = v.to_string();
    text.to_jsval(cx, out.handle_mut());
    out.get()
}

/// 内嵌 ESM 源（JSON 桥 + `$blob` revive；`query()` 按 SQL 缓存 Statement）。
pub const SOURCE: &str = r#"
class SqliteError extends Error {
  constructor(message) { super(message); this.name = "SqliteError"; }
}
function __wjs_sqlite_u8(v) {
  if (v instanceof Uint8Array) return v;
  if (v instanceof ArrayBuffer) return new Uint8Array(v);
  if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  return null;
}
function __wjs_sqlite_b64(u8) {
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
  return btoa(s);
}
function __wjs_sqlite_unb64(s) {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
function __wjs_sqlite_revive(v) {
  if (v !== null && typeof v === "object" && "$blob" in v) return __wjs_sqlite_unb64(v.$blob);
  return v;
}
function __wjs_sqlite_param(v) {
  if (v === null) return null;
  if (typeof v === "number") {
    if (!Number.isFinite(v)) throw new SqliteError("cannot bind non-finite number");
    return v;
  }
  if (typeof v === "string") return v;
  const u8 = __wjs_sqlite_u8(v);
  if (u8) return { $blob: __wjs_sqlite_b64(u8) };
  if (typeof v === "bigint") throw new TypeError("bigint parameters are not supported yet");
  throw new TypeError(`Unsupported parameter type: ${typeof v}`);
}
// 参数形态：单普通对象 → named（键须带 $/:/@ 前缀）；单数组 → positional；其余 → variadic positional
function __wjs_sqlite_args(args) {
  if (args.length === 1 && args[0] !== null && typeof args[0] === "object"
      && !Array.isArray(args[0]) && !__wjs_sqlite_u8(args[0])) {
    const named = [];
    for (const [k, v] of Object.entries(args[0])) named.push([k, __wjs_sqlite_param(v)]);
    return { named: 1, params: named };
  }
  const list = (args.length === 1 && Array.isArray(args[0])) ? args[0] : args;
  return { named: 0, params: Array.prototype.map.call(list, __wjs_sqlite_param) };
}
// native 报错转 SqliteError（native 侧异常是普通 Error，message 带前缀）
function __wjs_sqlite_wrap(fn) {
  try { return fn(); } catch (e) {
    const m = String((e && e.message) || e);
    // 权限拒绝直通（不转 SqliteError；fs 同款）
    if (m.startsWith("PermissionError:")) {
      const perr = new Error(m.slice("PermissionError: ".length));
      perr.name = "PermissionError";
      throw perr;
    }
    throw new SqliteError(m.startsWith("SqliteError: ") ? m.slice("SqliteError: ".length) : m);
  }
}
const __wjs_sqlite_dbState = new WeakMap();
const __wjs_sqlite_stmtState = new WeakMap();
let __wjs_sqlite_savepoint = 0;
function __wjs_sqlite_openCheck(st) {
  if (!st || st.closed) throw new SqliteError("database is not open");
}
// 实例经 Object.create(prototype) 造（构造器非法），私有方法无 brand 槽不可用，
// 状态一律走 WeakMap 自由函数。
function __wjs_sqlite_stmtOf(st) { return __wjs_sqlite_stmtState.get(st); }
function __wjs_sqlite_dbOf(st) { return __wjs_sqlite_dbState.get(st); }
// 取 (dbId, 参数包)（statement 各方法共用；finalized/未开库即抛）
function __wjs_sqlite_stmtId(stmt, args) {
  const st = __wjs_sqlite_stmtOf(stmt);
  if (st.finalized) throw new SqliteError("statement is finalized");
  const dst = __wjs_sqlite_dbState.get(st.db);
  __wjs_sqlite_openCheck(dst);
  const { named, params } = __wjs_sqlite_args(args);
  return { id: dst.id, named, json: JSON.stringify(params) };
}
class Statement {
  constructor() { throw new TypeError("Illegal constructor"); }
  static __wjs_make(db, sql, mode) {
    const stmt = Object.create(Statement.prototype);
    __wjs_sqlite_stmtState.set(stmt, { db, sql, mode: mode ?? "object", finalized: false });
    return stmt;
  }
  as(mode) {
    if (mode !== "object" && mode !== "array" && mode !== "raw") {
      throw new TypeError(`as() mode must be 'object', 'array' or 'raw', got ${String(mode)}`);
    }
    const st = __wjs_sqlite_stmtOf(this);
    return Statement.__wjs_make(st.db, st.sql, mode);
  }
  all(...args) {
    const { id, named, json } = __wjs_sqlite_stmtId(this, args);
    const resp = __wjs_sqlite_wrap(() => JSON.parse(__wjs_sqlite_rows(id, __wjs_sqlite_stmtOf(this).sql, json, named)));
    const mode = __wjs_sqlite_stmtOf(this).mode;
    return resp.rows.map((vals) => {
      const v = vals.map(__wjs_sqlite_revive);
      if (mode === "object") {
        const o = {};
        for (let i = 0; i < resp.columns.length; i++) o[resp.columns[i]] = v[i];
        return o;
      }
      return v;
    });
  }
  get(...args) {
    const rows = this.all(...args);
    return rows.length ? rows[0] : null;
  }
  values(...args) {
    const { id, named, json } = __wjs_sqlite_stmtId(this, args);
    const resp = __wjs_sqlite_wrap(() => JSON.parse(__wjs_sqlite_rows(id, __wjs_sqlite_stmtOf(this).sql, json, named)));
    return resp.rows.map((vals) => vals.map(__wjs_sqlite_revive));
  }
  *iterate(...args) { yield* this.all(...args); }
  run(...args) {
    const { id, named, json } = __wjs_sqlite_stmtId(this, args);
    return __wjs_sqlite_wrap(() => JSON.parse(__wjs_sqlite_run(id, __wjs_sqlite_stmtOf(this).sql, json, named)));
  }
  finalize() {
    __wjs_sqlite_stmtOf(this).finalized = true;
    return this;
  }
  get isFinalized() { return __wjs_sqlite_stmtOf(this).finalized; }
}
class Database {
  constructor(path, options) {
    if (typeof path !== "string") throw new TypeError("Database path must be a string");
    if (options !== undefined && options !== null && typeof options !== "object") {
      throw new TypeError("Database options must be an object");
    }
    // 偏差：readonly/create 等选项 turso 0.6.1 不支持，忽略（文档记录）。
    const id = __wjs_sqlite_wrap(() => __wjs_sqlite_open(path));
    __wjs_sqlite_dbState.set(this, { id, path, closed: false, cache: new Map() });
  }
  get filename() { return __wjs_sqlite_dbOf(this).path; }
  get isClosed() { return __wjs_sqlite_dbOf(this).closed; }
  get inTransaction() {
    const st = __wjs_sqlite_dbOf(this);
    __wjs_sqlite_openCheck(st);
    return __wjs_sqlite_wrap(() => __wjs_sqlite_txn(st.id)) === 1;
  }
  query(sql) {
    const st = __wjs_sqlite_dbOf(this);
    __wjs_sqlite_openCheck(st);
    if (typeof sql !== "string") throw new TypeError("query() requires a string");
    let stmt = st.cache.get(sql);
    if (!stmt) {
      stmt = Statement.__wjs_make(this, sql, "object");
      st.cache.set(sql, stmt);
    }
    return stmt;
  }
  prepare(sql) {
    const st = __wjs_sqlite_dbOf(this);
    __wjs_sqlite_openCheck(st);
    if (typeof sql !== "string") throw new TypeError("prepare() requires a string");
    return Statement.__wjs_make(this, sql, "object");
  }
  run(sql, ...args) {
    const st = __wjs_sqlite_dbOf(this);
    __wjs_sqlite_openCheck(st);
    if (typeof sql !== "string") throw new TypeError("run() requires a string");
    const { named, params } = __wjs_sqlite_args(args);
    __wjs_sqlite_wrap(() => __wjs_sqlite_run(st.id, sql, JSON.stringify(params), named));
    return this;
  }
  exec(sql) {
    const st = __wjs_sqlite_dbOf(this);
    __wjs_sqlite_openCheck(st);
    if (typeof sql !== "string") throw new TypeError("exec() requires a string");
    __wjs_sqlite_wrap(() => __wjs_sqlite_exec(st.id, sql));
    return this;
  }
  transaction(fn) {
    if (typeof fn !== "function") throw new TypeError("transaction() requires a function");
    const db = this;
    const wrap = (mode) => (...args) => {
      const st = __wjs_sqlite_dbState.get(db);
      __wjs_sqlite_openCheck(st);
      const nested = __wjs_sqlite_wrap(() => __wjs_sqlite_txn(st.id)) === 1;
      const sp = nested ? `__wjs_sp_${++__wjs_sqlite_savepoint}` : null;
      try {
        if (nested) __wjs_sqlite_wrap(() => __wjs_sqlite_exec(st.id, `SAVEPOINT ${sp}`));
        else __wjs_sqlite_wrap(() => __wjs_sqlite_exec(st.id, mode ?? "BEGIN"));
        const r = fn.call(db, ...args);
        if (nested) __wjs_sqlite_wrap(() => __wjs_sqlite_exec(st.id, `RELEASE ${sp}`));
        else __wjs_sqlite_wrap(() => __wjs_sqlite_exec(st.id, "COMMIT"));
        return r;
      } catch (e) {
        try {
          if (nested) {
            __wjs_sqlite_wrap(() => __wjs_sqlite_exec(st.id, `ROLLBACK TO ${sp}`));
            __wjs_sqlite_wrap(() => __wjs_sqlite_exec(st.id, `RELEASE ${sp}`));
          } else {
            __wjs_sqlite_wrap(() => __wjs_sqlite_exec(st.id, "ROLLBACK"));
          }
        } catch {}
        throw e;
      }
    };
    const base = wrap(null);
    base.deferred = wrap("BEGIN DEFERRED");
    base.immediate = wrap("BEGIN IMMEDIATE");
    base.exclusive = wrap("BEGIN EXCLUSIVE");
    return base;
  }
  close() {
    const st = __wjs_sqlite_dbOf(this);
    if (st.closed) return;
    __wjs_sqlite_wrap(() => __wjs_sqlite_close(st.id));
    st.closed = true;
    st.cache.clear();
  }
}
export { Database, SqliteError };
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_json_roundtrip() {
        // positional：int/real/text/null/blob 各型
        let p = parse_params(
            r#"[1, 1.5, "hi", null, {"$blob": "AQID"}]"#,
            false,
        )
        .expect("positional params parse");
        let turso::params::Params::Positional(vals) = p else {
            panic!("expected positional");
        };
        assert_eq!(vals[0], turso::Value::Integer(1));
        assert_eq!(vals[1], turso::Value::Real(1.5));
        assert_eq!(vals[2], turso::Value::Text("hi".into()));
        assert_eq!(vals[3], turso::Value::Null);
        assert_eq!(vals[4], turso::Value::Blob(vec![1, 2, 3]));
        // named：键带前缀直传
        let p = parse_params(r#"[["$name", "x"], [":n", 2]]"#, true).expect("named params parse");
        let turso::params::Params::Named(pairs) = p else {
            panic!("expected named");
        };
        assert_eq!(pairs[0].0, "$name");
        assert_eq!(pairs[0].1, turso::Value::Text("x".into()));
        assert_eq!(pairs[1].0, ":n");
        assert_eq!(pairs[1].1, turso::Value::Integer(2));
        // 空参数
        assert!(matches!(parse_params("[]", false).unwrap(), turso::params::Params::Positional(v) if v.is_empty()));
        // 报错形：布尔 / 嵌套数组 / 坏 named 形状 / 坏 blob
        assert!(parse_params("[true]", false).is_err());
        assert!(parse_params("[[1]]", false).is_err());
        assert!(parse_params("[[\"k\"]]", true).is_err());
        assert!(parse_params("[[\"k\",\"v\",\"x\"]]", true).is_err());
        assert!(parse_params(r#"[{"$blob": "!!!"}]"#, false).is_err());
        assert!(parse_params("[{}]", false).is_err());
    }

    #[test]
    fn value_json_blob_marker() {
        assert_eq!(value_json(turso::Value::Null), serde_json::json!(null));
        assert_eq!(value_json(turso::Value::Integer(-7)), serde_json::json!(-7));
        assert_eq!(value_json(turso::Value::Real(0.5)), serde_json::json!(0.5));
        assert_eq!(value_json(turso::Value::Text("s".into())), serde_json::json!("s"));
        let v = value_json(turso::Value::Blob(vec![0, 250]));
        assert_eq!(v, serde_json::json!({"$blob": base64::engine::general_purpose::STANDARD.encode([0u8, 250u8])}));
        // 回程一致
        assert_eq!(json_value(v).unwrap(), turso::Value::Blob(vec![0, 250]));
    }

    /// 开内存库走一轮 op 往返的测试脚手架（返回 req 端点；drop 即收线程）。
    struct TestDb(crossbeam_channel::Sender<SqliteOp>, crossbeam_channel::Receiver<Result<serde_json::Value, String>>);

    impl TestDb {
        fn open_memory() -> TestDb {
            let w = open_worker(":memory:".into()).expect("open :memory:");
            TestDb(w.req_tx, w.resp_rx)
        }
        fn call(&self, op: SqliteOp) -> serde_json::Value {
            self.0.send(op).expect("send op");
            self.1.recv().expect("recv resp").expect("op ok")
        }
    }

    #[test]
    fn worker_memory_roundtrip() {
        let db = TestDb::open_memory();
        db.call(SqliteOp::Exec("CREATE TABLE t (id INTEGER, v TEXT, b BLOB)".into()));
        let r = db.call(SqliteOp::Run {
            sql: "INSERT INTO t VALUES (?1, ?2, ?3)".into(),
            params: turso::params::Params::Positional(vec![
                turso::Value::Integer(1),
                turso::Value::Text("a".into()),
                turso::Value::Blob(vec![9, 8]),
            ]),
        });
        assert_eq!(r["changes"], serde_json::json!(1));
        assert_eq!(r["lastInsertRowid"], serde_json::json!(1));
        let r = db.call(SqliteOp::Rows {
            sql: "SELECT id, v, b FROM t WHERE id = ?1".into(),
            params: turso::params::Params::Positional(vec![turso::Value::Integer(1)]),
        });
        assert_eq!(r["columns"], serde_json::json!(["id", "v", "b"]));
        assert_eq!(r["rows"].as_array().unwrap().len(), 1);
        assert_eq!(r["rows"][0][0], serde_json::json!(1));
        assert_eq!(r["rows"][0][1], serde_json::json!("a"));
        assert_eq!(r["rows"][0][2], serde_json::json!({"$blob": base64::engine::general_purpose::STANDARD.encode([9u8, 8u8])}));
        // named 参数 + 事务旗
        assert_eq!(db.call(SqliteOp::Txn), serde_json::json!(false));
        db.call(SqliteOp::Exec("BEGIN".into()));
        assert_eq!(db.call(SqliteOp::Txn), serde_json::json!(true));
        db.call(SqliteOp::Exec("ROLLBACK".into()));
        assert_eq!(db.call(SqliteOp::Txn), serde_json::json!(false));
        // 坏 SQL 报错文本
        db.0.send(SqliteOp::Exec("NOPE".into())).unwrap();
        let err = db.1.recv().unwrap().unwrap_err();
        assert!(!err.is_empty());
        // close 后线程退出（req 断开）
        db.call(SqliteOp::Close);
    }

    #[test]
    fn open_bad_path_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bad = dir.path().join("no-such-dir").join("x.db");
        let err = match open_worker(bad.to_string_lossy().into_owned()) {
            Err(e) => e,
            Ok(_) => panic!("expected open failure"),
        };
        assert!(!err.is_empty());
    }
}
