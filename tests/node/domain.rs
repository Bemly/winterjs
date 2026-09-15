//! tests/node/domain.rs — 对齐 src/builtins/node/domain.rs（node:domain，10e）。

use crate::helpers::*;

#[test]
fn phase10e_domain_thin() {
    let dir = assert_fs::TempDir::new().unwrap();
    const_probe(&dir);
    dir.close().unwrap();
}

/// 薄面：同步路由 + bind/intercept + enter/exit/active + add/remove（hermetic）。
fn const_probe(dir: &assert_fs::TempDir) {
    let out = run_fs_file(
        dir,
        "p.mjs",
        r#"
import domain, { create, createDomain, Domain, active } from "node:domain";
import { EventEmitter } from "node:events";
console.log("surface", typeof Domain, typeof create, typeof createDomain, active);
const d = create();
console.log("api", ["run", "bind", "intercept", "add", "remove", "enter", "exit"].map((m) => typeof d[m]).join(","));
// run 同步抛错路由
d.on("error", (e) => console.log("run-caught", e.message));
d.run(() => { throw new Error("sync-boom"); });
console.log("run-alive", active === null);
// 返回值透传
console.log("run-ret", d.run(() => 42));
// bind 同步路由
const b = d.bind(() => { throw new Error("bind-boom"); });
b();
console.log("bind-alive", active === null);
// intercept：首参错路由 / 正常透传
const it = d.intercept((x) => x * 2);
it(new Error("cb-boom"));
console.log("intercept-err-routed", true);
console.log("intercept-ok", it(null, 21));
// enter/exit/active 栈
const d2 = createDomain();
console.log("ctor", d2 instanceof Domain, d2 instanceof EventEmitter);
d.enter();
console.log("active-in", domain.active === d);
d2.enter();
console.log("active-nest", domain.active === d2);
d2.exit();
console.log("active-back", domain.active === d);
d.exit();
console.log("active-out", domain.active === null, active === null);
// add/remove：emitter error 路由与摘除
const em = new EventEmitter();
d.on("error", () => {});
d.add(em);
em.emit("error", new Error("em-boom"));
console.log("add-routed", true);
d.remove(em);
let escaped = false;
d.on("error", () => { escaped = true; });
em.on("error", () => console.log("em-local"));
em.emit("error", new Error("after-remove"));
console.log("remove-ok", d.members.length === 0, escaped === false);
"#,
    );
    assert!(out.contains("surface function function function null"), "out: {out}");
    assert!(out.contains("api function,function,function,function,function,function,function"), "out: {out}");
    assert!(out.contains("run-caught sync-boom"), "out: {out}");
    assert!(out.contains("run-alive true"), "out: {out}");
    assert!(out.contains("run-ret 42"), "out: {out}");
    assert!(out.contains("run-caught bind-boom"), "out: {out}");
    assert!(out.contains("bind-alive true"), "out: {out}");
    assert!(out.contains("run-caught cb-boom"), "out: {out}");
    assert!(out.contains("intercept-err-routed true"), "out: {out}");
    assert!(out.contains("intercept-ok 42"), "out: {out}");
    assert!(out.contains("ctor true true"), "out: {out}");
    assert!(out.contains("active-in true"), "out: {out}");
    assert!(out.contains("active-nest true"), "out: {out}");
    assert!(out.contains("active-back true"), "out: {out}");
    assert!(out.contains("active-out true true"), "out: {out}");
    assert!(out.contains("run-caught em-boom"), "out: {out}");
    assert!(out.contains("add-routed true"), "out: {out}");
    assert!(out.contains("em-local"), "out: {out}");
    assert!(out.contains("remove-ok true true"), "out: {out}");
}
