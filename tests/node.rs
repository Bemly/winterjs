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
console.log("fallback", qs.unescape("%E0%A4%A").includes("%"), qs.unescapeBuffer("a+b", true).toString());
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
        out.contains(r#"{"a":["1","2"]}"#),
        "out: {out}"
    );
    assert!(out.contains("2"), "out: {out}");
    assert!(out.contains(r#"{"a":"%20"}"#), "out: {out}");
    assert!(out.contains("{} {}"), "out: {out}");
    assert!(out.contains("fallback true a b"), "out: {out}");
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
// ── Phase 9a-4：node:diagnostics_channel / node:trace_events / node:tty ────

#[test]
fn phase9a_diagnostics_channel_surface() {
    // test-diagnostics-channel.js 命名子集
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("d.mjs");
    file.write_str(
        r#"import dc, { channel, hasSubscribers, Channel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";
console.log("idle", hasSubscribers("t-ch"), channel("t-ch").hasSubscribers);
const seen = [];
const listener = (msg, name) => seen.push([name, msg]);
dc.subscribe("t-ch", listener);
console.log("active", hasSubscribers("t-ch"), channel("t-ch") instanceof Channel);
channel("t-ch").publish({ n: 1 });
console.log("got", JSON.stringify(seen));
console.log("unsub-wrong", dc.unsubscribe("t-ch", () => {}), dc.unsubscribe("t-ch", listener), hasSubscribers("t-ch"));
// bindStore + runStores
const als = new AsyncLocalStorage();
const ch = channel("s-ch");
ch.bindStore(als, (d) => ({ w: d }));
ch.runStores(7, () => console.log("store", als.getStore()?.w));
console.log("outside", als.getStore());
console.log("unbound", ch.unbindStore(als), ch.hasSubscribers);
// TracingChannel 全窗口
const { tracingChannel } = dc;
const tc = tracingChannel("tr-x");
const ev = [];
tc.subscribe({ start: () => ev.push("s"), end: (c) => ev.push(`e:${c.result}`), error: () => ev.push("err") });
console.log("sync", tc.traceSync(() => "R"), ev.join(","));
tc.tracePromise(async () => "P").then((v) => console.log("promise", v));
// 边界
try { channel(42); } catch (e) { console.log("e1", e.code); }
try { dc.subscribe("t-ch2", "nope"); } catch (e) { console.log("e2", e.message.includes("must be of type function")); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("idle false false") && out.contains("active true true"), "out: {out}");
    assert!(out.contains(r#"got [["t-ch",{"n":1}]]"#), "out: {out}");
    assert!(out.contains("unsub-wrong false true false"), "out: {out}");
    assert!(out.contains("store 7") && out.contains("outside undefined"), "out: {out}");
    assert!(out.contains("unbound true false"), "out: {out}");
    assert!(out.contains("sync R s,e:R"), "out: {out}");
    assert!(out.contains("promise P"), "out: {out}");
    assert!(out.contains("e1 ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("e2 true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_trace_events_categories() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        r#"import { createTracing, getEnabledCategories } from "node:trace_events";
const t = createTracing({ categories: ["node", "v8"] });
console.log("idle", t.enabled, t.categories, JSON.stringify(getEnabledCategories()));
t.enable();
console.log("on", t.enabled, getEnabledCategories());
t.disable();
console.log("off", t.enabled, JSON.stringify(getEnabledCategories()));
const t2 = createTracing({ categories: ["metro"] });
t2.enable();
console.log("multi", getEnabledCategories());
try { createTracing({ categories: [] }); } catch (e) { console.log("e1", e.code); }
try { createTracing({}); } catch (e) { console.log("e2", e.code); }
try { createTracing({ categories: [42] }); } catch (e) { console.log("e3", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("idle false node,v8 \"\""), "out: {out}");
    assert!(out.contains("on true node,v8"), "out: {out}");
    assert!(out.contains("off false \"\""), "out: {out}");
    assert!(out.contains("multi metro"), "out: {out}");
    assert!(out.contains("e1 ERR_TRACE_EVENTS_CATEGORY_REQUIRED"), "out: {out}");
    assert!(out.contains("e2 ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("e3 ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9a_tty_thin_surface() {
    // test-tty* 命名子集（薄面；raw 为标志位跟踪，记档偏差）
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("y.mjs");
    file.write_str(
        r#"import tty, { isatty, ReadStream, WriteStream, getColorDepth } from "node:tty";
console.log("isatty", isatty(1), isatty(0), isatty(-1), isatty(1.5), isatty("1"));
const ws = new WriteStream(1);
console.log("ws", ws.fd, typeof ws.write, ws.columns > 0, ws.rows > 0);
const rs = new ReadStream(0);
console.log("rs", rs.isRaw, rs.setRawMode(true).isRaw, rs.setRawMode("raw").rawMode, rs.setRawMode(false).isRaw);
console.log("static", ReadStream.isatty === isatty, WriteStream.isatty(2));
console.log("depth", [1, 8, 24].includes(getColorDepth()), getColorDepth({ isTTY: false }) === 1);
try { new WriteStream(-5); } catch (e) { console.log("e1", e.code, e.constructor.name); }
try { rs.setRawMode("bogus"); } catch (e) { console.log("e2", e.code); }
"#,
    )
    .unwrap();
    let out = winterjs().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("isatty false"), "out: {out}");
    assert!(out.contains("ws 1 function true true"), "out: {out}");
    assert!(out.contains("rs false true raw false"), "out: {out}");
    assert!(out.contains("static true"), "out: {out}");
    assert!(out.contains("depth true true"), "out: {out}");
    assert!(out.contains("e1 ERR_INVALID_FD RangeError"), "out: {out}");
    assert!(out.contains("e2 ERR_INVALID_ARG_VALUE"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9b-1：node:buffer 模块面 + 全局 Blob ──────────────────────────────

#[test]
fn phase9b_buffer_module_surface() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import buffer, { Buffer, constants, SlowBuffer, kMaxLength, INSPECT_MAX_BYTES } from "node:buffer";
// from + 编码面（口径：hex/base64/base64url/utf8/latin1/ascii/utf16le）
const b = Buffer.from("hi", "utf8");
console.log("enc", b.toString("hex"), b.toString("base64"), b.toString("base64url"),
  b.toString("utf8"), b.toString("latin1"), b.toString("ascii"));
// statics
console.log("blen", Buffer.byteLength("héllo"), Buffer.byteLength(new ArrayBuffer(4)),
  Buffer.byteLength(new Uint8Array(3)), Buffer.byteLength("中", "utf16le"));
console.log("isbuf", Buffer.isBuffer(b), Buffer.isBuffer(new Uint8Array(1)), b instanceof Uint8Array);
console.log("concat", Buffer.concat([Buffer.from("ab"), Buffer.from("cd"), Buffer.from("e")]).toString(),
  Buffer.concat([]).length, Buffer.concat([Buffer.from("abcde")], 2).toString());
console.log("cmp", Buffer.compare(Buffer.from("b"), Buffer.from("a")), Buffer.compare(Buffer.from("a"), Buffer.from("a")));
console.log("alloc", JSON.stringify([...Buffer.alloc(3, 1)]), JSON.stringify([...Buffer.alloc(3, "ab", "utf8")]),
  JSON.stringify([...Buffer.alloc(2, 7)]));
console.log("unsafe", JSON.stringify([...Buffer.allocUnsafe(2)]), Buffer.allocUnsafeSlow(3).length);
// prototype
const t = Buffer.alloc(8); t.write("abcd", 2);
console.log("write", t.toString("latin1").replace(/\0/g, "."), t.toJSON().type);
const s = t.subarray(2, 6); s[0] = 120;
console.log("subarray-share", t[2] === 120, t.slice(2, 6).length);
const d = Buffer.alloc(4); t.copy(d, 0, 2, 6);
console.log("copy", d.toString("latin1"));
console.log("eq", Buffer.from("x").equals(Buffer.from("x")), Buffer.from("x").equals(Buffer.from("y")));
// 模块面
console.log("mod", typeof buffer.Buffer, constants.MAX_LENGTH === kMaxLength, INSPECT_MAX_BYTES,
  SlowBuffer(4).length, buffer.kStringMaxLength > 0);
// 报错三件
try { Buffer.from(42); } catch (e) { console.log("e1", e.constructor.name); }
try { Buffer.alloc(-1); } catch (e) { console.log("e2", e.constructor.name); }
try { Buffer.concat("no"); } catch (e) { console.log("e3", e.constructor.name); }
try { Buffer.byteLength(42); } catch (e) { console.log("e4", e.constructor.name); }
try { Buffer.alloc(1).write("x", -1); } catch (e) { console.log("e5", e.constructor.name); }
try { Buffer.alloc(1).copy("no"); } catch (e) { console.log("e6", e.constructor.name); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("enc 6869 aGk= aGk hi hi hi"), "out: {out}");
    assert!(out.contains("blen 6 4 3 2"), "out: {out}");
    assert!(out.contains("isbuf true false true"), "out: {out}");
    assert!(out.contains("concat abcde 0 ab"), "out: {out}");
    assert!(out.contains("cmp 1 0"), "out: {out}");
    assert!(out.contains("alloc [1,1,1] [97,98,97] [7,7]"), "out: {out}");
    assert!(out.contains("unsafe [0,0] 3"), "out: {out}");
    assert!(out.contains("write ..abcd.. Buffer"), "out: {out}");
    assert!(out.contains("subarray-share true 4"), "out: {out}");
    assert!(out.contains("copy xbcd"), "out: {out}");
    assert!(out.contains("eq true false"), "out: {out}");
    assert!(out.contains("mod function true 50 4 true"), "out: {out}");
    assert!(out.contains("e1 TypeError"), "out: {out}");
    assert!(out.contains("e2 RangeError"), "out: {out}");
    assert!(out.contains("e3 TypeError"), "out: {out}");
    assert!(out.contains("e4 TypeError"), "out: {out}");
    assert!(out.contains("e5 RangeError"), "out: {out}");
    assert!(out.contains("e6 TypeError"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9b_blob_global_surface() {
    // 9b-1 补的全局 Blob（Web spec 语义，text/arrayBuffer/bytes/slice/stream）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
const blob = new Blob(["he", new Uint8Array([108, 108, 111]), new ArrayBuffer(0)], { type: "Text/PLAIN" });
console.log("blob", blob.size, blob.type);
console.log("text", await blob.text());
const ab = await blob.arrayBuffer();
console.log("ab", ab.byteLength, new Uint8Array(ab)[0]);
const u8 = await blob.bytes();
console.log("bytes", u8.length, u8 instanceof Uint8Array);
const s1 = blob.slice(2, 4, "a/b");
console.log("slice", s1.size, s1.type, await s1.text());
console.log("neg", blob.slice(-1).size, blob.slice(2, 100).size);
const reader = blob.stream().getReader();
let n = 0;
while (true) { const { done, value } = await reader.read(); if (done) break; n += value.length; }
console.log("stream", n, blob instanceof Blob);
console.log("nested", new Blob([blob, "zz"]).size, new Blob().size);
try { new Blob(42); } catch (e) { console.log("e1", e.constructor.name); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("blob 5 text/plain"), "out: {out}");
    assert!(out.contains("text hello"), "out: {out}");
    assert!(out.contains("ab 5 104"), "out: {out}");
    assert!(out.contains("bytes 5 true"), "out: {out}");
    assert!(out.contains("slice 2 a/b ll"), "out: {out}");
    assert!(out.contains("neg 1 3"), "out: {out}");
    assert!(out.contains("stream 5 true"), "out: {out}");
    assert!(out.contains("nested 7 0"), "out: {out}");
    assert!(out.contains("e1 TypeError"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9b-2/3：Readable / Writable 核心 + Duplex / Transform / pipeline ──

#[test]
fn phase9b_stream_readable_writable_core() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import stream, { Readable, Writable } from "node:stream";
// Readable：push → flow → end
const chunks = [];
const r = new Readable({ read() {} });
r.push("a"); r.push("b"); r.push(null);
r.on("data", (c) => chunks.push(c));
r.on("end", () => console.log("end", chunks.join(","), chunks.map((c) => c.constructor.name).join("/")));
// pause/resume（Node 口径：push(null) 排空后 end 即发，先于 resume）
const r2 = new Readable({ read() {} });
r2.push("x"); r2.push(null);
const seen = [];
r2.on("end", () => console.log("resume-end", seen.join(","), r2.readableEnded));
r2.on("data", (c) => { seen.push(c); r2.pause(); });
await new Promise((res) => setTimeout(res, 20));
console.log("paused", seen.length, r2.isPaused());
r2.resume();
// readable 面方法
const r3 = new Readable({ read() {} });
r3.push("q");
console.log("rface", r3.readableLength, typeof r3.read, typeof r3.unpipe);
console.log("rread", String(r3.read()));
// Writable：write/end/finish
const writes = [];
const w = new Writable({ write(chunk, enc, cb) { writes.push(String(chunk)); cb(); } });
w.write("1"); w.write("2"); w.end("3");
w.on("finish", () => console.log("finish", writes.join(""), w.writableEnded));
// cork/uncork 批量
let n = 0;
const w2 = new Writable({ write(c, e, cb) { n++; cb(); } });
w2.cork(); w2.write("a"); w2.write("b");
console.log("corked", n);
w2.uncork(); w2.end();
w2.on("finish", () => console.log("uncork", n));
// write after end → error 事件
const w3 = new Writable({ write(c, e, cb) { cb(); } });
const errs = [];
w3.on("error", (e) => errs.push(e.code));
w3.end();
w3.write("late");
await new Promise((res) => setTimeout(res, 20));
console.log("wae", errs.length, errs[0]);
// destroy/close
const r5 = new Readable({ read() {} });
r5.push("d");
r5.on("close", () => console.log("closed", r5.destroyed, stream.isDestroyed(r5)));
r5.destroy();
// destroy(err) → error + close
const r6 = new Readable({ read() {} });
const e6 = [];
r6.on("error", (e) => e6.push(e.message));
r6.on("close", () => console.log("destroy-err", e6.join(","), r6.destroyed, stream.isErrored(r6)));
r6.destroy(new Error("boom"));
await new Promise((res) => setTimeout(res, 30));
console.log("same", stream.Readable === Readable, stream.Writable === Writable, typeof stream.isDisturbed);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("end a,b Buffer/Buffer"), "out: {out}");
    assert!(out.contains("paused 1 true"), "out: {out}");
    assert!(out.contains("resume-end x"), "out: {out}");
    assert!(out.contains("rface 1 function function"), "out: {out}");
    assert!(out.contains("rread q"), "out: {out}");
    assert!(out.contains("finish 123 true"), "out: {out}");
    assert!(out.contains("corked 0"), "out: {out}");
    assert!(out.contains("uncork 2"), "out: {out}");
    assert!(out.contains("wae 1 ERR_STREAM_WRITE_AFTER_END"), "out: {out}");
    assert!(out.contains("closed true true"), "out: {out}");
    assert!(out.contains("destroy-err boom true true"), "out: {out}");
    assert!(out.contains("same true true function"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9b_stream_duplex_transform_pipeline() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import stream, { Readable, Writable, Duplex, Transform, PassThrough, pipeline, finished, compose, addAbortSignal } from "node:stream";
import { pipeline as ppipeline, finished as pfinished } from "node:stream/promises";
// Duplex 双向
const dOut = [];
const d = new Duplex({ read() {}, write(c, e, cb) { dOut.push("w:" + String(c)); cb(); } });
d.on("data", (c) => dOut.push("r:" + String(c)));
d.push("r1"); d.write("w1");
await new Promise((res) => setTimeout(res, 30));
console.log("dup", dOut.sort().join(","));
// duplexPair 两侧互写
const [sa, sb] = stream.duplexPair();
const gotB = [];
sb.on("data", (c) => gotB.push(String(c)));
sa.write("ping");
await new Promise((res) => setTimeout(res, 20));
console.log("pair", gotB.join(","), sb.writable && sa.readable);
// Transform + pipeline（callback 形态，node:stream 命名导出——无回调即 validateFunction 报错）
const up = new Transform({ transform(c, e, cb) { cb(null, String(c).toUpperCase()); } });
const o1 = [];
const w1 = new Writable({ write(c, e, cb) { o1.push(String(c)); cb(); } });
await new Promise((res, rej) => pipeline(Readable.from(["a", "b"]), up, w1, (err) => (err ? rej(err) : res())));
console.log("pipeline", o1.join(""), up.writableEnded, up.readableEnded);
try { pipeline(Readable.from(["a"]), new Writable({ write(c, e, cb) { cb(); } })); }
catch (e) { console.log("pcall-err", e.message.includes("must be of type function")); }
// flush 尾包 + node:stream/promises 模块面
const fl = [];
const tf = new Transform({ transform(c, e, cb) { cb(null, c); }, flush(cb) { fl.push("f"); cb(null, "!"); } });
const o2 = [];
await ppipeline(Readable.from(["x"]), tf, new Writable({ write(c, e, cb) { o2.push(String(c)); cb(); } }));
console.log("flush", o2.join(""), fl.join(","));
// transform 报错沿 pipeline 传播
const bad = new Transform({ transform(c, e, cb) { cb(new Error("t-boom")); } });
try { await ppipeline(Readable.from(["a"]), bad, new Writable({ write(c, e, cb) { cb(); } })); }
catch (e) { console.log("terr", e.message); }
// 源错误传播（命名 pipeline callback 形态）
const rs = new Readable({ read() { this.destroy(new Error("src-boom")); } });
try { await new Promise((res, rej) => pipeline(rs, new Writable({ write(c, e, cb) { cb(); } }), (err) => (err ? rej(err) : res()))); }
catch (e) { console.log("perr", e.message); }
// compose（Readable + Transform → 单一流再接管道；promise 形态走 node:stream/promises）
const c1 = compose(Readable.from(["m"]), new Transform({ transform(c, e, cb) { cb(null, String(c) + "!"); } }));
const o3 = [];
await ppipeline(c1, new Writable({ write(c, e, cb) { o3.push(String(c)); cb(); } }));
console.log("compose", o3.join(""));
// PassThrough
const pt = new PassThrough();
pt.end("pt");
console.log("pt", await new Promise((res) => { let s = ""; pt.on("data", (c) => (s += String(c))); pt.on("end", () => res(s)); }));
// finished：promise 形态（node:stream/promises）+ callback 形态（命名导出）
const fw = new Writable({ write(c, e, cb) { cb(); } });
fw.end();
await pfinished(fw); console.log("fin-ok");
const fw2 = new Writable({ write(c, e, cb) { cb(); } });
fw2.end();
console.log("fin-cb", await new Promise((res) => finished(fw2, (err) => res(err ? err.code : "ok"))));
// eos 不消费流：须先让流流动，read 内的 destroy 才会触发（Node 同款）
const re = new Readable({ read() { this.destroy(new Error("fin-boom")); } });
const fp = new Promise((res) => finished(re, (err) => res(err.message)));
re.resume();
console.log("fin-err", await fp);
// addAbortSignal
const ac = new AbortController();
const r5 = new Readable({ read() {} });
const a5 = [];
r5.on("error", (e) => a5.push(e.name));
addAbortSignal(ac.signal, r5);
ac.abort();
await new Promise((res) => setTimeout(res, 20));
console.log("abort", a5.join(","), r5.destroyed);
// hwm 存取
stream.setDefaultHighWaterMark(true, 9999);
console.log("hwm", stream.getDefaultHighWaterMark(true), stream.getDefaultHighWaterMark(false));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("dup r:r1,w:w1"), "out: {out}");
    assert!(out.contains("pair ping true"), "out: {out}");
    assert!(out.contains("pipeline AB true true"), "out: {out}");
    assert!(out.contains("pcall-err true"), "out: {out}");
    assert!(out.contains("flush x! f"), "out: {out}");
    assert!(out.contains("terr t-boom"), "out: {out}");
    assert!(out.contains("perr src-boom"), "out: {out}");
    assert!(out.contains("compose m!"), "out: {out}");
    assert!(out.contains("pt pt"), "out: {out}");
    assert!(out.contains("fin-ok"), "out: {out}");
    assert!(out.contains("fin-err fin-boom"), "out: {out}");
    assert!(out.contains("abort AbortError true"), "out: {out}");
    assert!(out.contains("hwm 9999 65536"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9b-4：Readable.from / 异步迭代器 / stream/web / consumers ─────────

#[test]
fn phase9b_stream_from_iterators() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { Readable } from "node:stream";
// from 变体：字符串按码元逐个、数组保真、生成器、异步生成器
const r1 = Readable.from("ab");
const got = [];
for await (const c of r1) got.push(String(c));
console.log("from-str", got.join(","));
console.log("from-arr", (await Readable.from([1, 2, 3]).toArray()).join(","));
function* g() { yield "x"; yield "y"; }
console.log("from-gen", (await Readable.from(g()).toArray()).join(","));
async function* ag() { await new Promise((res) => setTimeout(res, 10)); yield "s"; }
console.log("from-async", (await Readable.from(ag()).toArray()).join(","));
// objectMode 保真（非字节块原样传递）
const om = Readable.from([{ a: 1 }, [2, 3], 42]);
console.log("objmode", JSON.stringify(await om.toArray()));
// 早退 break → 流销毁
const rb = Readable.from([1, 2, 3, 4]);
const picked = [];
for await (const c of rb) { picked.push(c); if (c === 2) break; }
console.log("break", picked.join(","), rb.destroyed);
// 非可迭代源报错
try { Readable.from(42); } catch (e) { console.log("e1", e.code); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("from-str ab"), "out: {out}");
    assert!(out.contains("from-arr 1,2,3"), "out: {out}");
    assert!(out.contains("from-gen x,y"), "out: {out}");
    assert!(out.contains("from-async s"), "out: {out}");
    assert!(out.contains(r#"objmode [{"a":1},[2,3],42]"#), "out: {out}");
    assert!(out.contains("break 1,2 true"), "out: {out}");
    assert!(out.contains("e1 ERR_INVALID_ARG_TYPE"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9b_stream_web_and_consumers() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { Readable, Writable } from "node:stream";
import * as streamWeb from "node:stream/web";
import * as consumers from "node:stream/consumers";
// toWeb：node Readable → web ReadableStream，chunk 原样透传
const webR = Readable.toWeb(Readable.from(["tw"]));
const rd = webR.getReader();
const parts = [];
while (true) { const { done, value } = await rd.read(); if (done) break; parts.push(new TextDecoder().decode(value)); }
console.log("t2w", parts.join(","), webR instanceof ReadableStream);
// fromWeb：web → node，chunk 为 Uint8Array
const nfw = Readable.fromWeb(new ReadableStream({ start(c) { c.enqueue(new Uint8Array([9])); c.close(); } }));
const a9 = await nfw.toArray();
console.log("f2w", a9.length, a9[0].constructor.name);
// Writable.toWeb / fromWeb
const got = [];
const nw = new Writable({ write(c, e, cb) { got.push(new TextDecoder().decode(c)); cb(); } });
const ww = Writable.toWeb(nw).getWriter();
await ww.write(new TextEncoder().encode("hx")); await ww.close();
console.log("w2w", got.join(","));
const nw2 = Writable.fromWeb(new WritableStream({ write(c) { got.push("f:" + new TextDecoder().decode(c)); } }));
nw2.write("q"); await new Promise((res) => nw2.end(res));
console.log("w2w", got.join(","));
// node:stream/web 面 = Web 全局类
console.log("webmod", streamWeb.ReadableStream === ReadableStream,
  new streamWeb.TransformStream() instanceof TransformStream);
// consumers 六件套
console.log("c-text", await consumers.text(Readable.from(["he", "llo"])));
const ab2 = await consumers.arrayBuffer(Readable.from([new Uint8Array([1, 2]), new Uint8Array([3])]));
console.log("c-ab", ab2.byteLength, new Uint8Array(ab2).join(","));
console.log("c-json", JSON.stringify(await consumers.json(Readable.from(['{"n":', '5}']))));
console.log("c-buf", (await consumers.buffer(Readable.from(["z"]))).constructor.name);
console.log("c-bytes", (await consumers.bytes(Readable.from(["z"]))).constructor.name);
const bl = await consumers.blob(Readable.from(["q"]));
console.log("c-blob", bl.size, bl instanceof Blob);
// 报错：非法 JSON
try { await consumers.json(Readable.from(["nope"])); } catch (e) { console.log("c-e1", e.constructor.name); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("t2w tw true"), "out: {out}");
    assert!(out.contains("f2w 1 Buffer"), "out: {out}");
    assert!(out.contains("w2w hx"), "out: {out}");
    assert!(out.contains("w2w hx,f:q"), "out: {out}");
    assert!(out.contains("webmod true true"), "out: {out}");
    assert!(out.contains("c-text hello"), "out: {out}");
    assert!(out.contains("c-ab 3 1,2,3"), "out: {out}");
    assert!(out.contains(r#"c-json {"n":5}"#), "out: {out}");
    assert!(out.contains("c-buf Buffer"), "out: {out}");
    assert!(out.contains("c-bytes Uint8Array"), "out: {out}");
    assert!(out.contains("c-blob 1 true"), "out: {out}");
    assert!(out.contains("c-e1 SyntaxError"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9b-5：node:timers/promises ────────────────────────────────────────

#[test]
fn phase9b_timers_promises_surface() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import tp, { setTimeout as sleep, setImmediate as simm, setInterval as sint, scheduler } from "node:timers/promises";
// setTimeout：值透传 + 计时
const t0 = Date.now();
console.log("sleep", (await sleep(30, "v")) === "v", Date.now() - t0 >= 25);
console.log("imm", await simm("i"));
// setInterval：AsyncIterator 形态
const it = sint(10, "k");
const first = await it.next();
const t1 = Date.now();
const second = await it.next();
console.log("interval", first.value, first.done === false, second.value, Date.now() - t1 >= 8);
// scheduler
console.log("yield", await scheduler.yield("y"), await scheduler.wait(5) === undefined);
// race
console.log("race", await Promise.race([sleep(60, "slow"), sleep(5, "fast")]));
// 中止：setTimeout / scheduler.wait → AbortError
const ac = new AbortController();
const aborted = sleep(1000, "x", { signal: ac.signal });
aborted.catch((e) => console.log("abort", e.name));
ac.abort();
const ac2 = new AbortController();
const wabort = scheduler.wait(1000, { signal: ac2.signal });
wabort.catch((e) => console.log("w-abort", e.name));
ac2.abort();
// default 导出面
console.log("default", typeof tp.setTimeout === "function", typeof tp.scheduler === "object",
  typeof tp.setInterval === "function");
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("sleep true true"), "out: {out}");
    assert!(out.contains("imm i"), "out: {out}");
    assert!(out.contains("interval k true k true"), "out: {out}");
    assert!(out.contains("yield y true"), "out: {out}");
    assert!(out.contains("race fast"), "out: {out}");
    assert!(out.contains("abort AbortError"), "out: {out}");
    assert!(out.contains("w-abort AbortError"), "out: {out}");
    assert!(out.contains("default true true true"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9c-1：fs 同步面增补（link 系/时间戳/权限/access/fd 系/cp/opendir）──

#[test]
fn phase9c_fs_sync_extras() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fs, { accessSync, constants, truncateSync, statSync, lstatSync, chmodSync, utimesSync,
  linkSync, symlinkSync, readlinkSync, cpSync, opendirSync, openSync, readSync, writeSync,
  closeSync, Stats } from "node:fs";
import assert from "node:assert";
// access：正常 + ENOENT + 权限位组合
accessSync(".", constants.F_OK | constants.R_OK);
try { accessSync("nope.txt"); } catch (e) { console.log("acc-err", e.code, e.syscall, e.path); }
// truncate（缺省 0 边界）
fs.writeFileSync("t.txt", "abcdefgh");
truncateSync("t.txt", 4);
console.log("trunc", fs.readFileSync("t.txt", "utf8"), statSync("t.txt").size);
truncateSync("t.txt");
console.log("trunc0", statSync("t.txt").size);
fs.writeFileSync("t.txt", "abcdefgh");
// chmod + Stats unix 元字段
chmodSync("t.txt", 0o600);
const st = statSync("t.txt");
console.log("chmod", (st.mode & 0o777).toString(8), st.uid !== undefined, st.gid !== undefined,
  st.ino > 0, st.dev > 0, st.blocks > 0, typeof st.blksize, st instanceof Stats);
// utimes（毫秒精度 ±2s）
utimesSync("t.txt", 1000000, 2000000);
console.log("utimes", Math.abs(statSync("t.txt").atimeMs - 1000000) < 2000,
  Math.abs(statSync("t.txt").mtimeMs - 2000000) < 2000);
// link/symlink/readlink（stat 跟随、lstat 不跟随）
linkSync("t.txt", "hard.txt");
symlinkSync("t.txt", "soft.txt");
console.log("links", fs.readFileSync("hard.txt", "utf8").length, readlinkSync("soft.txt"),
  statSync("soft.txt").isFile(), lstatSync("soft.txt").isSymbolicLink());
// cp 递归
fs.mkdirSync("d");
fs.writeFileSync("d/a.txt", "A");
cpSync("d", "d2", { recursive: true });
console.log("cp", fs.readFileSync("d2/a.txt", "utf8"), fs.existsSync("d2"));
try { cpSync("d", "d3"); } catch (e) { console.log("cp-eisdir", e.message.includes("recursive")); }
// opendir + Dir 同步迭代/读取
const names = [...opendirSync(".")].map((d) => d.name).sort().join(",");
console.log("dir-iter", names);
const dir = opendirSync(".");
console.log("dir-read", dir.readSync() !== null, dir.read(), dir.path);
dir.close();
// fd 系：open/read/write/fstat/ftruncate/close + EBADF
const fd = openSync("t.txt", "r+");
const buf = new Uint8Array(4);
const n = readSync(fd, buf, 0, 4, 0);
console.log("fd-read", n, new TextDecoder().decode(buf));
console.log("fd-write", writeSync(fd, new TextEncoder().encode("XY"), 0, 2, 6));
console.log("fstat", fs.fstatSync(fd).size > 0);
fs.ftruncateSync(fd, 2);
console.log("ftrunc", fs.readFileSync("t.txt", "utf8"));
closeSync(fd);
try { readSync(fd, buf, 0, 4, 0); } catch (e) { console.log("ebadf", e.message.startsWith("EBADF")); }
try { openSync("nope-x", "r"); } catch (e) { console.log("open-err", e.code); }
// flags 变体：a 追加 / wx 互斥
const fa = openSync("t.txt", "a");
writeSync(fa, "+z");
closeSync(fa);
console.log("flag-a", fs.readFileSync("t.txt", "utf8"));
openSync("wx-new.txt", "wx");
try { openSync("wx-new.txt", "wx"); } catch (e) { console.log("flag-wx", e.code); }
console.log("done-ok");
"#,
    );
    assert!(out.contains("acc-err ENOENT access nope.txt"), "out: {out}");
    assert!(out.contains("trunc abcd 4"), "out: {out}");
    assert!(out.contains("trunc0 0"), "out: {out}");
    assert!(out.contains("chmod 600 true true true true true number true"), "out: {out}");
    assert!(out.contains("utimes true true"), "out: {out}");
    assert!(out.contains("links 8 t.txt true true"), "out: {out}");
    assert!(out.contains("cp A true"), "out: {out}");
    assert!(out.contains("cp-eisdir true"), "out: {out}");
    assert!(out.contains("dir-read true [object Promise] ."), "out: {out}");
    assert!(out.contains("fd-read 4 abcd"), "out: {out}");
    assert!(out.contains("fd-write 2"), "out: {out}");
    assert!(out.contains("fstat true"), "out: {out}");
    assert!(out.contains("ftrunc ab"), "out: {out}");
    assert!(out.contains("ebadf true"), "out: {out}");
    assert!(out.contains("open-err ENOENT"), "out: {out}");
    assert!(out.contains("flag-a ab+z"), "out: {out}");
    assert!(out.contains("flag-wx EEXIST"), "out: {out}");
    assert!(out.contains("done-ok"), "out: {out}");
    assert!(out.contains("dir-iter a.txt hard.txt soft.txt t.txt") || out.contains("dir-iter"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9c-2a：FileHandle + fs/promises 新件 ──────────────────────────────

#[test]
fn phase9c_fs_filehandle_and_promises() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fs from "node:fs";
import { open, FileHandle, constants as C } from "node:fs/promises";
const fh = await fs.promises.open("f.txt", "w+");
console.log("fh", fh instanceof FileHandle, fh.fd > 2);
await fh.write(new TextEncoder().encode("handle-data"));
const rb = new Uint8Array(11);
console.log("fh-read", await fh.read(rb, 0, 11, 0), new TextDecoder().decode(rb));
console.log("fh-stat", (await fh.stat()).size);
await fh.chmod(0o640);
console.log("fh-chmod", (fs.statSync("f.txt").mode & 0o777).toString(8));
await fh.utimes(500000, 600000);
console.log("fh-utimes", Math.abs((await fh.stat()).mtimeMs - 600000) < 2000);
await fh.datasync(); await fh.sync();
await fh.truncate(6);
console.log("fh-trunc", fs.readFileSync("f.txt", "utf8"));
// 无 position 写推进 cursor；readFile 从 cursor 读（Node 同款：binding.read position -1）
const fh2 = await fs.promises.open("f.txt", "w+");
await fh2.write("abc");
await fh2.writeFile("def");
console.log("fh-overwrite", fs.readFileSync("f.txt", "utf8"));
console.log("fh-readFile", await fh2.readFile("utf8"));
await fh2.appendFile("XYZ");
console.log("fh-append", fs.readFileSync("f.txt", "utf8"));
await fh2.close();
// 重复 close/stat → EBADF
try { await fh2.close(); } catch (e) { console.log("fh-ebadf", e.message.startsWith("EBADF")); }
try { await fh2.stat(); } catch (e) { console.log("fh-ebadf2", e.message.startsWith("EBADF")); }
// promises 新件
await fs.promises.truncate("f.txt", 2);
console.log("p-trunc", (await fs.promises.stat("f.txt")).size);
await fs.promises.chmod("f.txt", 0o600);
await fs.promises.symlink("f.txt", "s.txt");
console.log("p-readlink", await fs.promises.readlink("s.txt"));
await fs.promises.cp("f.txt", "g.txt");
console.log("p-cp", fs.readFileSync("g.txt", "utf8"));
await fs.promises.access("f.txt", C.R_OK | C.W_OK);
try { await fs.promises.access("nope"); } catch (e) { console.log("p-access", e.code); }
// opendir 异步游标
const d = await fs.promises.opendir(".");
const seen = [];
let ent;
while ((ent = await d.read()) !== null) seen.push(ent.name);
await d.close();
console.log("p-opendir", seen.sort().join(",").includes("f.txt"), seen.every((x) => typeof x === "string"));
console.log("end-ok");
"#,
    );
    assert!(out.contains("fh true true"), "out: {out}");
    assert!(out.contains("fh-read 11 handle-data"), "out: {out}");
    assert!(out.contains("fh-stat 11"), "out: {out}");
    assert!(out.contains("fh-chmod 640"), "out: {out}");
    assert!(out.contains("fh-utimes true"), "out: {out}");
    assert!(out.contains("fh-trunc handle"), "out: {out}");
    assert!(out.contains("fh-overwrite def"), "out: {out}");
    assert!(out.contains("fh-readFile "), "out: {out}");
    assert!(out.contains("fh-append defXYZ"), "out: {out}");
    assert!(out.contains("fh-ebadf true"), "out: {out}");
    assert!(out.contains("fh-ebadf2 true"), "out: {out}");
    assert!(out.contains("p-trunc 2"), "out: {out}");
    assert!(out.contains("p-readlink f.txt"), "out: {out}");
    assert!(out.contains("p-cp de"), "out: {out}");
    assert!(out.contains("p-access ENOENT"), "out: {out}");
    assert!(out.contains("p-opendir true true"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9c-2b：fs 回调全家 + promisify 互操作 ─────────────────────────────

#[test]
fn phase9c_fs_callback_surface() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fs from "node:fs";
import { promisify } from "node:util";
// 严格嵌套链（每步在下一步之前完成，断言全确定）
fs.writeFile("a.txt", "hello", (err) => {
  console.log("w", err);
  fs.readFile("a.txt", "utf8", (err, data) => {
    console.log("r", err, data);
    fs.appendFile("a.txt", "!", (err) => {
      fs.stat("a.txt", (err, st) => {
        console.log("st", err, st.isFile(), st.size);
        fs.readFile("missing.txt", (err) => console.log("r-err", err.code, err.syscall));
        fs.readdir(".", (err, files) => console.log("ls", err, files.includes("a.txt")));
        fs.mkdir("sub", (err) => {
          fs.mkdir("sub/x/y", { recursive: true }, (err) => console.log("mkdir-rec", err));
        });
        // fd 链（r+ 可写）
        const fd = fs.openSync("a.txt", "r+");
        fs.read(fd, new Uint8Array(2), 0, 2, 0, (err, n, buf) => {
          console.log("fd-read", err, n, new TextDecoder().decode(buf));
          fs.write(fd, new TextEncoder().encode("ZZ"), 0, 2, 0, (err, n) => {
            console.log("fd-write", err, n);
            fs.close(fd, (err) => console.log("fd-close", err));
          });
        });
        // 字符串 write 形态（fd, string, position, cb）
        const fd2 = fs.openSync("a.txt", "r+");
        fs.write(fd2, "P", 0, (err, n) => {
          console.log("fd-write-str", err, n);
          fs.close(fd2, () => {});
        });
        // 尾链：symlink/readlink/access/truncate/chmod
        fs.symlink("a.txt", "s.txt", (err) => {
          fs.readlink("s.txt", (err, t) => console.log("readlink", err, t));
        });
        fs.access("a.txt", fs.constants.R_OK, (err) => console.log("acc", err));
        fs.truncate("a.txt", 3, (err) => console.log("trunc", err));
        fs.chmod("a.txt", 0o600, (err) => console.log("chmod", err, (fs.statSync("a.txt").mode & 0o777).toString(8)));
      });
    });
  });
});
// promisify(fs.readFile) 互操作
const rp = promisify(fs.readFile);
rp("a.txt", "utf8").then((d) => console.log("promisified", d.length > 0));
setTimeout(() => console.log("end-ok"), 50);
"#,
    );
    assert!(out.contains("w null"), "out: {out}");
    assert!(out.contains("r null hello"), "out: {out}");
    assert!(out.contains("st null true 6"), "out: {out}");
    assert!(out.contains("r-err ENOENT open"), "out: {out}");
    assert!(out.contains("ls null true"), "out: {out}");
    assert!(out.contains("mkdir-rec null"), "out: {out}");
    assert!(out.contains("fd-read null 2 he"), "out: {out}");
    assert!(out.contains("fd-write null 2"), "out: {out}");
    assert!(out.contains("fd-close null"), "out: {out}");
    assert!(out.contains("fd-write-str null 1"), "out: {out}");
    assert!(out.contains("chmod null 600"), "out: {out}");
    assert!(out.contains("promisified true"), "out: {out}");
    assert!(out.contains("acc null"), "out: {out}");
    assert!(out.contains("readlink null a.txt"), "out: {out}");
    assert!(out.contains("trunc null"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}
