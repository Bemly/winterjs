//! tests/node/child/spawn.rs — spawn 基础/管道/表面（对齐 src/builtins/node/child.rs）。

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
    // on('data')；旧 Web getReader 用法编码的是实现偏差，§4.65 翻转）。stdin 同批
    // 换 legacy Writable（真机 Socket 写半部：write/end 直调；旧 Web getWriter 形退役）。
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
c2.stdin.write("hi-stdin");
c2.stdin.end();
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
fn phase10f_spawn_default_pipe_close_args() {
    // 10f：spawn 缺省 stdio = pipe×3（node 口径——child.stderr 非 null 可
    // setEncoding/on('data')）；exit/close 事件 node 双参 (code, signal)，
    // 用户代码解构可收（§4.101）。正常+报错+边界。
    // stdin 面 10f 二轮更新：WritableStream → legacy Writable（Socket 写半部
    // write/writable/readable——stdin 套件口径；旧 getWriter 断言已翻转）。
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
console.log("stdin-writable", typeof c.stdin.write === "function", c.stdin.writable === true, c.stdin.readable === false);
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
        "stdin-writable true true true",
        "exit-sig true true",
        "killed true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_child_spawn_abort_and_surface() {
    // 10f：spawn/execFile AbortSignal 面（预中止/中止中/自定义 reason/abort
    // 后 error+exit 形）、spawn PATH 解析与 ENOENT error 事件、cwd file URL、
    // execvp 失败 close(-errno)。标签互不为子串（§4.42）。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("abort-surface.mjs");
    file.write_str(
        r#"
import { spawn, execFile } from "node:child_process";
import assert from "node:assert";
const self = process.execPath;
// spawn 预中止：error(AbortError) + exit(null, SIGTERM)
{
  const ac = new AbortController();
  const c = spawn(self, ["--eval", "setTimeout(() => {}, 30000)"], { signal: ac.signal });
  const got = {};
  c.on("error", (e) => { got.err = e.name + ":" + e.code; });
  c.on("exit", (code, sig) => { got.exit = code === null && sig === "SIGTERM"; console.log("pre-abort", JSON.stringify(got)); });
  ac.abort();
}
// 中止中：自定义 reason cause 透传
{
  const boom = new Error("boom");
  const ac = new AbortController();
  const c = spawn(self, ["--eval", "setTimeout(() => {}, 30000)"], { signal: ac.signal });
  c.on("error", (e) => console.log("mid-abort", e.name === "AbortError", e.cause === boom));
  c.on("exit", (code, sig) => console.log("mid-exit", code === null, sig === "SIGTERM"));
  setTimeout(() => ac.abort(boom), 50);
}
// 非法 signal：同步 ARG_TYPE
assert.throws(() => spawn("echo", ["x"], { signal: {} }), (e) => e.code === "ERR_INVALID_ARG_TYPE" && e.name === "TypeError");
// ENOENT：error 事件（不抛）+ close(-errno) + exit 不发 + pid undefined
{
  const c = spawn("definitely-missing-binary-xyz");
  let sawExit = false;
  c.on("error", (e) => {
    console.log("spawn-err", e.code === "ENOENT", typeof c.pid === "undefined");
    c.on("close", (code) => console.log("spawn-close", code === -2, sawExit === false));
  });
  c.on("exit", () => { sawExit = true; });
}
// execFile 预中止：回调收 AbortError（异步）
{
  const ac = new AbortController();
  ac.abort();
  execFile("sleep", ["5"], { signal: ac.signal }, (e) => {
    console.log("execfile-preabort", e.name === "AbortError", e.code === "ABORT_ERR");
  });
}
// PATH 解析：裸名 spawn 可用
{
  const c = spawn("pwd", [], { cwd: "/tmp" });
  c.on("exit", (code) => console.log("path-resolve", code === 0));
}
// cwd file URL + 坏 URL 同步抛
{
  const c = spawn("pwd", [], { cwd: new URL("file:///tmp") });
  c.on("exit", (code) => console.log("cwd-url", code === 0));
  assert.throws(() => spawn("pwd", [], { cwd: new URL("http://example.com/") }), (e) => e.code === "ERR_INVALID_URL_SCHEME");
}
setTimeout(() => process.exit(0), 500);
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
        "pre-abort {\"err\":\"AbortError:ABORT_ERR\",\"exit\":true}",
        "mid-abort true true",
        "mid-exit true true",
        "spawn-err true true",
        "spawn-close true true",
        "execfile-preabort true true",
        "path-resolve true",
        "cwd-url true",
    ] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_child_constructor_spawn_method() {
    // G5：`new ChildProcess().spawn(options)` 方法面（constructor 套件）+
    // kill 未知信号 ERR_UNKNOWN_SIGNAL + 成功 spawn 后 pid 自有属性。
    // 正常：sleep 起后 hasOwn(pid)/整数/kill() true；报错：四组校验逐项
    // ARG_TYPE；边界：kill('foo') 抛 ERR_UNKNOWN_SIGNAL。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { ChildProcess } = await import("node:child_process");
const codes = [];
for (const bad of [undefined, null, "foo", 0, 1, NaN, true, false]) {
  try { new ChildProcess().spawn(bad); codes.push("no-throw"); }
  catch (e) { codes.push(e.code); }
}
console.log("opt", codes.every((c) => c === "ERR_INVALID_ARG_TYPE"));
for (const bad of [undefined, null, 0, 1, NaN, true, false, {}]) {
  try { new ChildProcess().spawn({ file: bad }); codes.push("no-throw"); }
  catch (e) { codes.push(e.code); }
}
console.log("file", codes.slice(8).every((c) => c === "ERR_INVALID_ARG_TYPE"));
const envBad = [];
for (const bad of [null, 0, 1, NaN, true, false, {}, "foo"]) {
  try { new ChildProcess().spawn({ file: "foo", envPairs: bad, stdio: ["ignore", "ignore", "ignore", "ipc"] }); envBad.push("no-throw"); }
  catch (e) { envBad.push(e.code); }
}
console.log("envpairs", envBad.every((c) => c === "ERR_INVALID_ARG_TYPE"));
const argBad = [];
for (const bad of [null, 0, 1, NaN, true, false, {}, "foo"]) {
  try { new ChildProcess().spawn({ file: "foo", args: bad }); argBad.push("no-throw"); }
  catch (e) { argBad.push(e.code); }
}
console.log("args", argBad.every((c) => c === "ERR_INVALID_ARG_TYPE"));
const c = new ChildProcess();
c.spawn({ file: "sleep", args: ["30"], stdio: "pipe" });
console.log("pid", Object.hasOwn(c, "pid"), Number.isInteger(c.pid));
try { c.kill("foo"); console.log("killsig no-throw"); }
catch (e) { console.log("killsig", e.code === "ERR_UNKNOWN_SIGNAL"); }
console.log("kill", c.kill() === true);
// 多监听并存（spawn-event 套件：两 'spawn' 监听都到；off 只摘命中）。
{
  const { spawn } = await import("node:child_process");
  const p = spawn("echo", ["multi"]);
  let n = 0;
  const a = () => { n++; };
  const b = () => { n++; };
  p.on("spawn", a);
  p.on("spawn", b);
  p.off("spawn", a);
  await new Promise((res) => p.on("close", res));
  console.log("multi", n === 1);
}
// ENOENT 路径 stdio 数组同一性 + spawnargs（spawn-error 套件）。
{
  const { spawn } = await import("node:child_process");
  const e = spawn("definitely-missing-xyz", ["bar"]);
  console.log("stdio-arr", Array.isArray(e.stdio), e.stdio[0] === e.stdin, e.stdio[1] === e.stdout, e.stdio[2] === e.stderr, e.pid === undefined);
  const err = await new Promise((res) => e.on("error", res));
  console.log("err-shape", err.code === "ENOENT", err.syscall === "spawn definitely-missing-xyz", err.path === "definitely-missing-xyz", JSON.stringify(err.spawnargs) === JSON.stringify(["bar"]));
}
// dispose 即 kill（destroy 套件）。
{
  const { spawn } = await import("node:child_process");
  const k = spawn("sleep", ["30"]);
  k[Symbol.dispose]();
  console.log("dispose", k.killed === true);
}"#]));
    assert_eq!(
        out,
        "opt true\nfile true\nenvpairs true\nargs true\npid true true\nkillsig true\nkill true\nmulti true\nstdio-arr true true true true true\nerr-shape true true true true\ndispose true\n",
        "constructor-spawn: {out}"
    );
}

#[test]
fn phase10f_child_spawn_arg_validation() {
    // spawn-typeerror 套件回归：spawn file/args/options/uid-gid 逐项 code +
    // execFile 位移 + fork 位移 + fork 子会话续命（无监听即退/有监听迟发可达）。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawn, execFile, fork } = await import("node:child_process");
const tags = [];
const want = (tag, fn, code) => {
  try { const c = fn(); tags.push(`${tag} no-throw`); try { c.kill(); } catch {} }
  catch (e) { tags.push(`${tag} ${e.code}`); }
};
want("nofile", () => spawn(), "ERR_INVALID_ARG_TYPE");
want("empty", () => spawn(""), "ERR_INVALID_ARG_VALUE");
want("boolargs", () => spawn("ls", true), "ERR_INVALID_ARG_TYPE");
want("nullopts", () => spawn("ls", [], null), "ERR_INVALID_ARG_TYPE");
want("arropts", () => spawn("ls", [], []), "ERR_INVALID_ARG_TYPE");
want("uidbig", () => spawn("ls", [], { uid: 2 ** 63 }), "ERR_OUT_OF_RANGE");
want("gidbig", () => spawn("ls", [], { gid: 2 ** 63 }), "ERR_OUT_OF_RANGE");
want("uidstr", () => spawn("ls", [], { uid: "x" }), "ERR_INVALID_ARG_TYPE");
want("ex-s", () => execFile("ls", "s", {}, () => {}), "ERR_INVALID_ARG_TYPE");
want("ex-opts", () => execFile("ls", [], "s"), "ERR_INVALID_ARG_TYPE");
want("ex-cb", () => execFile("ls", [], {}, "s"), "ERR_INVALID_ARG_TYPE");
want("ex-arropts", () => execFile("ls", [], []), "ERR_INVALID_ARG_TYPE");
want("fk-s", () => fork("m.js", "s"), "ERR_INVALID_ARG_TYPE");
want("fk-opts", () => fork("m.js", [], "s"), "ERR_INVALID_ARG_TYPE");
want("fk-arropts", () => fork("m.js", [], []), "ERR_INVALID_ARG_TYPE");
// 有效组合不抛（spawn 起 echo 即关；execFile 回调形；fork 缺失文件走异步 error）。
for (const [tag, fn] of [
  ["v-spawn", () => spawn("echo", ["x"])],
  ["v-exec", () => execFile("ls", [], () => {})],
  ["v-fork", () => { const c = fork("definitely-missing-xyz.mjs"); c.on("error", () => {}); }],
]) {
  try { const c = fn(); tags.push(`${tag} ok`); try { c.kill(); } catch {} }
  catch (e) { tags.push(`${tag} THROW ${e.code}`); }
}
console.log(tags.join("\n"));"#]));
    for line in [
        "nofile ERR_INVALID_ARG_TYPE",
        "empty ERR_INVALID_ARG_VALUE",
        "boolargs ERR_INVALID_ARG_TYPE",
        "nullopts ERR_INVALID_ARG_TYPE",
        "arropts ERR_INVALID_ARG_TYPE",
        "uidbig ERR_OUT_OF_RANGE",
        "gidbig ERR_OUT_OF_RANGE",
        "uidstr ERR_INVALID_ARG_TYPE",
        "ex-s ERR_INVALID_ARG_TYPE",
        "ex-opts ERR_INVALID_ARG_TYPE",
        "ex-cb ERR_INVALID_ARG_TYPE",
        "ex-arropts ERR_INVALID_ARG_TYPE",
        "fk-s ERR_INVALID_ARG_TYPE",
        "fk-opts ERR_INVALID_ARG_TYPE",
        "fk-arropts ERR_INVALID_ARG_TYPE",
        "v-spawn ok",
        "v-exec ok",
        "v-fork ok",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    // fork 子会话续命：无监听子进程即退（exit 0），有监听迟发可达。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("bye.mjs").write_str("").unwrap();
    dir.child("echo-late.mjs")
        .write_str("process.on('message', (m) => { process.send({ back: m }); });\n")
        .unwrap();
    let bye = dir.path().join("bye.mjs").to_string_lossy().into_owned();
    let late = dir.path().join("echo-late.mjs").to_string_lossy().into_owned();
    let out = stdout_of(&mut winterjs().args(["--eval", &format!(
        r#"const {{ fork }} = await import("node:child_process");
const a = fork({bye:?});
a.on("exit", (code) => console.log("bye-exit", code));
const b = fork({late:?});
b.on("message", (m) => {{ console.log("late-back", m.back.n === 7); b.disconnect(); }});
setTimeout(() => b.send({{ n: 7 }}), 300);
setTimeout(() => console.log("done"), 2500);"#)]));
    for line in ["bye-exit 0", "late-back true", "done"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_child_kill_stdin_surface() {
    // kill 套件回归：kill 即 SIGTERM + stdout/stderr end + kill(0) 只验活不杀 +
    // 父写 stdin/子回显（cat）+ 自家 stdout 逐次 flush（常驻不滞留）。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawn } = await import("node:child_process");
const c = spawn("cat");
let ends = 0;
c.stdout.on("end", () => { ends++; });
c.stderr.on("end", () => { ends++; });
c.on("exit", (code, signal) => console.log("exit", code, signal, c.signalCode));
c.kill();
await new Promise((res) => c.on("close", res));
console.log("ends", ends === 2);
const s = spawn("sleep", ["10"]);
console.log("k0", s.kill(0) === true);
await new Promise((res) => setTimeout(res, 300));
console.log("alive", s.exitCode === null && s.signalCode === null);
s.kill();
await new Promise((res) => s.on("close", res));
console.log("dead", s.signalCode === "SIGTERM");
const e = spawn("cat");
let got = "";
e.stdout.on("data", (d) => { got += d.toString(); });
e.stdin.write("hello-cat");
await new Promise((res) => setTimeout(res, 500));
console.log("echo", got === "hello-cat");
e.kill();
await new Promise((res) => e.on("close", res));
console.log("bye", e.signalCode === "SIGTERM");"#]));
    for line in [
        "exit null SIGTERM SIGTERM",
        "ends true",
        "k0 true",
        "alive true",
        "dead true",
        "echo true",
        "bye true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
}

#[test]
fn phase10f_child_stdin_backpressure() {
    // big-write-end 套件回归：stdin.write 持续写必回 false（16KB 高水位）+
    // drain 到达 + 全量按序回显 + end 关。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawn } = await import("node:child_process");
const c = spawn("cat");
let sent = 0;
let sawFalse = false;
let bufsize = 0;
let buf;
do {
  bufsize += 1024;
  buf = Buffer.alloc(bufsize, 46);
  sent += bufsize;
} while (c.stdin.write(buf) !== false);
sawFalse = true;
for (let i = 0; i < 20; i++) {
  const b = Buffer.alloc(bufsize, 46);
  sent += bufsize;
  c.stdin.write(b);
}
c.stdin.end();
let n = 0;
c.stdout.on("data", (d) => { n += d.length; });
const closedP = new Promise((res) => c.on("close", res));
await new Promise((res) => c.stdout.on("end", res));
console.log("backpressure", sawFalse, n === sent);
await closedP;
console.log("closed", c.exitCode === 0);"#]));
    for line in ["backpressure true true", "closed true"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
}

#[test]
fn phase10f_child_stdio_stream_handoff() {
    // pipe-dataflow/merge/reuse 套件回归：stdio 数组流对象转交（stdin 位读流
    // data/end 转入、stdout 位写流只转 data 不转 end）+ stdout._handle 桩。
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const { spawn } = await import("node:child_process");
// stdin 位：读流转入
{
  const src = spawn("echo", ["hello-in"]);
  const dst = spawn("cat", { stdio: [src.stdout, "pipe", "pipe"] });
  let n = "";
  dst.stdout.on("data", (d) => { n += d.toString(); });
  await new Promise((res) => dst.stdout.on("end", res));
  console.log("handoff-in", n === "hello-in\n");
  await new Promise((res) => dst.on("close", res));
  src.kill();
  await new Promise((res) => src.on("close", res));
}
// stdout 位：写流只转 data（end 不转，共享写端持有者关）
{
  const p3 = spawn("cat", { stdio: ["pipe", "pipe", "pipe"] });
  const p1 = spawn("echo", ["hello-out"], { stdio: ["pipe", p3.stdin, "pipe"] });
  let n = "";
  p3.stdout.on("data", (d) => { n += d.toString(); });
  await new Promise((res) => p1.on("close", res));
  await new Promise((res) => {
    const t = setInterval(() => { if (n === "hello-out\n") { clearInterval(t); res(); } }, 20);
  });
  console.log("handoff-out", n === "hello-out\n");
  p3.stdin.end();
  await new Promise((res) => p3.on("close", res));
}
// _handle 桩存在且泵不经 readStart
{
  const c = spawn("echo", ["z"]);
  console.log("handle", c.stdout._handle !== undefined && typeof c.stdout._handle.readStart === "function");
  c.kill();
  await new Promise((res) => c.on("close", res));
}"#]));
    for line in ["handoff-in true", "handoff-out true", "handle true"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
}

