//! tests/node/perf_hooks.rs — 对齐 src/builtins/node/perf_hooks.rs（node:perf_hooks）。

use crate::helpers::*;

#[test]
fn phase9e_perf_hooks_surface() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import p, { performance, PerformanceObserver, createHistogram, monitorEventLoopDelay, timerify, constants } from "node:perf_hooks";
console.log("now", typeof performance.now() === "number" && performance.now() >= 0, performance.timeOrigin > 0, performance instanceof p.Performance);
performance.mark("a"); performance.mark("b");
const m = performance.measure("m1", "a", "b");
console.log("measure", m.name === "m1" && m.entryType === "measure" && m.duration >= 0);
console.log("entries", performance.getEntriesByName("a").length === 1, performance.getEntriesByType("mark").length === 2);
const got = [];
const o = new PerformanceObserver((l) => { for (const e of l.getEntries()) got.push(e.name + ":" + e.entryType); });
o.observe({ entryTypes: ["mark"] });
performance.mark("c");
setTimeout(() => {
  console.log("obs", JSON.stringify(got) === JSON.stringify(["c:mark"]));
  o.disconnect();
  const h = createHistogram(); h.record(10); h.record(20);
  console.log("hist", h.count === 2 && h.min === 10 && h.max === 20 && h.mean === 15 && h.percentile(50) === 10);
  const e = createHistogram();
  console.log("empty", e.min === 9223372036854776000, e.max === 0, e.count === 0);
  const u = performance.eventLoopUtilization();
  console.log("elu", typeof u.active === "number" && u.utilization === 1);
  const f = timerify((x) => x * 2);
  console.log("timerify", f(21) === 42, performance.getEntriesByType("function").length === 1);
  console.log("const", constants.NODE_PERFORMANCE_GC_MAJOR === 4 && constants.NODE_PERFORMANCE_GC_FLAGS_NO === 0);
  const mel = monitorEventLoopDelay({ resolution: 10 });
  mel.enable();
  setTimeout(() => { mel.disable(); console.log("mel", mel.count >= 0, mel.min >= 0); }, 60);
}, 20);
"#,
    );
    assert!(out.contains("now true true true"), "out: {out}");
    assert!(out.contains("measure true"), "out: {out}");
    assert!(out.contains("entries true true"), "out: {out}");
    assert!(out.contains("obs true"), "out: {out}");
    assert!(out.contains("hist true"), "out: {out}");
    assert!(out.contains("empty true true true"), "out: {out}");
    assert!(out.contains("elu true"), "out: {out}");
    assert!(out.contains("timerify true true"), "out: {out}");
    assert!(out.contains("const true"), "out: {out}");
    assert!(out.contains("mel true true"), "out: {out}");
    dir.close().unwrap();
}
