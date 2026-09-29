//! tests/node/async_hooks.rs — 对齐 src/builtins/node/async_hooks.rs（node:async_hooks）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9a_async_hooks_als_and_async_resource() {
    // test-async-local-storage* 子集（同步链路）+ AsyncResource runInAsyncScope
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("a.mjs");
    file.write_str(
        r#"import { AsyncLocalStorage, AsyncResource, createHook, executionAsyncId } from "node:async_hooks";
const als = new AsyncLocalStorage();
als.run({ id: 42 }, () => {
  console.log("store", als.getStore().id);
  const res = new AsyncResource("TEST");
  res.runInAsyncScope(() => console.log("in-res", als.getStore().id, executionAsyncId() > 1));
  console.log("bind", als.bind(() => als.getStore()?.id ?? "none")(), als.getStore()?.id ?? "none");
});
console.log("outside", als.getStore());
// snapshot
const snap = als.run({ s: 1 }, () => als.snapshot());
snap(() => console.log("snapshot", als.getStore()?.s));
// AsyncResource.bind 静态 + emitDestroy
const bound = AsyncResource.bind(() => executionAsyncId() > 1, "BOUND");
console.log("static-bind", bound(), AsyncResource.AsyncResource === AsyncResource);
const hook = createHook({ init() {} }).enable();
console.log("hook", typeof hook.disable);
"#,
    )
    .unwrap();
    let out = winterjs2()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.contains("store 42") && out.contains("in-res 42 true"),
        "out: {out}"
    );
    assert!(
        out.contains("bind 42 42") && out.contains("outside undefined"),
        "out: {out}"
    );
    assert!(
        out.contains("snapshot 1") && out.contains("static-bind true true"),
        "out: {out}"
    );
    assert!(out.contains("hook function"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_async_hooks_stub_and_validation_boundary() {
    // stub 口径边界：createHook 非法回调 → ERR_ASYNC_CALLBACK；ALS 非法 callback → TypeError
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("b.mjs");
    file.write_str(
        r#"import { createHook, AsyncResource, AsyncLocalStorage } from "node:async_hooks";
try { createHook({ init: 1 }); } catch (e) { console.log("h", e.code); }
try { new AsyncResource(42); } catch (e) { console.log("t", e.code, e.message.includes("must be of type string")); }
try { new AsyncLocalStorage().run({}, "nope"); } catch (e) { console.log("r", e instanceof TypeError); }
try { new AsyncResource("X", { triggerAsyncId: "no" }); } catch (e) { console.log("o", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs2()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("h ERR_ASYNC_CALLBACK"), "out: {out}");
    assert!(out.contains("t ERR_INVALID_ARG_TYPE true"), "out: {out}");
    assert!(
        out.contains("r true") && out.contains("o ERR_INVALID_ARG_TYPE"),
        "out: {out}"
    );
    dir.close().unwrap();
}

// ── Phase 9a-2：node:util / node:util/types ────────────────────────────────
