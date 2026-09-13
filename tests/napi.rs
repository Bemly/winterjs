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
