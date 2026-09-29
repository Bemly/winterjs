//! `node:sqlite`：Node 22+ 实验面（DatabaseSync/StatementSync，turso 0.6.1 底座，10d）。
//!
//! 落法复用 `bun:sqlite` 的 worker 哲学（每 Database 一条专用线程 + 自持
//! current-thread tokio runtime，native 经 crossbeam 阻塞往返；JS 线程独占不破坏）。
//! 与 bun 面差异：整数超 2^53 经 `$bigint` 标记保真（JS 侧按 `setReadBigInts`
//! 决定 BigInt 化或抛 `ERR_OUT_OF_RANGE`，真机口径）；列元数据经 turso
//! `columns()`（name + decltype；database/table/origin 真机有而 turso 未暴露，
//! 一律 null，记档）；`iterate()` 为全量取回后 JS 侧生成器（非真游标，记档）。
//!
//! 偏差记档：`readOnly` 接受存储但 turso 0.6.1 Builder 无对应项（读写仍通，
//! 与 `bun:sqlite` 同口径忽略）；高级面（Session/backup/aggregate/function/
//! authorizer/defensive/loadExtension/serialize）未做，访问即 `ERR_NOT_SUPPORTED`；
//! `location` 为方法（真机同款，非属性）；`isOpen` 为 getter。

use std::borrow::Cow;

use base64::Engine as _;
use mozjs::conversions::ToJSValConvertible as _;
use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;

pub enum NodeSqliteOp {
    Exec(String),
    Run { sql: String, params: turso::params::Params },
    Rows { sql: String, params: turso::params::Params },
    Cols { sql: String },
    Close,
}

pub struct NodeSqliteWorker {
    pub req_tx: crossbeam_channel::Sender<NodeSqliteOp>,
    pub resp_rx: crossbeam_channel::Receiver<Result<serde_json::Value, String>>,
}

fn err_string(e: turso::Error) -> String {
    e.to_string()
}

fn roundtrip(id: u64, op: NodeSqliteOp) -> Result<serde_json::Value, String> {
    let NodeSqliteWorker { req_tx: tx, resp_rx: rx } =
        state::nsqlite_worker(id).ok_or_else(|| "database is not open".to_string())?;
    tx.send(op).map_err(|_| "sqlite worker died".to_string())?;
    rx.recv().map_err(|_| "sqlite worker died".to_string())?
}

fn open_worker(path: String) -> Result<NodeSqliteWorker, String> {
    let (req_tx, req_rx) = crossbeam_channel::unbounded::<NodeSqliteOp>();
    let (resp_tx, resp_rx) = crossbeam_channel::unbounded::<Result<serde_json::Value, String>>();
    let spawned = std::thread::Builder::new()
        .name("winterjs2-nsqlite".into())
        .spawn(move || worker_main(path, req_rx, resp_tx));
    spawned.map_err(|e| format!("failed to spawn sqlite worker: {e}"))?;
    match resp_rx.recv() {
        Ok(Ok(_)) => Ok(NodeSqliteWorker { req_tx, resp_rx }),
        Ok(Err(msg)) => Err(msg),
        Err(_) => Err("sqlite worker died".to_string()),
    }
}

fn worker_main(
    path: String,
    req_rx: crossbeam_channel::Receiver<NodeSqliteOp>,
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
            NodeSqliteOp::Close => {
                let _ = resp.send(Ok(serde_json::json!({})));
                break;
            }
            other => {
                let _ = resp.send(run_op(&rt, &conn, other));
            }
        }
    }
}

