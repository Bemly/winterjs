//! napi 黑盒测试（对齐 src/napi/；plan-napi M0）。
//! fixture 现场编译（cc -dynamiclib + `-undefined dynamic_lookup`，include
//! vendored 头；hermetic，零网络）。三件：正常回环 / dlsym 自检 / 沙箱与报错。

mod common;

use common::*;

use assert_fs::prelude::*;

/// 从 tests/fixtures/napi/ 读 C 源现场编译（与手工探针同源，防两处漂移）。
#[cfg(unix)]
fn build_fixture_dylib(dir: &assert_fs::TempDir, name: &str) -> std::path::PathBuf {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/napi/{name}.c")),
    )
    .expect("fixture source exists");
    build_napi_dylib(dir, name, &src)
}


/// 现场编 `.node` fixture（bun:ffi `build_ffi_dylib` 同款 shell-out；napi 需
/// `-undefined dynamic_lookup`——addon 符号由宿主运行期解析，链接期不可见）。
#[cfg(unix)]
fn build_napi_dylib(dir: &assert_fs::TempDir, name: &str, c_src: &str) -> std::path::PathBuf {
    let c = dir.child(format!("{name}.c"));
    c.write_str(c_src).unwrap();
    let out = dir.child(format!("{name}.node"));
    let mut cmd = std::process::Command::new("cc");
    if cfg!(target_os = "macos") {
        cmd.arg("-dynamiclib");
    } else {
        cmd.args(["-shared", "-fPIC"]);
    }
    cmd.arg("-undefined").arg("dynamic_lookup");
    // Node-API 10 面（node_api_* 错误族等；rolldown/napi-rs 3 同款口径）
    cmd.arg("-DNAPI_VERSION=10");
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    cmd.arg("-I").arg(manifest.join("src/napi/include"));
    let status = cmd
        .arg("-o")
        .arg(out.path())
        .arg(c.path())
        .status()
        .expect("cc runs");
    assert!(status.success(), "cc failed for {name}");
    out.path().to_path_buf()
}

