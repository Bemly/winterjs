//! tests/node/timers_promises.rs — 对齐 src/builtins/node/timers_promises.rs（node:timers/promises）。

use crate::helpers::*;

#[test]
fn phase9b_timers_promises_surface() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import * as tp from "node:timers/promises";
import { setTimeout as sleep, setImmediate as simm, setInterval as sint, scheduler } from "node:timers/promises";
// setTimeout：值透传 + 计时
const t0 = Date.now();
console.log("sleep", (await sleep(30, "v")) === "v", Date.now() - t0 >= 25);
console.log("imm", await simm("i"));
// setInterval：AsyncIterator 形态
const it = sint(10, "k");
const first = await it.next();
const t1 = Date.now();
const second = await it.next();
console.log("interval", first.value, first.done === false, second.value, Date.now() - t1 >= 8);
// scheduler
console.log("yield", await scheduler.yield("y"), await scheduler.wait(5) === undefined);
// race
console.log("race", await Promise.race([sleep(60, "slow"), sleep(5, "fast")]));
// 中止：setTimeout / scheduler.wait → AbortError
const ac = new AbortController();
const aborted = sleep(1000, "x", { signal: ac.signal });
aborted.catch((e) => console.log("abort", e.name));
ac.abort();
const ac2 = new AbortController();
const wabort = scheduler.wait(1000, { signal: ac2.signal });
wabort.catch((e) => console.log("w-abort", e.name));
ac2.abort();
// namespace 导出面（10f 对拍：去 default 后 require 走 namespace 回落，
// 与 node:timers 的 .promises 同一对象——真机 require(esm) 返回 namespace）。
console.log("default", typeof tp.setTimeout === "function", typeof tp.scheduler === "object",
  typeof tp.setInterval === "function");
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("sleep true true"), "out: {out}");
    assert!(out.contains("imm i"), "out: {out}");
    assert!(out.contains("interval k true k true"), "out: {out}");
    assert!(out.contains("yield y true"), "out: {out}");
    assert!(out.contains("race fast"), "out: {out}");
    assert!(out.contains("abort AbortError"), "out: {out}");
    assert!(out.contains("w-abort AbortError"), "out: {out}");
    assert!(out.contains("default true true true"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9c-1：fs 同步面增补（link 系/时间戳/权限/access/fd 系/cp/opendir）──

#[test]
fn phase10f_timers_promises_namespace_scheduler() {
    // 10f 对拍：CJS 双取同一对象（真机 `require(tp) === require(timers).promises`；
    // ESM namespace 恒带 default 键、与对象不等，真机同——黑盒按真机比法；
    // R2-iter 翻转：旧断言 `import * === .promises` 超真机严格，4.65）。
    // scheduler 不可 new（ERR_ILLEGAL_CONSTRUCTOR）+ this 校验（ERR_INVALID_THIS）
    // + 已中止信号同步拒绝（套件 test-timers-promises-scheduler）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import timers from "node:timers";
import tpDefault from "node:timers/promises";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
console.log("ident", tpDefault === timers.promises && require("node:timers/promises") === require("node:timers").promises);
try { new (tpDefault.scheduler.constructor)(); } catch (e) { console.log("ctor", e.code); }
try { tpDefault.scheduler.yield.call({}); } catch (e) { console.log("this", e.code); }
try { await tpDefault.scheduler.wait(10000, { signal: AbortSignal.abort() }); }
catch (e) { console.log("preabort", e.code, e.message); }
const ac = new AbortController();
const w = tpDefault.scheduler.wait(10000, { signal: ac.signal });
ac.abort();
try { await w; } catch (e) { console.log("postabort", e.code); }
"#,
    );
    for line in [
        "ident true",
        "ctor ERR_ILLEGAL_CONSTRUCTOR",
        "this ERR_INVALID_THIS",
        "preabort ABORT_ERR The operation was aborted",
        "postabort ABORT_ERR",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
