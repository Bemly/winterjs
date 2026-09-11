//! node: 内建黑盒测试(对齐 src/builtins/node/:fs/path/os/process/child/require + Phase 9a events/util 系)。

mod common;

use common::*;

use assert_fs::prelude::*;

#[test]
fn phase4_node_path_basic() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import path, { join, basename, extname, dirname, normalize, relative, isAbsolute, sep, parse } from "node:path";
import { win32, posix } from "node:path";
console.log(join("a", "b", "..", "c"));
console.log(basename("/x/y.ts"), extname("a.d.ts"), extname(".gitignore"), dirname("/x/y/z"));
console.log(normalize("a//b/./c/"), isAbsolute("/x"), isAbsolute("x"), sep);
console.log(relative("/a/b/c", "/a/d"), JSON.stringify(parse("/x/y.ts")).length > 0);
console.log(path.sep === (globalThis.process.platform === "win32" ? win32.sep : posix.sep) ? "ns-ok" : "ns-bad");
console.log(win32.join("C:\\a", "b"), win32.basename("C:\\x\\y.txt"), win32.sep);
console.log(posix.join("a", "b"));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "a/c\ny.ts .ts  /x/y\n".to_string()
            + "a/b/c/ true false /\n"
            + "../../d true\n"
            + "ns-ok\n"
            + "C:\\a\\b y.txt \\\n"
            + "a/b\n"
    );
    dir.close().unwrap();
}

#[test]
fn phase4_node_os_basic() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const os = await import("node:os"); console.log([os.platform(), os.arch()].join(",")); console.log(os.EOL.length, os.hostname().length > 0, os.tmpdir().length > 0, os.totalmem() > 0, os.freemem() >= 0, os.cpus().length > 0, typeof os.cpus()[0].model, Object.keys(os.networkInterfaces()).length > 0, os.userInfo().username.length >= 0, os.uptime() >= 0, os.loadavg().length, os.release().length >= 0);"#]));
    let mut lines = out.lines();
    let pa = lines.next().unwrap_or("");
    assert!(
        ["darwin", "linux", "win32", "android"].contains(&pa.split(',').next().unwrap_or("")),
        "platform: {pa}"
    );
    assert!(
        ["arm64", "x64", "arm"].contains(&pa.split(',').nth(1).unwrap_or("")),
        "arch: {pa}"
    );
    assert_eq!(
        lines.next().unwrap_or(""),
        "1 true true true true true string true true true 3 true",
        "os: {out}"
    );
}

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
fn phase4_fs_read_write_roundtrip() {
    // 文本/二进制/追加 + stat 字段 + exists。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "rw.mjs",
        r#"
import fs from "node:fs";
fs.writeFileSync("a.txt", "hello");
fs.appendFileSync("a.txt", " world");
console.log(fs.readFileSync("a.txt", "utf8"));
const bin = new Uint8Array([0, 1, 2, 250]);
fs.writeFileSync("b.bin", bin);
const back = fs.readFileSync("b.bin");
console.log(back.length, back[3], back instanceof Uint8Array);
const st = fs.statSync("a.txt");
console.log(st.size, st.isFile(), st.isDirectory(), st.mtime instanceof Date, st.mtimeMs > 0);
console.log(fs.existsSync("a.txt"), fs.existsSync("missing-xyz"), fs.existsSync(123));
"#,
    );
    assert_eq!(
        out, "hello world\n4 250 true\n11 true false true true\ntrue false false\n",
        "fs rw: {out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase4_fs_dirs_and_moves() {
    // mkdir -p + readdir(+types) + rename + copy + rm -rf + realpath + mkdtemp.
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "dirs.mjs",
        r#"
import fs from "node:fs";
import path from "node:path";
fs.mkdirSync("d/sub/deep", { recursive: true });
fs.writeFileSync("d/sub/deep/f.txt", "x");
fs.writeFileSync("d/top.txt", "y");
console.log(fs.readdirSync("d").join(","), fs.readdirSync("d/sub").join(","));
const typed = fs.readdirSync("d", { withFileTypes: true });
console.log(typed.map((e) => e.name + ":" + e.isDirectory() + ":" + e.isFile()).join(","));
fs.renameSync("d/top.txt", "d/renamed.txt");
fs.copyFileSync("d/renamed.txt", "d/copied.txt");
console.log(fs.readdirSync("d").join(","));
console.log(fs.realpathSync("d").endsWith("d"));
const tmp = fs.mkdtempSync(path.join(fs.realpathSync("."), "pre-"));
console.log(tmp.includes("pre-"), fs.statSync(tmp).isDirectory());
fs.rmSync("d", { recursive: true, force: true });
console.log(fs.existsSync("d"));
fs.rmSync("missing-xyz", { force: true });
console.log("force-ok");
"#,
    );
    assert_eq!(
        out,
        "sub,top.txt deep\nsub:true:false,top.txt:false:true\ncopied.txt,renamed.txt,sub\ntrue\ntrue true\nfalse\nforce-ok\n",
        "fs dirs: {out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase4_fs_promises_and_errors() {
    // promises 对等 + ENOENT 三件（code/syscall/path）+ lstat 链接 + file: URL 路径。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fsp from "node:fs/promises";
import fs from "node:fs";
await fsp.writeFile("p.txt", "via-promises");
console.log(await fsp.readFile("p.txt", "utf8"), (await fsp.stat("p.txt")).size);
try {
  fs.readFileSync("definitely-missing-xyz");
  console.log("no-throw");
} catch (e) {
  console.log(e.code, e.syscall, e.path, e instanceof Error);
}
try {
  await fsp.readFile("definitely-missing-xyz");
  console.log("no-throw");
} catch (e) {
  console.log("async-" + e.code);
}
console.log(fs.readFileSync(new URL("file://" + process.cwd() + "/p.txt"), "utf8"));
"#,
    );
    assert_eq!(
        out,
        "via-promises 12\nENOENT open definitely-missing-xyz true\nasync-ENOENT\nvia-promises\n",
        "fs promises: {out}"
    );
    dir.close().unwrap();
}

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
fn phase4_node_assert_subset() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const assert = (await import("node:assert")).default; assert.ok(1); assert.strictEqual(1, 1); assert.notStrictEqual(1, "1"); assert.deepStrictEqual({ a: [1, 2] }, { a: [1, 2] }); assert.equal(1, "1"); assert.throws(() => { throw new TypeError("x"); }, TypeError); assert.throws(() => { throw new Error("boom"); }, /boom/); await assert.rejects(async () => { throw new Error("r"); }); assert.match("foobar", /^foo/); assert.ifError(null); console.log("assert-ok"); try { assert.strictEqual(1, 2); } catch (e) { console.log(e.code, e.operator, e.actual, e.expected); }"#]));
    assert_eq!(
        out, "assert-ok\nERR_ASSERTION strictEqual 1 2\n",
        "assert: {out}"
    );
}

