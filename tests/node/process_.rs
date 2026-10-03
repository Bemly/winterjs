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
    let out = winterjs2()
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
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"process.env.WINTERJS2_T4 = "v1"; console.log(process.env.WINTERJS2_T4, "WINTERJS2_T4" in process.env, Object.keys(process.env).includes("WINTERJS2_T4")); delete process.env.WINTERJS2_T4; console.log(process.env.WINTERJS2_T4, "WINTERJS2_T4" in process.env);"#]));
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
        winterjs2().arg("--run").arg(f.path()).output().unwrap()
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
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"process.stdout.write("out-direct"); const order = []; process.nextTick(() => order.push("tick")); Promise.resolve().then(() => order.push("promise")); await new Promise((r) => setTimeout(r, 20)); console.log("|" + order.join(","), process.cwd().length > 0, typeof process.uptime(), typeof process.hrtime.bigint(), process.memoryUsage().rss > 0, process.versions.winterjs2.length > 0);"#]));
    assert!(out.starts_with("out-direct|"), "stdio: {out}");
    assert!(
        out.contains("tick,promise true number bigint true true\n"),
        "order: {out}"
    );
}

#[test]
fn phase4_node_errors() {
    // 未知内建真机文案（R2-iter 按 4.65 翻转：`No such built-in module: node:nope`，
    // 旧断言编码的是自带可用列表的自家文案）；exitCode 非整数 TypeError。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "bad.mjs",
        "import x from \"node:nope\";\nconsole.log(x);\n",
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("No such built-in module: node:nope"),
        "stderr: {stderr}"
    );
    let out = winterjs2()
        .args(["--eval", "await import(\"node:nope\")"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = winterjs2()
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
    let out = winterjs2()
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

#[test]
fn phase10f_process_stdin_destroy() {
    // stdin.destroy 即关（listen-after-destroying-stdin 套件）：不抛、
    // 挂 data 后 destroy 即退（无 8s 悬挂），管道输入仍可读。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
console.log("destroy-ret", process.stdin.destroy() === process.stdin);
process.stdin.on("data", () => console.log("data-after-destroy"));
setTimeout(() => console.log("exited-clean"), 300);
"#,
    );
    assert!(out.contains("destroy-ret true"), "out: {out}");
    assert!(out.contains("exited-clean"), "out: {out}");
    assert!(!out.contains("data-after-destroy"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase11_exit_event_and_hang_exit() {
    // 2026-09-25：自然退出派发 process 'exit'（修前 this 绑成 global，监听从不触发——
    // node 套件 common.mustCall 的退出核对形同虚设）；WINTERJS2_HANG_EXIT 到点发 'exit'
    // 并以 1 退出（把挂死件变成带定位的红件）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("x.js").write_str("process.on('exit', (c) => console.log('exit-cjs', c));\n").unwrap();
    dir.child("x.mjs").write_str("process.on('exit', (c) => console.log('exit-esm', c));\n").unwrap();
    dir.child("h.js")
        .write_str("process.on('exit', () => console.log('exit-hang'));\nrequire('net').createServer().listen(0);\n")
        .unwrap();
    // 正常：CJS / ESM 自然退出均触发，退出码 0。
    let (ok, out, _) = wjs(&["--run", "x.js"], &dir);
    assert!(ok && out.contains("exit-cjs 0"), "out: {out}");
    let (ok, out, _) = wjs(&["--run", "x.mjs"], &dir);
    assert!(ok && out.contains("exit-esm 0"), "out: {out}");
    // 报错：exit 监听里 process.exit(3) 决定退出码（mustCall 失配的 common 路径）。
    dir.child("e.js").write_str("process.on('exit', () => process.exit(3));\n").unwrap();
    let out = winterjs2().args(["--run", "e.js"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    // 边界：挂死（监听中的 server 永不关）+ HANG_EXIT=1 → 发 exit、stderr 报存活句柄、退出 1。
    let out = winterjs2()
        .args(["--run", "h.js"])
        .env("WINTERJS2_HANG_EXIT", "1")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("exit-hang"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no progress for 1s"));
}

#[test]
fn phase11_before_exit_and_fatal_exit_event() {
    // 2026-09-26：循环排空派发 'beforeExit'（监听排新任务即续转、排空再发）；致命错先打印
    // 再以 code 1 派发 'exit'（监听可改 exitCode）；process.emit 监听抛错原样上抛。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("b.js")
        .write_str(
            "let n = 0;\nprocess.on('beforeExit', (c) => { console.log('be', c, n); if (++n < 3) setTimeout(() => {}, 1); });\n\
             process.on('exit', (c) => console.log('exit', c));\n",
        )
        .unwrap();
    // 正常：排空 3 次发 3 次，之后 exit。
    let (ok, out, _) = wjs(&["--run", "b.js"], &dir);
    assert!(ok, "out: {out}");
    assert_eq!(out.lines().collect::<Vec<_>>(), ["be 0 0", "be 0 1", "be 0 2", "exit 0"]);
    // 报错：beforeExit 抛错 → stderr 打印错误，exit 监听收到 1 并改 exitCode=0 → 退出 0。
    dir.child("t.js")
        .write_str(
            "process.on('exit', (c) => { console.log('exit', c); process.exitCode = 0; });\n\
             process.on('beforeExit', () => { throw new Error('be-boom'); });\n",
        )
        .unwrap();
    let out = winterjs2().args(["--run", "t.js"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("exit 1"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Error: be-boom"));
    // 边界：process.exit() 不发 beforeExit；process.emit 抛错可被 catch；无监听 'error' 即抛。
    dir.child("x.js")
        .write_str(
            "process.on('beforeExit', () => console.log('BAD'));\n\
             process.on('foo', () => { throw new Error('l'); });\n\
             try { process.emit('foo'); } catch (e) { console.log('caught', e.message); }\n\
             try { process.emit('error', 5); } catch (e) { console.log(e.code); }\n\
             process.exit(0);\n",
        )
        .unwrap();
    let (ok, out, _) = wjs(&["--run", "x.js"], &dir);
    assert!(ok, "out: {out}");
    assert_eq!(out.lines().collect::<Vec<_>>(), ["caught l", "ERR_UNHANDLED_ERROR"]);
}

#[test]
fn phase11_process_validation_faces() {
    // P2-process R1: hrtime/nextTick/chdir 参数校验 + release 面（node 原文口径）。
    // 正常：hrtime 无参/差值元组 + 借位非负；chdir 来回；release name/lts。
    // 报错：三处 code 精确（ERR_INVALID_ARG_TYPE/ERR_OUT_OF_RANGE/ENOENT）。
    // 边界：hrtime 三元数组；chdir 缺参。
    let dir = assert_fs::TempDir::new().unwrap();
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "const t = process.hrtime(); console.log('tuple', Array.isArray(t) && t.length === 2);\
             const d = process.hrtime([0, 1e9 - 1]); console.log('borrow', d[1] >= 0);\
             const c0 = process.cwd(); process.chdir('..'); process.chdir(c0);\
             console.log('chdir-roundtrip', process.cwd() === c0);\
             console.log('release', process.release.name === 'node' && process.release.lts === 'Jod');\
             const code = (f) => { try { f(); } catch (e) { return e.code; } return 'NO-THROW'; };\
             console.log('hrtime-num', code(() => process.hrtime(1)));\
             console.log('hrtime-empty', code(() => process.hrtime([])));\
             console.log('hrtime-three', code(() => process.hrtime([1, 2, 3])));\
             console.log('nexttick', code(() => process.nextTick(1)));\
             console.log('chdir-obj', code(() => process.chdir({})));\
             console.log('chdir-missing', code(() => process.chdir()));\
             console.log('chdir-enoent', code(() => process.chdir('does-not-exist-wjs2')));",
        ],
        &dir,
    );
    assert!(ok, "out: {out}");
    for line in [
        "tuple true",
        "borrow true",
        "chdir-roundtrip true",
        "release true",
        "hrtime-num ERR_INVALID_ARG_TYPE",
        "hrtime-empty ERR_OUT_OF_RANGE",
        "hrtime-three ERR_OUT_OF_RANGE",
        "nexttick ERR_INVALID_ARG_TYPE",
        "chdir-obj ERR_INVALID_ARG_TYPE",
        "chdir-missing ERR_INVALID_ARG_TYPE",
        "chdir-enoent ENOENT",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_process_env_faces() {
    // P2-process R8: env Proxy 全家（node 原文口径）。
    // 正常：原型回落（hasOwnProperty）、空键静默忽略、DEP0104 后照赋。
    // 报错：符号键/值、坏描述符双文案。
    // 边界：flags 白名单正反例 + 冻结；TZ 写入不崩（时区生效记档另案）。
    let dir = assert_fs::TempDir::new().unwrap();
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "'use strict'; const assert = require('node:assert');\
             console.log('proto', process.env.hasOwnProperty === Object.prototype.hasOwnProperty);\
             process.env[''] = ''; console.log('empty', process.env[''] === undefined);\
             const sym = Symbol('s');\
             console.log('sym-get', process.env[sym] === undefined);\
             try { process.env[sym] = 1; console.log('sym-key NO-THROW'); } catch (e) { console.log('sym-key', e.name); }\
             try { process.env.WJS2_SYM = sym; console.log('sym-val NO-THROW'); } catch (e) { console.log('sym-val', e.name); }\
             try { Object.defineProperty(process.env, 'x', { value: 1 }); console.log('desc NO-THROW'); }\
             catch (e) { console.log('desc', e.code); }\
             const f = process.allowedNodeEnvironmentFlags;\
             console.log('flags', f.has('-r') && f.has('r') && !f.has('--cheeseburgers') && Object.isFrozen(f));\
             process.env.WJS2_DEP = undefined;\
             console.log('dep', process.env.WJS2_DEP === 'undefined');\
             process.env.WJS2_TZ = 'Europe/Amsterdam'; console.log('tz-set true');",
        ],
        &dir,
    );
    assert!(ok, "out: {out}");
    for line in [
        "proto true",
        "empty true",
        "sym-get true",
        "sym-key TypeError",
        "sym-val TypeError",
        "desc ERR_INVALID_OBJECT_DEFINE_PROPERTY",
        "flags true",
        "dep true",
        "tz-set true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_process_capture_faces() {
    // P2-process R7: uncaught capture 路由（node execution.js 口径）。
    // 正常：capture 接住入口抛错（uncaughtException 不发、exit 0）；null 清除。
    // 报错：非函数非 null 入参码；重复设置码。
    // 边界：无 capture 时入口抛错仍 fatal（exit 1）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("cap.js")
        .write_str(
            "console.log('has', process.hasUncaughtExceptionCaptureCallback());\n\
             process.setUncaughtExceptionCaptureCallback((e) => console.log('cap', e.message));\n\
             console.log('has2', process.hasUncaughtExceptionCaptureCallback());\n\
             process.on('uncaughtException', () => console.log('BAD'));\n\
             process.setUncaughtExceptionCaptureCallback(null);\n\
             console.log('has3', process.hasUncaughtExceptionCaptureCallback());\n\
             process.setUncaughtExceptionCaptureCallback((e) => console.log('cap', e.message));\n\
             throw new Error('foo');\n",
        )
        .unwrap();
    let (ok, out, _) = wjs(&["--run", "cap.js"], &dir);
    assert!(ok, "out: {out}");
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        ["has false", "has2 true", "has3 false", "cap foo"]
    );
    // 报错面经 --eval（进程存活可连断言）。
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "const code = (f) => { try { f(); } catch (e) { return e.code || 'THREW'; } return 'NO-THROW'; };\n\
             console.log('badarg', code(() => process.setUncaughtExceptionCaptureCallback(42)));\n\
             process.setUncaughtExceptionCaptureCallback(() => {});\n\
             console.log('twice', code(() => process.setUncaughtExceptionCaptureCallback(() => {})));",
        ],
        &dir,
    );
    assert!(ok, "out: {out}");
    assert!(out.contains("badarg ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(
        out.contains("twice ERR_UNCAUGHT_EXCEPTION_CAPTURE_ALREADY_SET"),
        "out: {out}"
    );
    // 边界：无 capture 的入口抛错仍 fatal。
    dir.child("nocap.js").write_str("throw new Error('nope');\n").unwrap();
    let out = winterjs2().args(["--run", "nocap.js"]).current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Error: nope"));
    dir.close().unwrap();
}

#[test]
fn phase11_process_execve_faces() {
    // P2-process R6: execve 校验面（node 原文口径；真调替换测试进程，
    // 此处只验报错面 + 失败形 ENOENT，不做成功替换）。
    // 正常：无（成功不返回，不断言）。
    // 报错：execPath 非串、args 非数组/坏元素、env 非对象/坏键值、ENOENT 形状。
    // 边界：空 env 对象（校验过，调后 ENOENT——路径必不存在）。
    let dir = assert_fs::TempDir::new().unwrap();
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "const code = (f) => { try { f(); } catch (e) { return e.code || 'THREW'; } return 'NO-THROW'; };\
             console.log('path', code(() => process.execve(123)));\
             console.log('args', code(() => process.execve(process.execPath, '123')));\
             console.log('args-elem', code(() => process.execve(process.execPath, [123])));\
             console.log('env', code(() => process.execve(process.execPath, [], '123')));\
             console.log('env-val', code(() => process.execve(process.execPath, [], { abc: 123 })));\
             try { process.execve('/wjs2-no-such-bin-xyz', ['x']); }\
             catch (e) { console.log('enoent', e.code, e.syscall, typeof e.errno, /xyz$/.test(e.path)); }",
        ],
        &dir,
    );
    assert!(ok, "out: {out}");
    for line in [
        "path ERR_INVALID_ARG_TYPE",
        "args ERR_INVALID_ARG_TYPE",
        "args-elem ERR_INVALID_ARG_VALUE",
        "env ERR_INVALID_ARG_TYPE",
        "env-val ERR_INVALID_ARG_VALUE",
        "enoent ENOENT execve number true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_process_spawn_faces() {
    // P2-process R5: ppid + reallyExit 路由 + execPath canonical（node 原文口径）。
    // 正常：ppid 为正整数；子进程 ppid 即父 pid；exit 经 reallyExit（mock 可截）。
    // 报错：不适用（本轮三件皆正常面；非法码走 exitCode setter 门，另案）。
    // 边界：execPath === realpath（软链起亦然）；reallyExit mock 后代码继续跑。
    let dir = assert_fs::TempDir::new().unwrap();
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "console.log('ppid', Number.isInteger(process.ppid) && process.ppid > 0);\
             const fs = require('fs');\
             console.log('execpath', process.execPath === fs.realpathSync(process.execPath));\
             let hit = null;\
             process.reallyExit = (c) => { hit = c; };\
             process.exitCode = 0; process.exit();\
             console.log('exit-mock', hit === 0);\
             const cp = require('child_process');\
             const out = cp.spawnSync(process.execPath, ['-e', 'console.log(process.ppid)']);\
             console.log('child-ppid', Number(out.stdout.toString().trim()) === process.pid);",
        ],
        &dir,
    );
    assert!(ok, "out: {out}");
    for line in [
        "ppid true",
        "execpath true",
        "exit-mock true",
        "child-ppid true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_process_kill_prototype_title_faces() {
    // P2-process R4: kill 校验/_kill 可 mock + 原型链 + title（node 原文口径）。
    // 正常：kill 自检真；_kill mock 透传（pid/信号数值化）；原型链五断言；title 读写。
    // 报错：pid 非数、未知信号名、987 数值信号码。
    // 边界：kill 字符串 pid '0'；title 空串。
    let dir = assert_fs::TempDir::new().unwrap();
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "console.log('self', process.kill(process.pid, 0) === true);\
             const orig = process._kill; let got = null;\
             process._kill = (pid, sig) => { got = [pid, sig]; process._kill = orig; return 0; };\
             process.kill('0', 'SIGHUP'); console.log('mock', JSON.stringify(got));\
             const EE = require('events'); const proto = Object.getPrototypeOf(process);\
             console.log('proto', (proto !== EE.prototype) && (proto instanceof EE) && (process.constructor !== EE));\
             console.log('ctor', process instanceof process.constructor);\
             console.log('title-rw', (() => { const t = process.title; process.title = ''; const r = process.title === ''; process.title = t; return typeof t === 'string' && r; })());\
             const code = (f) => { try { f(); } catch (e) { return e.code || 'THREW'; } return 'NO-THROW'; };\
             console.log('pid-str', code(() => process.kill('SIGTERM')));\
             console.log('sig-name', code(() => process.kill(0, 'test')));\
             console.log('sig-num', code(() => process.kill(0, 987)));",
        ],
        &dir,
    );
    assert!(ok, "out: {out}");
    for line in [
        "self true",
        "mock [\"0\",1]",
        "proto true",
        "ctor true",
        "title-rw true",
        "pid-str ERR_INVALID_ARG_TYPE",
        "sig-name ERR_UNKNOWN_SIGNAL",
        "sig-num EINVAL",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_process_credential_faces() {
    // P2-process R3: setuid/setgid/seteuid/setegid/setgroups/initgroups
    //（node wrapPosixCredentialSetters 口径；只走无副作用路径——校验错/
    // 未知身份/自身份 no-op，不改测试进程身份）。
    // 正常：自身份 no-op；未知身份码精确。
    // 报错：id 非数串、数组缺省、组元素非法、initgroups 缺参。
    // 边界：setgroups 空数组（调 syscall， EPERM 或成功皆不断言码，只不断言崩）；
    // 数字形未知大 id（只断言抛错，不定码）。
    let dir = assert_fs::TempDir::new().unwrap();
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "process.seteuid(process.geteuid()); console.log('self-noop true');\
             const code = (f) => { try { f(); } catch (e) { return e.code || 'THREW'; } return 'NO-THROW'; };\
             console.log('uid-unknown', code(() => process.setuid('fhq-no-such-user-wjs2')));\
             console.log('gid-unknown', code(() => process.setgroups([1, 'fhq-no-such-group-wjs2'])));\
             console.log('init-unknown', code(() => process.initgroups('fhq-no-wjs2', 'fhq-no-wjs2')));\
             console.log('id-obj', code(() => process.setuid({})));\
             console.log('groups-missing', code(() => process.setgroups()));\
             console.log('groups-elem', code(() => process.setgroups([true])));\
             console.log('init-user', code(() => process.initgroups(null, 'x')));\
             try { process.setgroups([]); console.log('groups-empty THREW-NONE'); } catch (e) { console.log('groups-empty', e.code || 'THREW'); }",
        ],
        &dir,
    );
    assert!(ok, "out: {out}");
    for line in [
        "self-noop true",
        "uid-unknown ERR_UNKNOWN_CREDENTIAL",
        "gid-unknown ERR_UNKNOWN_CREDENTIAL",
        "init-unknown ERR_UNKNOWN_CREDENTIAL",
        "id-obj ERR_INVALID_ARG_TYPE",
        "groups-missing ERR_INVALID_ARG_TYPE",
        "groups-elem ERR_INVALID_ARG_TYPE",
        "init-user ERR_INVALID_ARG_TYPE",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    assert!(
        out.lines().any(|l| l == "groups-empty THREW-NONE" || l.starts_with("groups-empty ")),
        "groups-empty line missing\nout: {out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase11_process_resource_faces() {
    // P2-process R2: abort/内存/cpu/umask 面（node 原文口径）。
    // 正常：abort 无 prototype；内存两数；cpu/thread 双数非负；umask 串数互通。
    // 报错：cpu prevValue 非对象/坏字段码；umask 非法串/对象码。
    // 边界：umask 高位（0o10000）被内核截断；cpu 差值非负（同值自减）。
    let dir = assert_fs::TempDir::new().unwrap();
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "console.log('abort', typeof process.abort === 'function' && process.abort.prototype === undefined);\
             try { new process.abort(); console.log('abort-new NO-THROW'); } catch (e) { console.log('abort-new', e.name); }\
             console.log('mem', typeof process.availableMemory() === 'number' && typeof process.constrainedMemory() === 'number');\
             const r = process.cpuUsage();\
             console.log('cpu', Number.isFinite(r.user) && r.user >= 0 && Number.isFinite(r.system) && r.system >= 0);\
             const t = process.threadCpuUsage();\
             console.log('tcpu', Number.isFinite(t.user) && t.user >= 0 && Number.isFinite(t.system) && t.system >= 0);\
             const d = process.cpuUsage(r);\
             console.log('cpu-diff', d.user >= 0 && d.system >= 0);\
             const o = process.umask(); process.umask('0664');\
             console.log('umask-str', process.umask().toString(8) === '664');\
             process.umask(o); console.log('umask-back', process.umask() === o);\
             process.umask(0o664 | 0o10000); console.log('umask-trunc', process.umask().toString(8) === '664'); process.umask(o);\
             const code = (f) => { try { f(); } catch (e) { return e.code; } return 'NO-THROW'; };\
             console.log('cpu-num', code(() => process.cpuUsage(1)));\
             console.log('cpu-user', code(() => process.cpuUsage({ user: 'a' })));\
             console.log('cpu-range', code(() => process.cpuUsage({ user: -1, system: 2 })));\
             console.log('umask-obj', code(() => process.umask({})));\
             console.log('umask-bad', code(() => process.umask('999')));",
        ],
        &dir,
    );
    assert!(ok, "out: {out}");
    for line in [
        "abort true",
        "abort-new TypeError",
        "mem true",
        "cpu true",
        "tcpu true",
        "cpu-diff true",
        "umask-str true",
        "umask-back true",
        "umask-trunc true",
        "cpu-num ERR_INVALID_ARG_TYPE",
        "cpu-user ERR_INVALID_ARG_TYPE",
        "cpu-range ERR_INVALID_ARG_VALUE",
        "umask-obj ERR_INVALID_ARG_TYPE",
        "umask-bad ERR_INVALID_ARG_VALUE",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_emit_warning_node_semantics() {
    // 2026-09-26：emitWarning 按 node lib/internal/process/warning.js 移植——缺省打印是表内
    // 普通监听（可 off 摘除）；once 监听只触发一次；noDeprecation/throwDeprecation 门控；
    // CJS 栈帧是绝对路径（node 口径，stack.includes(__filename)）。
    // 2026-10-04 翻转（§4.65/4.266，真机实测）：throwDeprecation 不走同步抛——nextTick
    // 异步抛经 uncaughtException 交付；旧"thrown DeprecationWarning"同步断言作废。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("w.js")
        .write_str(
            "process.once('warning', (w) => console.log('once', w.name, w.code, w.stack.includes(__filename)));\n\
             process.emitWarning('a', 'CustomWarning', 'C1');\n\
             process.emitWarning('b');\n\
             setTimeout(() => {\n\
               process.removeListener('warning', process.listeners('warning')[0]);\n\
               process.emitWarning('silent');\n\
               process.noDeprecation = true; process.emitWarning('d', 'DeprecationWarning');\n\
               process.noDeprecation = false; process.throwDeprecation = true;\n\
               process.once('uncaughtException', (e) => console.log('thrown-async', e.name));\n\
               try { process.emitWarning('t', 'DeprecationWarning'); console.log('no-sync'); } catch (e) { console.log('thrown', e.name); }\n\
             }, 5);\n",
        )
        .unwrap();
    // 正常：once 收一次；缺省打印两条（含 code 前缀）+ 一次 trace 提示；摘除后静默。
    let out = winterjs2().args(["--run", "w.js"]).current_dir(dir.path()).output().unwrap();
    let (so, se) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "stderr: {se}");
    assert_eq!(so.lines().collect::<Vec<_>>(), ["once CustomWarning C1 true", "no-sync", "thrown-async DeprecationWarning"]);
    assert!(se.contains("[C1] CustomWarning: a") && se.contains("Warning: b"), "stderr: {se}");
    assert_eq!(se.matches("--trace-warnings").count(), 1, "stderr: {se}");
    assert!(!se.contains("silent") && !se.contains("DeprecationWarning: d"), "stderr: {se}");
    // 报错：非 string/Error 参数 → ERR_INVALID_ARG_TYPE 同步抛。
    let (ok, out, _) = wjs(
        &["--eval", "try { process.emitWarning(1) } catch (e) { console.log(e.code) }; 0"],
        &dir,
    );
    assert!(ok && out.contains("ERR_INVALID_ARG_TYPE"), "out: {out}");
    // 边界：--no-warnings 不登记缺省打印（监听表为空，stderr 无输出）。
    let out = winterjs2()
        .args(["--no-warnings", "--eval", "process.emitWarning('x'); process.listenerCount('warning')"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).trim() == "0", "{out:?}");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("Warning: x"));
}

#[test]
fn phase11_process_r9_misc_faces() {
    // P2-process R9-A: exitCode 校验 + binding/config/_rawDebug/setSourceMaps/
    // ref-unref/getBuiltin（node 原文口径）。
    // 正常：exitCode 合法串/清零；binding util 16 键恒等；config 冻结；
    //   ref/unref 双形；getBuiltin 双形同引用。
    // 报错：exitCode 非法三形码；binding 未知模块；setSourceMaps 非布尔；
    //   getBuiltin 非串。
    // 边界：exitCode delete 不可删；getBuiltin('test')/internal/* 回 undefined。
    let dir = assert_fs::TempDir::new().unwrap();
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "console.log('exit-str', (process.exitCode = '2', process.exitCode === 2));\
             process.exitCode = undefined; console.log('exit-undef', process.exitCode === undefined);\
             const b = process.binding('util');\
             console.log('bind-keys', Object.keys(b).length === 16 && b.isPromise === require('node:util').types.isPromise);\
             console.log('config-frozen', Object.isFrozen(process.config));\
             let rc = 0, uc = 0;\
             const o = { ref() { rc++; }, unref() { uc++; } }; process.ref(o); process.unref(o);\
             console.log('ref-legacy', rc === 1 && uc === 1);\
             let rc2 = 0; const o2 = { [Symbol.for('nodejs.ref')]() { rc2++; } }; process.ref(o2);\
             console.log('ref-sym', rc2 === 1);\
             console.log('gb-same', process.getBuiltinModule('node:os') === require('node:os'));\
             const code = (f) => { try { f(); } catch (e) { return e.code; } return 'NO-THROW'; };\
             console.log('exit-empty', code(() => { process.exitCode = ''; }));\
             console.log('exit-obj', code(() => { process.exitCode = {}; }));\
             console.log('exit-float', code(() => { process.exitCode = 2.1; }));\
             console.log('bind-miss', (() => { try { process.binding('test'); } catch (e) { return e.message; } return ''; })());\
             console.log('sms-num', code(() => process.setSourceMapsEnabled(1)));\
             console.log('gb-num', code(() => process.getBuiltinModule(1)));\
             console.log('exit-del', (() => { try { delete process.exitCode; } catch (e) { return e.message; } return ''; })());\
             console.log('gb-test', process.getBuiltinModule('test') === undefined);\
             console.log('gb-internal', process.getBuiltinModule('internal/util') === undefined);\
             process.exitCode = undefined;",
        ],
        &dir,
    );
    assert!(ok, "out: {out}");
    for line in [
        "exit-str true",
        "exit-undef true",
        "bind-keys true",
        "config-frozen true",
        "ref-legacy true",
        "ref-sym true",
        "gb-same true",
        "exit-empty ERR_INVALID_ARG_TYPE",
        "exit-obj ERR_INVALID_ARG_TYPE",
        "exit-float ERR_OUT_OF_RANGE",
        "bind-miss No such module: test",
        "sms-num ERR_INVALID_ARG_TYPE",
        "gb-num ERR_INVALID_ARG_TYPE",
        "exit-del Cannot delete property 'exitCode' of #<process>",
        "gb-test true",
        "gb-internal true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_process_r9b_faces() {
    // P2-process R9-B：--disable-warning 过滤 + uncaughtExceptionMonitor 先行 +
    // _fatalException=undefined 即 exit 6（node 原文口径）。
    // 正常：monitor 与 uncaughtException 同 err 同 origin 依次触发。
    // 报错：monitor 内再抛即新错 fatal（退出码 7）。
    // 边界：--disable-warning 按 code/name 精确过滤；_fatalException 置 undefined
    //   即回落默认 exit 6。
    let dir = assert_fs::TempDir::new().unwrap();
    // 正常：in-process monitor/uncaught 双触发（timer 抛错走 drain 分发）。
    let (ok, out, _) = wjs(
        &[
            "--eval",
            "process.on('uncaughtExceptionMonitor', (e, o) => console.log('mon', e.message, o));\
             process.on('uncaughtException', (e, o) => console.log('un', e.message, o));\
             setTimeout(() => { throw new Error('boom'); }, 5);",
        ],
        &dir,
    );
    assert!(ok, "out: {out}");
    assert!(out.lines().any(|l| l == "mon boom uncaughtException"), "out: {out}");
    assert!(out.lines().any(|l| l == "un boom uncaughtException"), "out: {out}");
    // 边界：--disable-warning=DEP1 只滤 DEP1（DEP2 照打）。
    let out = winterjs2()
        .args([
            "--disable-warning=DEP1",
            "--eval",
            "process.emitWarning('a', { code: 'DEP1', type: 'DeprecationWarning' });\
             process.emitWarning('b', { code: 'DEP2', type: 'DeprecationWarning' });",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let se = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {se}");
    assert!(!se.contains("[DEP1]") && se.contains("[DEP2]"), "stderr: {se}");
    // 边界：_fatalException 置 undefined 即 exit 6（CJS 入口抛错回落默认；
    // .mjs 走模块路径无分发，套件本体亦为 .js）。
    dir.child("fatal6.js")
        .write_str("process._fatalException = undefined;\nthrow new Error('ok');\n")
        .unwrap();
    let out = winterjs2().arg("--run").arg("fatal6.js").current_dir(dir.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(6), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    dir.close().unwrap();
}

#[test]
fn p2_process_exit_detached_receiver() {
    // base16回归：process.exit 裸传当回调（cluster-net-listen 套件 listen(process.exit)，
    // this=server）——真机与 receiver 无关；mock reallyExit 仍生效（really-exit 口径）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("e.mjs").write_str("const e = process.exit; e(3);\n").unwrap();
    let out = winterjs2().arg("--run").arg(dir.child("e.mjs").path()).output().unwrap();
    assert_eq!(out.status.code(), Some(3), "detached exit code");
    let out = winterjs2().args(["--eval",
        "process.reallyExit = (c) => console.log('mocked', c); process.exit(7);"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("mocked 7"),
        "reallyExit mock still honored: {}", String::from_utf8_lossy(&out.stdout));
    dir.close().unwrap();
}

#[test]
fn p2_process_warning_throw_deprecation_async() {
    // base16回归：throwDeprecation 不走同步抛——nextTick 异步抛经 uncaughtException
    // 交付（真机实测；旧同步抛致套件 catch 误杀）。
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "process.throwDeprecation = true; process.on('uncaughtException', (e) => console.log('caught', String(e))); try { process.emitWarning('test', 'DeprecationWarning'); console.log('no-sync-throw'); } catch { console.log('sync-throw BAD'); } process.throwDeprecation = false;"]));
    assert!(out.contains("no-sync-throw"), "must not throw sync: {out}");
    assert!(out.contains("caught DeprecationWarning: test"), "async uncaught delivery: {out}");
}
