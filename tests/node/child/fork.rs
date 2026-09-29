//! tests/node/child/fork.rs — fork/ipc/退出（对齐 src/builtins/node/child.rs）。

use crate::common::*;
use assert_fs::prelude::*;

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
    let out = winterjs2()
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
    let out = winterjs2()
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
fn phase10f_child_stdin_legacy_and_fork_silent() {
    // 10f：stdin legacy Writable 面（write/end/writable/readable）、fork
    // silent 管形流（pipe 可用）、fork abort 合成 exit(null, killSignal)、
    // exec maxBuffer 截断 RangeError。标签互不为子串（§4.42）。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("stdin-fork.mjs");
    file.write_str(
        r#"
import { spawn, fork, exec } from "node:child_process";
import assert from "node:assert";
const self = process.execPath;
// stdin：legacy Writable（cat 回显）
{
  const cat = spawn("cat");
  assert.strictEqual(cat.stdin.writable, true);
  assert.strictEqual(cat.stdin.readable, false);
  let response = "";
  cat.stdin.write("hello");
  cat.stdin.write(" ");
  cat.stdin.write("world");
  cat.stdin.end();
  cat.stdout.setEncoding("utf8");
  cat.stdout.on("data", (chunk) => { response += chunk; });
  cat.on("exit", (code) => console.log("cat-exit", code === 0));
  cat.on("close", () => console.log("cat-close", response === "hello world"));
}
// fork silent：stdout/stderr 管形流 pipe 可用（无数据面偏差记档）
{
  const mod = new URL("fork-echo-child.mjs", import.meta.url).pathname;
  const child = fork(mod, [], { silent: true });
  console.log("fork-silent", typeof child.stdout.pipe === "function", typeof child.stderr.pipe === "function", child.stdout !== child.stderr);
  child.on("exit", (code) => console.log("fork-exit", code === 0));
  // node silent 套件同款：disconnect 关 IPC 通道，子端 parentPort.close → 退出
  //（fork 子端 shim 常驻 message 监听，不 disconnect 即不退）。
  child.disconnect();
}
// fork abort：error(AbortError) + exit(null, SIGKILL)
{
  const mod = new URL("fork-slow-child.mjs", import.meta.url).pathname;
  const ac = new AbortController();
  const child = fork(mod, [], { signal: ac.signal, killSignal: "SIGKILL" });
  child.on("error", (e) => console.log("fork-abort-err", e.name === "AbortError"));
  child.on("exit", (code, sig) => console.log("fork-abort-exit", code === null, sig === "SIGKILL"));
  setTimeout(() => ac.abort(), 50);
}
// exec maxBuffer：截断 + RangeError 码
{
  exec("echo hello world", { maxBuffer: 5 }, (err, stdout) => {
    console.log("maxbuf-cut", err instanceof RangeError, err.code === "ERR_CHILD_PROCESS_STDIO_MAXBUFFER", stdout === "hello");
    setTimeout(() => process.exit(0), 500);
  });
}
"#,
    )
    .unwrap();
    dir.child("fork-echo-child.mjs")
        .write_str(r#"console.log("fork-child-out");"#)
        .unwrap();
    dir.child("fork-slow-child.mjs")
        .write_str(r#"setTimeout(() => {}, 30000);"#)
        .unwrap();
    let out = winterjs2()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in [
        "cat-exit true",
        "cat-close true",
        "fork-silent true true true",
        "fork-exit true",
        "fork-abort-err true",
        "fork-abort-exit true true",
        "maxbuf-cut true true true",
    ] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_entry_failure_open_handle_exit() {
    // §4.70 姊妹（10f 根修）：入口失败（throw / 未处理 rejection）+ 开着的子进程
    // 句柄 = 事件循环永不 idle、循环尾收割永不到的 hang。修后 fatal 检查点提前
    // 跳出：eval 包装路径经 entry reactions 重抛挂载；模块路径经 unhandled 表
    // 检查点。内容断言不走退出码对拍（§4.126③）。
    let out = winterjs2()
        .args(["--eval",
            r#"const { spawn } = await import("node:child_process"); spawn("sleep", ["30"]); throw new Error("boom-handle");"#])
        .output()
        .unwrap();
    assert!(!out.status.success(), "throw+句柄必须非零退出");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("boom-handle"), "err: {err}");
    assert!(!err.contains("unhandled"), "同步 throw 不走 unhandled 通道: {err}");

    let out = winterjs2()
        .args(["--eval",
            r#"const { spawn } = await import("node:child_process"); spawn("sleep", ["30"]); Promise.reject(new Error("rej-handle"));"#])
        .output()
        .unwrap();
    assert!(!out.status.success(), "rejection+句柄必须非零退出");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("unhandled rejection") && err.contains("rej-handle"), "err: {err}");

    // 模块路径（--run）：文件内 spawn + 未处理 rejection，同检查点收敛。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"
import { spawn } from "node:child_process";
spawn("sleep", ["30"]);
Promise.reject(new Error("mod-rej-handle"));
"#,
    )
    .unwrap();
    let out = winterjs2().arg("--run").arg(file.path()).output().unwrap();
    assert!(!out.status.success(), "模块路径必须非零退出");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("mod-rej-handle"), "err: {err}");
    dir.close().unwrap();
}

#[test]
fn phase10f_fork_nonsilent_stdio_null() {
    // 真机 26 逐项：fork 非 silent（stdio 继承）`c.stdout/stderr/stdin === null`
    // 三面（10f 起恢复——silent 才挂管形流；旧构造器 null 缺省被流式面覆盖丢失）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("idle-child.mjs").write_str("process.on('message', () => {});\n").unwrap();
    let child_abs = dir.path().join("idle-child.mjs");
    let child_str = child_abs.to_string_lossy().into_owned();
    let out = stdout_of(&mut winterjs2().args(["--eval", &format!(
        r#"const {{ fork }} = await import("node:child_process");
const c = fork({child_str:?});
console.log("nonsilent", c.stdout === null, c.stderr === null, c.stdin === null);
c.disconnect();"#)]));
    assert_eq!(out, "nonsilent true true true\n", "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10f_child_fork_env_and_internal() {
    // fork env 透传（旧忽略致子复走父分支指数 fork）+ NODE_ 前缀 internalMessage 分流。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("env-child.mjs")
        .write_str("process.send({ marker: process.env.WJS_MARKER ?? null });\n")
        .unwrap();
    dir.child("int-child.mjs")
        .write_str("process.send({ cmd: 'NODE_bar' });\nprocess.send({ cmd: 'fooNODE_' });\n")
        .unwrap();
    let env_child = dir.path().join("env-child.mjs").to_string_lossy().into_owned();
    let int_child = dir.path().join("int-child.mjs").to_string_lossy().into_owned();
    let out = stdout_of(&mut winterjs2().args(["--eval", &format!(
        r#"const {{ fork }} = await import("node:child_process");
const a = fork({env_child:?}, [], {{ env: {{ WJS_MARKER: "m42" }} }});
a.on("message", (m) => {{ console.log("env", m.marker === "m42"); }});
a.on("error", (e) => console.log("env-err", e.code));
const b = fork({int_child:?});
b.on("message", (m) => console.log("msg", m.cmd === "fooNODE_"));
b.once("internalMessage", (m) => console.log("internal", m.cmd === "NODE_bar"));
b.on("error", (e) => console.log("int-err", e.code));
setTimeout(() => console.log("done"), 2500);"#)]));
    for line in ["env true", "msg true", "internal true", "done"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    assert!(!out.contains("-err"), "out: {out}");
    dir.close().unwrap();
}
