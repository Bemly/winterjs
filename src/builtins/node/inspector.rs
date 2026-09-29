//! `node:inspector`：调试会话薄层（9e-4，调试向）。
//! `Session.post('Runtime.evaluate')` 真求值（同线程嵌套 `evaluate_script`，
//! 结果经 `JSON.stringify` 包络回传）；`*.enable/disable`  domains 仅 ack（不断点、
//! 不剖析，记档）；其余方法报 `ERR_NOT_SUPPORTED`。
//! 偏差记档：
//! - 无 DevTools 端点：`open()` 空转、`url()` 恒 `undefined`、`waitForDebugger()`
//!   立即返回（Node 会阻塞到调试器接入）。`console` 直通全局 console。
//! - `Runtime.evaluate` 只收表达式（语句形报 SyntaxError 口径错）；恒按值返回
//!   （`returnByValue` 忽略）；`awaitPromise` 不支持（Promise 串化为 `{}`）；
//!   `bigint`/`function`/`undefined` 值按 JSON 口径退化（`type` 字段保真）。
//! - 事件通知面（`Debugger.paused` 等）无来源，`Session` 为静默 EventEmitter。
//! - `Network`/`NetworkResources`/`DOMStorage` 为占位类；`verify()` 系无。

use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};
use crate::state;

/// 求值直透（表达式/语句皆可；`throw` 即 pending 异常转文案错）。
/// 成功 → rval 为 completion 值（JS 侧再拼 CDP 形）；失败报
/// `InspectorEval: <message>`（JS 侧剥前缀组 `exceptionDetails`）。
/// `__wjs2_inspector_eval(exprStr)` → completion 值（失败抛 `InspectorEval:` 错）。
pub unsafe extern "C" fn inspector_eval(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: inspector evaluate needs an expression");
        return false;
    }
    let expr = value_to_string(&mut cx, frame.arg(0));
    rooted!(&in(cx) let mut rval = UndefinedValue());
    rooted!(&in(cx) let global = state::global());
    // SAFETY: 同线程嵌套求值（prelude 求值同款调用；global 由 ROOTED 保活）
    let filename = std::ffi::CString::new("node:inspector Runtime.evaluate").expect("no NUL");
    let opts = mozjs::rust::CompileOptionsWrapper::new(&cx, filename, 1);
    let ok =
        mozjs::rust::evaluate_script(&mut cx, global.handle(), &expr, rval.handle_mut(), opts);
    if ok.is_err() {
        // require.rs `pending_message` 同款（消费 pending 异常转文案）
        rooted!(&in(cx) let mut exc = UndefinedValue());
        let msg = match mozjs::rust::error_info_from_exception_stack(&mut cx, exc.handle_mut()) {
            Some(info) => info.message,
            None => "evaluation failed".to_string(),
        };
        report_error(&mut cx, &format!("InspectorEval: {msg}"));
        return false;
    }
    frame.set_rval(rval.get());
    true
}

/// 内嵌 ESM 源（`node:inspector` + `node:inspector/promises` 同源）。
pub const SOURCE: &str = r#"
import { EventEmitter } from "node:events";

function __inspectorErr(e) {
  const m = String((e && e.message) || e);
  const code = (m.match(/^([A-Z][A-Z0-9_]*): /) || [])[1];
  if (!code) {
    const err = new Error(m);
    err.code = "ERR_NOT_SUPPORTED";
    throw err;
  }
  const err = new Error(m.replace(/^[A-Z][A-Z0-9_]*: /, "") || code);
  err.code = code;
  throw err;
}
function __callNative(fn) {
  try {
    return fn();
  } catch (e) {
    __inspectorErr(e);
  }
}

class Session extends EventEmitter {
  constructor() {
    super();
    this.__connected = false;
  }
  connect() {
    this.__connected = true;
  }
  disconnect() {
    this.__connected = false;
  }
  post(method, params, callback) {
    if (typeof params === "function") { callback = params; params = {}; }
    const run = () => this.__dispatch(String(method), params ?? {});
    if (typeof callback !== "function") {
      return new Promise((resolve, reject) => {
        queueMicrotask(() => {
          try {
            resolve(run());
          } catch (e) {
            reject(e);
          }
        });
      });
    }
    queueMicrotask(() => {
      try {
        callback(null, run());
      } catch (e) {
        callback(e);
      }
    });
    return undefined;
  }
  __dispatch(method, params) {
    if (method === "Runtime.evaluate") {
      const expr = String(params.expression ?? "");
      let v;
      try {
        v = __callNative(() => __wjs2_inspector_eval(expr));
      } catch (e) {
        const m = String((e && e.message) || e).replace(/^InspectorEval: /, "");
        return {
          exceptionDetails: {
            exceptionId: 1,
            text: "Uncaught",
            exception: { description: m },
          },
        };
      }
      const t = typeof v;
      let value;
      if (v === undefined) {
        value = undefined;
      } else {
        try {
          const j = JSON.stringify(v);
          value = (j === undefined) ? undefined : JSON.parse(j);
        } catch {
          value = String(v);
        }
      }
      return { result: { type: t, value } };
    }
    const domain = method.split(".")[0];
    const verb = method.split(".")[1];
    if ((verb === "enable" || verb === "disable") && domain !== undefined) {
      return {};
    }
    const err = new Error(`Inspector method ${method} not supported`);
    err.code = "ERR_NOT_SUPPORTED";
    throw err;
  }
}

const inspectorConsole = {
  log: (...a) => console.log(...a),
  error: (...a) => console.error(...a),
  warn: (...a) => console.warn(...a),
  info: (...a) => console.info(...a),
  debug: (...a) => console.debug(...a),
  dir: (...a) => console.dir(...a),
  dirxml: (...a) => console.dir(...a),
  table: (...a) => console.log(...a),
  trace: (...a) => console.trace(...a),
  group: (...a) => console.group(...a),
  groupEnd: (...a) => console.groupEnd(...a),
  clear: () => console.clear(),
  count: (...a) => console.count(...a),
  countReset: (...a) => console.countReset(...a),
  assert: (...a) => console.assert(...a),
  time: (...a) => console.time(...a),
  timeLog: (...a) => console.timeLog(...a),
  timeEnd: (...a) => console.timeEnd(...a),
};

class Network {}
class NetworkResources {}
class DOMStorage {}

export function open() {
  return undefined;
}
export function close() {
  return undefined;
}
export function url() {
  return undefined;
}
export function waitForDebugger() {
  return undefined;
}
export { Session, inspectorConsole as console, Network, NetworkResources, DOMStorage };

const __api = {
  open, close, url, waitForDebugger, console: inspectorConsole,
  Session, Network, NetworkResources, DOMStorage,
};
export default __api;
"#;
