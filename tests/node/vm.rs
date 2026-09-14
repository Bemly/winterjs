//! tests/node/vm.rs — 对齐 src/builtins/node/vm.rs（node:vm）。

use crate::helpers::*;

#[test]
fn phase9f_vm_context_spawns_and_isolates() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import vm from "node:vm";
const sb = { a: 5 };
const r = vm.runInNewContext("b = a + 1; b", sb);
console.log("v-run", r === 6, sb.b === 6, typeof b === "undefined");
const c1 = vm.createContext({ x: 1 });
const c2 = vm.createContext({ x: 2 });
console.log("v-ctx", vm.isContext(c1), vm.isContext(c2), vm.isContext({}));
vm.runInContext("y = x * 10", c1);
vm.runInContext("y = x * 10", c2);
console.log("v-iso", c1.y === 10, c2.y === 20);
const s = new vm.Script("40 + 2");
console.log("v-script", s.runInNewContext() === 42, s.runInThisContext() === 42);
const f = vm.compileFunction("return a + b", ["a", "b"]);
console.log("v-cf", f(20, 22) === 42);
const o = vm.runInNewContext("({ z: 7 })", {});
console.log("v-ccw", o.z === 7, typeof o === "object");
const sb2 = {};
vm.runInNewContext("Promise.resolve(1).then(v => { globalThis.px = v; })", sb2);
console.log("v-micro", sb2.px === 1);
console.log("v-std", vm.runInNewContext("typeof Object") === "function", vm.runInNewContext("typeof console") === "undefined");
console.log("v-const", typeof vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER, typeof vm.constants.DONT_CONTEXTIFY);
console.log("v-timeout", vm.runInNewContext("1 + 1", {}, { timeout: 100 }) === 2);
const mm = await vm.measureMemory().then(() => "no", (e) => e.code);
console.log("v-mm", mm === "ERR_CONTEXT_NOT_INITIALIZED");
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("v-run true true true"), "out: {out}");
    assert!(out.contains("v-ctx true true false"), "out: {out}");
    assert!(out.contains("v-iso true true"), "out: {out}");
    assert!(out.contains("v-script true true"), "out: {out}");
    assert!(out.contains("v-cf true"), "out: {out}");
    assert!(out.contains("v-ccw true true"), "out: {out}");
    assert!(out.contains("v-micro true"), "out: {out}");
    assert!(out.contains("v-std true true"), "out: {out}");
    assert!(out.contains("v-const symbol symbol"), "out: {out}");
    assert!(out.contains("v-timeout true"), "out: {out}");
    assert!(out.contains("v-mm true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_vm_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import vm from "node:vm";
try { new vm.Script("}{"); } catch (e) { console.log("w-ctor", e.constructor.name === "SyntaxError"); }
try { vm.compileFunction("}{"); } catch (e) { console.log("w-cf", e.constructor.name === "SyntaxError"); }
try { vm.runInNewContext("throw new RangeError('nope')"); } catch (e) { console.log("w-range", e.constructor.name === "RangeError", e.message === "nope"); }
try { vm.runInNewContext("throw 'strval'"); } catch (e) { console.log("w-str", e.constructor.name === "Error", e.message.includes("strval")); }
try { vm.runInNewContext("noSuchVar + 1"); } catch (e) { console.log("w-ref", e.constructor.name === "ReferenceError"); }
try { vm.runInContext("1", {}); } catch (e) { console.log("w-badctx", e.code === "ERR_INVALID_ARG_TYPE"); }
try { vm.runInNewContext("1", 42); } catch (e) { console.log("w-badsb", e.code === "ERR_INVALID_ARG_TYPE"); }
try { vm.isContext(42); } catch (e) { console.log("w-isctx", e.code === "ERR_INVALID_ARG_TYPE"); }
try { vm.runInNewContext("1", {}, { microtaskMode: "nope" }); } catch (e) { console.log("w-mmode", e.code === "ERR_INVALID_ARG_VALUE"); }
try { vm.runInNewContext("1", {}, { timeout: -1 }); } catch (e) { console.log("w-timeout", e.code === "ERR_OUT_OF_RANGE"); }
const pc = vm.createContext({ q: 41 });
const f2 = vm.compileFunction("return q + 1", [], { parsingContext: pc });
console.log("w-pc", f2() === 42);
const ce = vm.compileFunction("return ex + 1", [], { contextExtensions: [{ ex: 41 }] });
console.log("w-ext", ce() === 42);
const cached = new vm.Script("9", { cachedData: Buffer.alloc(0), produceCachedData: true });
console.log("w-cache", cached.runInNewContext() === 9, cached.cachedDataProduced === false);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("w-ctor true"), "out: {out}");
    assert!(out.contains("w-cf true"), "out: {out}");
    assert!(out.contains("w-range true true"), "out: {out}");
    assert!(out.contains("w-str true true"), "out: {out}");
    assert!(out.contains("w-ref true"), "out: {out}");
    assert!(out.contains("w-badctx true"), "out: {out}");
    assert!(out.contains("w-badsb true"), "out: {out}");
    assert!(out.contains("w-isctx true"), "out: {out}");
    assert!(out.contains("w-mmode true"), "out: {out}");
    assert!(out.contains("w-timeout true"), "out: {out}");
    assert!(out.contains("w-pc true"), "out: {out}");
    assert!(out.contains("w-ext true"), "out: {out}");
    assert!(out.contains("w-cache true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9i_vm_source_module_chain() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import vm from "node:vm";
const m = new vm.SourceTextModule("export const a = 40 + 1;");
console.log("m9i-st0", m.status === "unlinked", m.identifier === "vm:module(0)", Array.isArray(m.dependencySpecifiers) && m.dependencySpecifiers.length === 0);
console.log("m9i-inst", m instanceof vm.SourceTextModule, m instanceof vm.Module);
await m.link(() => {});
console.log("m9i-st1", m.status === "linked");
const er = m.evaluate();
console.log("m9i-evret", er instanceof Promise);
await er;
console.log("m9i-st2", m.status === "evaluated", m.namespace.a === 41);
// 重复求值照真机成功（无操作）。
await m.evaluate();
console.log("m9i-reev", m.status === "evaluated");
// 上下文隔离：同名种子不同值。
const c1 = vm.createContext({ seed: 3 });
const c2 = vm.createContext({ seed: 4 });
const m1 = new vm.SourceTextModule("export const v = seed * 2;", { context: c1, identifier: "m1" });
const m2 = new vm.SourceTextModule("export const v = seed * 2;", { context: c2, identifier: "m2" });
await m1.link(() => {});
await m2.link(() => {});
await m1.evaluate();
await m2.evaluate();
console.log("m9i-iso", m1.namespace.v === 6, m2.namespace.v === 8, m1.identifier === "m1", m1.context === c1);
// 顶层 await 模块（异步求值认领路径）。
const t = new vm.SourceTextModule("export const v = await Promise.resolve(41);");
await t.link(() => {});
await t.evaluate();
console.log("m9i-tla", t.status === "evaluated", t.namespace.v === 41);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "m9i-st0 true true true",
        "m9i-inst true true",
        "m9i-st1 true",
        "m9i-evret true",
        "m9i-st2 true true",
        "m9i-reev true",
        "m9i-iso true true true true",
        "m9i-tla true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_vm_module_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import vm from "node:vm";
const t = async (n, f) => { try { const r = await f(); console.log(n, "OK", r === undefined ? "undef" : "val"); } catch (e) { console.log(n, "THROW", e.code || "(nocode)"); } };
await t("m9iB-syntax", async () => new vm.SourceTextModule("export const q = ;"));
await t("m9iB-nonstr", async () => new vm.SourceTextModule(123));
await t("m9iB-badctx", async () => new vm.SourceTextModule("export const a = 1;", { context: {} }));
const m = new vm.SourceTextModule("export const a = 1;");
await t("m9iB-linknofn", async () => m.link());
await t("m9iB-nsearly", async () => m.namespace);
await t("m9iB-evunlinked", async () => m.evaluate());
await m.link(() => {});
await t("m9iB-relink", async () => m.link(() => {}));
await t("m9iB-errearly", async () => m.error);
const e = new vm.SourceTextModule("throw new Error('boom');");
await e.link(() => {});
await t("m9iB-evthrow", async () => e.evaluate());
console.log("m9iB-est", e.status === "errored", e.error && e.error.message === "boom");
const im = new vm.SourceTextModule("import {x} from './nope.js'; export const a = x;");
console.log("m9iB-deps", JSON.stringify(im.dependencySpecifiers) === JSON.stringify(["./nope.js"]));
await t("m9iB-linkimports", async () => im.link(() => {}));
const s = new vm.SyntheticModule(["x"], function () { this.setExport("x", 42); });
console.log("m9iB-syn0", s.status === "linked", s.dependencySpecifiers === undefined);
await s.link();
await s.evaluate();
console.log("m9iB-syn1", s.status === "evaluated", s.namespace.x === 42);
await t("m9iB-synset", async () => s.setExport("x", 1));
const se = new vm.SyntheticModule(["d"], function () { throw new Error("cbboom"); });
await se.link(() => {});
await t("m9iB-syncb", async () => se.evaluate());
console.log("m9iB-synest", se.status === "errored");
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("m9iB-syntax THROW"), "out: {out}");
    assert!(out.contains("m9iB-nonstr THROW ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("m9iB-badctx THROW ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("m9iB-linknofn THROW ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("m9iB-nsearly THROW ERR_VM_MODULE_STATUS"), "out: {out}");
    assert!(out.contains("m9iB-evunlinked THROW ERR_VM_MODULE_STATUS"), "out: {out}");
    assert!(out.contains("m9iB-relink THROW ERR_VM_MODULE_STATUS"), "out: {out}");
    assert!(out.contains("m9iB-errearly THROW ERR_VM_MODULE_STATUS"), "out: {out}");
    assert!(out.contains("m9iB-evthrow THROW"), "out: {out}");
    assert!(out.contains("m9iB-est true true"), "out: {out}");
    assert!(out.contains("m9iB-deps true"), "out: {out}");
    assert!(out.contains("m9iB-linkimports THROW"), "out: {out}");
    assert!(out.contains("m9iB-syn0 true true"), "out: {out}");
    assert!(out.contains("m9iB-syn1 true true"), "out: {out}");
    assert!(out.contains("m9iB-synset THROW ERR_VM_MODULE_STATUS"), "out: {out}");
    assert!(out.contains("m9iB-syncb THROW"), "out: {out}");
    assert!(out.contains("m9iB-synest true"), "out: {out}");
    dir.close().unwrap();
}
