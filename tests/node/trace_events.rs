//! tests/node/trace_events.rs — 对齐 src/builtins/node/trace_events.rs（node:trace_events）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9a_trace_events_categories() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        r#"import { createTracing, getEnabledCategories } from "node:trace_events";
const t = createTracing({ categories: ["node", "v8"] });
console.log("idle", t.enabled, t.categories, JSON.stringify(getEnabledCategories()));
t.enable();
console.log("on", t.enabled, getEnabledCategories());
t.disable();
console.log("off", t.enabled, JSON.stringify(getEnabledCategories()));
const t2 = createTracing({ categories: ["metro"] });
t2.enable();
console.log("multi", getEnabledCategories());
try { createTracing({ categories: [] }); } catch (e) { console.log("e1", e.code); }
try { createTracing({}); } catch (e) { console.log("e2", e.code); }
try { createTracing({ categories: [42] }); } catch (e) { console.log("e3", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("idle false node,v8 \"\""), "out: {out}");
    assert!(out.contains("on true node,v8"), "out: {out}");
    assert!(out.contains("off false \"\""), "out: {out}");
    assert!(out.contains("multi metro"), "out: {out}");
    assert!(out.contains("e1 ERR_TRACE_EVENTS_CATEGORY_REQUIRED"), "out: {out}");
    assert!(out.contains("e2 ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("e3 ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}
