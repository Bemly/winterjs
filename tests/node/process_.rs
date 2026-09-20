//! tests/node/process_.rs — 对齐 src/builtins/node/process_.rs（node:process + 全局别名/错误面）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

#[test]
fn phase4_node_process_argv_env() {
    // argv 透传 + env 读写删查（Proxy 活视图）。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("argv.mjs");
    file.write_str(r#"console.log(process.argv.length, process.argv[2], process.execPath.length > 0, process.pid > 0);"#).unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .arg("hello")
        .arg("--flag")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .starts_with("4 hello true true\n"),
        "argv"
    );
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"process.env.WINTERJS_T4 = "v1"; console.log(process.env.WINTERJS_T4, "WINTERJS_T4" in process.env, Object.keys(process.env).includes("WINTERJS_T4")); delete process.env.WINTERJS_T4; console.log(process.env.WINTERJS_T4, "WINTERJS_T4" in process.env);"#]));
    assert_eq!(out, "v1 true true\nundefined false\n", "env: {out}");
    dir.close().unwrap();
}

#[test]
fn phase4_process_exit_codes() {
    // 正常/显式/默认/模块顶层/异步后设码，全走静默退出（无 stderr）。
    let dir = assert_fs::TempDir::new().unwrap();
    let run = |name: &str, src: &str| {
        let f = dir.child(name);
        f.write_str(src).unwrap();
        winterjs().arg("--run").arg(f.path()).output().unwrap()
    };
    let out = run("e3.mjs", "process.exit(3);");
    assert_eq!(out.status.code(), Some(3));
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run("e0.mjs", "process.exit();");
    assert_eq!(out.status.code(), Some(0));
    let out = run("c7.mjs", "process.exitCode = 7;");
    assert_eq!(out.status.code(), Some(7));
    assert!(out.stderr.is_empty());
    let out = run("t.mjs", "setTimeout(() => { process.exitCode = 5; }, 10);");
    assert_eq!(out.status.code(), Some(5));
    // exit 被 catch 也照退（Node 同 outcome；此处验证退出码，不断言抛）。
    let out = run("caught.mjs", "try { process.exit(4); } catch (e) {}\n");
    assert_eq!(out.status.code(), Some(4));
    assert!(out.stderr.is_empty());
    // 首个码赢（realpath-pipe 套件：try{exit(2)}catch{exit(1)} 必须 rc=2；
    // ESM/CJS 双入口，无 stderr）。
    let out = run("first.mjs", "try { process.exit(2); } catch (e) { process.exit(1); }\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
    let out = run("first.cjs", "try { process.exit(2); } catch (e) { process.exit(1); }\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
    dir.close().unwrap();
}

#[test]
fn phase4_process_stdio_nexttick_cwd() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"process.stdout.write("out-direct"); const order = []; process.nextTick(() => order.push("tick")); Promise.resolve().then(() => order.push("promise")); await new Promise((r) => setTimeout(r, 20)); console.log("|" + order.join(","), process.cwd().length > 0, typeof process.uptime(), typeof process.hrtime.bigint(), process.memoryUsage().rss > 0, process.versions.winterjs.length > 0);"#]));
    assert!(out.starts_with("out-direct|"), "stdio: {out}");
    assert!(
        out.contains("tick,promise true number bigint true true\n"),
        "order: {out}"
    );
}

#[test]
fn phase4_node_errors() {
    // 未知内建（静态/动态）给可用列表；exitCode 非整数 TypeError。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "bad.mjs",
        "import x from \"node:nope\";\nconsole.log(x);\n",
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("node:nope") && stderr.contains("node:path"),
        "stderr: {stderr}"
    );
    let out = winterjs()
        .args(["--eval", "await import(\"node:nope\")"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = winterjs()
        .args(["--eval", "process.exitCode = 1.5;"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("integer"), "stderr: {stderr}");
    dir.close().unwrap();
}

#[test]
fn phase9j_global_alias() {
    // Node 口径：global 为全局自引用（vite bin 直引，-r dev 实测补齐）。
    let out = winterjs()
        .args(["--eval", "console.log(global === globalThis, typeof global.setTimeout, global.process === process)"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "true function true\n");
}

#[test]
fn phase9m_process_stdio_faces() {
    // stdout/stderr 写回调 + maxListeners 记账 + listeners/eventNames +
    // execArgv + availableParallelism + 全局 performance（M5 vitest 牵引面）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import os from "node:os";
let fired = false;
process.stdout.write("", () => { fired = true; });
await new Promise((r) => setTimeout(r, 20));
console.log("wcb", fired);
console.log("ml", process.stdout.getMaxListeners() === 10 && process.stdout.setMaxListeners(3) === process.stdout && process.stdout.getMaxListeners() === 3);
const onFoo = () => {};
process.on("w9m-foo", onFoo);
console.log("listeners", process.listeners("w9m-foo").length === 1, process.eventNames().includes("w9m-foo"), typeof process.rawListeners("w9m-foo")[0] === "function");
process.off("w9m-foo", onFoo);
console.log("off", process.listeners("w9m-foo").length === 0);
console.log("execArgv", Array.isArray(process.execArgv), Array.isArray((await import("node:process")).execArgv));
console.log("par", os.availableParallelism() > 0 && Number.isInteger(os.availableParallelism()));
console.log("perf", typeof performance.now() === "number" && performance.timeOrigin > 0);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in ["wcb true", "ml true", "listeners true true true", "off true", "execArgv true true", "par true", "perf true"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_process_config_features_umask() {
    // 10f：process.config/features/umask（跑 test/common 前置）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import process from "node:process";
console.log("config", process.config.target_defaults.default_configuration, process.config.variables.node_shared === false);
console.log("features", process.features.uv, process.features.debug === false, typeof process.features.quic);
console.log("versions", typeof process.versions.openssl, typeof process.versions.sqlite);
const before = process.umask();
process.umask(0o027);
console.log("umask-set", process.umask().toString(8));
process.umask(before);
console.log("umask-back", process.umask() === before);
"#,
    );
    assert!(out.contains("config Release true"), "out: {out}");
    assert!(out.contains("features true true boolean"), "out: {out}");
    assert!(out.contains("versions string string"), "out: {out}");
    assert!(out.contains("umask-set 27"), "out: {out}");
    assert!(out.contains("umask-back true"), "out: {out}");
    dir.close().unwrap();
}
