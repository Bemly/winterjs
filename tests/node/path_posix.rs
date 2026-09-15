//! tests/node/path_posix.rs — 对齐 src/builtins/node/path_posix.rs（node:path/posix 子路径面（兼 win32/timers/assert_strict/dns_promises，单测））。

use crate::helpers::*;

#[test]
fn phase9m_subpath_timers_surface() {
    // 正常：path/posix-win32 双命名空间 + 回调 timers（触发/取消/immediate/
    // promises 重导出）+ assert/strict + dns/promises；
    // 报错：未知子路径报可用列表；边界：Timeout unref 链式。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import posix, { join as pjoin } from "node:path/posix";
import win32 from "node:path/win32";
import { setTimeout as st, setImmediate as si, clearTimeout as ct, promises as tp } from "node:timers";
import { strict } from "node:assert";
import assetStrict, { ok as sok, equal as sequal } from "node:assert/strict";
import dnsPromises, { lookup as dnsLookup } from "node:dns/promises";
console.log("posix", posix.sep, pjoin("a", "b"), posix === (await import("node:path")).posix);
console.log("win32", win32.sep, win32.join("C:\\a", "b"));
console.log("timers-prom", typeof tp.setTimeout);
console.log("strict", typeof strict.ok, strict === assetStrict, sequal(1, 1) === undefined, sok(true) === undefined);
console.log("dns-prom", typeof dnsPromises.lookup, typeof dnsLookup, !!(await dnsPromises.lookup("localhost"))?.address);
const t = st(() => console.log("NO-FIRE"), 50);
// 10f timers 对拍翻转（真机口径）：unref 后 hasRef 为 false，ref 恢复 true
// （旧断言编码的是 no-op unref 的伪语义）。
console.log("timeout-obj", typeof t.unref === "function" && t.unref() === t && t.hasRef() === false && t.ref().hasRef() === true);
ct(t);
si(() => console.log("immediate-ok"));
try { await import("node:path/nope"); console.log("NO-ERR"); }
catch (e) { console.log("subpath-err", String(e.message).includes("is not a builtin")); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "posix / a/b true",
        "win32 \\ C:\\a\\b",
        "timers-prom function",
        "strict function true true true",
        "dns-prom function function true",
        "timeout-obj true",
        "immediate-ok",
        "subpath-err true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    assert!(!out.contains("NO-FIRE"), "cancelled timer fired:\n{out}");
    dir.close().unwrap();
}