fn run_op(
    rt: &tokio::runtime::Runtime,
    conn: &turso::Connection,
    op: NodeSqliteOp,
) -> Result<serde_json::Value, String> {
    rt.block_on(async {
        match op {
            NodeSqliteOp::Close => Ok(serde_json::json!({})),
            NodeSqliteOp::Exec(sql) => conn
                .execute_batch(&sql)
                .await
                .map(|_| serde_json::json!({}))
                .map_err(err_string),
            NodeSqliteOp::Run { sql, params } => {
                let changes = conn.execute(&sql, params).await.map_err(err_string)?;
                let last = conn.last_insert_rowid();
                Ok(serde_json::json!({ "changes": changes, "lastInsertRowid": last }))
            }
            NodeSqliteOp::Rows { sql, params } => {
                let mut rows = conn.query(&sql, params).await.map_err(err_string)?;
                let cols = rows.columns();
                let meta: Vec<serde_json::Value> = cols
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "column": c.name(), "database": serde_json::Value::Null,
                            "name": c.name(), "table": serde_json::Value::Null,
                            "type": c.decl_type().map(|s| s.to_string()).unwrap_or_default(),
                        })
                    })
                    .collect();
                // decltype 空串→null（真机无类型时为 null）
                let meta: Vec<serde_json::Value> = meta
                    .into_iter()
                    .map(|mut m| {
                        if m.get("type").and_then(|v| v.as_str()) == Some("") {
                            m["type"] = serde_json::Value::Null;
                        }
                        m
                    })
                    .collect();
                let mut out = Vec::new();
                while let Some(row) = rows.next().await.map_err(err_string)? {
                    let mut vals = Vec::with_capacity(row.column_count());
                    for i in 0..row.column_count() {
                        vals.push(value_json(row.get_value(i).map_err(err_string)?));
                    }
                    out.push(serde_json::Value::Array(vals));
                }
                Ok(serde_json::json!({ "columns": meta, "rows": out }))
            }
            NodeSqliteOp::Cols { sql } => {
                // 只取列元数据，不步进（prepare 校验/columns() 用，无副作用）。
                // 错误透出（坏 SQL 在 prepare 期即抛，真机口径；INSERT 等返回空列）。
                let rows = conn
                    .query(&sql, turso::params::Params::Positional(vec![]))
                    .await
                    .map_err(err_string)?;
                let meta: Vec<serde_json::Value> = rows
                    .columns()
                    .iter()
                    .map(|c| {
                        let mut m = serde_json::json!({
                            "column": c.name(), "database": serde_json::Value::Null,
                            "name": c.name(), "table": serde_json::Value::Null,
                            "type": c.decl_type().map(|s| s.to_string()).unwrap_or_default(),
                        });
                        if m.get("type").and_then(|v| v.as_str()) == Some("") {
                            m["type"] = serde_json::Value::Null;
                        }
                        m
                    })
                    .collect();
                Ok(serde_json::json!(meta))
            }
        }
    })
}

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
        serde_json::Value::Object(o) => {
            if let Some(serde_json::Value::String(b64)) = o.get("$blob") {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| format!("bad blob parameter: {e}"))?;
                return Ok(turso::Value::Blob(bytes));
            }
            if let Some(serde_json::Value::String(s)) = o.get("$bigint") {
                let i: i64 = s
                    .parse()
                    .map_err(|_| "bad bigint parameter".to_string())?;
                return Ok(turso::Value::Integer(i));
            }
            Err("unsupported parameter object".into())
        }
        serde_json::Value::Array(_) => Err("unsupported parameter: nested array".into()),
    }
}

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

/// 超 2^53 的整数经 `$bigint` 保真（JS 侧按 readBigInts 决定）。
fn value_json(v: turso::Value) -> serde_json::Value {
    const SAFE: i64 = 9007199254740991;
    match v {
        turso::Value::Null => serde_json::Value::Null,
        turso::Value::Integer(i) => {
            if (-SAFE..=SAFE).contains(&i) {
                serde_json::json!(i)
            } else {
                serde_json::json!({ "$bigint": i.to_string() })
            }
        }
        turso::Value::Real(f) => serde_json::json!(f),
        turso::Value::Text(s) => serde_json::json!(s),
        turso::Value::Blob(b) => {
            serde_json::json!({ "$blob": base64::engine::general_purpose::STANDARD.encode(b) })
        }
    }
}

// ── natives ──────────────────────────────────────────────────────────────

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

fn arg_sql_params(
    cx: &mut JSContext,
    frame: &Frame,
    what: &str,
) -> Option<(u64, String, turso::params::Params)> {
    let (Some(id), Some(sql), Some(pjson), named) = (
        arg_id(cx, frame, 0, what),
        arg_string(cx, frame, 1, what),
        arg_string(cx, frame, 2, what),
        frame.argc() > 3 && frame.arg(3).is_number() && frame.arg(3).to_number() != 0.0,
    ) else {
        if frame.argc() <= 2 {
            report_error(cx, &format!("TypeError: {what} requires id, sql and params"));
        }
        return None;
    };
    match parse_params(&pjson, named) {
        Ok(p) => Some((id, sql, p)),
        Err(msg) => {
            report_error(cx, &format!("SqliteError: {msg}"));
            None
        }
    }
}

fn json_string(cx: &mut JSContext, v: &serde_json::Value) -> JSVal {
    let text = v.to_string();
    rooted!(&in(cx) let mut r = UndefinedValue());
    text.as_str().to_jsval(cx, r.handle_mut());
    r.get()
}

