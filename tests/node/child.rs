//! tests/node/child.rs — 对齐 src/builtins/node/child.rs（node:child_process spawn/fork）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

#[test]
fn phase4_cp_exec_spawn_sync() {
    // 回显/管道输入/env/cwd + 非零抛错形状 + spawn 缺失命令。
    // （真机口径：execSync/spawnSync 缺省 Buffer；旧 .trim() 直调为伪语义，已翻转。）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "cp.mjs",
        r#"
import { execSync, spawnSync } from "node:child_process";
console.log(execSync("echo hi").toString().trim());
console.log(execSync("cat", { input: "piped" }).toString().trim());
const r = spawnSync("echo", ["a", "b"], { env: { PATH: process.env.PATH } });
console.log(r.status, r.signal, r.stdout.toString().trim(), r.pid > 0, r.error);
const e = spawnSync("definitely-missing-binary-xyz", []);
console.log(e.status, e.error.code);
try {
  execSync("exit 3");
  console.log("no-throw");
} catch (err) {
  console.log("code:", err.status, err.signal);
}
"#,
    );
    assert_eq!(
        out, "hi\npiped\n0 null a b true undefined\nnull ENOENT\ncode: 3 null\n",
        "child_process: {out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase4_cp_timeout_and_shell() {
    // 超时杀（真机缺省 SIGTERM；旧 SIGKILL 形为伪语义，已翻转）+ shell:false 直跑。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawnSync, execSync } = await import("node:child_process"); const r = spawnSync("sleep", ["5"], { timeout: 200 }); console.log(r.signal, !!r.error); console.log(execSync("echo noshell", { shell: false }).toString().trim());"#]));
    assert_eq!(out, "SIGTERM true\nnoshell\n", "timeout: {out}");
}

#[test]
fn phase4_spawn_async_exit_close_kill() {
    // exit+close 双调 + kill 中断（SIGTERM 形）。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawn } = await import("node:child_process"); const log = []; const c = spawn("echo", ["async-hi"], { stdio: "ignore" }); console.log("pid:", c.pid > 0, "killed:", c.killed); c.on("exit", (code) => log.push("exit:" + code)); c.on("close", () => { log.push("close"); console.log(log.join("|")); });"#]));
    assert_eq!(
        out, "pid: true killed: false\nexit:0|close\n",
        "spawn: {out}"
    );
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawn } = await import("node:child_process"); const log = []; const c = spawn("sleep", ["30"]); c.on("exit", (code, signal) => log.push("exit:" + signal)); c.on("close", () => { log.push("close"); console.log(log.join("|")); }); setTimeout(() => console.log("killed:", c.kill()), 100);"#]));
    assert_eq!(out, "killed: true\nexit:SIGTERM|close\n", "kill: {out}");
}

#[test]
#[cfg(unix)]
fn node_spawn_pipe_streams() {
    // pipe：echo 回环 + cat stdin 写/关 + exit/close（--eval 经动态 import，见既有 spawn 用例）。
    // 注意：close 监听必须在 read 之前注册（echo 退出快，否则分发时无监听即摘除，后续 await 永挂）。
    // 10f 起 stdout/stderr 为 legacy Readable 面（真机 `Readable`：setEncoding +
    // on('data')；旧 Web getReader 用法编码的是实现偏差，§4.65 翻转）。stdin 仍
    // Web WritableStream（legacy Writable 记偏差）。
    let code = r#"const { spawn } = await import("node:child_process");
const c = spawn("/bin/echo", ["hi-echo"], { stdio: ["ignore", "pipe", "ignore"] });
const closed = new Promise((res) => c.on("close", res));
let got = "";
c.stdout.setEncoding("utf8");
c.stdout.on("data", (d) => { got += d; });
await closed;
if (got.trim() !== "hi-echo") throw new Error("echo failed: " + JSON.stringify(got));
const c2 = spawn("cat", [], { stdio: "pipe" });
const closed2 = new Promise((res) => c2.on("close", res));
const w = c2.stdin.getWriter();
await w.write("hi-stdin");
await w.close();
let out = "";
c2.stdout.setEncoding("utf8");
c2.stdout.on("data", (d) => { out += d; });
await closed2;
if (out !== "hi-stdin") throw new Error("cat failed: " + JSON.stringify(out));
console.log("pipe-ok");
"#;
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", code])),
        "pipe-ok\n"
    );
}

