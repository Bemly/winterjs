//! tests/node/util_types.rs — 对齐 src/builtins/node/util_types.rs（node:util/types）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9a_util_types_surface() {
    // test-util-types* 命名子集（isProxy 恒 false 为记档偏差，不点名）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        r#"import types from "node:util/types";
import { types as t2 } from "node:util";
console.log("same", types === t2, typeof types.isPromise);
console.log(
  types.isPromise(Promise.resolve()),
  types.isMap(new Map()),
  types.isSet(new Set()),
  types.isDate(new Date()),
  types.isRegExp(/r/),
  types.isTypedArray(new Uint8Array(1)),
  types.isUint8Array(new Uint8Array(1)),
  types.isUint32Array(new Uint32Array(1)),
  types.isDataView(new DataView(new ArrayBuffer(2))),
  types.isArrayBuffer(new ArrayBuffer(1)),
  types.isNativeError(new TypeError()),
  types.isNativeError(new Error()),
  types.isNativeError({}),
  types.isAsyncFunction(async () => {}),
  types.isGeneratorFunction(function* () {}),
  types.isPromise(new Map()),
  types.isWeakSet(new WeakSet()),
  types.isNumberObject(new Number(1)),
  types.isBoxedPrimitive(new Boolean(true)),
  types.isArgumentsObject((function () { return arguments; })()),
);
try { types.isUint8Array(42) === false; console.log("num-ok"); } catch (e) { console.log("num-throw"); }
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("same true function"), "out: {out}");
    assert!(
        out.contains("true true true true true true true true true true true true false true true false true true true true"),
        "out: {out}"
    );
    assert!(out.contains("num-ok"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9a-3：node:querystring / node:punycode / node:string_decoder ─────
