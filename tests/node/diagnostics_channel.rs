//! tests/node/diagnostics_channel.rs — 对齐 src/builtins/node/diagnostics_channel.rs（node:diagnostics_channel）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9a_diagnostics_channel_surface() {
    // test-diagnostics-channel.js 命名子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("d.mjs");
    file.write_str(
        r#"import dc, { channel, hasSubscribers, Channel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";
console.log("idle", hasSubscribers("t-ch"), channel("t-ch").hasSubscribers);
const seen = [];
const listener = (msg, name) => seen.push([name, msg]);
dc.subscribe("t-ch", listener);
console.log("active", hasSubscribers("t-ch"), channel("t-ch") instanceof Channel);
channel("t-ch").publish({ n: 1 });
console.log("got", JSON.stringify(seen));
console.log("unsub-wrong", dc.unsubscribe("t-ch", () => {}), dc.unsubscribe("t-ch", listener), hasSubscribers("t-ch"));
// bindStore + runStores
const als = new AsyncLocalStorage();
const ch = channel("s-ch");
ch.bindStore(als, (d) => ({ w: d }));
ch.runStores(7, () => console.log("store", als.getStore()?.w));
console.log("outside", als.getStore());
console.log("unbound", ch.unbindStore(als), ch.hasSubscribers);
// TracingChannel 全窗口
const { tracingChannel } = dc;
const tc = tracingChannel("tr-x");
const ev = [];
tc.subscribe({ start: () => ev.push("s"), end: (c) => ev.push(`e:${c.result}`), error: () => ev.push("err") });
console.log("sync", tc.traceSync(() => "R"), ev.join(","));
tc.tracePromise(async () => "P").then((v) => console.log("promise", v));
// 边界
try { channel(42); } catch (e) { console.log("e1", e.code); }
try { dc.subscribe("t-ch2", "nope"); } catch (e) { console.log("e2", e.message.includes("must be of type function")); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("idle false false") && out.contains("active true true"), "out: {out}");
    assert!(out.contains(r#"got [["t-ch",{"n":1}]]"#), "out: {out}");
    assert!(out.contains("unsub-wrong false true false"), "out: {out}");
    assert!(out.contains("store 7") && out.contains("outside undefined"), "out: {out}");
    assert!(out.contains("unbound true false"), "out: {out}");
    assert!(out.contains("sync R s,e:R"), "out: {out}");
    assert!(out.contains("promise P"), "out: {out}");
    assert!(out.contains("e1 ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("e2 true"), "out: {out}");
    dir.close().unwrap();
}