// ── publish 真 PUT（stub registry 接 PUT；token 经 npmrc）──────────────────────

#[test]
fn phase9e_child_corners() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { exec, execFile, execFileSync, spawn } from "node:child_process";
console.log("sync", execFileSync("echo", ["sync-ok"]).toString().trim() === "sync-ok");
exec("echo hello-exec", (e, stdout) => {
  console.log("exec", e === null && stdout.trim() === "hello-exec");
  execFile("echo", ["hello-file"], (e2, stdout2) => {
    console.log("execFile", e2 === null && stdout2.trim() === "hello-file");
    exec("exit 3", (e3, o3, err3) => {
      console.log("execfail", e3 !== null && e3.code === 3);
      console.log("done");
    });
  });
});
const p = spawn("echo", ["live"]);
console.log("meta", p.spawnfile === "echo", p.spawnargs.join(",") === "live", p.exitCode === null);
p.on("exit", () => console.log("exit", p.exitCode === 0));
p.on("close", () => console.log("close", p.exitCode === 0));
try { p.send("x"); } catch (e) { console.log("send", e.code === "ERR_NOT_SUPPORTED"); }
"#,
    );
    assert!(out.contains("sync true"), "out: {out}");
    assert!(out.contains("exec true"), "out: {out}");
    assert!(out.contains("execFile true"), "out: {out}");
    assert!(out.contains("execfail true"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    assert!(out.contains("meta true true true"), "out: {out}");
    assert!(out.contains("exit true"), "out: {out}");
    assert!(out.contains("close true"), "out: {out}");
    assert!(out.contains("send true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9m_child_fork_ipc() {
    // 正常：fork 回显（双向消息 + argv + spawnfile/spawnargs + connected/
    // channel/stdin-null）→ disconnect（事件 + 后续 send false）→ exit 0；
    // 报错：无参 TypeError + 缺失文件 error 事件 + exit 非零；
    // 边界：send-after-disconnect 报 ERR_IPC_CHANNEL_CLOSED（异步）；
    // kill-after-exit 回 false；once/off 生效；spawn 子进程 on(message) 照旧抛。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("echo-child.mjs")
        .write_str("process.on('message', (m) => { process.send({ echo: m, argv1: process.argv[2], connected: process.connected }); });\n")
        .unwrap();
    let child_abs = dir.path().join("echo-child.mjs");
    let child_str = child_abs.to_string_lossy().into_owned();
    let file = dir.child("p.mjs");
    file.write_str(&format!(
        r#"
import {{ fork, spawn }} from "node:child_process";
const c = fork({child_str:?});
console.log("meta", c.spawnfile === process.execPath, c.spawnargs[1].endsWith("echo-child.mjs"), c.spawnargs[2] === undefined, c.connected, c.channel !== null, c.stdin === null, c.stdout === null);
c.on("message", (m) => {{
  if (m.ready === undefined) {{
    console.log("MSG", JSON.stringify(m.echo) === JSON.stringify({{ hello: 1 }}), m.argv1 === undefined, m.connected);
    c.disconnect();
    console.log("after-disc", c.connected === false, c.send({{ late: 1 }}) === false);
  }}
}});
c.on("disconnect", () => console.log("DISC-EV"));
c.on("error", (e) => console.log("ERR-EV", e.code));
c.on("exit", (code) => console.log("EXIT-EV", code, c.exitCode));
c.send({{ hello: 1 }});
console.log("send-open", true);
// spawn 子进程的 message 监听照旧明错（非 fork 无通道）。
try {{ spawn("echo", ["x"]).on("message", () => {{}}); console.log("SPAWN-NO-ERR"); }}
catch (e) {{ console.log("spawn-msg-err", e.code); }}
// once/off（用 fork 路径永不触发的 spawn 事件占位：未知事件名按设计抛
// NotSupportedError，且单槽位 once 会顶掉同名常驻监听，故不用 error/exit）。
let n = 0;
const inc = () => {{ n++; }};
c.once("spawn", inc);
c.off("spawn", inc);
setTimeout(() => console.log("once-off", n === 0, c.kill() === false), 2500);
"#,
    ))
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "meta true true true true true true true",
        "MSG true true true",
        "send-open true",
        "DISC-EV",
        "after-disc true true",
        "ERR-EV ERR_IPC_CHANNEL_CLOSED",
        "spawn-msg-err ERR_NOT_SUPPORTED",
        "once-off true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    assert!(
        out.lines().any(|l| l == "EXIT-EV 0 0"),
        "clean exit missing:\n{out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase9m_child_fork_errors() {
    // 报错：无参 TypeError 带码；缺失文件 error 事件 + exit 非零 + kill 语义。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"
import { fork } from "node:child_process";
try { fork(); console.log("NO-ERR"); }
catch (e) { console.log("bad-arg", e.constructor.name, e.code); }
const c = fork("/no/such/fork-target-9m.mjs");
c.on("error", (e) => console.log("ERR-EV", typeof (e && e.message) === "string"));
c.on("exit", (code) => console.log("EXIT-EV", code !== 0, c.exitCode !== 0, c.kill() === false));
"#,
    )
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in ["bad-arg TypeError ERR_INVALID_ARG_TYPE", "ERR-EV true", "EXIT-EV true true true"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_spawn_default_pipe_close_args() {
    // 10f：spawn 缺省 stdio = pipe×3（node 口径——child.stderr 非 null 可
    // setEncoding/on('data')）；exit/close 事件 node 双参 (code, signal)，
    // 用户代码解构可收（§4.101）。正常+报错+边界。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("s.mjs");
    file.write_str(
        r#"
import { spawn } from "node:child_process";
// 缺省 stdio：stdout/stderr 为 legacy Readable（pipe），stdin 为 WritableStream
const c = spawn("/bin/sh", ["-c", "echo out-hi; echo err-hi 1>&2"]);
const closed = new Promise((res) => c.on("close", (code, signal) => {
  console.log("close", code === 0, signal === null);
  res();
}));
let out = "", err = "";
c.stdout.setEncoding("utf8"); c.stderr.setEncoding("utf8");
c.stdout.on("data", (d) => { out += d; });
c.stderr.on("data", (d) => { err += d; });
await closed;
console.log("pipes", out.trim() === "out-hi", err.trim() === "err-hi");
console.log("stdin-writable", typeof c.stdin.getWriter === "function");
// exit 双参：signal 死亡时 (null, 'SIGTERM')，正常退出 (0, null)
const c2 = spawn("sleep", ["30"]);
c2.on("exit", (code, signal) => console.log("exit-sig", code === null, signal === "SIGTERM"));
setTimeout(() => console.log("killed", c2.kill() === true), 60);
await new Promise((r) => setTimeout(r, 200));
"#,
    )
    .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "close true true",
        "pipes true true",
        "stdin-writable true",
        "exit-sig true true",
        "killed true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_exec_live_handle() {
    // 10f：exec/execFile 换 node 架构（spawn+收集+close 回调，返回 live
    // ChildProcess）——pid 同步可见、ENOENT 死句柄 pid undefined、
    // ERR_CHILD_PROCESS_STDIO_MAXBUFFER 错误码。标签互不为子串（§4.42）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p10f.mjs",
        r#"
import { exec, execFile } from "node:child_process";

// live 句柄：pid 同步可见（shell），回调 (null, stdout, "")
const c = exec("echo hello-exec", (e, stdout, stderr) => {
  console.log("live-cb", e === null, stdout.trim() === "hello-exec", stderr === "", typeof c.pid === "number");
});
console.log("live-sync", typeof c.pid === "number", typeof c.stdout.on === "function");

// execFile：args 数组 + 分离 stdout
execFile("echo", ["hello-file"], (e, stdout) => {
  console.log("file-cb", e === null, stdout.trim() === "hello-file");
});

// ENOENT：死句柄 pid undefined（真机口径）+ err.code/cmd 挂载
const d = execFile("does-not-exist-cmd", (err) => {
  console.log("enoent", err.code === "ENOENT", typeof err.cmd === "string", err.cmd.includes("does-not-exist-cmd"), typeof d.pid === "undefined");
});

// 非 0 退出：err.code = 退出码数字（真机口径，无 status 属性）
exec("exit 3", (e) => {
  console.log("exitcode", e !== null, e.code === 3, e.status === undefined, e.killed === false);
});
"#,
    );
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in [
        "live-sync true true",
        "live-cb true true true true",
        "file-cb true true",
        "enoent true true true true",
        "exitcode true true true true",
    ] {
        assert!(text.lines().any(|l| l.starts_with(line)), "missing: {line}\nout: {text}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_child_sync_surface() {
    // 10f child 同步族口径（真机 26.8.2 对拍）：选项校验族 + 错误形状
    // （syscall/errno/message/path/pid/output）+ 自举翻译（-e/裸文件）+
    // 缺省 Buffer + killSignal/timeout/ETIMEDOUT + ENOBUFS。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import cs from "node:child_process";
import fs from "node:fs";
import { getSystemErrorName } from "node:util";
// — 校验族抽样 —
for (const [k, v, code] of [["cwd", 0, "ERR_INVALID_ARG_TYPE"], ["detached", 1, "ERR_INVALID_ARG_TYPE"], ["uid", -3.1, "ERR_OUT_OF_RANGE"], ["shell", {}, "ERR_INVALID_ARG_TYPE"], ["argv0", 0, "ERR_INVALID_ARG_TYPE"], ["timeout", 3.1, "ERR_OUT_OF_RANGE"], ["timeout", "x", "ERR_INVALID_ARG_TYPE"], ["maxBuffer", -1, "ERR_OUT_OF_RANGE"], ["maxBuffer", true, "ERR_INVALID_ARG_TYPE"], ["killSignal", "NOSUCH", "ERR_UNKNOWN_SIGNAL"], ["killSignal", 0, "ERR_UNKNOWN_SIGNAL"], ["killSignal", [], "ERR_INVALID_ARG_TYPE"]]) {
  try { cs.spawnSync("nope_xyz", { [k]: v }); console.log("BAD no-throw", k); }
  catch (e) { console.log("v", k, e.code); }
}
// — 错误形状 —
const e = cs.spawnSync("not_a_real_command_xyz", ["a"]).error;
console.log("enoent", e.code, e.errno, getSystemErrorName(e.errno), e.syscall, e.path, JSON.stringify(e.spawnargs));
const e2 = cs.spawnSync("not_a_real_command_xyz").error;
console.log("enoent2", JSON.stringify(e2.spawnargs));
// — 自举 + 缺省 Buffer —
const r = cs.spawnSync(process.execPath, ["-e", 'console.log("self-ok")']);
console.log("self", r.status, r.error, r.stdout.toString().trim(), Buffer.isBuffer(r.stdout), JSON.stringify(r.output && r.output.map((x) => x && x.toString())));
console.log("inf", cs.spawnSync(process.execPath, ["-e", "1"], { maxBuffer: Infinity }).error);
// — 超时/kill 信号 —
const t = cs.spawnSync("sleep", ["5"], { timeout: 200 });
console.log("tmout", t.error && t.error.code, t.error && t.error.errno, t.status, t.signal);
const t2 = cs.spawnSync("sleep", ["5"], { timeout: 200, killSignal: "SIGKILL" });
console.log("tmout2", t2.signal);
// — maxBuffer 越限 —
const m = cs.spawnSync(process.execPath, ["-e", "console.log('a'.repeat(100))"], { maxBuffer: 10 });
console.log("maxbuf", m.error && m.error.code, m.error && m.error.errno, m.stdout.length > 10);
// — args null 不吞 opts —
const n = cs.spawnSync("pwd", null, { cwd: "/tmp" });
console.log("nullargs", n.status === 0, n.stdout.toString().trim() === fs.realpathSync("/tmp"));
// — argv0 回显（自举子报真 argv0；argv0 选项改写；错型校验） —
const a0 = cs.spawnSync(process.execPath, ["-e", "console.log(process.argv0)"]);
console.log("argv0-dflt", a0.stdout.toString().trim() === process.execPath);
const a1 = cs.spawnSync(process.execPath, ["-e", "console.log(process.argv0)"], { argv0: "custom0" });
console.log("argv0-set", a1.stdout.toString().trim() === "custom0");
try { cs.spawnSync("nope_xyz", { argv0: [] }); console.log("BAD argv0-nothrow"); }
catch (e) { console.log("argv0-err", e.code); }
// — execSync 缺省 Buffer —
console.log("exec-buf", Buffer.isBuffer(cs.execSync("echo hi")));
// — error.spawnargs 为参数数组 —
console.log("spawnargs", JSON.stringify(cs.spawnSync("nope_xyz", ["a", "b"]).error.spawnargs));
"#,
    );
    for line in [
        "v cwd ERR_INVALID_ARG_TYPE",
        "v detached ERR_INVALID_ARG_TYPE",
        "v uid ERR_OUT_OF_RANGE",
        "v shell ERR_INVALID_ARG_TYPE",
        "v argv0 ERR_INVALID_ARG_TYPE",
        "v timeout ERR_OUT_OF_RANGE",
        "v timeout ERR_INVALID_ARG_TYPE",
        "v maxBuffer ERR_OUT_OF_RANGE",
        "v maxBuffer ERR_INVALID_ARG_TYPE",
        "v killSignal ERR_UNKNOWN_SIGNAL",
    ] {
        assert!(out.lines().any(|l| l.starts_with(line)), "missing: {line}\nout: {out}");
    }
    assert!(out.lines().any(|l| l == "enoent ENOENT -2 ENOENT spawnSync not_a_real_command_xyz not_a_real_command_xyz [\"a\"]"), "out: {out}");
    assert!(out.lines().any(|l| l == "enoent2 []"), "out: {out}");
    assert!(out.contains("self 0 undefined self-ok true"), "out: {out}");
    assert!(out.contains("[null,\"self-ok\\n\",\"\"]"), "out: {out}");
    assert!(out.contains("inf undefined"), "out: {out}");
    assert!(out.contains("tmout ETIMEDOUT -60 null SIGTERM"), "out: {out}");
    assert!(out.contains("tmout2 SIGKILL"), "out: {out}");
    assert!(out.contains("maxbuf ENOBUFS -55 true"), "out: {out}");
    assert!(out.contains("nullargs true true"), "out: {out}");
    assert!(out.contains("argv0-dflt true"), "out: {out}");
    assert!(out.contains("argv0-set true"), "out: {out}");
    assert!(out.contains("argv0-err ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("exec-buf true"), "out: {out}");
    assert!(out.contains("spawnargs [\"a\",\"b\"]"), "out: {out}");
    assert!(!out.contains("BAD "), "out: {out}");
    dir.close().unwrap();
}
