//! tests/node/inspector.rs — 对齐 src/builtins/node/inspector.rs（node:inspector）。

use crate::helpers::*;

#[test]
fn phase9e_inspector_session() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import ins, { Session, open, close, url, waitForDebugger } from "node:inspector";
console.log("shape", typeof open === "function" && typeof close() === "undefined" && url() === undefined && waitForDebugger() === undefined);
const s = new Session();
s.connect();
const post = (m, p) => new Promise((res, rej) => s.post(m, p, (e, r) => (e ? rej(e) : res(r))));
const r1 = await post("Runtime.evaluate", { expression: "40 + 2" });
console.log("eval", r1.result.type === "number" && r1.result.value === 42);
const r2 = await post("Runtime.evaluate", { expression: "({a: [1,2]})" });
console.log("obj", r2.result.value.a.join(",") === "1,2");
const r3 = await post("Runtime.evaluate", { expression: "throw new Error('boom')" });
console.log("exc", r3.exceptionDetails.exception.description === "boom");
const r4 = await post("Debugger.enable", {});
console.log("ack", JSON.stringify(r4) === "{}");
try {
  await post("Nope.nope", {});
} catch (e) { console.log("unk", e.code === "ERR_NOT_SUPPORTED"); }
const r5 = await post("Runtime.evaluate", { expression: "1+1" });
console.log("promise", r5.result.value === 2);
s.disconnect();
console.log("done");
"#,
    );
    assert!(out.contains("shape true"), "out: {out}");
    assert!(out.contains("eval true"), "out: {out}");
    assert!(out.contains("obj true"), "out: {out}");
    assert!(out.contains("exc true"), "out: {out}");
    assert!(out.contains("ack true"), "out: {out}");
    assert!(out.contains("unk true"), "out: {out}");
    assert!(out.contains("promise true"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    dir.close().unwrap();
}
