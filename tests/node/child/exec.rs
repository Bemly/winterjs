//! tests/node/child/exec.rs — exec/sync 表面（对齐 src/builtins/node/child.rs）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

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

#[test]
fn phase10f_child_exec_shell_self_and_timeout() {
    // 10f：exec 族自举翻译 env 间接形（${VAR}/$NODE + 裸文件 → --run/--eval，
    // escapePOSIXShell 形）、timeout/killSignal 错形（killed/code=null/signal）、
    // encoding 'invalid' 落 Buffer、exec 无回调 live child。标签互不为子串
    //（§4.42）：sigkilltag/sigtermtag/encbuf/encstr。正常+报错+边界。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("exec-self.mjs");
    file.write_str(
        r#"
import { exec, execFile } from "node:child_process";
import assert from "node:assert";
const self = process.execPath;
const selfFile = new URL("child-ran.mjs", import.meta.url).pathname;
// env 间接形（escapePOSIXShell 口径）+ 裸文件自举：子进程即自身
const env = { ...process.env, ESC: self, ESCF: selfFile };
exec(`"${"$"}{ESC}" "${"$"}{ESCF}" child`, { env }, (e, stdout) => {
  console.log("envself", e === null, stdout.trim() === "child-ran");
});
// 裸文件形（直接路径）：--run 翻译 + argv[2] 可见
execFile(self, [selfFile, "arg-child"], (e, stdout) => {
  console.log("fileself", e === null, stdout.trim() === "arg-child");
});
// timeout 到点：killed=true、code=null、signal 缺省 SIGTERM
exec("sleep 5", { timeout: 60 }, (e) => {
  console.log("sigtermtag", e.killed === true, e.code === null, e.signal === "SIGTERM");
  // killSignal 自定：SIGKILL
  exec("sleep 5", { timeout: 60, killSignal: "SIGKILL" }, (e2) => {
    console.log("sigkilltag", e2.killed === true, e2.code === null, e2.signal === "SIGKILL");
    // timeout 未到点：正常收
    exec("echo notexp", { timeout: 60000 }, (e3, stdout3) => {
      console.log("notexp", e3 === null, stdout3.trim() === "notexp");
      // encoding 'invalid' 落 Buffer（真机 exec 异步口径）+ utf8 串
      exec("echo enc", { encoding: "invalid" }, (e4, out4) => {
        console.log("encbuf", e4 === null, out4 instanceof Buffer, out4.toString().trim() === "enc");
        exec("echo enc", {}, (e5, out5) => {
          console.log("encstr", e5 === null, typeof out5 === "string", out5.trim() === "enc");
          // 无回调：live child（真机 exec/execFile 口径，不抛）
          const c = exec("echo nocb");
          console.log("nocb", typeof c.pid === "number", typeof c.stdout.on === "function");
          setTimeout(() => process.exit(0), 300);
        });
      });
    });
  });
});
console.log("child-ran-marker");
"#,
    )
    .unwrap();
    // child 分支：argv[2] 回显（自举翻译后的 argv 形）
    let child_file = dir.child("child-ran.mjs");
    child_file.write_str(
        r#"
import { exec, execFile } from "node:child_process";
const self = process.execPath;
if (process.argv[2] === "child") {
  console.log("child-ran");
} else if (process.argv[2] === "arg-child") {
  console.log("arg-child");
} else {
  void self;
}
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
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in [
        "child-ran-marker",
        "envself true true",
        "fileself true true",
        "sigtermtag true true true",
        "sigkilltag true true true",
        "notexp true true",
        "encbuf true true true",
        "encstr true true true",
        "nocb true true",
    ] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_child_g5_validators_and_readable() {
    // G5-3：\0 横向校验（file/args/env/cwd/shell/command 全面 code 名）+
    // `-p` 自举（promisified 套件）+ stdio ipc 门（单裸/双 ipc）+
    // paused read（flush-stdio 套件 readable+read 循环）。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const cp = await import("node:child_process");
const codes = [];
const t = (fn) => { try { const c = fn(); if (c && c.on) c.on("error", () => {}); codes.push("no-throw"); } catch (e) { codes.push(e.code); } };
t(() => cp.spawn("B\0XXX"));
t(() => cp.spawn("echo", ["a", "B\0"]));
t(() => cp.spawn("echo", [], { env: { A: "B\0" } }));
t(() => cp.spawn("echo", [], { cwd: "a\0b" }));
t(() => cp.spawnSync("B\0"));
t(() => cp.execSync("echo \0"));
t(() => cp.exec("echo \0", () => {}));
t(() => cp.execFile("echo\0", () => {}));
console.log("nullbytes", codes.every((c) => c === "ERR_INVALID_ARG_VALUE"));
t(() => cp.spawn("echo", [], { stdio: "ipc" }));
console.log("bare-ipc", codes[codes.length - 1] === "ERR_INVALID_ARG_VALUE");
t(() => cp.spawn("echo", [], { stdio: ["pipe", "pipe", "pipe", "ipc", "ipc"] }));
console.log("dbl-ipc", codes[codes.length - 1] === "ERR_IPC_ONE_PIPE");
const r = await new Promise((res, rej) => cp.execFile(process.execPath, ["-p", "42"], (e, so, se) => e ? rej(e) : res({ stdout: so, stderr: se })));
console.log("dash-p", r.stdout === "42\n", r.stderr === "");
const q = cp.spawn("echo", ["123"]);
const bufs = [];
q.stdout.on("readable", () => { let b; while ((b = q.stdout.read()) !== null) bufs.push(b); });
await new Promise((res) => q.on("close", res));
console.log("readable", Buffer.concat(bufs).toString().trim() === "123");"#]));
    assert_eq!(
        out,
        "nullbytes true\nbare-ipc true\ndbl-ipc true\ndash-p true true\nreadable true\n",
        "g5-validators: {out}"
    );
}

#[test]
fn phase10f_child_g5_surface_batch2() {
    // G5-4：removeAllListeners/二次 disconnect 抛错/uid-gid EPERM/pipe 透传/
    // fork send 参数校验（message 缺席/非法型/options 非对象/句柄拒收）。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("g5b2.mjs");
    file.write_str(
        r#"
import { spawn, fork } from "node:child_process";
import assert from "node:assert";
// removeAllListeners（sigwinch 套件形：清 exit 后 kill 不再触发旧监听）
{
  const c = spawn("sleep", ["30"], { stdio: "ignore" });
  let fired = false;
  c.on("exit", () => { fired = true; });
  c.removeAllListeners("exit");
  c.on("exit", () => console.log("batch2-exit-clean", fired === false));
  c.kill("SIGKILL");
}
// uid/gid 非特权抛 EPERM（真机同步抛；message 正则匹配）
{
  let uidOk = false, gidOk = false;
  try { spawn("echo", ["x"], { uid: 0 }); } catch (e) { uidOk = /EPERM/.test(e.message); }
  try { spawn("echo", ["x"], { gid: 0 }); } catch (e) { gidOk = /EPERM/.test(e.message); }
  const root = typeof process.getuid === "function" ? process.getuid() === 0 : true;
  console.log("batch2-idcheck", root || (uidOk && gidOk));
}
// pipe 最小面（stderr.pipe 透传；stdio-inherit 套件形）
{
  const c = spawn("echo", ["piped"]);
  assert.strictEqual(typeof c.stderr.pipe, "function");
  const dest = { write() {}, end() {} };
  assert.strictEqual(c.stderr.pipe(dest), dest);
  console.log("batch2-pipeface", true);
  c.on("close", () => {});
}
// fork send 参数校验（send-type-error 套件；子端 message 常驻监听保活）
{
  const mod = new URL("g5b2-child.mjs", import.meta.url).pathname;
  const t = fork(mod, []);
  t.on("message", () => {});
  // 注：此处不挂 error 监听——二次 disconnect 的 ERR_IPC_DISCONNECTED 经
  // error 发射，无监听即同步抛（套件 assert.throws 形）；挂了反而被吞。
  const codes = [];
  const try_ = (label, fn) => { try { fn(); codes.push("no-throw"); } catch (e) { codes.push(e.code); } };
  try_("msg-undef", () => t.send(undefined));
  console.log("batch2-sendmsg", codes[0] === "ERR_MISSING_ARGS");
  try_("opt-null", () => t.send("msg", null, null));
  console.log("batch2-sendopt", codes[1] === "ERR_INVALID_ARG_TYPE");
  try_("handle-meow", () => t.send("msg", "meow", undefined));
  console.log("batch2-sendhandle", codes[2] === "ERR_INVALID_HANDLE_TYPE");
  // 二次 disconnect 抛 ERR_IPC_DISCONNECTED（disconnect 套件形；error 发射转同步抛）
  t.disconnect();
  let d2 = "";
  try { t.disconnect(); } catch (e) { d2 = e.code; }
  console.log("batch2-disconnect2", d2 === "ERR_IPC_DISCONNECTED");
  setTimeout(() => process.exit(0), 500);
}
"#,
    )
    .unwrap();
    dir.child("g5b2-child.mjs")
        .write_str(r#"process.on("message", () => {}); setTimeout(() => {}, 30000);"#)
        .unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in [
        "batch2-exit-clean true",
        "batch2-idcheck true",
        "batch2-pipeface true",
        "batch2-sendmsg true",
        "batch2-sendopt true",
        "batch2-sendhandle true",
        "batch2-disconnect2 true",
    ] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    dir.close().unwrap();
}