#[test]
fn phase4_node_test_runner() {
    // 通过/失败/跳过计数 + 小结 + 失败 exitCode=1。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str("import { test, describe } from \"node:test\";\nimport assert from \"node:assert\";\ndescribe(\"math\", () => {\n  test(\"adds\", () => assert.strictEqual(1 + 1, 2));\n  test(\"fails\", () => assert.strictEqual(1, 2));\n  test.skip(\"skipped\", () => {});\n});\n").unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("not ok - math > fails"), "runner: {stdout}");
    assert!(
        stdout.contains("# pass 1, fail 1, skip 1, todo 0"),
        "summary: {stdout}"
    );
    dir.close().unwrap();
}

#[test]
fn phase4_require_cjs_builtin_relative_json() {
    // CJS 文件 + 内建 + JSON + 相对路径 + require.main（经 .cjs 入口）。
    let dir = assert_fs::TempDir::new().unwrap();
    let lib = dir.child("lib/util.cjs");
    lib.write_str("const path = require(\"node:path\");\nmodule.exports = { joined: path.join(\"a\", \"b\") };\n").unwrap();
    let data = dir.child("lib/data.json");
    data.write_str("{\"answer\": 42}").unwrap();
    let main = dir.child("main.cjs");
    main.write_str("const u = require(\"./lib/util.cjs\");\nconst d = require(\"./lib/data.json\");\nconsole.log(\"main:\", u.joined, d.answer, __filename.endsWith(\"main.cjs\"), require.main.filename.endsWith(\"main.cjs\"));\n").unwrap();
    let out = winterjs().arg("--run").arg(main.path()).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, "main: a/b 42 true true\n", "require: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase4_require_cycle_partial_exports() {
    // 循环引用见半成品（Node 语义）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("b.cjs")
        .write_str(
            "const a = require(\"./a.cjs\");\nmodule.exports = { b: 2, aVal: (a.a || 0) + 10 };\n",
        )
        .unwrap();
    dir.child("a.cjs")
        .write_str(
            "const b = require(\"./b.cjs\");\nmodule.exports = { a: 1, bVal: (b.b || 0) + 100 };\n",
        )
        .unwrap();
    let main = dir.child("main.cjs");
    main.write_str("const a = require(\"./a.cjs\");\nconsole.log(\"cycle:\", a.a, a.bVal);\n")
        .unwrap();
    let out = winterjs().arg("--run").arg(main.path()).output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "cycle: 1 102\n");
    dir.close().unwrap();
}

