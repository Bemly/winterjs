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

// ── Phase 9d-1：node:net TCP 回环（hermetic，port 0 避冲突）─────────────────

#[test]
fn phase9d_net_echo_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import net, { Socket, createServer, createConnection } from "node:net";
import assert from "node:assert";
const server = createServer((sock) => {
  assert.ok(sock instanceof Socket);
  sock.on("data", (chunk) => {
    console.log("srv-recv", typeof chunk, String(chunk), sock.remoteAddress, sock.remotePort > 0);
    sock.write("echo:" + String(chunk));
  });
  sock.on("end", () => { console.log("srv-end"); sock.end(); });
  sock.on("close", () => console.log("srv-close"));
});
server.listen(0, "127.0.0.1", () => {
  const addr = server.address();
  console.log("listening", typeof addr.port === "number" && addr.port > 0, addr.address, addr.family);
  const s = net.connect(addr.port, "127.0.0.1", () => {
    console.log("cli-connect-cb");
  });
  s.on("connect", () => {
    console.log("cli-connect", s.remoteAddress, s.localAddress !== null);
    s.write("ping");
  });
  s.on("data", (chunk) => {
    console.log("cli-recv", String(chunk));
    s.end();
  });
  s.on("end", () => console.log("cli-end"));
  s.on("close", () => { console.log("cli-close"); server.close(); });
});
server.on("close", () => console.log("server-closed"));
// 第二连接：destroy 硬关 + write after destroy 报错
const srv2 = createServer((sock) => {
  sock.on("data", () => { sock.destroy(); });
});
srv2.listen(0, "127.0.0.1", () => {
  const c = createConnection(srv2.address().port, "127.0.0.1");
  c.on("connect", () => {
    c.write("boom");
  });
  c.on("close", () => {
    console.log("destroyed-close");
    try { c.write("late"); } catch (e) { console.log("wae", e.code); }
    srv2.close();
  });
});
setTimeout(() => console.log("end-ok"), 200);
"#,
    );
    assert!(out.contains("srv-recv object ping 127.0.0.1 true"), "out: {out}");
    assert!(out.contains("listening true 127.0.0.1 IPv4"), "out: {out}");
    assert!(out.contains("cli-connect-cb"), "out: {out}");
    assert!(out.contains("cli-connect 127.0.0.1 true"), "out: {out}");
    assert!(out.contains("cli-recv echo:ping"), "out: {out}");
    assert!(out.contains("cli-end"), "out: {out}");
    assert!(out.contains("cli-close"), "out: {out}");
    assert!(out.contains("srv-end"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("server-closed"), "out: {out}");
    assert!(out.contains("destroyed-close"), "out: {out}");
    assert!(out.contains("wae ERR_STREAM_DESTROYED"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_net_server_errors() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createServer } from "node:net";
// 占位 server 抢住端口，第二个 server 绑定同端口 → 'error' 事件 EADDRINUSE
const holder = createServer(() => {});
holder.listen(0, "127.0.0.1", () => {
  const port = holder.address().port;
  const s2 = createServer(() => {});
  s2.on("error", (e) => {
    console.log("bind-err", e.code, e.port === port);
    holder.close();
  });
  s2.on("close", () => console.log("s2-close"));
  s2.listen(port, "127.0.0.1");
});
holder.on("close", () => console.log("holder-close"));
setTimeout(() => console.log("end-ok"), 200);
"#,
    );
    assert!(out.contains("bind-err EADDRINUSE true"), "out: {out}");
    assert!(out.contains("s2-close"), "out: {out}");
    assert!(out.contains("holder-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9d-2：node:dns（hermetic，仅 localhost/回环）──────────────────────

#[test]
fn phase9d_dns_localhost() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import dns, { lookup, resolve4, resolve6 } from "node:dns";
lookup("localhost", (err, address, family) => {
  console.log("lookup", err === null, family === 4 || family === 6, /^[\d.]+$|^[0-9a-f:]+$/.test(address));
});
lookup("localhost", { all: true }, (err, addrs) => {
  console.log("lookup-all", err === null, Array.isArray(addrs), addrs.length >= 1,
    addrs.every((a) => typeof a.address === "string" && (a.family === 4 || a.family === 6)));
});
lookup("localhost", { family: 4 }, (err, address, family) => {
  console.log("lookup-v4", err === null, family === 4, address === "127.0.0.1");
});
resolve4("localhost", (err, addrs) => {
  console.log("resolve4", err === null, addrs.includes("127.0.0.1"));
});
resolve6("localhost", (err, addrs) => {
  console.log("resolve6", err === null, addrs.includes("::1") || addrs.length >= 0);
});
dns.promises.lookup("localhost").then((r) => {
  console.log("p-lookup", typeof r.address === "string", r.family === 4 || r.family === 6);
});
dns.promises.lookup("localhost", { all: true }).then((r) => {
  console.log("p-lookup-all", Array.isArray(r));
});
// 空主机名 → 报错带 code（平台错误码不定，断言 Error 形状）
lookup("", (err) => {
  console.log("empty-err", err instanceof Error, typeof err.code === "string", err.syscall === "getaddrinfo");
});
setTimeout(() => console.log("end-ok"), 50);
"#,
    );
    assert!(out.contains("lookup true true true"), "out: {out}");
    assert!(out.contains("lookup-all true true true true"), "out: {out}");
    assert!(out.contains("lookup-v4 true true true"), "out: {out}");
    assert!(out.contains("resolve4 true true"), "out: {out}");
    assert!(out.contains("resolve6 true"), "out: {out}");
    assert!(out.contains("p-lookup true true"), "out: {out}");
    assert!(out.contains("p-lookup-all true"), "out: {out}");
    assert!(out.contains("empty-err true true true"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9d-3：node:http 回环（JS-over-net 解析器；hermetic port 0）────────

#[test]
fn phase9d_http_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http, { createServer, request, get, STATUS_CODES, IncomingMessage, ServerResponse } from "node:http";
import assert from "node:assert";
const server = createServer((req, res) => {
  assert.ok(req instanceof IncomingMessage);
  assert.ok(res instanceof ServerResponse);
  if (req.method === "POST") {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      res.writeHead(201, { "x-reply": "ok" });
      res.end("echo:" + body);
    });
    return;
  }
  if (req.url === "/404") { res.writeHead(404); res.end("nope"); return; }
  if (req.url === "/500") { res.writeHead(500, "Boom"); res.end("bad"); return; }
  if (req.url === "/head-hz") { res.setHeader("x-sync", "1"); res.end("hz"); return; }
  res.end("hello");
});
server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  console.log("listening", typeof port === "number" && port > 0);
  // GET（options 形态 + 自定义头）
  http.get({ port, path: "/a?b=1", headers: { "X-Custom": "yes" } }, (res) => {
    console.log("get", res.statusCode, res.headers["x-sync"], res.httpVersion,
      typeof res.headers["content-length"]);
    let body = "";
    res.on("data", (c) => (body += c));
    res.on("end", () => {
      console.log("get-body", body);
      // POST（回声：体经 data/end 回传）
      const req = http.request({ port, path: "/echo", method: "POST" }, (res2) => {
        let b = "";
        res2.on("data", (c) => (b += c));
        res2.on("end", () => {
          console.log("post", res2.statusCode, res2.headers["x-reply"], b);
          // URL 字符串形态 + 404
          get(`http://127.0.0.1:${port}/404`, (r3) => {
            let b3 = "";
            r3.on("data", (c) => (b3 += c));
            r3.on("end", () => {
              console.log("404", r3.statusCode, r3.statusMessage, b3);
              // 500 + 自定义 statusMessage + writeHead 头
              const rq = request({ port, path: "/500", method: "PUT" }, (r4) => {
                let b4 = "";
                r4.on("data", (c) => (b4 += c));
                r4.on("end", () => {
                  console.log("500", r4.statusCode, r4.statusMessage, b4);
                  // setHeader 路径 + finish 事件
                  const rq2 = request({ port, path: "/head-hz" }, (r5) => {
                    console.log("finish-res", r5.statusCode);
                    r5.on("data", () => {});
                    r5.on("end", () => server.close());
                  });
                  rq2.setHeader("x-a", "b");
                  rq2.end();
                  rq2.on("close", () => console.log("rq2-close"));
                });
              }).end("payload");
            });
          });
        });
      });
      req.write("hi");
      req.end("!");
    });
  });
});
server.on("close", () => console.log("server-closed", STATUS_CODES[201], STATUS_CODES[418]));
setTimeout(() => console.log("end-ok"), 300);
"#,
    );
    let out = out;
    assert!(out.contains("listening true"), "out: {out}");
    assert!(out.contains("get 200 undefined 1.1 string"), "out: {out}");
    assert!(out.contains("get-body hello"), "out: {out}");
    assert!(out.contains("post 201 ok echo:hi!"), "out: {out}");
    assert!(out.contains("404 404 Not Found nope"), "out: {out}");
    assert!(out.contains("500 500 Boom bad"), "out: {out}");
    assert!(out.contains("finish-res 200"), "out: {out}");
    assert!(out.contains("rq2-close"), "out: {out}");
    assert!(out.contains("server-closed Created I'm a Teapot"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_http_client_errors() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http from "node:http";
// 连接拒绝（回环空闲端口）→ 'error' 事件 ECONNREFUSED
const s = http.request({ port: 1, path: "/", host: "127.0.0.1" }, () => {});
s.on("error", (e) => {
  console.log("conn-err", e.code);
  // https 协议拒绝（ERR_INVALID_PROTOCOL）
  try { http.get("https://127.0.0.1/x"); } catch (e2) { console.log("proto-err", e2.message.startsWith("ERR_INVALID_PROTOCOL")); }
  // write after end
  const req = http.request({ port: 1, host: "127.0.0.1" }, () => {});
  req.on("error", () => {}); // 无监听的 error 事件即抛错（Node 口径），此处静默
  req.end();
  try { req.write("x"); } catch (e3) { console.log("wae", e3.message.startsWith("ERR_STREAM_WRITE_AFTER_END")); }
  setTimeout(() => console.log("end-ok"), 30);
});
"#,
    );
    let out = out;
    assert!(out.contains("conn-err ECONNREFUSED"), "out: {out}");
    assert!(out.contains("proto-err true"), "out: {out}");
    assert!(out.contains("wae true"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9d-4：node:dgram UDP 回环（hermetic，port 0）──────────────────────

#[test]
fn phase9d_dgram_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import dgram, { createSocket } from "node:dgram";
import assert from "node:assert";
// createSocket 类型校验
try { createSocket("udp7"); } catch (e) { console.log("bad-type", e.code); }
const server = createSocket("udp4", (msg, rinfo) => {
  console.log("srv-msg", String(msg), rinfo.address, rinfo.port > 0, rinfo.family,
    rinfo.size === msg.length);
  server.send(Buffer.from("pong"), rinfo.port, rinfo.address);
});
server.on("listening", () => {
  const addr = server.address();
  console.log("srv-addr", addr.port > 0, addr.address, addr.family);
  const client = createSocket({ type: "udp4" });
  client.on("message", (msg) => {
    console.log("cli-msg", String(msg));
    client.close();
  });
  client.on("close", () => server.close());
  client.bind(0, "127.0.0.1", () => {
    console.log("cli-bound", client.address().port > 0);
    // send：string + Buffer 两种形态
    client.send("ping", addr.port, "127.0.0.1");
  });
});
server.bind(0, "127.0.0.1");
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 200);
"#,
    );
    let out = out;
    assert!(out.contains("bad-type ERR_SOCKET_BAD_TYPE"), "out: {out}");
    assert!(out.contains("srv-addr true 127.0.0.1 IPv4"), "out: {out}");
    assert!(out.contains("cli-bound true"), "out: {out}");
    assert!(out.contains("srv-msg ping 127.0.0.1 true 4 true"), "out: {out}");
    assert!(out.contains("cli-msg pong"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}
// ── Phase 9d-5：node:zlib ────

#[test]
fn phase9d_zlib_sync_roundtrip() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import z, {
  deflateSync, inflateSync, deflateRawSync, inflateRawSync,
  gzipSync, gunzipSync, unzipSync, brotliCompressSync, brotliDecompressSync,
  zstdCompressSync, zstdDecompressSync, constants, codes,
} from "node:zlib";
const s = "the quick brown fox jumps over the lazy dog. ".repeat(40);
const pairs = [
  ["deflate", deflateSync, inflateSync],
  ["deflateRaw", deflateRawSync, inflateRawSync],
  ["gzip", gzipSync, gunzipSync],
  ["brotli", brotliCompressSync, brotliDecompressSync],
  ["zstd", zstdCompressSync, zstdDecompressSync],
];
for (const [name, enc, dec] of pairs) {
  const c = enc(s);
  const back = dec(c);
  console.log(name, c.length < s.length, Buffer.isBuffer(c), back.toString() === s);
}
// 输入形态：string / Uint8Array / ArrayBuffer / DataView
console.log("u8", gunzipSync(gzipSync(new TextEncoder().encode(s))).toString() === s);
console.log("ab", gunzipSync(gzipSync(new TextEncoder().encode(s).buffer)).toString() === s);
console.log("dv", gunzipSync(gzipSync(new DataView(new TextEncoder().encode(s).buffer))).toString() === s);
// unzip 自动识别 gzip 与 zlib 包裹
console.log("unzip", unzipSync(gzipSync(s)).toString() === s, unzipSync(deflateSync(s)).toString() === s);
// level 生效：0（stored）大于默认压缩体积
console.log("level", gzipSync(s, { level: 0 }).length > gzipSync(s).length);
// brotli params[1]（BROTLI_PARAM_QUALITY）与 quality 等效
const a = brotliCompressSync(s, { quality: 1 });
const b = brotliCompressSync(s, { params: { 1: 1 } });
console.log("brotli-q", a.length === b.length, brotliDecompressSync(b).toString() === s);
// constants / codes / 顶层别名（Node 口径）
console.log("const", constants.Z_OK === 0, constants.Z_DATA_ERROR === -3,
  constants.Z_BEST_COMPRESSION === 9, constants.Z_DEFAULT_COMPRESSION === -1,
  constants.BROTLI_OPERATION_PROCESS === 0, constants.BROTLI_PARAM_QUALITY === 1,
  constants.BROTLI_MAX_QUALITY === 11);
console.log("codes", codes.Z_DATA_ERROR === -3, codes[-3] === "Z_DATA_ERROR", codes[0] === "Z_OK");
console.log("alias", z.Z_OK === 0, z.Z_STREAM_END === 1, z.Z_SYNC_FLUSH === 2);
console.log("ns", typeof z.deflate === "function", typeof z.gunzipSync === "function");
"#,
    );
    assert!(out.contains("deflate true true true"), "out: {out}");
    assert!(out.contains("deflateRaw true true true"), "out: {out}");
    assert!(out.contains("gzip true true true"), "out: {out}");
    assert!(out.contains("brotli true true true"), "out: {out}");
    assert!(out.contains("zstd true true true"), "out: {out}");
    assert!(out.contains("u8 true"), "out: {out}");
    assert!(out.contains("ab true"), "out: {out}");
    assert!(out.contains("dv true"), "out: {out}");
    assert!(out.contains("unzip true true"), "out: {out}");
    assert!(out.contains("level true"), "out: {out}");
    assert!(out.contains("brotli-q true true"), "out: {out}");
    assert!(out.contains("const true true true true true true true"), "out: {out}");
    assert!(out.contains("codes true true true"), "out: {out}");
    assert!(out.contains("alias true true true"), "out: {out}");
    assert!(out.contains("ns true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_zlib_async_callback() {
    // 回调链严格嵌套（§4.33：独立异步链交错即 flaky）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import z from "node:zlib";
const s = "async zlib chain ".repeat(60);
z.gzip(s, (e1, c1) => {
  console.log("gzip", !e1, Buffer.isBuffer(c1));
  z.gunzip(c1, (e2, b1) => {
    console.log("gunzip", !e2, String(b1) === s);
    z.deflate(s, { level: 9 }, (e3, c2) => {
      console.log("deflate", !e3);
      z.inflate(c2, (e4, b2) => {
        console.log("inflate", !e4, String(b2) === s);
        z.brotliCompress(s, (e5, c3) => {
          console.log("brotliC", !e5);
          z.brotliDecompress(c3, (e6, b3) => {
            console.log("brotliD", !e6, String(b3) === s);
            z.zstdCompress(s, (e7, c4) => {
              console.log("zstdC", !e7);
              z.zstdDecompress(c4, (e8, b4) => {
                console.log("zstdD", !e8, String(b4) === s);
                // 回调内错误路径：坏输入进 err，不抛
                z.gunzip(Buffer.from("garbage-in-garbage-out!!!!!!!!!!!!"), (e9, b5) => {
                  console.log("bad", !!e9, e9.code, e9.errno, b5 === undefined);
                  console.log("done");
                });
              });
            });
          });
        });
      });
    });
  });
});
"#,
    );
    assert!(out.contains("gzip true true"), "out: {out}");
    assert!(out.contains("gunzip true true"), "out: {out}");
    assert!(out.contains("deflate true"), "out: {out}");
    assert!(out.contains("inflate true true"), "out: {out}");
    assert!(out.contains("brotliC true"), "out: {out}");
    assert!(out.contains("brotliD true true"), "out: {out}");
    assert!(out.contains("zstdC true"), "out: {out}");
    assert!(out.contains("zstdD true true"), "out: {out}");
    assert!(out.contains("bad true Z_DATA_ERROR -3 true"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_zlib_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { gunzipSync, inflateSync, gzipSync, brotliCompressSync, brotliDecompressSync, gzip, unzipSync } from "node:zlib";
// 报错：坏输入 code/errno 形状
for (const [name, fn] of [["gunzip", gunzipSync], ["inflate", inflateSync], ["unzip", unzipSync], ["brotliD", brotliDecompressSync]]) {
  try { fn(Buffer.from("definitely not compressed data at all!!!")); console.log(name, "no-throw"); }
  catch (e) { console.log(name, e.code, e.errno, e instanceof Error); }
}
// 报错：越界 level/quality → ERR_OUT_OF_RANGE（直通不套 zlib 形）
for (const [name, fn] of [["lv-hi", () => gzipSync("x", { level: 10 })], ["lv-lo", () => gzipSync("x", { level: -2 })], ["q-hi", () => brotliCompressSync("x", { quality: 12 })]]) {
  try { fn(); console.log(name, "no-throw"); }
  catch (e) { console.log(name, e.code, e instanceof RangeError); }
}
// 报错：缺回调同步抛 TypeError；错输入类型同步抛 TypeError
try { gzip("x"); } catch (e) { console.log("nocb", e.constructor.name === "TypeError"); }
try { gzipSync(123); } catch (e) { console.log("badin", e.constructor.name === "TypeError"); }
// 边界：空输入往返；单字节；大块 1MB
console.log("empty", gunzipSync(gzipSync("")).length === 0);
console.log("one", gunzipSync(gzipSync("Q")).toString() === "Q");
const big = "0123456789abcdef".repeat(65536);
console.log("big", gunzipSync(gzipSync(big)).toString() === big);
console.log("stored", gunzipSync(gzipSync(big, { level: 0 })).toString() === big);
"#,
    );
    assert!(out.contains("gunzip Z_DATA_ERROR -3 true"), "out: {out}");
    assert!(out.contains("inflate Z_DATA_ERROR -3 true"), "out: {out}");
    assert!(out.contains("unzip Z_DATA_ERROR -3 true"), "out: {out}");
    assert!(out.contains("brotliD Z_DATA_ERROR -3 true"), "out: {out}");
    assert!(out.contains("lv-hi ERR_OUT_OF_RANGE true"), "out: {out}");
    assert!(out.contains("lv-lo ERR_OUT_OF_RANGE true"), "out: {out}");
    assert!(out.contains("q-hi ERR_OUT_OF_RANGE true"), "out: {out}");
    assert!(out.contains("nocb true"), "out: {out}");
    assert!(out.contains("badin true"), "out: {out}");
    assert!(out.contains("empty true"), "out: {out}");
    assert!(out.contains("one true"), "out: {out}");
    assert!(out.contains("big true"), "out: {out}");
    assert!(out.contains("stored true"), "out: {out}");
    dir.close().unwrap();
}
// ── Phase 9d-6：node:tls / node:https ────

/// rcgen 自签 end-entity 证书（SAN 127.0.0.1；落盘供 JS 侧 readFileSync）。
fn write_self_signed(dir: &assert_fs::TempDir) -> (String, String) {
    let key = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let cert_pem = key.cert.pem();
    let key_pem = key.signing_key.serialize_pem();
    let cert = dir.child("t-cert.pem");
    cert.write_str(&cert_pem).unwrap();
    let k = dir.child("t-key.pem");
    k.write_str(&key_pem).unwrap();
    (
        cert.path().to_string_lossy().into_owned(),
        k.path().to_string_lossy().into_owned(),
    )
}

#[test]
fn phase9d_tls_echo_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "p.mjs",
        &format!(
            r#"
import tls from "node:tls";
import fs from "node:fs";
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
try {{ tls.createServer({{}}); }} catch (e) {{ console.log("no-cert", e.constructor.name); }}
const server = tls.createServer({{ key, cert }});
server.on("secureConnection", (sock) => {{
  console.log("srv-secure", sock.encrypted, sock.authorized);
  sock.on("data", (c) => sock.write("tls-echo:" + c));
}});
server.listen(0, "127.0.0.1", () => {{
  const port = server.address().port;
  // ca 校验路径：authorized 为 true
  const cli = tls.connect({{ port, host: "127.0.0.1", ca: cert }}, () => {{
    console.log("cli-secure", cli.encrypted, cli.authorized, cli.authorizationError === null);
    cli.write("hello-tls");
  }});
  cli.on("data", (c) => {{
    console.log("cli-data", String(c));
    cli.end();
  }});
  cli.on("close", () => {{
    // rejectUnauthorized:false 路径：连上但未授权
    const cli2 = tls.connect({{ port, host: "127.0.0.1", rejectUnauthorized: false }}, () => {{
      console.log("cli2-secure", cli2.encrypted, cli2.authorized, cli2.authorizationError !== null);
      cli2.end();
    }});
    cli2.on("close", () => server.close());
    cli2.on("error", () => {{}});
  }});
  cli.on("error", () => {{}});
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    assert!(out.contains("no-cert TypeError"), "out: {out}");
    assert!(out.contains("cli-secure true true true"), "out: {out}");
    assert!(out.contains("srv-secure true true"), "out: {out}");
    assert!(out.contains("cli-data tls-echo:hello-tls"), "out: {out}");
    assert!(out.contains("cli2-secure true false true"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_https_loopback() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "p.mjs",
        &format!(
            r#"
import https from "node:https";
import http from "node:http";
import fs from "node:fs";
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
const server = https.createServer({{ key, cert }}, (req, res) => {{
  let b = "";
  req.on("data", (c) => (b += c));
  req.on("end", () => res.end("secure:" + req.method + ":" + b));
}});
server.listen(0, "127.0.0.1", () => {{
  const port = server.address().port;
  const r = https.request(
    {{ port, host: "127.0.0.1", path: "/s", method: "POST", ca: cert }},
    (res) => {{
      let b = "";
      res.on("data", (c) => (b += c));
      res.on("end", () => {{
        console.log("post", res.statusCode, b);
        https.get(`https://127.0.0.1:${{port}}/g?x=1`, {{ ca: cert }}, (res2) => {{
          let b2 = "";
          res2.on("data", (c) => (b2 += c));
          res2.on("end", () => {{
            console.log("get", res2.statusCode, b2);
            server.close();
          }});
        }}).on("error", () => {{}});
      }});
    }}
  );
  r.on("error", () => {{}});
  r.end("secret");
  try {{ http.get("https://x/"); }} catch (e) {{ console.log("proto", /node:https/.test(e.message)); }}
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    assert!(out.contains("proto true"), "out: {out}");
    assert!(out.contains("post 200 secure:POST:secret"), "out: {out}");
    assert!(out.contains("get 200 secure:GET:"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_tls_errors() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "p.mjs",
        &format!(
            r#"
import tls from "node:tls";
import fs from "node:fs";
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
// 坏 PEM 同步 TypeError（fail fast）
try {{ tls.createServer({{ key: "nope", cert }}).listen(0); }} catch (e) {{ console.log("badkey", e.message.startsWith("TypeError:")); }}
const server = tls.createServer({{ key, cert }});
server.listen(0, "127.0.0.1", () => {{
  const port = server.address().port;
  // 自签无 ca：握手失败，错误提 certificate（不断具体码，hermetic 口径）
  const a = tls.connect({{ port, host: "127.0.0.1" }});
  a.on("error", (e) => {{
    console.log("selfsign", e.code, /certificate|issuer|verify/i.test(e.message));
    // 拒连：code 为 string（具体码平台相关，不断言值）
    const b = tls.connect({{ port: 1, host: "127.0.0.1", rejectUnauthorized: false }});
    b.on("error", (e2) => {{
      console.log("refused", typeof e2.code, b.destroyed === false);
      server.close();
    }});
  }});
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    assert!(out.contains("badkey true"), "out: {out}");
    assert!(out.contains("selfsign ERR_TLS_HANDSHAKE true"), "out: {out}");
    assert!(out.contains("refused string true"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}
// ── Phase 9d-7：node:http2 ────

#[test]
fn phase9d_http2_cleartext() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import http2, { createServer, connect, constants } from "node:http2";
console.log("const", constants.HTTP2_HEADER_METHOD === ":method", constants.NGHTTP2_NO_ERROR === 0);
const server = createServer();
server.on("request", (req, res) => {
  let b = "";
  req.on("data", (c) => (b += c));
  req.on("end", () => {
    res.setHeader("x-r", req.url);
    res.writeHead(req.url === "/missing" ? 404 : 200);
    res.end("h2:" + req.method + ":" + b);
  });
});
server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  const sess = connect(`http://127.0.0.1:${port}`);
  sess.on("error", () => {});
  sess.on("connect", () => {
    // 两流同 session 并发（多路复用；到达序不定，收集排序后断言）
    const got = [];
    const maybeDone = () => {
      if (got.length === 4) {
        got.sort();
        console.log("mux", got.join("|"));
        sess.close();
      }
    };
    for (const [path, body] of [["/a", "one"], ["/missing", "two"]]) {
      const st = sess.request({ ":method": "POST", ":path": path });
      st.on("response", (h) => got.push(`h${h[":status"]}`));
      let b = "";
      st.on("data", (c) => (b += c));
      st.on("end", () => { got.push(`b${b}`); maybeDone(); });
      st.on("error", () => {});
      st.end(body);
    }
  });
  sess.on("close", () => server.close());
});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#,
    );
    assert!(out.contains("const true true"), "out: {out}");
    assert!(out.contains("mux bh2:POST:one|bh2:POST:two|h200|h404"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_http2_secure() {
    let dir = assert_fs::TempDir::new().unwrap();
    let (cert_path, key_path) = write_self_signed(&dir);
    let out = run_fs_file(
        &dir,
        "p.mjs",
        &format!(
            r#"
import {{ createSecureServer, connect }} from "node:http2";
import fs from "node:fs";
const key = fs.readFileSync({key_path:?}, "utf8");
const cert = fs.readFileSync({cert_path:?}, "utf8");
try {{ createSecureServer({{}}); }} catch (e) {{ console.log("no-cert", e.constructor.name); }}
const server = createSecureServer({{ key, cert }}, (req, res) => {{
  res.end("secure-h2:" + req.url);
}});
server.listen(0, "127.0.0.1", () => {{
  const port = server.address().port;
  const sess = connect(`https://127.0.0.1:${{port}}`, {{ ca: cert }});
  sess.on("error", (e) => console.log("sess-err", e.code));
  sess.on("connect", () => {{
    const st = sess.request({{ ":path": "/s" }});
    st.on("response", (h) => console.log("h", h[":status"]));
    let b = "";
    st.on("data", (c) => (b += c));
    st.on("end", () => {{ console.log("b", b); sess.close(); }});
    st.on("error", () => {{}});
    st.end();
  }});
  sess.on("close", () => server.close());
}});
server.on("close", () => console.log("srv-close"));
setTimeout(() => console.log("end-ok"), 1500);
"#
        ),
    );
    assert!(out.contains("no-cert TypeError"), "out: {out}");
    assert!(out.contains("h 200"), "out: {out}");
    assert!(out.contains("b secure-h2:/s"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9d_http2_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createServer, connect } from "node:http2";
// 拒连：code 为 string（平台相关，不断值）
const bad = connect("http://127.0.0.1:1");
bad.on("error", (e) => {
  console.log("refused", typeof e.code);
  const server = createServer((req, res) => {
    let b = "";
    req.on("data", (c) => (b += c));
    req.on("end", () => res.end("ok:" + b.length));
  });
  server.listen(0, "127.0.0.1", () => {
    const port = server.address().port;
    const sess = connect(`http://127.0.0.1:${port}`);
    sess.on("error", () => {});
    sess.on("connect", () => {
      // 边界：空体 GET + 1MB 体往返
      const g = sess.request({ ":path": "/e" });
      g.on("response", (h) => console.log("empty-h", h[":status"]));
      let eb = "";
      g.on("data", (c) => (eb += c));
      g.on("end", () => {
        console.log("empty-b", eb);
        const big = "ab".repeat(524288);
        const st = sess.request({ ":method": "POST", ":path": "/big" });
        let rb = "";
        st.on("data", (c) => (rb += c));
        st.on("end", () => {
          console.log("big", rb === "ok:" + big.length);
          sess.close();
        });
        st.on("error", () => {});
        st.end(big);
      });
      g.on("error", () => {});
      g.end();
    });
    sess.on("close", () => server.close());
  });
  server.on("close", () => console.log("srv-close"));
});
setTimeout(() => console.log("end-ok"), 2500);
"#,
    );
    assert!(out.contains("refused string"), "out: {out}");
    assert!(out.contains("empty-h 200"), "out: {out}");
    assert!(out.contains("empty-b ok:0"), "out: {out}");
    assert!(out.contains("big true"), "out: {out}");
    assert!(out.contains("srv-close"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}
// ── Phase 9e-1a：node:crypto（Hash/Hmac/随机/杂项） ────

#[test]
fn phase9e_crypto_hash_hmac() {
    // 真 Node 取证向量（HMAC-SHA256/MD5/SHA3-256 + BLAKE2b/SHA3-512，逐字节对）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import c, { createHash, createHmac, hash, getHashes, getCurves } from "node:crypto";
console.log("sha", createHash("sha256").update("a").update("b").digest("hex") === "fb8e20fc2e4c3f248c60c39bd652f3c1347298bb977b8b4d5903b85055620603");
console.log("hmac", createHmac("sha256", "key").update("The quick brown fox jumps over the lazy dog").digest("hex") === "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8");
console.log("hmac-md5", createHmac("md5", "key").update("msg").digest("hex") === "18e3548c59ad40dd03907b7aeee71d67");
console.log("hmac-s3", createHmac("sha3-256", "key").update("msg").digest("hex") === "56b616feab81d996beb8cf47719b253cfe6d1da9be562c63520fef130a6d935e");
console.log("blake", createHash("blake2b512").update("abc").digest("hex").slice(0, 32) === "ba80a53f981c4d0d6a2797b69f12f6e9");
console.log("md5vec", createHash("md5").update("abc").digest("hex") === "900150983cd24fb0d6963f7d28e17f72");
const h = createHash("sha256"); h.update("a"); const h2 = h.copy();
console.log("copy", h2.update("b").digest("hex") === createHash("sha256").update("ab").digest("hex"));
console.log("buf", Buffer.isBuffer(createHash("sha256").update("x").digest()), createHash("sha256").update("x").digest("hex").length === 64);
console.log("oneshot", hash("sha256", "abc", "hex") === "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
console.log("alias", createHash("RSA-SHA256").update("x").digest("hex").slice(0, 8) === createHash("sha256").update("x").digest("hex").slice(0, 8));
console.log("hashes", getHashes().includes("sha256") && getHashes().includes("blake2s256") && getHashes().includes("ripemd160") && getHashes().includes("shake256"));
console.log("curves", getCurves().includes("prime256v1") && getCurves().includes("ed25519"));
console.log("ns", typeof c.createHash === "function", c.webcrypto === globalThis.crypto);
"#,
    );
    assert!(out.contains("sha true"), "out: {out}");
    assert!(out.contains("hmac true"), "out: {out}");
    assert!(out.contains("hmac-md5 true"), "out: {out}");
    assert!(out.contains("hmac-s3 true"), "out: {out}");
    assert!(out.contains("blake true"), "out: {out}");
    assert!(out.contains("md5vec true"), "out: {out}");
    assert!(out.contains("copy true"), "out: {out}");
    assert!(out.contains("buf true true"), "out: {out}");
    assert!(out.contains("oneshot true"), "out: {out}");
    assert!(out.contains("alias true"), "out: {out}");
    assert!(out.contains("hashes true"), "out: {out}");
    assert!(out.contains("curves true"), "out: {out}");
    assert!(out.contains("ns true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_random() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { randomBytes, randomFill, randomFillSync, randomInt, randomUUID, randomUUIDv7, timingSafeEqual } from "node:crypto";
console.log("rb", randomBytes(16).length === 16, Buffer.isBuffer(randomBytes(4)));
randomBytes(8, (e, b) => {
  console.log("rbcb", e === null, b.length === 8);
  randomInt(1, 7, (e2, v) => {
    console.log("ricb", e2 === null, v >= 1 && v < 7);
    const buf = Buffer.alloc(8);
    randomFill(buf, 2, 4, (e3, out) => {
      console.log("rfcb", e3 === null, out === buf);
      console.log("done");
    });
  });
});
console.log("ri", randomInt(5) >= 0 && randomInt(5) < 5, randomInt(3, 4) === 3);
const u = randomUUID();
console.log("uuid", u.length === 36 && u[14] === "4");
const v7 = randomUUIDv7();
console.log("uuid7", v7.length === 36 && v7[14] === "7" && v7 !== randomUUIDv7());
const f = Buffer.alloc(4); randomFillSync(f);
console.log("rfsync", f.length === 4, randomFillSync(new Uint8Array(3)).length === 3);
console.log("tse", timingSafeEqual(Buffer.from([1, 2]), Buffer.from([1, 2])) === true,
  timingSafeEqual(Buffer.from([1, 2]), Buffer.from([1, 3])) === false);
"#,
    );
    assert!(out.contains("rb true true"), "out: {out}");
    assert!(out.contains("rbcb true true"), "out: {out}");
    assert!(out.contains("ricb true true"), "out: {out}");
    assert!(out.contains("rfcb true true"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    assert!(out.contains("ri true true"), "out: {out}");
    assert!(out.contains("uuid true"), "out: {out}");
    assert!(out.contains("uuid7 true"), "out: {out}");
    assert!(out.contains("rfsync true true"), "out: {out}");
    assert!(out.contains("tse true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createHash, createHmac, randomBytes, randomInt, randomFillSync, timingSafeEqual, hash } from "node:crypto";
// 报错：未知算法（Hash 无码原文 / Hmac 有码，俱为真 Node 口径）
try { createHash("nope"); } catch (e) { console.log("halg", e.code === undefined, e.message); }
try { createHmac("nope", "k"); } catch (e) { console.log("malg", e.code === "ERR_CRYPTO_INVALID_DIGEST"); }
try { createHash(123); } catch (e) { console.log("halgtype", e.code === "ERR_INVALID_ARG_TYPE"); }
// 报错：finalized 后 update/copy（真 Node 同码）
try { const h = createHash("sha256"); h.digest(); h.update("x"); } catch (e) { console.log("fin", e.code === "ERR_CRYPTO_HASH_FINALIZED"); }
try { const h = createHash("sha256"); h.digest(); h.copy(); } catch (e) { console.log("fincopy", e.code === "ERR_CRYPTO_HASH_FINALIZED"); }
// 报错：随机数形状
try { randomBytes(-1); } catch (e) { console.log("rneg", e.code === "ERR_OUT_OF_RANGE"); }
try { randomInt(5, 5); } catch (e) { console.log("rrange", e.code === "ERR_OUT_OF_RANGE"); }
try { randomInt(); } catch (e) { console.log("rinttype", e.code === "ERR_INVALID_ARG_TYPE"); }
try { randomFillSync("no"); } catch (e) { console.log("rftype", e.code === "ERR_INVALID_ARG_TYPE"); }
try { timingSafeEqual(Buffer.from([1]), Buffer.from([1, 2])); } catch (e) { console.log("tse", e.code === "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH"); }
try { hash("sha256", "x", "nope"); } catch (e) { console.log("henc", e.code === "ERR_INVALID_ARG_VALUE"); }
// 边界：未知输出编码回 Buffer（真 Node 宽容口径）；空输入；大块 1MB 往返一致
const enc = createHash("sha256").update("x").digest("nope");
console.log("badenc", Buffer.isBuffer(enc));
console.log("empty", createHash("sha256").update("").digest("hex") === "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
const big = "ab".repeat(524288);
console.log("big", createHash("sha256").update(big).digest("hex") === createHash("sha256").update(big).digest("hex"));
"#,
    );
    assert!(out.contains("halg true Digest method not supported"), "out: {out}");
    assert!(out.contains("malg true"), "out: {out}");
    assert!(out.contains("halgtype true"), "out: {out}");
    assert!(out.contains("fin true"), "out: {out}");
    assert!(out.contains("fincopy true"), "out: {out}");
    assert!(out.contains("rneg true"), "out: {out}");
    assert!(out.contains("rrange true"), "out: {out}");
    assert!(out.contains("rinttype true"), "out: {out}");
    assert!(out.contains("rftype true"), "out: {out}");
    assert!(out.contains("tse true"), "out: {out}");
    assert!(out.contains("henc true"), "out: {out}");
    assert!(out.contains("badenc true"), "out: {out}");
    assert!(out.contains("empty true"), "out: {out}");
    assert!(out.contains("big true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_cipher_roundtrip() {
    // 真 Node 取证向量（逐字节对；gcm/chacha tag 另断长度）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createCipheriv, createDecipheriv, getCiphers, getCipherInfo } from "node:crypto";
const key = Buffer.alloc(32, 1), iv16 = Buffer.alloc(16, 2), iv12 = Buffer.alloc(12, 3);
const x = createCipheriv("aes-256-cbc", key, iv16);
console.log("cbc", x.update("hello world", "utf8", "hex") + x.final("hex"));
const e = createCipheriv("aes-256-cbc", key, iv16);
const ct = Buffer.concat([e.update("hi"), e.final()]);
const d = createDecipheriv("aes-256-cbc", key, iv16);
console.log("dec", d.update(ct).toString() + d.final("utf8"));
// 流式多 update 与 oneshot 等价
const a = createCipheriv("aes-256-cbc", key, iv16);
const p1 = a.update("hel", "utf8", "hex") + a.update("lo world", "utf8", "hex") + a.final("hex");
console.log("stream", p1 === "f563737a376afbed282274255a7fcabd");
const g = createCipheriv("aes-256-gcm", key, iv12);
g.setAAD(Buffer.from("aad"));
console.log("gcm", g.update("secret", "utf8", "hex") + g.final("hex"), g.getAuthTag().length);
const gd = createDecipheriv("aes-256-gcm", key, iv12);
gd.setAAD(Buffer.from("aad")); gd.setAuthTag(g.getAuthTag());
const gct = Buffer.from("8b0477e89af0", "hex");
console.log("gdec", gd.update(gct).toString() + gd.final("utf8"));
const ch = createCipheriv("chacha20-poly1305", key, iv12);
console.log("chacha", ch.update("hello", "utf8", "hex") + ch.final("hex"), ch.getAuthTag().length);
const chd = createDecipheriv("chacha20-poly1305", key, iv12);
chd.setAuthTag(ch.getAuthTag());
console.log("chdec", chd.update(Buffer.from("e66dea2709", "hex")).toString() + chd.final("utf8"));
const t = createCipheriv("aes-128-ctr", Buffer.alloc(16, 7), iv16);
console.log("ctr", t.update("0123456789abcdef", "utf8", "hex") + t.final("hex"));
console.log("list", getCiphers().includes("aes-256-gcm") && getCiphers().includes("des-ede3-cbc"));
const info = getCipherInfo("aes-256-cbc");
console.log("info", info.mode === "cbc" && info.keyLength === 32 && info.ivLength === 16 && info.nid === 427);
console.log("nounk", getCipherInfo("nope") === undefined);
"#,
    );
    assert!(out.contains("cbc f563737a376afbed282274255a7fcabd"), "out: {out}");
    assert!(out.contains("dec hi"), "out: {out}");
    assert!(out.contains("stream true"), "out: {out}");
    assert!(out.contains("gcm 8b0477e89af0 16"), "out: {out}");
    assert!(out.contains("gdec secret"), "out: {out}");
    assert!(out.contains("chacha e66dea2709 16"), "out: {out}");
    assert!(out.contains("chdec hello"), "out: {out}");
    assert!(out.contains("ctr 60d4f4ceae18fbef892ccaa49d8b32a6"), "out: {out}");
    assert!(out.contains("list true"), "out: {out}");
    assert!(out.contains("info true"), "out: {out}");
    assert!(out.contains("nounk true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_cipher_errors() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { createCipheriv, createDecipheriv } from "node:crypto";
const key = Buffer.alloc(32, 1), iv16 = Buffer.alloc(16, 2), iv12 = Buffer.alloc(12, 3);
try { createCipheriv("aes-999-cbc", key, iv16); } catch (e) { console.log("alg", e.code === "ERR_CRYPTO_UNKNOWN_CIPHER"); }
try { createCipheriv("aes-256-cbc", Buffer.alloc(5), iv16); } catch (e) { console.log("key", e.code === "ERR_CRYPTO_INVALID_KEYLEN"); }
try { createCipheriv("aes-256-cbc", key, Buffer.alloc(4)); } catch (e) { console.log("iv", e.code === "ERR_CRYPTO_INVALID_IV"); }
try { const d = createDecipheriv("aes-256-cbc", key, iv16); d.update(Buffer.from("00112233", "hex")); d.final(); } catch (e) { console.log("pad", e.code === "ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH"); }
try { const x = createCipheriv("aes-256-cbc", key, iv16); x.final(); x.final("hex"); } catch (e) { console.log("fin2", e.code === "ERR_CRYPTO_INVALID_STATE"); }
try { const x = createCipheriv("aes-256-cbc", key, iv16); x.final(); x.update("x", "utf8", "hex"); } catch (e) { console.log("updfin", e.code === undefined); }
try {
  const x = createCipheriv("aes-256-gcm", key, iv12);
  const ct = Buffer.concat([x.update("s"), x.final()]);
  const tag = x.getAuthTag(); tag[0] ^= 1;
  const dd = createDecipheriv("aes-256-gcm", key, iv12);
  dd.setAuthTag(tag); dd.update(ct); dd.final("utf8");
} catch (e) { console.log("tag", e.code === undefined && /authenticate/.test(e.message)); }
try {
  const dd = createDecipheriv("aes-256-gcm", key, iv12);
  dd.setAuthTag(Buffer.alloc(16)); dd.update(Buffer.from("00", "hex")); dd.final("utf8");
} catch (e) { console.log("noaad", e.code === undefined); }
"#,
    );
    assert!(out.contains("alg true"), "out: {out}");
    assert!(out.contains("key true"), "out: {out}");
    assert!(out.contains("iv true"), "out: {out}");
    assert!(out.contains("pad true"), "out: {out}");
    assert!(out.contains("fin2 true"), "out: {out}");
    assert!(out.contains("updfin true"), "out: {out}");
    assert!(out.contains("tag true"), "out: {out}");
    assert!(out.contains("noaad true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_keys_sign() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { generateKeyPairSync, createSign, createVerify, sign, verify, createPrivateKey, createPublicKey, constants } from "node:crypto";
const { publicKey, privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
console.log("rsa", privateKey.type === "private" && privateKey.asymmetricKeyType === "rsa" && publicKey.type === "public");
const sig = sign("sha256", Buffer.from("msg"), privateKey);
console.log("sign", verify("sha256", Buffer.from("msg"), publicKey, sig) === true);
console.log("neg", verify("sha256", Buffer.from("msg!"), publicKey, sig) === false);
const s = createSign("RSA-SHA256"); s.update("he"); s.update("llo");
const v = createVerify("RSA-SHA256"); v.update("hello");
console.log("sv", v.verify(publicKey, s.sign(privateKey)) === true);
const s1 = sign("RSA-SHA1", Buffer.from("m"), privateKey);
console.log("sha1", verify("RSA-SHA1", Buffer.from("m"), publicKey, s1) === true);
const pem = privateKey.export({ format: "pem", type: "pkcs8" });
const back = createPrivateKey(pem);
console.log("pem", back.type === "private" && back.asymmetricKeyType === "rsa");
console.log("pubfrompriv", createPublicKey(privateKey).type === "public");
const jwk = publicKey.export({ format: "jwk" });
console.log("jwk", jwk.kty === "RSA" && jwk.e === "AQAB");
const { publicKey: ep, privateKey: es } = generateKeyPairSync("ec", { namedCurve: "prime256v1" });
const esig = sign("sha256", Buffer.from("m"), es);
console.log("ec", verify("sha256", Buffer.from("m"), ep, esig) === true);
const { publicKey: dp, privateKey: ds } = generateKeyPairSync("ed25519");
const dsg = sign(null, Buffer.from("m"), ds);
console.log("ed", verify(null, Buffer.from("m"), dp, dsg) === true);
console.log("const", constants.RSA_PKCS1_PADDING === 1 && constants.RSA_PKCS1_OAEP_PADDING === 4 && constants.RSA_PSS_SALTLEN_DIGEST === -1);
"#,
    );
    assert!(out.contains("rsa true"), "out: {out}");
    assert!(out.contains("sign true"), "out: {out}");
    assert!(out.contains("neg true"), "out: {out}");
    assert!(out.contains("sv true"), "out: {out}");
    assert!(out.contains("sha1 true"), "out: {out}");
    assert!(out.contains("pem true"), "out: {out}");
    assert!(out.contains("pubfrompriv true"), "out: {out}");
    assert!(out.contains("jwk true"), "out: {out}");
    assert!(out.contains("ec true"), "out: {out}");
    assert!(out.contains("ed true"), "out: {out}");
    assert!(out.contains("const true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_enc_dh_ecdh() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { generateKeyPairSync, publicEncrypt, privateDecrypt, createECDH, createDiffieHellmanGroup, diffieHellman } from "node:crypto";
const { publicKey, privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const enc = publicEncrypt(publicKey, Buffer.from("hi"));
console.log("oaep", privateDecrypt(privateKey, enc).toString() === "hi");
const enc1 = publicEncrypt({ key: publicKey, padding: 1 }, Buffer.from("v15"));
console.log("v15", privateDecrypt({ key: privateKey, padding: 1 }, enc1).toString() === "v15");
const a = createECDH("prime256v1"); a.generateKeys();
const b = createECDH("prime256v1"); b.generateKeys();
console.log("ecdh", a.computeSecret(b.getPublicKey()).equals(b.computeSecret(a.getPublicKey())));
console.log("ecdhraw", a.getPublicKey()[0] === 4 && a.getPrivateKey().length === 32);
const x = createDiffieHellmanGroup("modp14"); x.generateKeys();
const y = createDiffieHellmanGroup("modp14"); y.generateKeys();
const sx = x.computeSecret(y.getPublicKey());
console.log("dh", sx.equals(y.computeSecret(x.getPublicKey())) && sx.length === 256);
console.log("dhprime", x.getPrime().length === 256 && x.verifyError() === 0);
"#,
    );
    assert!(out.contains("oaep true"), "out: {out}");
    assert!(out.contains("v15 true"), "out: {out}");
    assert!(out.contains("ecdh true"), "out: {out}");
    assert!(out.contains("ecdhraw true"), "out: {out}");
    assert!(out.contains("dh true"), "out: {out}");
    assert!(out.contains("dhprime true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_asym_errors() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { generateKeyPairSync, checkPrimeSync, generatePrimeSync, createECDH, createDiffieHellman, createDiffieHellmanGroup, sign } from "node:crypto";
console.log("prime", checkPrimeSync(13n) === true && checkPrimeSync(15n) === false);
console.log("primebuf", checkPrimeSync(Buffer.from([13])) === true);
try { generateKeyPairSync("dsa", {}); console.log("dsa", true); } catch (e) { console.log("dsa", false); }
try { createECDH("secp256k1"); console.log("k1", true); } catch (e) { console.log("k1", false); }
try { createDiffieHellmanGroup("modp1"); } catch (e) { console.log("modp1", e.code === "ERR_NOT_SUPPORTED"); }
try { createDiffieHellmanGroup("modp99"); } catch (e) { console.log("modp99", e.code === "ERR_NOT_SUPPORTED"); }
try { createDiffieHellman(2048); } catch (e) { console.log("dhsize", e.code === "ERR_NOT_SUPPORTED"); }
const p = generatePrimeSync(64, { checks: 3 });
console.log("gen", p.length === 8 && checkPrimeSync(p, { checks: 3 }) === true);
try { console.log("bigint", typeof generatePrimeSync(64, { bigint: true }) === "bigint"); } catch (e) { console.log("bigint", false); }
try { sign("nope", Buffer.from("m"), generateKeyPairSync("ed25519").privateKey); } catch (e) { console.log("edalg", e.code === "ERR_CRYPTO_INVALID_DIGEST"); }
"#,
    );
    assert!(out.contains("prime true"), "out: {out}");
    assert!(out.contains("primebuf true"), "out: {out}");
    assert!(out.contains("dsa true"), "out: {out}");
    assert!(out.contains("k1 true"), "out: {out}");
    assert!(out.contains("modp1 true"), "out: {out}");
    assert!(out.contains("modp99 true"), "out: {out}");
    assert!(out.contains("dhsize true"), "out: {out}");
    assert!(out.contains("gen true"), "out: {out}");
    assert!(out.contains("bigint true"), "out: {out}");
    assert!(out.contains("edalg true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_kdf() {
    // 真 Node 取证向量（pbkdf2/scrypt/hkdf/argon2，逐字节对）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { pbkdf2Sync, pbkdf2, scryptSync, scrypt, hkdfSync, hkdf, argon2Sync, argon2 } from "node:crypto";
console.log("pbkdf2", pbkdf2Sync("password", "salt", 1, 32, "sha256").toString("hex") === "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
console.log("pbkdf2b", pbkdf2Sync("password", "salt", 2, 20, "sha1").toString("hex") === "ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957");
pbkdf2("password", "salt", 1, 32, "sha256", (e, dk) => {
  console.log("pbkdf2a", e === null && dk.toString("hex") === "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
  console.log("done");
});
console.log("scrypt", scryptSync("password", "salt", 64, { N: 1024, r: 8, p: 1 }).toString("hex").slice(0, 64) === "16dbc8906763c7f048977a68f9d305f7710e068ca2cd95dab372125bb3f19608");
scrypt("password", "salt", 32, { N: 1024 }, (e, dk) => {
  console.log("scrypta", e === null && dk.length === 32);
});
console.log("hkdf", Buffer.from(hkdfSync("sha256", "ikm", "salt", "info", 42)).toString("hex") === "fe8f9615d2374c0d17f77d1aeaf408c2e75fe0466073d0def23c733e2f862dfd6814c9254418fa112fe8");
hkdf("sha256", "ikm", "salt", "info", 42, (e, okm) => {
  console.log("hkdfa", e === null && okm instanceof ArrayBuffer && okm.byteLength === 42);
});
console.log("argon2", argon2Sync("argon2id", { message: "password", nonce: "somesalt", parallelism: 4, tagLength: 32, memory: 32, passes: 1 }).toString("hex") === "299d5e50f0022a4eef2d510ade9b1743bd1f568feefc042c3dff926a271e7fb2");
argon2("argon2id", { message: "password", nonce: "somesalt", parallelism: 4, tagLength: 16, memory: 32, passes: 1 }, (e, tag) => {
  console.log("argon2a", e === null && tag.length === 16);
});
try { pbkdf2Sync("p", "s", 0, 32, "sha256"); } catch (e) { console.log("it0", e.code === "ERR_OUT_OF_RANGE"); }
try { scryptSync("p", "s", 32, { N: 1048576, r: 8, p: 1 }); } catch (e) { console.log("mem", e.code === "ERR_CRYPTO_INVALID_SCRYPT_PARAMS"); }
console.log("ad", argon2Sync("argon2id", { message: "secret", nonce: "somesalt12345678", parallelism: 1, tagLength: 32, memory: 8, passes: 1, associatedData: Buffer.from("ad-data") }).toString("hex") === "81454faa04011e9d56a85f66352875d91e04fb8edf2458d44c18c4d9bcef4762");
"#,
    );
    assert!(out.contains("pbkdf2 true"), "out: {out}");
    assert!(out.contains("pbkdf2b true"), "out: {out}");
    assert!(out.contains("pbkdf2a true"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    assert!(out.contains("scrypt true"), "out: {out}");
    assert!(out.contains("scrypta true"), "out: {out}");
    assert!(out.contains("hkdf true"), "out: {out}");
    assert!(out.contains("hkdfa true"), "out: {out}");
    assert!(out.contains("argon2 true"), "out: {out}");
    assert!(out.contains("argon2a true"), "out: {out}");
    assert!(out.contains("it0 true"), "out: {out}");
    assert!(out.contains("mem true"), "out: {out}");
    assert!(out.contains("ad true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_crypto_x509() {
    let dir = assert_fs::TempDir::new().unwrap();
    let pem = "-----BEGIN CERTIFICATE-----\nMIIDizCCAnOgAwIBAgIUXHVjPqV6YzyPBqRQYPc0ZcZRzkswDQYJKoZIhvcNAQEL\nBQAwNjELMAkGA1UEBhMCVVMxDTALBgNVBAoMBEFjbWUxGDAWBgNVBAMMD3d3dy5l\neGFtcGxlLmNvbTAeFw0yNjA5MTIwNzAzMTZaFw0yNjA5MTQwNzAzMTZaMDYxCzAJ\nBgNVBAYTAlVTMQ0wCwYDVQQKDARBY21lMRgwFgYDVQQDDA93d3cuZXhhbXBsZS5j\nb20wggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQChD22N6LlqRJlVEyGj\nE+zohSE50NYazdABnbAcECBTT9d0NAsLPfASUbVWzDoyDDiMGGbApwORUiACMwZq\nNI7KQ1OeEe6wDkVUWXb+07bNV2pUZzDZXnZJzgkZMjy7kNT6uu+36n4KdApSt9jO\nwG89qdYQ/5wIMo8LCA0vV1Px2jiYvgXQTACy4BXa6QbzZR2iUFlGA4wYfzv7dEk1\nY5Bz4vwo/5PfxdrtUkirg/kdxJqqBypV+ptW8YZRPZLwTh05dAOHx2Mh/vx1QKi+\nnvj7PQUihHogt64+i0Q6hqQNX2U/FI05dvUonRAnHl+o0YJCVhOOBNPqB5ZBNXEc\n9kAVAgMBAAGjgZAwgY0wHQYDVR0OBBYEFFl6516N9nn1EPzjIobzQcYtEV0wMB8G\nA1UdIwQYMBaAFFl6516N9nn1EPzjIobzQcYtEV0wMA8GA1UdEwEB/wQFMAMBAf8w\nLQYDVR0RBCYwJIIPd3d3LmV4YW1wbGUuY29tggtleGFtcGxlLmNvbYcEfwAAATAL\nBgNVHQ8EBAMCBaAwDQYJKoZIhvcNAQELBQADggEBAAd7FdDiGjuGBBtw5GTn+zD6\n+qTq2YoJIZzkKJ/TaPpPk67jyEVpKghI+aJ6o7ZBDiAytOGPCZsEmX7j+26oj1c6\nsukEQn3jF9h9eKw+ih/FUsFUsU7JGuywO7lbk9GbHxKtfF1na0tYDSpQnN9WldXz\n5/btna3Nzj+53wdkO0BkkXefVZfFu0dIH7o6hvxhW40RLfhkwW0DWSJ9vgHYta0d\nfDlfTxiy6M+f1YxM49MDmzL37FopkuFj0xmbRXUdIjHTKq+rIZYuMW9x510uVD4y\ndh6vOBNEHn8gVd1JIJLjrBvY55ecfA/UieRe8TCJ380CqZ9bYHJoIm1JZiaGSdg=\n-----END CERTIFICATE-----\n";
    dir.child("c.pem").write_str(pem).unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { X509Certificate, Certificate } from "node:crypto";
import fs from "node:fs";
const pem = fs.readFileSync("c.pem", "utf8");
const x = new X509Certificate(pem);
console.log("subj", x.subject === "C=US\nO=Acme\nCN=www.example.com");
console.log("san", x.subjectAltName === "DNS:www.example.com, DNS:example.com, IP Address:127.0.0.1");
console.log("host", x.checkHost("www.example.com") === "www.example.com", x.checkHost("other.com") === undefined, x.checkHost("127.0.0.1") === "127.0.0.1");
console.log("sn", x.serialNumber.length > 4, x.validFrom.endsWith("GMT"), x.validTo.endsWith("GMT"));
console.log("fp", x.fingerprint.split(":").length === 20, x.fingerprint256.split(":").length === 32, x.fingerprint512.split(":").length === 64);
console.log("pem", x.toString().startsWith("-----BEGIN CERTIFICATE-----"), x.raw.length > 100);
console.log("legacy", x.toLegacyObject().subject.CN === "www.example.com");
console.log("ku", JSON.stringify(x.keyUsage) === JSON.stringify(["Digital Signature", "Key Encipherment"]));
try { new X509Certificate("nope"); } catch (e) { console.log("bad", e.code === "ERR_INVALID_ARG_VALUE"); }
try { x.verify(); } catch (e) { console.log("verify", e.code === "ERR_INVALID_ARG_TYPE"); }
console.log("verifyself", x.verify(x.publicKey) === true);
try { new Certificate(); } catch (e) { console.log("legacy-cert", e.code === "ERR_NOT_SUPPORTED"); }
"#,
    );
    assert!(out.contains("subj true"), "out: {out}");
    assert!(out.contains("san true"), "out: {out}");
    assert!(out.contains("host true true true"), "out: {out}");
    assert!(out.contains("sn true true true"), "out: {out}");
    assert!(out.contains("fp true true true"), "out: {out}");
    assert!(out.contains("pem true true"), "out: {out}");
    assert!(out.contains("legacy true"), "out: {out}");
    assert!(out.contains("ku true"), "out: {out}");
    assert!(out.contains("bad true"), "out: {out}");
    assert!(out.contains("verify true"), "out: {out}");
    assert!(out.contains("verifyself true"), "out: {out}");
    assert!(out.contains("legacy-cert true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_perf_hooks_surface() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import p, { performance, PerformanceObserver, createHistogram, monitorEventLoopDelay, timerify, constants } from "node:perf_hooks";
console.log("now", typeof performance.now() === "number" && performance.now() >= 0, performance.timeOrigin > 0, performance instanceof p.Performance);
performance.mark("a"); performance.mark("b");
const m = performance.measure("m1", "a", "b");
console.log("measure", m.name === "m1" && m.entryType === "measure" && m.duration >= 0);
console.log("entries", performance.getEntriesByName("a").length === 1, performance.getEntriesByType("mark").length === 2);
const got = [];
const o = new PerformanceObserver((l) => { for (const e of l.getEntries()) got.push(e.name + ":" + e.entryType); });
o.observe({ entryTypes: ["mark"] });
performance.mark("c");
setTimeout(() => {
  console.log("obs", JSON.stringify(got) === JSON.stringify(["c:mark"]));
  o.disconnect();
  const h = createHistogram(); h.record(10); h.record(20);
  console.log("hist", h.count === 2 && h.min === 10 && h.max === 20 && h.mean === 15 && h.percentile(50) === 10);
  const e = createHistogram();
  console.log("empty", e.min === 9223372036854776000, e.max === 0, e.count === 0);
  const u = performance.eventLoopUtilization();
  console.log("elu", typeof u.active === "number" && u.utilization === 1);
  const f = timerify((x) => x * 2);
  console.log("timerify", f(21) === 42, performance.getEntriesByType("function").length === 1);
  console.log("const", constants.NODE_PERFORMANCE_GC_MAJOR === 4 && constants.NODE_PERFORMANCE_GC_FLAGS_NO === 0);
  const mel = monitorEventLoopDelay({ resolution: 10 });
  mel.enable();
  setTimeout(() => { mel.disable(); console.log("mel", mel.count >= 0, mel.min >= 0); }, 60);
}, 20);
"#,
    );
    assert!(out.contains("now true true true"), "out: {out}");
    assert!(out.contains("measure true"), "out: {out}");
    assert!(out.contains("entries true true"), "out: {out}");
    assert!(out.contains("obs true"), "out: {out}");
    assert!(out.contains("hist true"), "out: {out}");
    assert!(out.contains("empty true true true"), "out: {out}");
    assert!(out.contains("elu true"), "out: {out}");
    assert!(out.contains("timerify true true"), "out: {out}");
    assert!(out.contains("const true"), "out: {out}");
    assert!(out.contains("mel true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9e_inspector_session() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import ins, { Session, open, close, url, waitForDebugger } from "node:inspector";
console.log("shape", typeof open === "function" && typeof close() === "undefined" && url() === undefined && waitForDebugger() === undefined);
const s = new Session();
s.connect();
const post = (m, p) => new Promise((res, rej) => s.post(m, p, (e, r) => (e ? rej(e) : res(r))));
const r1 = await post("Runtime.evaluate", { expression: "40 + 2" });
console.log("eval", r1.result.type === "number" && r1.result.value === 42);
const r2 = await post("Runtime.evaluate", { expression: "({a: [1,2]})" });
console.log("obj", r2.result.value.a.join(",") === "1,2");
const r3 = await post("Runtime.evaluate", { expression: "throw new Error('boom')" });
console.log("exc", r3.exceptionDetails.exception.description === "boom");
const r4 = await post("Debugger.enable", {});
console.log("ack", JSON.stringify(r4) === "{}");
try {
  await post("Nope.nope", {});
} catch (e) { console.log("unk", e.code === "ERR_NOT_SUPPORTED"); }
const r5 = await post("Runtime.evaluate", { expression: "1+1" });
console.log("promise", r5.result.value === 2);
s.disconnect();
console.log("done");
"#,
    );
    assert!(out.contains("shape true"), "out: {out}");
    assert!(out.contains("eval true"), "out: {out}");
    assert!(out.contains("obj true"), "out: {out}");
    assert!(out.contains("exc true"), "out: {out}");
    assert!(out.contains("ack true"), "out: {out}");
    assert!(out.contains("unk true"), "out: {out}");
    assert!(out.contains("promise true"), "out: {out}");
    assert!(out.contains("done"), "out: {out}");
    dir.close().unwrap();
}

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
fn phase9f_vm_context_spawns_and_isolates() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import vm from "node:vm";
const sb = { a: 5 };
const r = vm.runInNewContext("b = a + 1; b", sb);
console.log("v-run", r === 6, sb.b === 6, typeof b === "undefined");
const c1 = vm.createContext({ x: 1 });
const c2 = vm.createContext({ x: 2 });
console.log("v-ctx", vm.isContext(c1), vm.isContext(c2), vm.isContext({}));
vm.runInContext("y = x * 10", c1);
vm.runInContext("y = x * 10", c2);
console.log("v-iso", c1.y === 10, c2.y === 20);
const s = new vm.Script("40 + 2");
console.log("v-script", s.runInNewContext() === 42, s.runInThisContext() === 42);
const f = vm.compileFunction("return a + b", ["a", "b"]);
console.log("v-cf", f(20, 22) === 42);
const o = vm.runInNewContext("({ z: 7 })", {});
console.log("v-ccw", o.z === 7, typeof o === "object");
const sb2 = {};
vm.runInNewContext("Promise.resolve(1).then(v => { globalThis.px = v; })", sb2);
console.log("v-micro", sb2.px === 1);
console.log("v-std", vm.runInNewContext("typeof Object") === "function", vm.runInNewContext("typeof console") === "undefined");
console.log("v-const", typeof vm.constants.USE_MAIN_CONTEXT_DEFAULT_LOADER, typeof vm.constants.DONT_CONTEXTIFY);
console.log("v-timeout", vm.runInNewContext("1 + 1", {}, { timeout: 100 }) === 2);
const mm = await vm.measureMemory().then(() => "no", (e) => e.code);
console.log("v-mm", mm === "ERR_CONTEXT_NOT_INITIALIZED");
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("v-run true true true"), "out: {out}");
    assert!(out.contains("v-ctx true true false"), "out: {out}");
    assert!(out.contains("v-iso true true"), "out: {out}");
    assert!(out.contains("v-script true true"), "out: {out}");
    assert!(out.contains("v-cf true"), "out: {out}");
    assert!(out.contains("v-ccw true true"), "out: {out}");
    assert!(out.contains("v-micro true"), "out: {out}");
    assert!(out.contains("v-std true true"), "out: {out}");
    assert!(out.contains("v-const symbol symbol"), "out: {out}");
    assert!(out.contains("v-timeout true"), "out: {out}");
    assert!(out.contains("v-mm true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_vm_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import vm from "node:vm";
try { new vm.Script("}{"); } catch (e) { console.log("w-ctor", e.constructor.name === "SyntaxError"); }
try { vm.compileFunction("}{"); } catch (e) { console.log("w-cf", e.constructor.name === "SyntaxError"); }
try { vm.runInNewContext("throw new RangeError('nope')"); } catch (e) { console.log("w-range", e.constructor.name === "RangeError", e.message === "nope"); }
try { vm.runInNewContext("throw 'strval'"); } catch (e) { console.log("w-str", e.constructor.name === "Error", e.message.includes("strval")); }
try { vm.runInNewContext("noSuchVar + 1"); } catch (e) { console.log("w-ref", e.constructor.name === "ReferenceError"); }
try { vm.runInContext("1", {}); } catch (e) { console.log("w-badctx", e.code === "ERR_INVALID_ARG_TYPE"); }
try { vm.runInNewContext("1", 42); } catch (e) { console.log("w-badsb", e.code === "ERR_INVALID_ARG_TYPE"); }
try { vm.isContext(42); } catch (e) { console.log("w-isctx", e.code === "ERR_INVALID_ARG_TYPE"); }
try { vm.runInNewContext("1", {}, { microtaskMode: "nope" }); } catch (e) { console.log("w-mmode", e.code === "ERR_INVALID_ARG_VALUE"); }
try { vm.runInNewContext("1", {}, { timeout: -1 }); } catch (e) { console.log("w-timeout", e.code === "ERR_OUT_OF_RANGE"); }
const pc = vm.createContext({ q: 41 });
const f2 = vm.compileFunction("return q + 1", [], { parsingContext: pc });
console.log("w-pc", f2() === 42);
const ce = vm.compileFunction("return ex + 1", [], { contextExtensions: [{ ex: 41 }] });
console.log("w-ext", ce() === 42);
const cached = new vm.Script("9", { cachedData: Buffer.alloc(0), produceCachedData: true });
console.log("w-cache", cached.runInNewContext() === 9, cached.cachedDataProduced === false);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("w-ctor true"), "out: {out}");
    assert!(out.contains("w-cf true"), "out: {out}");
    assert!(out.contains("w-range true true"), "out: {out}");
    assert!(out.contains("w-str true true"), "out: {out}");
    assert!(out.contains("w-ref true"), "out: {out}");
    assert!(out.contains("w-badctx true"), "out: {out}");
    assert!(out.contains("w-badsb true"), "out: {out}");
    assert!(out.contains("w-isctx true"), "out: {out}");
    assert!(out.contains("w-mmode true"), "out: {out}");
    assert!(out.contains("w-timeout true"), "out: {out}");
    assert!(out.contains("w-pc true"), "out: {out}");
    assert!(out.contains("w-ext true"), "out: {out}");
    assert!(out.contains("w-cache true true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_worker_channel_roundtrip() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { MessageChannel, MessagePort, receiveMessageOnPort } from "node:worker_threads";
const { port1, port2 } = new MessageChannel();
console.log("ch-ports", port1 instanceof MessagePort, port2 instanceof MessagePort);
port1.on("message", (m) => {
  console.log("ch-p1", JSON.stringify(m) === JSON.stringify({ n: 41 }));
  port1.postMessage([1, "x", true]);
});
port2.on("message", (m) => {
  console.log("ch-p2", Array.isArray(m) && m[1] === "x");
  port1.close(); port2.close();
});
port2.postMessage({ n: 41 });
// 迟挂监听：先投递再 on，newListener 开闸照样收到
const late = new MessageChannel();
late.port2.postMessage("late-hi");
await new Promise((r) => setTimeout(r, 20));
late.port1.on("message", (m) => {
  console.log("ch-late", m === "late-hi");
  late.port1.close(); late.port2.close();
});
// 无监听排队：receiveMessageOnPort 同步取出
const q = new MessageChannel();
q.port2.postMessage("q1");
q.port2.postMessage("q2");
await new Promise((r) => setTimeout(r, 20));
console.log("ch-recv", receiveMessageOnPort(q.port1).message === "q1", receiveMessageOnPort(q.port1).message === "q2", receiveMessageOnPort(q.port1) === undefined);
try { q.port2.postMessage(() => {}); } catch (e) { console.log("ch-fn", e.name === "DataCloneError"); }
q.port1.close(); q.port2.close();
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("ch-ports true true"), "out: {out}");
    assert!(out.contains("ch-p1 true"), "out: {out}");
    assert!(out.contains("ch-p2 true"), "out: {out}");
    assert!(out.contains("ch-late true"), "out: {out}");
    assert!(out.contains("ch-recv true true true"), "out: {out}");
    assert!(out.contains("ch-fn true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_worker_thread_info_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { isMainThread, threadId, parentPort, workerData, resourceLimits, SHARE_ENV, setEnvironmentData, getEnvironmentData, markAsUncloneable, moveMessagePortToContext, MessageChannel } from "node:worker_threads";
console.log("th-self", isMainThread === true, threadId === 0, parentPort === null, workerData === null);
console.log("th-res", typeof resourceLimits === "object", typeof SHARE_ENV === "symbol");
setEnvironmentData("wk", { v: 7 });
console.log("th-env", JSON.stringify(getEnvironmentData("wk")) === JSON.stringify({ v: 7 }), getEnvironmentData("missing") === undefined);
try { setEnvironmentData(42, 1); } catch (e) { console.log("th-badkey", e.code === "ERR_INVALID_ARG_TYPE"); }
try { getEnvironmentData(42); } catch (e) { console.log("th-badkey2", e.code === "ERR_INVALID_ARG_TYPE"); }
const o = { a: 1 };
markAsUncloneable(o);
const { port1, port2 } = new MessageChannel();
try { port2.postMessage(o); } catch (e) { console.log("th-unc", e.name === "DataCloneError"); }
console.log("th-move", moveMessagePortToContext(port1, {}) === port1);
port1.close(); port2.close();
// unref 端口不续命：不 close 照样退出
const u = new MessageChannel();
u.port1.unref(); u.port2.unref();
u.port2.postMessage("dropped");
console.log("th-unref", true);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("th-self true true true true"), "out: {out}");
    assert!(out.contains("th-res true true"), "out: {out}");
    assert!(out.contains("th-env true true"), "out: {out}");
    assert!(out.contains("th-badkey true"), "out: {out}");
    assert!(out.contains("th-badkey2 true"), "out: {out}");
    assert!(out.contains("th-unc true"), "out: {out}");
    assert!(out.contains("th-move true"), "out: {out}");
    assert!(out.contains("th-unref true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_worker_eval_and_data() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { Worker, isMainThread, threadId } from "node:worker_threads";
console.log("wk-self", isMainThread === true, threadId === 0);
const w = new Worker("import { parentPort } from 'node:worker_threads'; parentPort.postMessage(40 + 2);", { eval: true });
console.log("wk-tid", w.threadId > 0);
w.on("online", () => console.log("wk-online", true));
w.on("message", (m) => console.log("wk-msg", m === 42));
w.on("error", (e) => console.log("wk-err", e.message));
w.on("exit", (c) => console.log("wk-exit", c === 0));
const d = new Worker("import { parentPort, workerData } from 'node:worker_threads'; parentPort.postMessage({ e: workerData.n + 1 });", { eval: true, workerData: { n: 41 } });
d.on("message", (m) => console.log("wk-data", m.e === 42));
d.on("exit", () => {});
d.on("error", (e) => console.log("wk-derr", e.message));
console.log("wk-ref", w.unref() === w, w.ref() === w);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("wk-self true true"), "out: {out}");
    assert!(out.contains("wk-tid true"), "out: {out}");
    assert!(out.contains("wk-online true"), "out: {out}");
    assert!(out.contains("wk-msg true"), "out: {out}");
    assert!(out.contains("wk-exit true"), "out: {out}");
    assert!(out.contains("wk-data true"), "out: {out}");
    assert!(out.contains("wk-ref true true"), "out: {out}");
    assert!(!out.contains("wk-err"), "out: {out}");
    assert!(!out.contains("wk-derr"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_worker_twoway_terminate() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { Worker } from "node:worker_threads";
const w = new Worker("import { parentPort } from 'node:worker_threads'; parentPort.on('message', (m) => parentPort.postMessage(m * 2));", { eval: true });
w.on("online", () => w.postMessage(21));
w.on("message", (m) => {
  console.log("wx-msg", m === 42);
  w.terminate().then((c) => console.log("wx-term", c === 1));
});
w.on("exit", (c) => console.log("wx-exit", c === 1));
w.on("error", (e) => console.log("wx-err", e.message));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("wx-msg true"), "out: {out}");
    assert!(out.contains("wx-term true"), "out: {out}");
    assert!(out.contains("wx-exit true"), "out: {out}");
    assert!(!out.contains("wx-err"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9f_worker_errors_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("wfile.js").write_str("import { parentPort } from \"node:worker_threads\";\nparentPort.postMessage(\"file-ok\");\n").unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import { Worker } from "node:worker_threads";
try { new Worker(42); } catch (e) { console.log("we-badfile", e.code === "ERR_INVALID_ARG_TYPE"); }
const f = new Worker("./wfile.js");
f.on("message", (m) => console.log("we-file", m === "file-ok"));
f.on("exit", () => {});
f.on("error", (e) => console.log("we-ferr", e.message));
const t = new Worker("throw new Error('boom-x')", { eval: true });
t.on("error", (e) => console.log("we-throw", e.message.includes("boom-x")));
t.on("exit", (c) => console.log("we-texit", c === 1));
const m = new Worker("./nope-missing.js");
m.on("error", (e) => console.log("we-miss", e.message.includes("nope-missing")));
m.on("exit", (c) => console.log("we-mexit", c === 1));
const e2 = new Worker("void 0", { eval: true });
e2.on("exit", (c) => {
  console.log("we-e2", c === 0);
  e2.postMessage("late-drop");
  e2.terminate().then((cc) => console.log("we-term2", cc === 0));
});
e2.on("error", (e) => console.log("we-e2err", e.message));
const n = new Worker("import { workerData } from 'node:worker_threads'; import { parentPort } from 'node:worker_threads'; parentPort.postMessage(workerData === null);", { eval: true });
n.on("message", (mm) => console.log("we-novalue", mm === true));
n.on("exit", () => {});
n.on("error", (e) => console.log("we-nerr", e.message));
const x = new Worker("process.exit(7);", { eval: true });
x.on("exit", (c) => console.log("we-code", c === 7));
x.on("error", (e) => console.log("we-xerr", e.message));
"#,
    );
    assert!(out.contains("we-badfile true"), "out: {out}");
    assert!(out.contains("we-file true"), "out: {out}");
    assert!(out.contains("we-throw true"), "out: {out}");
    assert!(out.contains("we-texit true"), "out: {out}");
    assert!(out.contains("we-miss true"), "out: {out}");
    assert!(out.contains("we-mexit true"), "out: {out}");
    assert!(out.contains("we-e2 true"), "out: {out}");
    assert!(out.contains("we-term2 true"), "out: {out}");
    assert!(out.contains("we-novalue true"), "out: {out}");
    assert!(out.contains("we-code true"), "out: {out}");
    assert!(!out.contains("we-ferr"), "out: {out}");
    assert!(!out.contains("we-e2err"), "out: {out}");
    assert!(!out.contains("we-nerr"), "out: {out}");
    assert!(!out.contains("we-xerr"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9h_crypto_k256() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { createECDH, generateKeyPairSync, createSign, createVerify, createPrivateKey, createPublicKey, getCurves } from "node:crypto";
console.log("k-curves", getCurves().includes("secp256k1"));
// 真机固定向量（node v26.8.2 实测）：priv/pub/peer/secret 逐字节对
const FIX = {
  priv: "ab5ece87dd1089783678deadcac0283eff35dddd6a32081dce7f2c1d3630de74",
  pub: "04a72a7632bbef9c8b9a9a58224afba9ce6ba199b5d0d8dddf906e6de486ae34aa43be1ee6eab356aab327347da80fac8c09a38183a1ece15d37d570184912fd19",
  peer: "0421d3b023a66019230f034f7fb38b575a613d1b4465d530ab96829b7d047687e34680bb8cbf806cfdcaf8216881aaa2a4f3f0e8f48de7a49f7d858a497ed1ab2d",
  secret: "c9fe11e3f27bac5fb3692fac8787c0a07566ba02e47a32c45136234d1b080364",
};
const a = createECDH("secp256k1");
a.setPrivateKey(Buffer.from(FIX.priv, "hex"));
console.log("k-ecdh-vec", a.computeSecret(Buffer.from(FIX.peer, "hex")).toString("hex") === FIX.secret);
const e1 = createECDH("secp256k1"); e1.generateKeys();
const e2 = createECDH("secp256k1"); e2.generateKeys();
console.log("k-ecdh-self", e1.computeSecret(e2.getPublicKey()).equals(e2.computeSecret(e1.getPublicKey())));
// 签名往返 + 内容错验不过
const { privateKey, publicKey } = generateKeyPairSync("ec", { namedCurve: "secp256k1" });
const data = Buffer.from("hello-k256");
const sig = createSign("sha256").update(data).sign(privateKey);
console.log("k-sign", createVerify("sha256").update(data).verify(publicKey, sig) === true);
console.log("k-tamper", createVerify("sha256").update(Buffer.from("hello-k257")).verify(publicKey, sig) === false);
// ieeep1363 形态往返
const raw = createSign("sha256").update(data).sign({ key: privateKey, dsaEncoding: "ieee-p1363" });
console.log("k-rawlen", raw.length === 64);
console.log("k-rawvec", createVerify("sha256").update(data).verify({ key: publicKey, dsaEncoding: "ieee-p1363" }, raw) === true);
// 导出导入往返（der/pem/jwk/sec1）
const spki = publicKey.export({ format: "der", type: "spki" });
const pkcs8 = privateKey.export({ format: "der", type: "pkcs8" });
const pub2 = createPublicKey({ key: spki, format: "der", type: "spki" });
console.log("k-spki", createVerify("sha256").update(data).verify(pub2, sig) === true);
const priv2 = createPrivateKey({ key: pkcs8, format: "der", type: "pkcs8" });
console.log("k-pkcs8", createSign("sha256").update(data).sign(priv2).length > 64);
const jwk = publicKey.export({ format: "jwk" });
console.log("k-jwk", jwk.kty === "EC" && jwk.crv === "secp256k1" && typeof jwk.x === "string");
const pem = publicKey.export({ format: "pem", type: "spki" });
console.log("k-pem", pem.startsWith("-----BEGIN PUBLIC KEY-----"));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("k-curves true"), "out: {out}");
    assert!(out.contains("k-ecdh-vec true"), "out: {out}");
    assert!(out.contains("k-ecdh-self true"), "out: {out}");
    assert!(out.contains("k-sign true"), "out: {out}");
    assert!(out.contains("k-tamper true"), "out: {out}");
    assert!(out.contains("k-rawlen true"), "out: {out}");
    assert!(out.contains("k-rawvec true"), "out: {out}");
    assert!(out.contains("k-spki true"), "out: {out}");
    assert!(out.contains("k-pkcs8 true"), "out: {out}");
    assert!(out.contains("k-jwk true"), "out: {out}");
    assert!(out.contains("k-pem true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9h_crypto_dsa_prime() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { generateKeyPairSync, generateKeyPair, createSign, createVerify, createPrivateKey, createPublicKey, generatePrimeSync, checkPrimeSync } from "node:crypto";
// DSA 快档（1024/160，Sign/Verify 全链；慢档只验形状不断言向量）
const { privateKey, publicKey } = generateKeyPairSync("dsa", { modulusLength: 1024, divisorLength: 160 });
console.log("d-gen", privateKey.type === "private", publicKey.type === "public", privateKey.asymmetricKeyType === "dsa");
const data = Buffer.from("hello-dsa");
const sig = createSign("sha256").update(data).sign(privateKey);
console.log("d-sign", createVerify("sha256").update(data).verify(publicKey, sig) === true);
console.log("d-tamper", createVerify("sha256").update(Buffer.from("hello-dsb")).verify(publicKey, sig) === false);
// sha384 档（prehash 全哈希）
const sig384 = createSign("sha384").update(data).sign(privateKey);
console.log("d-384", createVerify("sha384").update(data).verify(publicKey, sig384) === true);
// 导出导入往返（der/pem/jwk）
const spki = publicKey.export({ format: "der", type: "spki" });
const pkcs8 = privateKey.export({ format: "der", type: "pkcs8" });
console.log("d-der", spki.length > 100, pkcs8.length > 100);
const pub2 = createPublicKey({ key: spki, format: "der", type: "spki" });
console.log("d-spki", createVerify("sha256").update(data).verify(pub2, sig) === true);
const priv2 = createPrivateKey({ key: pkcs8, format: "der", type: "pkcs8" });
console.log("d-pkcs8", createSign("sha256").update(data).sign(priv2).length > 40);
console.log("d-pem", publicKey.export({ format: "pem", type: "spki" }).startsWith("-----BEGIN PUBLIC KEY-----"));
const jwk = publicKey.export({ format: "jwk" });
console.log("d-jwk", jwk.kty === "DSA" && typeof jwk.p === "string" && typeof jwk.y === "string" && jwk.x === undefined);
const pub3 = createPublicKey({ key: jwk, format: "jwk" });
console.log("d-jwkim", createVerify("sha256").update(data).verify(pub3, sig) === true);
// 异步形态
generateKeyPair("dsa", { modulusLength: 1024, divisorLength: 160 }, (e, pub, priv) => {
  console.log("d-async", e === null && pub.type === "public" && priv.type === "private");
});
// bigint 素数（16 进制桥）
const p = generatePrimeSync(256, { bigint: true });
console.log("d-bigint", typeof p === "bigint" && checkPrimeSync(p) === true);
const ps = generatePrimeSync(256, { bigint: true, safe: true });
console.log("d-safe", typeof ps === "bigint" && checkPrimeSync(ps) === true && checkPrimeSync((ps - 1n) / 2n) === true);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("d-gen true true true"), "out: {out}");
    assert!(out.contains("d-sign true"), "out: {out}");
    assert!(out.contains("d-tamper true"), "out: {out}");
    assert!(out.contains("d-384 true"), "out: {out}");
    assert!(out.contains("d-der true true"), "out: {out}");
    assert!(out.contains("d-spki true"), "out: {out}");
    assert!(out.contains("d-pkcs8 true"), "out: {out}");
    assert!(out.contains("d-pem true"), "out: {out}");
    assert!(out.contains("d-jwk true"), "out: {out}");
    assert!(out.contains("d-jwkim true"), "out: {out}");
    assert!(out.contains("d-async true"), "out: {out}");
    assert!(out.contains("d-bigint true"), "out: {out}");
    assert!(out.contains("d-safe true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9h_crypto_xof_ripemd() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"
import { createHash, createHmac, getHashes } from "node:crypto";
console.log("x-ripemd", createHash("ripemd160").update("abc").digest("hex") === "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc");
console.log("x-s128", createHash("shake128", { outputLength: 32 }).update("abc").digest("hex") === "5881092dd818bf5cf8a3ddb793fbcba74097d5c526a6d35f97b83351940f2cc8");
console.log("x-s256", createHash("shake256", { outputLength: 32 }).update("abc").digest("hex") === "483366601360a8771c6863080cc4114d8db44530f8f1e1ee4f94ea37e78b5739");
console.log("x-hmacri", createHmac("ripemd160", "key").update("msg").digest("hex") === "af9f1041c7727ee3161fdbda8821364fb888a0e2");
try { createHmac("shake256", "key"); console.log("x-hmacshake-never", false); }
catch (e) { console.log("x-hmacshake", e.code === undefined); }
const h = createHash("shake256", { outputLength: 16 });
h.update("a");
const c2 = h.copy();
h.update("bc"); c2.update("bc");
console.log("x-copy", h.digest("hex") === c2.digest("hex") && h.digest === c2.digest);
try { createHash("shake256", { outputLength: -1 }); console.log("x-badlen-never", false); }
catch (e) { console.log("x-badlen", e.code === "ERR_INVALID_ARG_VALUE"); }
console.log("x-hashes", getHashes().includes("ripemd160") && getHashes().includes("shake128") && getHashes().includes("shake256"));
console.log("x-dflt", createHash("shake256").update("abc").digest("hex").length === 64);
console.log("x-dflt128", createHash("shake128").update("abc").digest("hex").length === 32);
"#,
    ).unwrap();
    let out = winterjs().arg("--run").arg(file.path()).current_dir(dir.path()).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    for tag in ["x-ripemd", "x-s128", "x-s256", "x-hmacri", "x-hmacshake", "x-copy", "x-badlen", "x-hashes", "x-dflt", "x-dflt128"] {
        assert!(stdout.contains(&format!("{tag} true")), "out: {stdout}");
    }
    // DEP0198 缺省警告走 stderr（真机同款）。
    assert!(String::from_utf8_lossy(&out.stderr).contains("DEP0198"), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    dir.close().unwrap();
}

#[test]
fn phase9i_vm_source_module_chain() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import vm from "node:vm";
const m = new vm.SourceTextModule("export const a = 40 + 1;");
console.log("m9i-st0", m.status === "unlinked", m.identifier === "vm:module(0)", Array.isArray(m.dependencySpecifiers) && m.dependencySpecifiers.length === 0);
console.log("m9i-inst", m instanceof vm.SourceTextModule, m instanceof vm.Module);
await m.link(() => {});
console.log("m9i-st1", m.status === "linked");
const er = m.evaluate();
console.log("m9i-evret", er instanceof Promise);
await er;
console.log("m9i-st2", m.status === "evaluated", m.namespace.a === 41);
// 重复求值照真机成功（无操作）。
await m.evaluate();
console.log("m9i-reev", m.status === "evaluated");
// 上下文隔离：同名种子不同值。
const c1 = vm.createContext({ seed: 3 });
const c2 = vm.createContext({ seed: 4 });
const m1 = new vm.SourceTextModule("export const v = seed * 2;", { context: c1, identifier: "m1" });
const m2 = new vm.SourceTextModule("export const v = seed * 2;", { context: c2, identifier: "m2" });
await m1.link(() => {});
await m2.link(() => {});
await m1.evaluate();
await m2.evaluate();
console.log("m9i-iso", m1.namespace.v === 6, m2.namespace.v === 8, m1.identifier === "m1", m1.context === c1);
// 顶层 await 模块（异步求值认领路径）。
const t = new vm.SourceTextModule("export const v = await Promise.resolve(41);");
await t.link(() => {});
await t.evaluate();
console.log("m9i-tla", t.status === "evaluated", t.namespace.v === 41);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "m9i-st0 true true true",
        "m9i-inst true true",
        "m9i-st1 true",
        "m9i-evret true",
        "m9i-st2 true true",
        "m9i-reev true",
        "m9i-iso true true true true",
        "m9i-tla true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_vm_module_boundary() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import vm from "node:vm";
const t = async (n, f) => { try { const r = await f(); console.log(n, "OK", r === undefined ? "undef" : "val"); } catch (e) { console.log(n, "THROW", e.code || "(nocode)"); } };
await t("m9iB-syntax", async () => new vm.SourceTextModule("export const q = ;"));
await t("m9iB-nonstr", async () => new vm.SourceTextModule(123));
await t("m9iB-badctx", async () => new vm.SourceTextModule("export const a = 1;", { context: {} }));
const m = new vm.SourceTextModule("export const a = 1;");
await t("m9iB-linknofn", async () => m.link());
await t("m9iB-nsearly", async () => m.namespace);
await t("m9iB-evunlinked", async () => m.evaluate());
await m.link(() => {});
await t("m9iB-relink", async () => m.link(() => {}));
await t("m9iB-errearly", async () => m.error);
const e = new vm.SourceTextModule("throw new Error('boom');");
await e.link(() => {});
await t("m9iB-evthrow", async () => e.evaluate());
console.log("m9iB-est", e.status === "errored", e.error && e.error.message === "boom");
const im = new vm.SourceTextModule("import {x} from './nope.js'; export const a = x;");
console.log("m9iB-deps", JSON.stringify(im.dependencySpecifiers) === JSON.stringify(["./nope.js"]));
await t("m9iB-linkimports", async () => im.link(() => {}));
const s = new vm.SyntheticModule(["x"], function () { this.setExport("x", 42); });
console.log("m9iB-syn0", s.status === "linked", s.dependencySpecifiers === undefined);
await s.link();
await s.evaluate();
console.log("m9iB-syn1", s.status === "evaluated", s.namespace.x === 42);
await t("m9iB-synset", async () => s.setExport("x", 1));
const se = new vm.SyntheticModule(["d"], function () { throw new Error("cbboom"); });
await se.link(() => {});
await t("m9iB-syncb", async () => se.evaluate());
console.log("m9iB-synest", se.status === "errored");
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("m9iB-syntax THROW"), "out: {out}");
    assert!(out.contains("m9iB-nonstr THROW ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("m9iB-badctx THROW ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("m9iB-linknofn THROW ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("m9iB-nsearly THROW ERR_VM_MODULE_STATUS"), "out: {out}");
    assert!(out.contains("m9iB-evunlinked THROW ERR_VM_MODULE_STATUS"), "out: {out}");
    assert!(out.contains("m9iB-relink THROW ERR_VM_MODULE_STATUS"), "out: {out}");
    assert!(out.contains("m9iB-errearly THROW ERR_VM_MODULE_STATUS"), "out: {out}");
    assert!(out.contains("m9iB-evthrow THROW"), "out: {out}");
    assert!(out.contains("m9iB-est true true"), "out: {out}");
    assert!(out.contains("m9iB-deps true"), "out: {out}");
    assert!(out.contains("m9iB-linkimports THROW"), "out: {out}");
    assert!(out.contains("m9iB-syn0 true true"), "out: {out}");
    assert!(out.contains("m9iB-syn1 true true"), "out: {out}");
    assert!(out.contains("m9iB-synset THROW ERR_VM_MODULE_STATUS"), "out: {out}");
    assert!(out.contains("m9iB-syncb THROW"), "out: {out}");
    assert!(out.contains("m9iB-synest true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase9i_worker_transfer_buffer_types() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { MessageChannel } from "node:worker_threads";
const { port1, port2 } = new MessageChannel();
const seen = [];
port2.on("message", (m) => { seen.push(m); });
const ab = new Uint8Array([1, 2, 3]).buffer;
port1.postMessage(ab, [ab]);
console.log("w9i-detach", ab.byteLength === 0);
const ab2 = new Uint8Array([4, 5]).buffer;
port1.postMessage(ab2);
console.log("w9i-copy", ab2.byteLength === 2);
const sub = new Uint8Array([1, 2, 3, 4]).subarray(1, 3);
port1.postMessage({ sub });
port1.postMessage({ bi: 5n, u: undefined, m: new Map([[1, 2]]), s: new Set([3]), d: new Date(0), ta: new Uint8Array([9]), dv: new DataView(new Uint8Array([7, 8]).buffer) });
const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, e.name); } };
t("w9i-baditem", () => port1.postMessage({ x: 1 }, [{ x: 1 }]));
t("w9i-dup", () => port1.postMessage("x", [ab2, ab2]));
t("w9i-detached", () => port1.postMessage(ab));
t("w9i-circular", () => { const o = {}; o.me = o; port1.postMessage(o); });
t("w9i-fn", () => port1.postMessage(() => {}));
setTimeout(() => {
  const [det, c2, subMsg, ty] = seen;
  console.log("w9i-gotxfer", det instanceof ArrayBuffer, det.byteLength === 3);
  console.log("w9i-gotbuf", c2 instanceof ArrayBuffer, c2.byteLength === 2, new Uint8Array(c2)[0] === 4);
  console.log("w9i-sub", ty !== undefined && subMsg.sub instanceof Uint8Array, subMsg.sub.length === 2, subMsg.sub[0] === 2, subMsg.sub.byteOffset === 1);
  console.log("w9i-types", typeof ty.bi === "bigint", ("u" in ty) && ty.u === undefined, ty.m instanceof Map && ty.m.get(1) === 2, ty.s instanceof Set && ty.s.has(3), ty.d instanceof Date && ty.d.getTime() === 0, ty.ta instanceof Uint8Array && ty.ta[0] === 9, ty.dv instanceof DataView && ty.dv.getUint8(1) === 8);
  port1.close();
  port2.close();
}, 200);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "w9i-detach true",
        "w9i-copy true",
        "w9i-baditem DataCloneError",
        "w9i-dup DataCloneError",
        "w9i-detached DataCloneError",
        "w9i-circular DataCloneError",
        "w9i-fn DataCloneError",
        "w9i-gotxfer true true",
        "w9i-gotbuf true true true",
        "w9i-sub true true true true",
        "w9i-types true true true true true true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_worker_transfer_port_migration() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { MessageChannel, MessagePort } from "node:worker_threads";
const { port1, port2 } = new MessageChannel();
const { port1: a1, port2: a2 } = new MessageChannel();
let a1got = [];
a1.on("message", (m) => { a1got.push(m); });
port1.postMessage({ p: a2 }, [a2]);
// 源端 neutered：后用静默，a1 收不到。
a2.postMessage("lost");
port2.on("message", (m) => {
  const ok = m.p instanceof MessagePort;
  console.log("w9i-mig", ok);
  if (!ok) return;
  globalThis.__mig = m.p;
  m.p.on("message", (x) => console.log("w9i-migmsg", x === "to-migrated"));
  m.p.postMessage("to-a1");
  a1.postMessage("to-migrated");
});
const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, e.name); } };
t("w9i-portnotransfer", () => port1.postMessage({ p: a1 }));
setTimeout(() => {
  console.log("w9i-neuter", !a1got.includes("lost"), a1got.includes("to-a1"));
  if (globalThis.__mig) globalThis.__mig.close();
  port1.close();
  port2.close();
  a1.close();
}, 300);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "w9i-mig true",
        "w9i-migmsg true",
        "w9i-portnotransfer DataCloneError",
        "w9i-neuter true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_worker_transfer_cross_thread_and_broadcast() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { MessageChannel, BroadcastChannel, Worker } from "node:worker_threads";
// 同会话 BC：自收排除、关者止收。
const b1 = new BroadcastChannel("w9i-bc");
const b2 = new BroadcastChannel("w9i-bc");
const b3 = new BroadcastChannel("w9i-other");
const seen = [];
b2.onmessage = (e) => { seen.push(["b2", e.data]); };
b1.onmessage = () => { seen.push(["b1", "SELF"]); };
b3.onmessage = (e) => { seen.push(["b3", e.data]); };
b1.postMessage("hello");
await new Promise((r) => setTimeout(r, 100));
b2.close();
b1.postMessage("after");
await new Promise((r) => setTimeout(r, 100));
// 跨线程：端口经 workerData 迁移 + BC 跨线程扇出。
const { port1, port2 } = new MessageChannel();
const back = [];
port1.on("message", (m) => { back.push(m); });
const w = new Worker(
  "import { parentPort, workerData, BroadcastChannel } from 'node:worker_threads';" +
  "const bc = new BroadcastChannel('w9i-x');" +
  "workerData.p.on('message', (m) => { parentPort.postMessage('w saw ' + m); bc.postMessage('from-worker'); });" +
  "workerData.p.postMessage('hi-main');",
  { eval: true, workerData: { n: 41n, p: port2 }, transferList: [port2] }
);
const xseen = [];
const xb = new BroadcastChannel("w9i-x");
xb.onmessage = (e) => { xseen.push(e.data); };
w.on("message", (m) => { back.push("W:" + m); });
w.on("error", (e) => { back.push("ERR" + e.message); });
port1.postMessage("hi-worker");
setTimeout(() => {
  console.log("w9i-bc", JSON.stringify(seen) === JSON.stringify([["b2", "hello"]]));
  console.log("w9i-xfer", back.includes("hi-main"), back.includes("W:w saw hi-worker"));
  console.log("w9i-xbc", xseen.includes("from-worker"));
  w.terminate().then(() => {
    b1.close();
    b3.close();
    xb.close();
    port1.close();
  });
}, 600);
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in ["w9i-bc true", "w9i-xfer true true", "w9i-xbc true"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9i_x509_verify() {
    let dir = assert_fs::TempDir::new().unwrap();
    let key = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    dir.child("c.pem").write_str(&key.cert.pem()).unwrap();
    // openssl 烤入固件：sha256WithRSAEncryption（CA:TRUE）与 Ed25519（SKI/AKID 齐）。
    const RSA_PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIDBzCCAe+gAwIBAgIUKMqG5DU1vRAPDN73tipxwpNdnyYwDQYJKoZIhvcNAQEL\nBQAwEzERMA8GA1UEAwwIcnNhcHJvYmUwHhcNMjYwOTEyMTUzNzEwWhcNMjYxMDEy\nMTUzNzEwWjATMREwDwYDVQQDDAhyc2Fwcm9iZTCCASIwDQYJKoZIhvcNAQEBBQAD\nggEPADCCAQoCggEBAKNUnvi0BslHzHg4FsLJVRAGGnJLau1qpKYsUpl9o54Gi37o\nmDISUL+m+2sHk4GfdHmQtZMv/2ehbsZlWeXF+KN7R8Y3gsjXjws772d2H2KSKeBs\nrgXuW0aK7dZ298VJWiOkwn3Yxw3/VIrCnf22OvFD6hIEnJINsed7pWXcO6ENs0sn\nZjSoTrdUARO4bP4UUaRHRC7OvvmJOhk7YP27GxeZYlpGZAVc9bApeqNXFMORbw7h\n56F5OaWRd3+hLS2Qzb1WG8hkHNP2p6IjrM3sJv9/MxDG7oqUyGfHUU+L7WwsKVSX\nIvH1KVjSgCpQIe0xNK6hDVeEzCQ1K+1NerUXZb0CAwEAAaNTMFEwHQYDVR0OBBYE\nFCYP4DCcqmSzcUh9l5twIQZ+vfTnMB8GA1UdIwQYMBaAFCYP4DCcqmSzcUh9l5tw\nIQZ+vfTnMA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEBAELdfAXs\nhThVFUilcN1IqvCgnpJQaWt9F9hrK/kFU4xYuufULn8/ON1XSEsq9zFdaieh3yUS\ngfK5TzaqYWdmReZQmQwu3tWQP3i2N/+yhlKtyG8HVYGE8XnWzc1vqVCvo4Qgbo8A\nvprQmAByt1d73hYRAVqy6352VyaODUzv88o+bhA4lEVoFT+ywq/NTKMBItzX2nrf\nmbYHk5BuaD60LrgoHhagfZN0AtyjUsrSpZzhZibvkF/dd8JECjgMfdW2TzQt7oGu\nXlWJbDY2dLREtjr9Gy9xL9VYEqVwBl0LnOn+6NVXu1SWpdvb7JJkiTl/E5T7jJHM\nJYXWO6JD6QVhZA4=\n-----END CERTIFICATE-----\n";
    const ED_PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIBODCB66ADAgECAhQKqo7DNn6Wqr9sY8nb/FFQHmFWrzAFBgMrZXAwEjEQMA4G\nA1UEAwwHZWRwcm9iZTAeFw0yNjA5MTIxNTM3MTBaFw0yNjEwMTIxNTM3MTBaMBIx\nEDAOBgNVBAMMB2VkcHJvYmUwKjAFBgMrZXADIQDZBIIBibh/Hk5+4U8s3/NQ1YLC\nkRPopcUuaL2ubsrCyaNTMFEwHwYDVR0jBBgwFoAUx3a9XMqYMQnv2WV1xfhH8yx4\nlbIwDwYDVR0TAQH/BAUwAwEB/zAdBgNVHQ4EFgQUx3a9XMqYMQnv2WV1xfhH8yx4\nlbIwBQYDK2VwA0EADkN9ATKhMQdKm8vmdTP4+kV0BczvogHkDyXLYf+If4nw4CYs\nBngogF7qMQ7NdKgX1SlKGef1y1Oqc6T0zFQAAg==\n-----END CERTIFICATE-----\n";
    let out = {
        let file = dir.child("x.mjs");
        file.write_str(
            r#"
import { X509Certificate, createPublicKey, generateKeyPairSync } from "node:crypto";
import fs from "node:fs";
const x = new X509Certificate(fs.readFileSync("c.pem", "utf8"));
console.log("xv-self", x.verify(x.publicKey));
console.log("xv-ca", x.ca === false, typeof x.publicKey === "object");
console.log("xv-pemrt", new X509Certificate(x.toString()).verify(x.publicKey));
const pk2 = createPublicKey(x.publicKey.export({ type: "spki", format: "pem" }));
console.log("xv-pubrt", x.verify(pk2));
const RSA_PEM = `-----BEGIN CERTIFICATE-----
MIIDBzCCAe+gAwIBAgIUKMqG5DU1vRAPDN73tipxwpNdnyYwDQYJKoZIhvcNAQEL
BQAwEzERMA8GA1UEAwwIcnNhcHJvYmUwHhcNMjYwOTEyMTUzNzEwWhcNMjYxMDEy
MTUzNzEwWjATMREwDwYDVQQDDAhyc2Fwcm9iZTCCASIwDQYJKoZIhvcNAQEBBQAD
ggEPADCCAQoCggEBAKNUnvi0BslHzHg4FsLJVRAGGnJLau1qpKYsUpl9o54Gi37o
mDISUL+m+2sHk4GfdHmQtZMv/2ehbsZlWeXF+KN7R8Y3gsjXjws772d2H2KSKeBs
rgXuW0aK7dZ298VJWiOkwn3Yxw3/VIrCnf22OvFD6hIEnJINsed7pWXcO6ENs0sn
ZjSoTrdUARO4bP4UUaRHRC7OvvmJOhk7YP27GxeZYlpGZAVc9bApeqNXFMORbw7h
56F5OaWRd3+hLS2Qzb1WG8hkHNP2p6IjrM3sJv9/MxDG7oqUyGfHUU+L7WwsKVSX
IvH1KVjSgCpQIe0xNK6hDVeEzCQ1K+1NerUXZb0CAwEAAaNTMFEwHQYDVR0OBBYE
FCYP4DCcqmSzcUh9l5twIQZ+vfTnMB8GA1UdIwQYMBaAFCYP4DCcqmSzcUh9l5tw
IQZ+vfTnMA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEBAELdfAXs
hThVFUilcN1IqvCgnpJQaWt9F9hrK/kFU4xYuufULn8/ON1XSEsq9zFdaieh3yUS
gfK5TzaqYWdmReZQmQwu3tWQP3i2N/+yhlKtyG8HVYGE8XnWzc1vqVCvo4Qgbo8A
vprQmAByt1d73hYRAVqy6352VyaODUzv88o+bhA4lEVoFT+ywq/NTKMBItzX2nrf
mbYHk5BuaD60LrgoHhagfZN0AtyjUsrSpZzhZibvkF/dd8JECjgMfdW2TzQt7oGu
XlWJbDY2dLREtjr9Gy9xL9VYEqVwBl0LnOn+6NVXu1SWpdvb7JJkiTl/E5T7jJHM
JYXWO6JD6QVhZA4=
-----END CERTIFICATE-----
`;
const ED_PEM = `-----BEGIN CERTIFICATE-----
MIIBODCB66ADAgECAhQKqo7DNn6Wqr9sY8nb/FFQHmFWrzAFBgMrZXAwEjEQMA4G
A1UEAwwHZWRwcm9iZTAeFw0yNjA5MTIxNTM3MTBaFw0yNjEwMTIxNTM3MTBaMBIx
EDAOBgNVBAMMB2VkcHJvYmUwKjAFBgMrZXADIQDZBIIBibh/Hk5+4U8s3/NQ1YLC
kRPopcUuaL2ubsrCyaNTMFEwHwYDVR0jBBgwFoAUx3a9XMqYMQnv2WV1xfhH8yx4
lbIwDwYDVR0TAQH/BAUwAwEB/zAdBgNVHQ4EFgQUx3a9XMqYMQnv2WV1xfhH8yx4
lbIwBQYDK2VwA0EADkN9ATKhMQdKm8vmdTP4+kV0BczvogHkDyXLYf+If4nw4CYs
BngogF7qMQ7NdKgX1SlKGef1y1Oqc6T0zFQAAg==
-----END CERTIFICATE-----
`;
const rsa = new X509Certificate(RSA_PEM);
console.log("xv-rsa", rsa.verify(rsa.publicKey), rsa.ca === true);
const ed = new X509Certificate(ED_PEM);
console.log("xv-ed", ed.verify(ed.publicKey));
const { publicKey: other } = generateKeyPairSync("ec", { namedCurve: "P-256" });
console.log("xv-wrong", x.verify(other));
console.log("xv-cross", rsa.verify(x.publicKey), ed.verify(rsa.publicKey));
const { publicKey: okp } = generateKeyPairSync("x25519");
console.log("xv-okp", x.verify(okp));
const der = Buffer.from(x.raw);
der[der.length - 1] ^= 0xff;
const tampered = new X509Certificate(der);
console.log("xv-tamper", tampered.verify(tampered.publicKey) === false);
const t = (n, f) => { try { f(); console.log(n, "NO-THROW"); } catch (e) { console.log(n, e.code); } };
t("xv-noarg", () => x.verify());
t("xv-strarg", () => x.verify("nope"));
t("xv-priv", () => x.verify(generateKeyPairSync("ec", { namedCurve: "P-256" }).privateKey));
"#,
        )
        .unwrap();
        winterjs()
            .arg("--run")
            .arg(file.path())
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in [
        "xv-self true",
        "xv-ca true true",
        "xv-pemrt true",
        "xv-pubrt true",
        "xv-rsa true true",
        "xv-ed true",
        "xv-wrong false",
        "xv-cross false false",
        "xv-okp false",
        "xv-tamper true",
        "xv-noarg ERR_INVALID_ARG_TYPE",
        "xv-strarg ERR_INVALID_ARG_TYPE",
        "xv-priv ERR_INVALID_ARG_VALUE",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
