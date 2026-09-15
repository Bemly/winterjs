//! tests/node/v8.rs — 对齐 src/builtins/node/v8.rs（node:v8（兼 readline，单测））。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9j_v8_readline_surface() {
    // v8：startupSnapshot 守卫（vite try 内调用）；readline：10c-2 起全面实现，
    // 旧宽松口径（question 抛未实现/非法入参带码）已退役，此处按新语义断言。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("vr.mjs");
    file.write_str(
        r#"
import v8, { startupSnapshot } from "node:v8";
console.log("v8snap", startupSnapshot.isBuildingSnapshot() === false, v8.startupSnapshot === startupSnapshot);
import rl, { createInterface, cursorTo, clearScreenDown, emitKeypressEvents } from "node:readline";
import { PassThrough } from "node:stream";
try { createInterface({ input: null, output: null }); } catch (e) { console.log("rl-null", e.constructor.name, e.code); }
const itf = createInterface({ input: new PassThrough(), output: new PassThrough() });
let closed = false;
itf.on("close", () => { closed = true; });
itf.setPrompt("> ");
console.log("rl-open", itf.getPrompt() === "> " && itf.closed === false);
itf.close();
console.log("rl-close", closed && itf.closed);
console.log("rl-cursor", cursorTo(null, 0, 0) === false && clearScreenDown(null) === false);
try { emitKeypressEvents(null); } catch (e) { console.log("rl-emitnull", e.code); }
itf.question("q?", () => {});
console.log("rl-q", "no-throw");
try { createInterface(42); } catch (e) { console.log("rl-bad", e.constructor.name, e.code); }
console.log("rl-def", typeof rl.createInterface === "function");
"#,
    )
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "v8snap true true",
        "rl-null TypeError undefined",
        "rl-open true",
        "rl-close true",
        "rl-cursor true",
        "rl-emitnull ERR_INVALID_ARG_TYPE",
        "rl-q no-throw",
        "rl-bad TypeError undefined",
        "rl-def true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