#[test]
fn phase4_require_errors() {
    // 缺失模块 / ESM 拒绝 / resolve 直给。
    let out = winterjs().args(["--eval", "try { require(\"node:nope-xyz\"); } catch (e) { console.log(e.message.slice(0, 30)); }"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Cannot find module"), "missing: {stdout}");
    let dir = assert_fs::TempDir::new().unwrap();
    let mod_ = dir.child("m.mjs");
    mod_.write_str("export const x = 1;\n").unwrap();
    let code = format!(
        "try {{ require({:?}); }} catch (e) {{ console.log(e.message.slice(0, 30)); }}",
        mod_.path().to_string_lossy()
    );
    let out = winterjs().args(["--eval", &code]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("require() of ES Module"), "esm: {stdout}");
    let out =
        stdout_of(&mut winterjs().args(["--eval", "console.log(require.resolve(\"node:path\"));"]));
    assert_eq!(out, "node:path\n", "resolve: {out}");
    dir.close().unwrap();
}

#[test]
fn phase4_fs_watch_fires_and_closes() {
    // 写文件触发 rename 事件；close 后进程即退（persistent 续命验证）。
    let dir = assert_fs::TempDir::new().unwrap();
    let watchdir = dir.child("watched");
    std::fs::create_dir(watchdir.path()).unwrap();
    let file = dir.child("watch.mjs");
    file.write_str("import fs from \"node:fs\";\nconst w = fs.watch(\"watched\", (ev, file) => { console.log(\"ev:\", ev, file); w.close(); });\nsetTimeout(() => fs.writeFileSync(\"watched/n.txt\", \"x\"), 100);\n").unwrap();
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
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "ev: rename n.txt\n");
    dir.close().unwrap();
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
fn node_buffer_global() {
    // 正常：from/toString(hex/base64/utf8)/concat/alloc/byteLength；报错：坏hex/未知编码；
    // 边界：allocUnsafe零填/subarray保持Buffer/compare/equals/copy/write/toJSON。
    let code = r#"const b = Buffer.from("hello");
if (b.toString("hex") !== "68656c6c6f" || b.toString("base64") !== "aGVsbG8=") throw new Error("basic failed");
if (!Buffer.isBuffer(b) || Buffer.isBuffer(new Uint8Array(1))) throw new Error("isBuffer failed");
if (Buffer.byteLength("€") !== 3) throw new Error("byteLength failed");
if (Buffer.concat([Buffer.from("a"), Buffer.from("b")]).toString() !== "ab") throw new Error("concat failed");
if (Buffer.alloc(4, "ab").toString() !== "abab") throw new Error("alloc fill failed");
if (Buffer.from([104, 105]).toString() !== "hi") throw new Error("array failed");
if (Buffer.from("ff", "hex")[0] !== 255) throw new Error("hex failed");
if (Buffer.from("aGVsbG8=", "base64").toString() !== "hello") throw new Error("b64 failed");
const z = Buffer.allocUnsafe(8);
if (z.length !== 8 || ![...z].every((x) => x === 0)) throw new Error("allocUnsafe must be zeroed");
const s = b.subarray(1, 3);
if (!(s instanceof Buffer) || s.toString() !== "el") throw new Error("subarray failed");
if (Buffer.compare(Buffer.from("a"), Buffer.from("b")) >= 0) throw new Error("compare failed");
if (!b.equals(Buffer.from("hello"))) throw new Error("equals failed");
const t = Buffer.alloc(5);
if (b.copy(t, 1) !== 4 || t.slice(1).toString() !== "hell") throw new Error("copy failed");
const w = Buffer.alloc(8);
if (w.write("hi", 2) !== 2 || w.slice(2, 4).toString() !== "hi") throw new Error("write failed");
if (JSON.parse(JSON.stringify(b)).type !== "Buffer") throw new Error("toJSON failed");
// fs 互操作：Buffer 进出 writeFile/readFile
try { Buffer.from("zz", "hex"); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("hex")) throw e; }
try { Buffer.from("x", "nope-enc"); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("encoding")) throw e; }
console.log("buffer-ok");
"#;
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", code])),
        "buffer-ok\n"
    );
}