// 与 tests/fixtures/napi/hello.c 同源（黑盒自包含；文件版供手工探针）。
#[cfg(unix)]
const HELLO_C: &str = r#"
#include <node_api.h>
static napi_value Hello(napi_env env, napi_callback_info info) {
  (void)env; (void)info;
  napi_value out;
  if (napi_create_int32(env, 42, &out) != napi_ok) return NULL;
  return out;
}
static napi_value Add(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  if (napi_get_cb_info(env, info, &argc, argv, NULL, NULL) != napi_ok) return NULL;
  double a = 0, b = 0;
  if (argc == 2 &&
      napi_get_value_double(env, argv[0], &a) == napi_ok &&
      napi_get_value_double(env, argv[1], &b) == napi_ok) {
    napi_value out;
    if (napi_create_double(env, a + b, &out) == napi_ok) return out;
  }
  napi_throw_error(env, NULL, "add needs two numbers");
  return NULL;
}
static napi_value Init(napi_env env, napi_value exports) {
  napi_value hello, add, version;
  napi_create_function(env, "hello", NAPI_AUTO_LENGTH, Hello, NULL, &hello);
  napi_create_function(env, "add", NAPI_AUTO_LENGTH, Add, NULL, &add);
  napi_create_string_utf8(env, "m0-ok", NAPI_AUTO_LENGTH, &version);
  napi_set_named_property(env, exports, "hello", hello);
  napi_set_named_property(env, exports, "add", add);
  napi_set_named_property(env, exports, "version", version);
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
"#;

#[test]
#[cfg(unix)]
fn phase_napi_hello_loopback() {
    // 正常：require → hello()/add()/version 三面 + 二次 require 同对象（幂等）。
    let dir = assert_fs::TempDir::new().unwrap();
    let node = build_napi_dylib(&dir, "hello", HELLO_C);
    let app = dir.child("app.js");
    app.write_str(&format!(
        r#"
const m = require({node:?});
const m2 = require({node:?});
console.log("hello", m.hello(), "add", m.add(19, 23), "ver", m.version);
console.log("same", m === m2);
console.log("typefn", typeof m.hello);
"#
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("hello 42 add 42 ver m0-ok"), "stdout: {so}");
    assert!(so.contains("same true"), "stdout: {so}");
    assert!(so.contains("typefn function"), "stdout: {so}");
    dir.close().unwrap();
}

// dlsym 自检（rolldown 查表机制同款）：宿主进程全局符号表必须能解析
// 宿主实现的 napi_* + uv_run——build.rs 导出清单的直接验证。
#[cfg(unix)]
const DLSYM_C: &str = r#"
#include <node_api.h>
#include <dlfcn.h>
static napi_value Check(napi_env env, napi_callback_info info) {
  (void)env; (void)info;
  void *h = dlopen(NULL, RTLD_NOW);
  const char *names[] = {
    "napi_create_function", "napi_set_named_property", "napi_create_string_utf8",
    "napi_create_int32", "napi_create_double", "napi_get_cb_info",
    "napi_get_value_double", "napi_get_version", "napi_module_register",
    "napi_get_last_error_info", "uv_run",
  };
  int ok = 1;
  for (unsigned long i = 0; i < sizeof(names) / sizeof(names[0]); i++) {
    if (!dlsym(h, names[i])) { ok = 0; break; }
  }
  napi_value out;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value Init(napi_env env, napi_value exports) {
  napi_value fn;
  napi_create_function(env, "check", NAPI_AUTO_LENGTH, Check, NULL, &fn);
  napi_set_named_property(env, exports, "check", fn);
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
"#;

#[test]
#[cfg(unix)]
fn phase_napi_dlsym_selfcheck() {
    let dir = assert_fs::TempDir::new().unwrap();
    let node = build_napi_dylib(&dir, "dlsym", DLSYM_C);
    let app = dir.child("app.js");
    app.write_str(&format!(
        r#"
const m = require({node:?});
console.log("dlsym", m.check());
"#
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("dlsym 1"), "stdout: {so}");
    dir.close().unwrap();
}

#[test]
#[cfg(unix)]
fn phase_napi_permission_sandbox() {
    // 沙箱开启（--allow-read）未授 --allow-ffi → PermissionError 可读可 catch；
    // 加 --allow-ffi 即放行（用户拍板：复用 --allow-ffi 类别）。
    let dir = assert_fs::TempDir::new().unwrap();
    let node = build_napi_dylib(&dir, "hello", HELLO_C);
    let app = dir.child("app.js");
    app.write_str(&format!(
        r#"
try {{
  require({node:?});
  console.log("loaded");
}} catch (e) {{
  console.log("denied", String(e.message).slice(0, 40));
}}
"#
    ))
    .unwrap();
    // 沙箱开、ffi 未授 → 拒。
    let out = winterjs()
        .args(["--run", app.path().to_string_lossy().as_ref(), "--allow-read"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("denied PermissionError"), "stdout: {so}");
    // ffi 授予 → 通。
    let out = winterjs()
        .args([
            "--run",
            app.path().to_string_lossy().as_ref(),
            "--allow-read",
            "--allow-ffi",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("loaded"), "stdout: {so}");
    dir.close().unwrap();
}

#[test]
#[cfg(unix)]
fn phase_napi_add_error_path() {
    // 报错：add 非数实参 → addon throw_error → JS 侧可 catch（trampoline
    // pending-exception 传播面）。
    let dir = assert_fs::TempDir::new().unwrap();
    let node = build_napi_dylib(&dir, "hello", HELLO_C);
    let app = dir.child("app.js");
    app.write_str(&format!(
        r#"
const m = require({node:?});
try {{
  m.add("x", 1);
  console.log("no-throw");
}} catch (e) {{
  console.log("caught", String(e.message).slice(0, 30));
}}
"#
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(
        so.contains("caught add needs two numbers"),
        "stdout: {so}"
    );
    dir.close().unwrap();
}

#[test]
#[cfg(unix)]
fn phase_napi_m1_values_matrix() {
    // 正常：M1 值系统全矩阵（roundtrip/typeof/strict_equals/instanceof/is_error/
    // coerce/pending-exception）——fixture 内部按位断言，1 = 全过。
    let dir = assert_fs::TempDir::new().unwrap();
    let node = build_fixture_dylib(&dir, "m1_values");
    let app = dir.child("app.js");
    app.write_str(&format!(
        r#"
const v = require({node:?});
console.log("num", v.numRoundtrip(), v.int32(), v.uint32(), v.int64());
console.log("bnu", v.boolNullUndef(), "str", v.stringRoundtrip(), "u16", v.utf16Surrogate(), "l1", v.latin1());
console.log("sym", v.symbol(), "arr", v.array(), "tof", v.typeofAndEquals());
console.log("err", v.errorFamily(new Error("x"), Error), "iserr", v.isErrorJsInstance(new TypeError("t")), v.isErrorJsInstance({{}}));
console.log("coerce", v.coerce(), "pend", v.pendingException());
"#
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("num 1 1 1 1"), "stdout: {so}");
    assert!(so.contains("bnu 1 str 1 u16 1 l1 1"), "stdout: {so}");
    assert!(so.contains("sym 1 arr 31 tof 1"), "stdout: {so}");
    assert!(so.contains("err 1 iserr 1 0"), "stdout: {so}");
    assert!(so.contains("coerce 1 pend 7"), "stdout: {so}");
    dir.close().unwrap();
}

#[test]
#[cfg(unix)]
fn phase_napi_m1_props_matrix() {
    // 正常：named/generic-key 属性族 + define_properties（value/method/data/
    // attrs/不可枚举缺席）+ prototype + array_length。
    let dir = assert_fs::TempDir::new().unwrap();
    let node = build_fixture_dylib(&dir, "m1_props");
    let app = dir.child("app.js");
    app.write_str(&format!(
        r#"
const p = require({node:?});
console.log("named", p.named(), "generic", p.genericKey());
console.log("defs", p.defineProperties(), "proto", p.prototypeAndArrayLen());
"#
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("named 1 generic 1"), "stdout: {so}");
    assert!(so.contains("defs 1 proto 1"), "stdout: {so}");
    dir.close().unwrap();
}

#[test]
#[cfg(unix)]
fn phase_napi_m2_class_matrix() {
    // 正常：define_class/new_instance/wrap-unwrap-remove_wrap/external（typeof
    // + 回读）/new.target/instanceof/访问器/static 成员/escapable scope/
    // node_api_* syntax error——fixture 内部按位断言，1 = 全过。
    let dir = assert_fs::TempDir::new().unwrap();
    let node = build_fixture_dylib(&dir, "m2_class");
    let app = dir.child("app.js");
    app.write_str(&format!(
        r#"
const m = require({node:?});
const p = new m.Person(42);
console.log("ctor", m.Person.newTargetOk(), m.Person.lastAge());
console.log("inst", p instanceof m.Person, typeof p);
console.log("wrap", p.getAge(), m.Person.newInstance(), m.Person.shape());
console.log("acc", (p.name = "alice"), p.name);
console.log("stat", m.Person.kind, typeof m.Person.make, m.Person.make(9).getAge());
console.log("plain", m.Person.plainCall());
console.log("ext", m.Person.external(), m.Person.escapable(), m.Person.syntaxShape());
try {{
  m.Person.throwSyntax();
  console.log("syn no-throw");
}} catch (e) {{
  console.log("syn", e instanceof SyntaxError, e.code, e.message);
}}
"#
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("ctor 1 42"), "stdout: {so}");
    assert!(so.contains("inst true object"), "stdout: {so}");
    assert!(so.contains("wrap 42 1 1"), "stdout: {so}");
    assert!(so.contains("acc alice alice"), "stdout: {so}");
    assert!(so.contains("stat human function 9"), "stdout: {so}");
    assert!(so.contains("plain 1"), "stdout: {so}");
    assert!(so.contains("ext 1 1 1"), "stdout: {so}");
    assert!(so.contains("syn true ERR_WJS_THROW thrown syntax"), "stdout: {so}");
    dir.close().unwrap();
}

#[test]
#[cfg(unix)]
fn phase_napi_m2_finalize_chain() {
    // 正常：finalize 释放链（dhat 等价的计数器口径）——external + wrap 实例
    // malloc/free 成对计数；trampoline 槽位截断 → 对象死态 → minor GC →
    // finalizer。两轮灌入保证首轮对象全部过 GC；free 追上一半即链路闭合，
    // 且 free ≤ alloc 恒成立（无双发/提前释放）。
    let dir = assert_fs::TempDir::new().unwrap();
    let node = build_fixture_dylib(&dir, "m2_finalize");
    let app = dir.child("app.js");
    app.write_str(&format!(
        r#"
const m = require({node:?});
for (let i = 0; i < 400000; i++) m.mk(i);
const a = m.counts();
for (let i = 0; i < 400000; i++) m.mk(i);
const b = m.counts();
// 分参逐项打印（§4.42：禁 && 打包）
console.log("fin", b[1] > 0, b[3] > 0, b[1] <= b[0], b[3] <= b[2]);
console.log("drain", b[1] >= Math.floor(a[0] / 2), b[3] >= Math.floor(a[2] / 2));
"#
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("fin true true true true"), "stdout: {so}");
    assert!(so.contains("drain true true"), "stdout: {so}");
    dir.close().unwrap();
}

#[test]
#[cfg(unix)]
fn phase_napi_m3_promise_ref_buffer_matrix() {
    // 正常：M3 值面矩阵（promise/deferred 回环 + refs 计数 + typedarray/
    // dataview 指针一致性 + BigInt64 + Buffer 读写/external）——fixture 内部
    // 按位断言，1 = 全过。
    let dir = assert_fs::TempDir::new().unwrap();
    let node = build_fixture_dylib(&dir, "m3_value");
    let app = dir.child("app.js");
    app.write_str(&format!(
        r#"
const m = require({node:?});
const p = m.makeDeferred();
console.log("isp", m.isPromise(p), m.isPromise({{}}));
p.then((v) => console.log("resolved", v));
m.resolveIt();
m.rejectIt().catch((e) => console.log("rejected", e));
console.log("ref", m.checkRef());
console.log("typed", m.checkTyped());
const ta = m.makeTA();
console.log("big", ta.constructor.name, ta.length, typeof ta[0]);
const buf = m.makeBuf();
console.log("buf", m.checkBuf(buf), buf.toString(), buf.length);
"#
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("isp 1 0"), "stdout: {so}");
    assert!(so.contains("resolved 77"), "stdout: {so}");
    assert!(so.contains("rejected nope"), "stdout: {so}");
    assert!(so.contains("ref 1"), "stdout: {so}");
    assert!(so.contains("typed 1"), "stdout: {so}");
    assert!(so.contains("big BigInt64Array 4 bigint"), "stdout: {so}");
    assert!(so.contains("buf 1 hello 5"), "stdout: {so}");
    dir.close().unwrap();
}

#[test]
#[cfg(unix)]
fn phase_napi_m3_async_work_tsfn() {
    // 正常（M3 验收线）：真 OS 线程回调进 JS——async_work 线程 execute →
    // complete resolve deferred（await 取值）；TSFN 线程 3 条消息 →
    // call_js_cb 逐条回 JS → release → thread_finalize 落定 "done"；
    // cancel 的 complete(napi_cancelled) 必达；keep-alive：全结算后进程自退。
    let dir = assert_fs::TempDir::new().unwrap();
    let node = build_fixture_dylib(&dir, "m3_async");
    let app = dir.child("app.js");
    app.write_str(&format!(
        r#"
const m = require({node:?});
(async () => {{
  console.log("work", await m.startWork(20));
  console.log("delete", m.deleteProbe());
  console.log("cancel", await m.cancelProbe());
  const msgs = [];
  const done = await m.startTsfn(function (v) {{ msgs.push(v); }});
  console.log("tsfn", done, msgs.join(","));
}})();
console.log("end");
"#
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(app.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("work 6765"), "stdout: {so}");
    assert!(so.contains("delete 1"), "stdout: {so}");
    assert!(so.contains("cancel cancelled"), "stdout: {so}");
    assert!(so.contains("tsfn done m1,m2,m3"), "stdout: {so}");
    dir.close().unwrap();
}

/// M4 验收（plan-napi）：真 rolldown 包经自家 pm 安装（**真网络**，故默认
/// ignore；验收跑 `cargo test --test napi -- --ignored`）。npm registry 偶发
/// 慢，超时上限给足。
#[test]
#[cfg(unix)]
#[ignore = "real network: installs rolldown via own pm (plan-napi M4 acceptance)"]
fn phase_napi_m4_rolldown_bundle_real_network() {
    let dir = assert_fs::TempDir::new().unwrap();
    let wjs = std::env::var("CARGO_BIN_EXE_winterjs")
        .unwrap_or_else(|_| "target/debug/winterjs".to_string());
    // 1) 自家 pm 真装 rolldown（连带 @rolldown/binding-darwin-arm64）
    let add = std::process::Command::new(&wjs)
        .args(["-a", "rolldown"])
        .current_dir(dir.path())
        .env("WINTERJS_LOG", "warn")
        .output()
        .expect("pm add runs");
    assert!(
        add.status.success(),
        "pm add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    // 2) 最小工程 + rolldown JS API bundle
    let src = dir.child("src");
    std::fs::create_dir_all(src.path()).unwrap();
    src.child("lib.js")
        .write_str("export function greet(name) { return `hello, ${name}!`; }\n")
        .unwrap();
    src.child("main.js")
        .write_str("import { greet } from './lib.js';\nconsole.log(greet('rolldown'));\n")
        .unwrap();
    dir.child("bundle.mjs")
        .write_str(
            r#"
import { rolldown } from 'rolldown';
const bundle = await rolldown({ input: 'src/main.js' });
const { output } = await bundle.generate({ format: 'es' });
console.log("chunks", output.length);
console.log("code>>>");
console.log(output[0].code);
await bundle.close();
"#,
        )
        .unwrap();
    let out = winterjs()
        .args(["--run", "bundle.mjs", "--allow-ffi", "--allow-env"])
        .current_dir(dir.path())
        .env("WINTERJS_LOG", "warn")
        .output()
        .expect("bundle runs");
    assert!(
        out.status.success(),
        "bundle failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("chunks 1"), "stdout: {so}");
    // 真内核产物断言：两模块内联 + 类型擦除/字符串拼接保留
    assert!(so.contains("function greet(name)"), "stdout: {so}");
    assert!(so.contains("console.log(greet(\"rolldown\"));"), "stdout: {so}");
    // 3) TS 输入 bundle（rolldown 内核 oxc transform 面）
    src.child("app.ts")
        .write_str("interface User { name: string; age: number }\nconst u: User = { name: \"winter\", age: 26 };\nexport const msg: string = `${u.name} is ${u.age}`;\n")
        .unwrap();
    dir.child("bundle2.mjs")
        .write_str(
            r#"
import { rolldown } from 'rolldown';
const bundle = await rolldown({ input: 'src/app.ts' });
const { output } = await bundle.generate({ format: 'es' });
console.log(output[0].code);
await bundle.close();
"#,
        )
        .unwrap();
    let out = winterjs()
        .args(["--run", "bundle2.mjs", "--allow-ffi", "--allow-env"])
        .current_dir(dir.path())
        .env("WINTERJS_LOG", "warn")
        .output()
        .expect("ts bundle runs");
    assert!(
        out.status.success(),
        "ts bundle failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("const u = {"), "stdout: {so}");
    assert!(!so.contains("interface User"), "interface must be erased: {so}");
    assert!(so.contains("export { msg };"), "stdout: {so}");
    dir.close().unwrap();
}

#[test]
#[ignore = "real network: installs vite via own pm (plan-napi M5 build acceptance)"]
fn phase_napi_m5_vite_build_real_network() {
    // 真网络：pm 装 vite（连带 rolldown/@rolldown/binding）→ vite build JS API
    // 全链（resolveConfig → vite.config.js 经 require.extensions/_compile 加载
    // → build → dist 落盘）→ 产物 --run 可执行。napi 面：cac 的 EventTarget
    // 基类、PromiseRaw.then/catch 的 napi_wrap 任意对象路、crypto.getRandomValues、
    // process.versions.node 22.12 地板。
    // 注：不带 --allow-ffi/--allow-env（权限沙箱会拒 fs 读，vite existsSync 门
    // 吞 EACCES 返 false——探针实录）；pm add 与 build 均在 tempdir 内（§4.20）。
    let dir = assert_fs::TempDir::new().unwrap();
    let wjs = std::env::var("CARGO_BIN_EXE_winterjs")
        .unwrap_or_else(|_| "target/debug/winterjs".to_string());
    // 1) 自家 pm 真装 vite（连带 rolldown + binding-darwin-arm64）
    let add = std::process::Command::new(&wjs)
        .args(["-a", "vite"])
        .current_dir(dir.path())
        .env("WINTERJS_LOG", "warn")
        .output()
        .expect("pm add runs");
    assert!(
        add.status.success(),
        "pm add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    // 2) 最小工程 + 关 modulePreload polyfill（产物为纯 JS 客户端 IIFE，可 --run）
    let src = dir.child("src");
    std::fs::create_dir_all(src.path()).unwrap();
    src.child("lib.js")
        .write_str("export function greet(name) { return `hello, ${name}!`; }\n")
        .unwrap();
    src.child("main.js")
        .write_str("import { greet } from './lib.js';\nconsole.log(greet('vite'));\n")
        .unwrap();
    dir.child("index.html").write_str(
        "<!doctype html>\n<html><body><script type=\"module\" src=\"/src/main.js\"></script></body></html>\n",
    ).unwrap();
    dir.child("vite.config.js")
        .write_str("export default { build: { modulePreload: { polyfill: false } } };\n")
        .unwrap();
    // 3) vite build JS API（config 文件加载走 require.extensions 链）
    dir.child("build-probe.mjs")
        .write_str(
            r#"
import { build } from 'vite';
await build({ logLevel: 'info' });
console.log("BUILD-OK");
"#,
        )
        .unwrap();
    let out = winterjs()
        .args(["--run", "build-probe.mjs"])
        .current_dir(dir.path())
        .env("WINTERJS_LOG", "warn")
        .output()
        .expect("build runs");
    assert!(
        out.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let so = String::from_utf8_lossy(&out.stdout);
    assert!(so.contains("BUILD-OK"), "stdout: {so}");
    // 4) 产物可执行（dist/assets/index-*.js 客户端 IIFE）
    let assets = dir.child("dist/assets");
    let mut chunk = None;
    for entry in std::fs::read_dir(assets.path()).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) == Some("js") {
            chunk = Some(p);
        }
    }
    let chunk = chunk.expect("dist chunk exists");
    let run = winterjs()
        .args(["--run"])
        .arg(&chunk)
        .current_dir(dir.path())
        .env("WINTERJS_LOG", "warn")
        .output()
        .expect("artifact runs");
    assert!(
        run.status.success(),
        "artifact failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("hello, vite!"),
        "artifact stdout: {}",
        String::from_utf8_lossy(&run.stdout)
    );
    dir.close().unwrap();
}