/// `__wjs2_nsqlite_open(path)` → id。
pub unsafe extern "C" fn nsqlite_open(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(path) = arg_string(&mut cx, &frame, 0, "sqlite open") else {
        return false;
    };
    if path != ":memory:" {
        if let Err(msg) = crate::permissions::check_read(&path)
            .and_then(|_| crate::permissions::check_write(&path))
        {
            report_error(&mut cx, &msg);
            return false;
        }
    }
    match open_worker(path) {
        Ok(worker) => {
            let id = state::nsqlite_add(worker);
            frame.set_rval(mozjs::jsval::Int32Value(id as i32));
            true
        }
        Err(msg) => {
            report_error(&mut cx, &format!("SqliteError: {msg}"));
            false
        }
    }
}

/// `__wjs2_nsqlite_exec(id, sql)`。
pub unsafe extern "C" fn nsqlite_exec(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(id), Some(sql)) = (
        arg_id(&mut cx, &frame, 0, "sqlite exec"),
        arg_string(&mut cx, &frame, 1, "sqlite exec"),
    ) else {
        return false;
    };
    match roundtrip(id, NodeSqliteOp::Exec(sql)) {
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

/// `__wjs2_nsqlite_run(id, sql, paramsJson, named)` → `{changes, lastInsertRowid}`。
pub unsafe extern "C" fn nsqlite_run(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some((id, sql, params)) = arg_sql_params(&mut cx, &frame, "sqlite run") else {
        return false;
    };
    match roundtrip(id, NodeSqliteOp::Run { sql, params }) {
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

/// `__wjs2_nsqlite_rows(id, sql, paramsJson, named)` → `{columns, rows}`。
pub unsafe extern "C" fn nsqlite_rows(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some((id, sql, params)) = arg_sql_params(&mut cx, &frame, "sqlite query") else {
        return false;
    };
    match roundtrip(id, NodeSqliteOp::Rows { sql, params }) {
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

/// `__wjs2_nsqlite_cols(id, sql)` → 列元数据数组。
pub unsafe extern "C" fn nsqlite_cols(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let (Some(id), Some(sql)) = (
        arg_id(&mut cx, &frame, 0, "sqlite columns"),
        arg_string(&mut cx, &frame, 1, "sqlite columns"),
    ) else {
        return false;
    };
    match roundtrip(id, NodeSqliteOp::Cols { sql }) {
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

/// `__wjs2_nsqlite_close(id)`（幂等）。
pub unsafe extern "C" fn nsqlite_close(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_id(&mut cx, &frame, 0, "sqlite close") else {
        return false;
    };
    if state::nsqlite_worker(id).is_none() {
        frame.set_rval(UndefinedValue());
        return true;
    }
    match roundtrip(id, NodeSqliteOp::Close) {
        Ok(_) => {
            state::nsqlite_remove(id);
            frame.set_rval(UndefinedValue());
            true
        }
        Err(msg) => {
            state::nsqlite_remove(id);
            report_error(&mut cx, &format!("SqliteError: {msg}"));
            false
        }
    }
}

/// 内嵌 ESM 源（`node:sqlite`）。
pub const SOURCE: &str = r#"
function SqliteError(msg, code) {
  const e = new Error(msg);
  e.name = "SqliteError";
  e.code = code || "ERR_SQLITE_ERROR";
  return e;
}
function asState(v) {
  if (v === null || v === undefined) return null;
  if (typeof v === "bigint") return { $bigint: v.toString() };
  if (typeof v === "number") {
    if (!Number.isFinite(v)) throw SqliteError("non-finite numbers are not supported", "ERR_INVALID_ARG_VALUE");
    if (Number.isInteger(v) && !Number.isSafeInteger(v)) return { $bigint: String(v) };
    return v;
  }
  if (typeof v === "string") return v;
  if (v instanceof Uint8Array || ArrayBuffer.isView(v)) {
    const u8 = new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return { $blob: btoa(s) };
  }
  if (v instanceof ArrayBuffer) {
    const u8 = new Uint8Array(v);
    let s = "";
    for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode(...u8.subarray(i, i + 0x8000));
    return { $blob: btoa(s) };
  }
  throw SqliteError(`unsupported bind value: ${typeof v}`, "ERR_INVALID_ARG_VALUE");
}
function revive(v, readBigInts) {
  if (v !== null && typeof v === "object") {
    if (typeof v.$bigint === "string") {
      if (readBigInts) return BigInt(v.$bigint);
      const e = new RangeError(`Value is too large to be represented as a JavaScript number: ${v.$bigint}`);
      e.code = "ERR_OUT_OF_RANGE";
      throw e;
    }
    if (typeof v.$blob === "string") {
      const bin = atob(v.$blob);
      const out = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
      return out;
    }
  }
  return v;
}
function namedKeys(sql) {
  const out = [];
  const re = /[@:$][A-Za-z_][A-Za-z0-9_]*/g;
  let m;
  while ((m = re.exec(sql)) !== null) out.push(m[0]);
  return [...new Set(out)];
}
function normKey(k) {
  k = String(k);
  return (k.startsWith(":") || k.startsWith("@") || k.startsWith("$")) ? k : `:${k}`;
}
function buildParams(sql, args, allowUnknown) {
  // 无参 → 空 positional
  if (args.length === 0) {
    // 命名占位缺省绑 NULL（真机口径：缺省不抛）
    const keys = namedKeys(sql);
    if (keys.length > 0) {
      return { json: JSON.stringify(keys.map((k) => [k, null])), named: 1 };
    }
    return { json: "[]", named: 0 };
  }
  // 单对象 + SQL 含命名占位 → named
  const keys = namedKeys(sql);
  if (args.length === 1 && args[0] !== null && typeof args[0] === "object"
      && !(args[0] instanceof Uint8Array) && !(args[0] instanceof ArrayBuffer)
      && !Array.isArray(args[0]) && typeof args[0] !== "bigint") {
    const obj = args[0];
    const want = new Set(keys);
    const pairs = [];
    for (const [k, v] of Object.entries(obj)) {
      const nk = normKey(k);
      if (keys.length > 0 && !want.has(nk)) {
        if (!allowUnknown) {
          const e = new Error(`Unknown named parameter '${k}'`);
          e.code = "ERR_INVALID_STATE";
          throw e;
        }
        continue;
      }
      pairs.push([nk, asState(v)]);
    }
    // 缺省命名键补 NULL
    for (const k of keys) {
      if (!pairs.some(([kk]) => kk === k
        || (kk.slice(1) === k.slice(1)))) pairs.push([k, null]);
    }
    return { json: JSON.stringify(pairs), named: 1 };
  }
  // positional（spread）
  return { json: JSON.stringify(args.map(asState)), named: 0 };
}
function callNative(fn, ...a) {
  try {
    return fn(...a);
  } catch (e) {
    const msg = String((e && e.message) || e);
    if (msg.startsWith("SqliteError:")) throw SqliteError(msg.slice("SqliteError:".length).trim());
    throw e;
  }
}
function ensureOpen(db) {
  if (!db.__wjs2_open) {
    const e = new Error("database is not open");
    e.code = "ERR_INVALID_STATE";
    throw e;
  }
}
const __stmtFlags = new WeakMap();
export class StatementSync {
  constructor(db, sql) {
    if (typeof sql !== "string" || sql.trim() === "") {
      const e = new TypeError("prepare: SQL must be a non-empty string");
      e.code = "ERR_INVALID_ARG_VALUE";
      throw e;
    }
    this.__wjs2_db = db;
    this.__wjs2_sql = sql;
    __stmtFlags.set(this, { returnArrays: false, readBigInts: false, allowUnknown: false, allowBare: true });
  }
  get sourceSQL() { return this.__wjs2_sql; }
  get expandedSQL() {
    // 未绑定时命名占位即 NULL（真机口径近似）
    let out = this.__wjs2_sql;
    for (const k of namedKeys(this.__wjs2_sql)) out = out.split(k).join("NULL");
    return out;
  }
  __params(args) {
    const fl = __stmtFlags.get(this);
    return buildParams(this.__wjs2_sql, args, fl.allowUnknown);
  }
  __rows(args) {
    const db = this.__wjs2_db;
    ensureOpen(db);
    const { json, named } = this.__params(args);
    const text = callNative(__wjs2_nsqlite_rows, db.__wjs2_id, this.__wjs2_sql, json, named);
    return JSON.parse(text);
  }
  run(...args) {
    const db = this.__wjs2_db;
    ensureOpen(db);
    const { json, named } = this.__params(args);
    const text = callNative(__wjs2_nsqlite_run, db.__wjs2_id, this.__wjs2_sql, json, named);
    return JSON.parse(text);
  }
  get(...args) {
    const fl = __stmtFlags.get(this);
    const { rows, columns } = this.__rows(args);
    if (rows.length === 0) return undefined;
    const vals = rows[0].map((v) => revive(v, fl.readBigInts));
    if (fl.returnArrays) return vals;
    const obj = {};
    columns.forEach((c, i) => { obj[c.name] = vals[i]; });
    return obj;
  }
  all(...args) {
    const fl = __stmtFlags.get(this);
    const { rows, columns } = this.__rows(args);
    return rows.map((r) => {
      const vals = r.map((v) => revive(v, fl.readBigInts));
      if (fl.returnArrays) return vals;
      const obj = {};
      columns.forEach((c, i) => { obj[c.name] = vals[i]; });
      return obj;
    });
  }
  *iterate(...args) {
    for (const row of this.all(...args)) yield row;
  }
  columns() {
    const db = this.__wjs2_db;
    ensureOpen(db);
    const text = callNative(__wjs2_nsqlite_cols, db.__wjs2_id, this.__wjs2_sql);
    return JSON.parse(text);
  }
  setAllowBareNamedParameters(v) { __stmtFlags.get(this).allowBare = !!v; }
  setAllowUnknownNamedParameters(v) { __stmtFlags.get(this).allowUnknown = !!v; }
  setReadBigInts(v) { __stmtFlags.get(this).readBigInts = !!v; }
  setReturnArrays(v) { __stmtFlags.get(this).returnArrays = !!v; }
}
export class DatabaseSync {
  constructor(path, options) {
    const opts = options || {};
    if (typeof path !== "string") {
      const e = new TypeError("DatabaseSync: path must be a string");
      e.code = "ERR_INVALID_ARG_VALUE";
      throw e;
    }
    this.__wjs2_path = path;
    this.__wjs2_readonly = !!opts.readOnly;
    this.__wjs2_open = false;
    this.__wjs2_id = 0;
    if (opts.open !== false) this.open();
  }
  get isOpen() { return this.__wjs2_open; }
  location() { return this.__wjs2_path === ":memory:" ? null : this.__wjs2_path; }
  open() {
    if (this.__wjs2_open) return;
    const id = callNative(__wjs2_nsqlite_open, this.__wjs2_path);
    this.__wjs2_id = Number(id);
    this.__wjs2_open = true;
  }
  close() {
    if (!this.__wjs2_open) return;
    try { callNative(__wjs2_nsqlite_close, this.__wjs2_id); } catch {}
    this.__wjs2_open = false;
    this.__wjs2_id = 0;
  }
  exec(sql) {
    ensureOpen(this);
    if (typeof sql !== "string") {
      const e = new TypeError("exec: SQL must be a string");
      e.code = "ERR_INVALID_ARG_VALUE";
      throw e;
    }
    callNative(__wjs2_nsqlite_exec, this.__wjs2_id, sql);
  }
  prepare(sql) {
    ensureOpen(this);
    const st = new StatementSync(this, sql);
    // 真机口径：坏 SQL 在 prepare 期即抛（turso 列元数据只备不步进，无副作用）
    callNative(__wjs2_nsqlite_cols, this.__wjs2_id, sql);
    return st;
  }
}
function notSupported(name) {
  return (..._) => {
    const e = new Error(`${name} is not supported`);
    e.code = "ERR_NOT_SUPPORTED";
    throw e;
  };
}
DatabaseSync.prototype.createSession = notSupported("createSession");
DatabaseSync.prototype.applyChangeset = notSupported("applyChangeset");
DatabaseSync.prototype.serialize = notSupported("serialize");
DatabaseSync.prototype.deserialize = notSupported("deserialize");
const __api = { DatabaseSync, StatementSync };
export default __api;
"#;

#[cfg(test)]
mod tests {
    #[test]
    fn nsqlite_param_shapes() {
        // parse_params 纯逻辑：positional 与 named 分流
        let p = super::parse_params("[1, 2]", false).unwrap();
        match p {
            turso::params::Params::Positional(v) => assert_eq!(v.len(), 2),
            _ => panic!("expected positional"),
        }
        let p = super::parse_params(r#"[[":v", 1]]"#, true).unwrap();
        match p {
            turso::params::Params::Named(v) => assert_eq!(v.len(), 1),
            _ => panic!("expected named"),
        }
    }

    #[test]
    fn nsqlite_bigint_marker() {
        let big = super::value_json(turso::Value::Integer(9007199254740993));
        assert!(big.get("$bigint").is_some());
        let small = super::value_json(turso::Value::Integer(42));
        assert_eq!(small.as_i64(), Some(42));
    }
}