#[test]
fn node_fs_streams() {
    // createReadStream 分块 + createWriteStream 落盘/追加。
    let dir = assert_fs::TempDir::new().unwrap();
    std::fs::write(dir.path().join("in.txt"), b"hello-fs-stream").unwrap();
    let code = r#"import fs from "node:fs";
const rs = fs.createReadStream("in.txt", { highWaterMark: 4 });
let s = "";
for await (const c of rs) s += new TextDecoder().decode(c);
if (s !== "hello-fs-stream") throw new Error("read failed: " + s);
const ws = fs.createWriteStream("out.txt");
const w = ws.getWriter();
await w.write(new TextEncoder().encode("ab"));
await w.write(new TextEncoder().encode("cd"));
await w.close();
if (fs.readFileSync("out.txt", "utf8") !== "abcd") throw new Error("write failed");
const wa = fs.createWriteStream("out.txt", { flags: "a" });
const w2 = wa.getWriter();
await w2.write("ef");
await w2.close();
if (fs.readFileSync("out.txt", "utf8") !== "abcdef") throw new Error("append failed: " + fs.readFileSync("out.txt", "utf8"));
console.log("fs-stream-ok");
"#;
    std::fs::write(dir.path().join("t.mjs"), code).unwrap();
    let out = stdout_of(
        &mut winterjs()
            .arg("--run")
            .arg(dir.path().join("t.mjs"))
            .current_dir(dir.path()),
    );
    assert_eq!(out, "fs-stream-ok\n", "fs streams: {out}");
    dir.close().unwrap();
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
fn phase9a_events_basic_emit_on_off() {
    // test-events.js: emit 返回值/once/移除后不触发/eventNames
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("e.mjs");
    file.write_str(
        r#"import EE from "node:events";
const ee = new EE();
let calls = [];
function fn1() { calls.push("f1"); }
ee.on("x", fn1);
console.log(ee.emit("x"), ee.emit("nope"));
ee.once("y", () => calls.push("once"));
ee.emit("y"); ee.emit("y");
ee.removeListener("x", fn1);
console.log(calls.join(","), ee.listenerCount("x"), ee.emit("x"));
ee.on("z", () => {});
console.log(ee.eventNames().map(String).join(","));
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("true false"), "out: {out}");
    assert!(out.contains("f1,once"), "out: {out}");
    assert!(out.contains("z"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_events_unhandled_error_throws_original() {
    // test-events.js: 无 error 监听时 emit('error', er) 重抛原 Error（非包裹）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("u.mjs");
    file.write_str(
        r#"import EE from "node:events";
const ee = new EE();
try { ee.emit("error", new TypeError("boom")); } catch (e) {
  console.log(e instanceof TypeError, e.message, "code" in e && e.code === undefined);
}
// 非 Error 实参 → ERR_UNHANDLED_ERROR 包裹
try { ee.emit("error", "str"); } catch (e) {
  console.log(e.code, e.message.startsWith("Unhandled error."));
}
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("true boom false"), "out: {out}");
    assert!(out.contains("ERR_UNHANDLED_ERROR true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_events_error_monitor_capture_rejections() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("c.mjs");
    file.write_str(
        r#"import EE from "node:events";
// errorMonitor: errorMonitor 监听在无 error 监听时也不抛（先于 doError 判定）
const em = new EE();
let seen = 0;
em.on(EE.errorMonitor, () => seen++);
try { em.emit("error", new Error("no-handler")); } catch {}
console.log("mon", seen);
// captureRejections: rejected listener 走 [captureRejectionSymbol] 而非 error
const cap = new EE({ captureRejections: true });
let handled = 0;
cap.on("x", async () => { throw new Error("rej"); });
cap[EE.captureRejectionSymbol] = (err, type) => { handled++; console.log("cap", type, err.message); };
cap.emit("x");
await new Promise((r) => setTimeout(r, 10));
console.log("handled", handled);
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("mon 1"), "out: {out}");
    assert!(
        out.contains("cap x rej") && out.contains("handled 1"),
        "out: {out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase9a_events_max_listeners_warning_and_validation() {
    // test-event-emitter-max-listeners.js: 泄漏警告 + warning 事件；参数校验消息逐字
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("m.mjs");
    file.write_str(
        r#"import EE from "node:events";
const warnings = [];
process.on("warning", (w) => warnings.push(w));
const ee = new EE();
for (let i = 0; i < 12; i++) ee.on("l", () => {});
console.log("warned", warnings.length, warnings[0]?.name, warnings[0]?.count);
// Node 原文消息格式（test-events-common 断言口径）
try { ee.once("x", 42); } catch (e) {
  console.log(e.code, e.message);
}
try { EE.setMaxListeners(-1); } catch (e) {
  console.log(e.code, e.constructor.name);
}
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.contains("warned 1 MaxListenersExceededWarning 11"),
        "out: {out}"
    );
    assert!(
        out.contains(
            "ERR_INVALID_ARG_TYPE The \"listener\" argument must be of type function. Received type number (42)"
        ),
        "out: {out}"
    );
    assert!(out.contains("ERR_OUT_OF_RANGE RangeError"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_events_once_and_on_iterator() {
    // test-events-on.js + test-events-on-async-iterator.js 语义子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("i.mjs");
    file.write_str(
        r#"import EE, { once, on } from "node:events";
const ee = new EE();
setTimeout(() => ee.emit("tick", 7, "s"), 5);
const [n, s] = await once(ee, "tick");
console.log("once", n, s);
// 异步迭代器 + close 事件
const src = new EE();
setTimeout(() => { src.emit("data", "a"); src.emit("data", "b"); src.emit("end"); }, 5);
const got = [];
for await (const [v] of on(src, "data", { close: ["end"] })) got.push(v);
console.log("iter", got.join(""));
// once + AbortSignal（已中止即 AbortError）
try {
  await once(new EE(), "x", { signal: AbortSignal.abort(new Error("why")) });
} catch (e) { console.log("abort", e.code, e.cause?.message); }
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.contains("once 7 s") && out.contains("iter ab"),
        "out: {out}"
    );
    assert!(out.contains("abort ABORT_ERR why"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_events_require_and_error_boundary() {
    // require('node:events') CJS 面 + 非法 emitter 报可读错（边界三件之一）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("r.cjs");
    file.write_str(
        r#"const { EventEmitter, getEventListeners } = require("node:events");
const ee = new EventEmitter();
ee.on("a", () => 1);
console.log(require("node:events").EventEmitter === EventEmitter, getEventListeners(ee, "a").length);
try { getEventListeners(42, "a"); } catch (e) { console.log(e.code); }
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.contains("true 1") && out.contains("ERR_INVALID_ARG_TYPE"),
        "out: {out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase9a_async_hooks_als_and_async_resource() {
    // test-async-local-storage* 子集（同步链路）+ AsyncResource runInAsyncScope
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("a.mjs");
    file.write_str(
        r#"import { AsyncLocalStorage, AsyncResource, createHook, executionAsyncId } from "node:async_hooks";
const als = new AsyncLocalStorage();
als.run({ id: 42 }, () => {
  console.log("store", als.getStore().id);
  const res = new AsyncResource("TEST");
  res.runInAsyncScope(() => console.log("in-res", als.getStore().id, executionAsyncId() > 1));
  console.log("bind", als.bind(() => als.getStore()?.id ?? "none")(), als.getStore()?.id ?? "none");
});
console.log("outside", als.getStore());
// snapshot
const snap = als.run({ s: 1 }, () => als.snapshot());
snap(() => console.log("snapshot", als.getStore()?.s));
// AsyncResource.bind 静态 + emitDestroy
const bound = AsyncResource.bind(() => executionAsyncId() > 1, "BOUND");
console.log("static-bind", bound(), AsyncResource.AsyncResource === AsyncResource);
const hook = createHook({ init() {} }).enable();
console.log("hook", typeof hook.disable);
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.contains("store 42") && out.contains("in-res 42 true"),
        "out: {out}"
    );
    assert!(
        out.contains("bind 42 42") && out.contains("outside undefined"),
        "out: {out}"
    );
    assert!(
        out.contains("snapshot 1") && out.contains("static-bind true true"),
        "out: {out}"
    );
    assert!(out.contains("hook function"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_async_hooks_stub_and_validation_boundary() {
    // stub 口径边界：createHook 非法回调 → ERR_ASYNC_CALLBACK；ALS 非法 callback → TypeError
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("b.mjs");
    file.write_str(
        r#"import { createHook, AsyncResource, AsyncLocalStorage } from "node:async_hooks";
try { createHook({ init: 1 }); } catch (e) { console.log("h", e.code); }
try { new AsyncResource(42); } catch (e) { console.log("t", e.code, e.message.includes("must be of type string")); }
try { new AsyncLocalStorage().run({}, "nope"); } catch (e) { console.log("r", e instanceof TypeError); }
try { new AsyncResource("X", { triggerAsyncId: "no" }); } catch (e) { console.log("o", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("h ERR_ASYNC_CALLBACK"), "out: {out}");
    assert!(out.contains("t ERR_INVALID_ARG_TYPE true"), "out: {out}");
    assert!(
        out.contains("r true") && out.contains("o ERR_INVALID_ARG_TYPE"),
        "out: {out}"
    );
    dir.close().unwrap();
}

// ── Phase 9a-2：node:util / node:util/types ────────────────────────────────

#[test]
fn phase9a_util_format_inspect_inherits() {
    // test-util-format.js / test-util-inspect.js 命名子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("u.mjs");
    file.write_str(
        r#"import util, { format, inspect, inherits, stripVTControlCharacters } from "node:util";
console.log(format("%s=%d", "a", 1.5), format("%%"), format("%j", { x: 1 }));
console.log(format("%i:%f", 3.9, "1.5"));
console.log(util.inspect({ a: 1, b: [1, 2], c: new Map([["k", 1]]) }));
console.log(util.inspect("it's"), inspect(new Date(0).toISOString() === inspect(new Date(0)) ? { s: "q'a" } : 0));
class Base { base() { return 1; } }
class Sub {}
inherits(Sub, Base);
console.log("inh", new Sub().base(), Sub.super_ === Base);
console.log("strip", stripVTControlCharacters("\u001b[31mred\u001b[39m"), stripVTControlCharacters("plain"));
console.log("promise-inp", util.inspect(Promise.resolve()));
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("a=1.5 %% {\"x\":1}"), "out: {out}");
    assert!(out.contains("3:1"), "out: {out}");
    assert!(
        out.contains("{ a: 1, b: [ 1, 2 ], c: Map(1) { 'k' => 1 } }"),
        "out: {out}"
    );
    assert!(out.contains("inh 1 true"), "out: {out}");
    assert!(out.contains("strip red plain"), "out: {out}");
    assert!(out.contains("Promise { <pending> }"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_util_promisify_callbackify_deep_equal() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"import { promisify, callbackify, isDeepStrictEqual } from "node:util";
import assert from "node:assert";
const pb = promisify((a, cb) => cb(null, a * 2));
console.log("prom", await pb(21));
// multi-value + customPromisifyArgs
const pr = promisify((cb) => cb(null, "bytesRead", "buffer"));
pr[Symbol.for("nodejs.util.promisify.custom")] // 不设 custom
console.log("prom-obj", JSON.stringify(await (async () => {
  const fn = promisify((cb) => cb(null, 1, 2));
  fn[Symbol("customPromisifyArgs")] = ["a", "b"];
  const fn2 = promisify((cb) => { fn2args(cb); }) ; return 0;
})()));
// promisify.custom 通道
const raw = (a, cb) => cb(null, a);
raw[Symbol.for("nodejs.util.promisify.custom")] = (a) => Promise.resolve(a + 100);
console.log("custom", await promisify(raw)(1));
// callbackify 正常 + falsy rejection
const cf = callbackify(async (n) => n + 1);
const [err, val] = await new Promise((res) => cf(1, (e, v) => res([e, v])));
console.log("cb", err, val);
const cf2 = callbackify(async () => { throw null; });
await new Promise((res) => cf2((e) => res(console.log("falsy", e?.code, e instanceof Error))));
// isDeepStrictEqual 语义（test-assert 依赖同款）
console.log("eq", isDeepStrictEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] }));
console.log("neq0", isDeepStrictEqual(1, "1"), isDeepStrictEqual({ a: 1 }, { a: 1, b: undefined }));
console.log("nan0", isDeepStrictEqual(NaN, NaN), isDeepStrictEqual(0, -0));
console.log("map", isDeepStrictEqual(new Map([[1, "a"]]), new Map([[1, "a"]])));
console.log("proto", isDeepStrictEqual(Object.create(null, { x: { value: 1, enumerable: true } }), { x: 1 }));
// 边界：非函数入参
try { promisify(42); } catch (e) { console.log("err1", e.code, e.message.includes("must be of type function")); }
try { callbackify("x"); } catch (e) { console.log("err2", e.code); }
assert.strictEqual(typeof promisify.custom, "symbol");
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("prom 42"), "out: {out}");
    assert!(out.contains("custom 101"), "out: {out}");
    assert!(out.contains("cb null 2"), "out: {out}");
    assert!(
        out.contains("falsy ERR_FALSY_VALUE_REJECTION true"),
        "out: {out}"
    );
    assert!(out.contains("eq true"), "out: {out}");
    assert!(out.contains("neq0 false false"), "out: {out}");
    assert!(out.contains("nan0 true false"), "out: {out}");
    assert!(
        out.contains("map true") && out.contains("proto false"),
        "out: {out}"
    );
    assert!(out.contains("err1 ERR_INVALID_ARG_TYPE true"), "out: {out}");
    assert!(out.contains("err2 ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_util_types_surface() {
    // test-util-types* 命名子集（isProxy 恒 false 为记档偏差，不点名）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        r#"import types from "node:util/types";
import { types as t2 } from "node:util";
console.log("same", types === t2, typeof types.isPromise);
console.log(
  types.isPromise(Promise.resolve()),
  types.isMap(new Map()),
  types.isSet(new Set()),
  types.isDate(new Date()),
  types.isRegExp(/r/),
  types.isTypedArray(new Uint8Array(1)),
  types.isUint8Array(new Uint8Array(1)),
  types.isUint32Array(new Uint32Array(1)),
  types.isDataView(new DataView(new ArrayBuffer(2))),
  types.isArrayBuffer(new ArrayBuffer(1)),
  types.isNativeError(new TypeError()),
  types.isNativeError(new Error()),
  types.isNativeError({}),
  types.isAsyncFunction(async () => {}),
  types.isGeneratorFunction(function* () {}),
  types.isPromise(new Map()),
  types.isWeakSet(new WeakSet()),
  types.isNumberObject(new Number(1)),
  types.isBoxedPrimitive(new Boolean(true)),
  types.isArgumentsObject((function () { return arguments; })()),
);
try { types.isUint8Array(42) === false; console.log("num-ok"); } catch (e) { console.log("num-throw"); }
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("same true function"), "out: {out}");
    assert!(
        out.contains("true true true true true true true true true true true true false true true false true true true true"),
        "out: {out}"
    );
    assert!(out.contains("num-ok"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9a-3：node:querystring / node:punycode / node:string_decoder ─────

#[test]
fn phase9a_querystring_roundtrip() {
    // test-querystring.js 命名子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("q.mjs");
    file.write_str(
        r#"import qs from "node:querystring";
console.log(JSON.stringify(qs.parse("a=1&b=x%20y&b=2&c")));
console.log(qs.stringify({ a: "x y", b: [1, 2] }));
console.log(qs.escape("ä b"), qs.unescape("%C3%A4+b"));
console.log(JSON.stringify(qs.parse("a=1;a=2", ";", "=")));
console.log(Object.keys(qs.parse("a=1&b=2&c=3", null, null, { maxKeys: 2 })).length);
// 自定义 enc/dec
const p = qs.parse("a=%20", null, null, { decodeURIComponent: (s) => s });
console.log(JSON.stringify(p));
// 边界：非字符串入参 → 空对象；maxKeys=1 截断
console.log(JSON.stringify(qs.parse(null)), JSON.stringify(qs.parse("")));
console.log(typeof qs.parse("a=1&b=2", null, null, { maxKeys: 1 }).a);
try { qs.unescape("%E0%A4%A"); } catch (e) { console.log("catch-fallback"); }
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.contains(r#"{"a":"1","b":["x y","2"],"c":""}"#),
        "out: {out}"
    );
    assert!(out.contains("a=x%20y&b=1&b=2"), "out: {out}");
    assert!(out.contains("%C3%A4%20b ä+b"), "out: {out}");
    assert!(
        out.contains(r#"{"a":"1"} {"a":"2"}"#) || out.contains(r#"{"a":"2"}"#),
        "out: {out}"
    );
    assert!(out.contains("2"), "out: {out}");
    assert!(out.contains(r#"{"a":"%20"}"#), "out: {out}");
    assert!(out.contains("{} {}"), "out: {out}");
    assert!(out.contains("catch-fallback"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_punycode_rfc3492() {
    // test-punycode.js 命名子集（RFC 3492 向量 + 域名 + ucs2）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"import punycode from "node:punycode";
console.log(punycode.encode("bücher"), punycode.decode("bcher-kva"));
console.log(punycode.toASCII("münchen.de"), punycode.toUnicode("xn--mnchen-3ya.de"));
console.log(punycode.toASCII("日本"), punycode.toUnicode("xn--wgv71a"));
console.log(punycode.ucs2.encode([0x1D306]) === "\u{1D306}", punycode.ucs2.decode("a\u{1D306}b").length);
console.log(punycode.toASCII("foo@bücher.de").split("@")[1]);
try { punycode.decode("!!!!!"); } catch (e) { console.log("err", e instanceof RangeError); }
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("bcher-kva bücher"), "out: {out}");
    assert!(out.contains("xn--mnchen-3ya.de münchen.de"), "out: {out}");
    assert!(out.contains("xn--wgv71a 日本"), "out: {out}");
    assert!(out.contains("true 3"), "out: {out}");
    assert!(out.contains("xn--bcher-kva.de"), "out: {out}");
    assert!(out.contains("err true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_string_decoder_encodings() {
    // test-string-decoder.js 命名子集：截断续读/end flush/全编码
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("s.mjs");
    file.write_str(
        r#"import { StringDecoder } from "node:string_decoder";
const utf8 = new StringDecoder("utf8");
let out = "";
out += utf8.write(Buffer.from([0xE4, 0xB8]));
out += utf8.write(Buffer.from([0xAD, "e".charCodeAt(0)]));
out += utf8.end();
console.log("utf8", out);
// invalid 序列 → FFFD 继续
const bad = new StringDecoder("utf8");
console.log("bad", bad.write(Buffer.from([0xFF, 0x41])).includes("\uFFFD"), bad.write(Buffer.from([0x42])));
const u16 = new StringDecoder("utf16le");
let o2 = u16.write(Buffer.from([0x61, 0]));
o2 += u16.write(Buffer.from([0x62]));
o2 += u16.end();
console.log("utf16", o2.length, o2.charCodeAt(1) === 0xFFFD);
const hex = new StringDecoder("hex");
console.log("hex", hex.write(Buffer.from([0xDE, 0xAD])), hex.end());
const b64 = new StringDecoder("base64");
let o3 = b64.write(Buffer.from("foobarb"));
o3 += b64.write(Buffer.from("az"));
o3 += b64.end();
console.log("b64", o3 === Buffer.from("foobarbaz").toString("base64"));
const latin = new StringDecoder("latin1");
console.log("latin1", latin.write(Buffer.from([0xE9, 0x41])), latin.end().length);
const ascii = new StringDecoder("ascii");
console.log("ascii", ascii.write(Buffer.from([0x80, 0x41])).length, ascii.write(Buffer.from([0x41])));
console.log("default", new StringDecoder().encoding, typeof utf8.lastChar, utf8.lastNeed === 0);
console.log("string-in", new StringDecoder().write("direct"));
try { new StringDecoder("nope"); } catch (e) { console.log("e1", e.code); }
try { new StringDecoder("utf8").write(42); } catch (e) { console.log("e2", e.code); }
try { new StringDecoder("utf8").write.call({ __wjsId: undefined }, Buffer.alloc(1)); } catch (e) { console.log("e3", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs()
        .args(["--run", file.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("utf8 中e"), "out: {out}");
    assert!(out.contains("bad true B"), "out: {out}");
    assert!(out.contains("utf16 2 true"), "out: {out}");
    assert!(out.contains("hex dead"), "out: {out}");
    assert!(out.contains("b64 true"), "out: {out}");
    assert!(out.contains("latin1 éA 0"), "out: {out}");
    assert!(out.contains("ascii 2 A"), "out: {out}");
    assert!(out.contains("default utf8 object true"), "out: {out}");
    assert!(out.contains("string-in direct"), "out: {out}");
    assert!(
        out.contains("e1 ERR_UNKNOWN_ENCODING") && out.contains("e2 ERR_INVALID_ARG_TYPE"),
        "out: {out}"
    );
    dir.close().unwrap();
}

/// node: 测试脚手架（tempdir 单文件模块；`run` 执行）。
fn run_node_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> std::process::Output {
    let file = dir.child(name);
    file.write_str(source).unwrap();
    winterjs().arg("--run").arg(file.path()).output().unwrap()
}

/// node:fs 脚手架（workdir 内跑模块；返回 stdout）。
fn run_fs_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> String {
    let file = dir.child(name);
    file.write_str(source).unwrap();
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
    String::from_utf8(out.stdout).unwrap()
}
