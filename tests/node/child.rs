//! tests/node/child.rs — 对齐 src/builtins/node/child.rs（node:child_process spawn/fork）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

#[test]
fn phase4_cp_exec_spawn_sync() {
    // 回显/管道输入/env/cwd + 非零抛错形状 + spawn 缺失命令。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "cp.mjs",
        r#"
import { execSync, spawnSync } from "node:child_process";
console.log(execSync("echo hi").trim());
console.log(execSync("cat", { input: "piped" }).trim());
const r = spawnSync("echo", ["a", "b"], { env: { PATH: process.env.PATH } });
console.log(r.status, r.signal, r.stdout.trim(), r.pid > 0, r.error);
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
    // 超时杀直系（SIGKILL 形）+ shell:false 直跑。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawnSync, execSync } = await import("node:child_process"); const r = spawnSync("sleep", ["5"], { timeout: 200 }); console.log(r.signal, !!r.error); console.log(execSync("echo noshell", { shell: false }).trim());"#]));
    assert_eq!(out, "SIGKILL true\nnoshell\n", "timeout: {out}");
}

#[test]
fn phase4_spawn_async_exit_close_kill() {
    // exit+close 双调 + kill 中断（SIGTERM 形）。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawn } = await import("node:child_process"); const log = []; const c = spawn("echo", ["async-hi"], { stdio: "ignore" }); console.log("pid:", c.pid > 0, "killed:", c.killed); c.on("exit", (e) => log.push("exit:" + e.status)); c.on("close", () => { log.push("close"); console.log(log.join("|")); });"#]));
    assert_eq!(
        out, "pid: true killed: false\nexit:0|close\n",
        "spawn: {out}"
    );
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawn } = await import("node:child_process"); const log = []; const c = spawn("sleep", ["30"]); c.on("exit", (e) => log.push("exit:" + e.signal)); c.on("close", () => { log.push("close"); console.log(log.join("|")); }); setTimeout(() => console.log("killed:", c.kill()), 100);"#]));
    assert_eq!(out, "killed: true\nexit:SIGTERM|close\n", "kill: {out}");
}

#[test]
#[cfg(unix)]
fn node_spawn_pipe_streams() {
    // pipe：echo 回环 + cat stdin 写/关 + exit/close（--eval 经动态 import，见既有 spawn 用例）。
    // 注意：close 监听必须在 read 之前注册（echo 退出快，否则分发时无监听即摘除，后续 await 永挂）。
    let code = r#"const { spawn } = await import("node:child_process");
const c = spawn("/bin/echo", ["hi-echo"], { stdio: ["ignore", "pipe", "ignore"] });
const closed = new Promise((res) => c.on("close", res));
const x = await c.stdout.getReader().read();
if (new TextDecoder().decode(x.value).trim() !== "hi-echo") throw new Error("echo failed");
await closed;
const c2 = spawn("cat", [], { stdio: "pipe" });
const closed2 = new Promise((res) => c2.on("close", res));
const w = c2.stdin.getWriter();
await w.write("hi-stdin");
await w.close();
let out = "";
const r = c2.stdout.getReader();
for (;;) { const y = await r.read(); if (y.done) break; out += new TextDecoder().decode(y.value); }
if (out !== "hi-stdin") throw new Error("cat failed: " + JSON.stringify(out));
await closed2;
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
console.log("sync", execFileSync("echo", ["sync-ok"]).trim() === "sync-ok");
exec("echo hello-exec", (e, stdout) => {
  console.log("exec", e === null && stdout.trim() === "hello-exec");
  execFile("echo", ["hello-file"], (e2, stdout2) => {
    console.log("execFile", e2 === null && stdout2.trim() === "hello-file");
    exec("exit 3", (e3, o3, err3) => {
      console.log("execfail", e3 !== null && e3.status === 3);
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
c.on("exit", (e) => console.log("EXIT-EV", e.status, c.exitCode));
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
c.on("exit", (e) => console.log("EXIT-EV", e.status !== 0, c.exitCode !== 0, c.kill() === false));
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
