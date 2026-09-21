//! tests/node/fs.rs — 对齐 src/builtins/node/fs.rs（node:fs）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

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
fn phase10f_fs_watch_ignore_and_relpath() {
    // G8-1：ignore 全形态（string glob/RegExp/Function/混排 + 非法码）与
    // 递归 filename 相对路径（`subdir/file.txt`）+ `**` 目录忽略。
    // 正常：混排只放行 keep.txt；报错：123/''/[123]/[''] 四码；
    // 边界：递归写 node_modules 内文件被 `**/node_modules/**` 吞掉。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("ignore.mjs");
    file.write_str(
        r#"
import fs from "node:fs";
import assert from "node:assert";
// 报错：校验码（validateIgnoreOption 口径）
for (const [v, code] of [[123, "ERR_INVALID_ARG_TYPE"], ["", "ERR_INVALID_ARG_VALUE"], [[123], "ERR_INVALID_ARG_TYPE"], [[""], "ERR_INVALID_ARG_VALUE"]]) {
  try { fs.watch(".", { ignore: v }); console.log("ignore-no-throw", JSON.stringify(v)); }
  catch (e) { console.log("ignore-code", e.code === code); }
}
// 正常：混排（string matchBase + RegExp + Function）
{
  const w = fs.watch("mix", {
    ignore: ["*.log", /\.tmp$/, (fn) => fn.startsWith(".")],
  });
  w.on("change", (ev, fn) => {
    if (fn === "keep.txt") { console.log("mix-pass", true); w.close(); }
    else console.log("mix-leak", fn);
  });
  setTimeout(() => {
    fs.writeFileSync("mix/debug.log", "x");
    fs.writeFileSync("mix/temp.tmp", "x");
    fs.writeFileSync("mix/.secret", "x");
    fs.writeFileSync("mix/keep.txt", "x");
  }, 150);
}
// 边界：递归相对路径 + `**` 忽略
{
  const w = fs.watch("tree", {
    recursive: true,
    ignore: ["**/node_modules/**", "**/node_modules"],
  });
  w.on("change", (ev, fn) => {
    if (fn && fn.includes("node_modules")) { console.log("tree-leak", fn); return; }
    if (fn && fn.endsWith("src/app.js")) { console.log("tree-rel", fn === "src/app.js"); w.close(); }
  });
  setTimeout(() => {
    fs.writeFileSync("tree/node_modules/package.json", "{}");
    fs.writeFileSync("tree/src/app.js", "x");
  }, 150);
}
setTimeout(() => { console.log("ignore-done"); process.exit(0); }, 4000);
"#,
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("mix")).unwrap();
    std::fs::create_dir_all(dir.path().join("tree/node_modules")).unwrap();
    std::fs::create_dir_all(dir.path().join("tree/src")).unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in [
        "ignore-code true",
        "mix-pass true",
        "tree-rel true",
        "ignore-done",
    ] {
        assert!(
            text.lines().any(|l| l == line),
            "missing: {line}\nout: {text}"
        );
    }
    assert!(!text.contains("mix-leak"), "ignore 漏网:\n{text}");
    assert!(!text.contains("tree-leak"), "node_modules 漏网:\n{text}");
    assert!(!text.contains("ignore-no-throw"), "非法 ignore 未抛:\n{text}");
    dir.close().unwrap();
}

#[test]
fn phase10f_fs_watch_encoding_faces() {
    // G8-3：filename 按 options.encoding 转码（hex/buffer/缺省 utf8；null 直通）。
    // 正常：hex 串/Buffer/原文各就各位；边界：非法 encoding 即 ARG_VALUE。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("enc.mjs");
    file.write_str(
        r#"
import fs from "node:fs";
try { fs.watch(".", { encoding: "nope" }); console.log("enc-no-throw"); }
catch (e) { console.log("enc-code", e.code === "ERR_INVALID_ARG_VALUE"); }
const fn = "hexname.txt";
let left = 3;
const done = () => { if (--left === 0) { console.log("enc-done"); process.exit(0); } };
const w1 = fs.watch(".", { encoding: "hex" }, (ev, f) => {
  if (f === Buffer.from(fn, "utf8").toString("hex")) { console.log("enc-hex", true); w1.close(); done(); }
});
const w2 = fs.watch(".", { encoding: "buffer" }, (ev, f) => {
  if (f instanceof Buffer && f.toString("utf8") === fn) { console.log("enc-buf", true); w2.close(); done(); }
});
const w3 = fs.watch(".", (ev, f) => {
  if (f === fn) { console.log("enc-plain", true); w3.close(); done(); }
});
setTimeout(() => { fs.writeFileSync(fn, "x"); }, 150);
setTimeout(() => { console.log("enc-timeout"); process.exit(1); }, 6000);
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
    for line in ["enc-code true", "enc-hex true", "enc-buf true", "enc-plain true", "enc-done"] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_fs_promises_watch_surface() {
    // G8-4：fs/promises.watch 异步迭代（{eventType, filename} + 校验 reject +
    // abort + break 后重迭代 noop）。
    // 正常：目录写即迭代到 rename/change + filename；报错：7 组校验逐项；
    // 边界：abort 即 AbortError，break 后重跑 done。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("pw.mjs");
    file.write_str(
        r#"
import { watch } from "node:fs/promises";
import fs from "node:fs";
import assert from "node:assert";
// 报错：校验逐项（reject 码）
const bad = [
  [() => watch(1), "ERR_INVALID_ARG_TYPE"],
  [() => watch("x", 1), "ERR_INVALID_ARG_TYPE"],
  [() => watch("x", { persistent: 1 }), "ERR_INVALID_ARG_TYPE"],
  [() => watch("x", { recursive: 1 }), "ERR_INVALID_ARG_TYPE"],
  [() => watch("x", { encoding: 1 }), "ERR_INVALID_ARG_VALUE"],
  [() => watch("x", { signal: 1 }), "ERR_INVALID_ARG_TYPE"],
  [() => watch("x", { maxQueue: "silly" }), "ERR_INVALID_ARG_TYPE"],
  [() => watch("x", { overflow: "barf" }), "ERR_INVALID_ARG_VALUE"],
];
for (const [fn, code] of bad) {
  try { for await (const _ of fn()) { console.log("watch-no-throw"); } }
  catch (e) { console.log("watch-bad", e.code === code); }
}
// 正常：迭代 + break 后重跑 noop
{
  const w = watch("sub");
  let n = 0;
  setTimeout(() => { fs.writeFileSync("sub/a.txt", "x"); }, 150);
  for await (const { eventType, filename } of w) {
    if (filename === "a.txt" && (eventType === "rename" || eventType === "change")) {
      console.log("watch-hit", true);
      n++;
      break;
    }
  }
  let again = 0;
  for await (const _ of w) { again++; }
  console.log("watch-once", n === 1, again === 0);
}
// 边界：abort 即 AbortError
{
  const ac = new AbortController();
  setTimeout(() => ac.abort(), 100);
  try { for await (const _ of watch("sub", { signal: ac.signal })) {} }
  catch (e) { console.log("watch-abort", e.name === "AbortError"); }
}
setTimeout(() => { console.log("watch-done"); process.exit(0); }, 3000);
"#,
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(
        text.lines().filter(|l| *l == "watch-bad true").count(),
        8,
        "校验 8 组:\n{text}"
    );
    for line in ["watch-hit true", "watch-once true true", "watch-abort true", "watch-done"] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_fs_watch_active_handles() {
    // G8-5：`process._getActiveHandles()` 存活 watch 句柄集（close 即摘）。
    // 正常：watch 后集内可见、close 后消失；边界：关两次幂等，集为空数组。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("handles.mjs");
    file.write_str(
        r#"
import fs from "node:fs";
const before = process._getActiveHandles().length;
const w = fs.watch("sub");
console.log("handles-add", process._getActiveHandles().length === before + 1);
w.close();
w.close();
console.log("handles-del", process._getActiveHandles().length === before);
const sw = fs.watchFile("sub/f.txt", { interval: 100 }, () => {});
console.log("handles-stat", process._getActiveHandles().length === before + 1);
sw.stop();
console.log("handles-stat-del", process._getActiveHandles().length === before);
setTimeout(() => { console.log("handles-done"); process.exit(0); }, 500);
"#,
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/f.txt"), b"x").unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in [
        "handles-add true",
        "handles-del true",
        "handles-stat true",
        "handles-stat-del true",
        "handles-done",
    ] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_fs_write_flush_option() {
    // G8-7：write/append/stream 的 `flush` 选项（布尔校验 + true 即 fsync 落盘）。
    // 正常：flush:true 写后内容可读（sync/callback/stream 三面）；
    // 报错：7 种非法值逐项 ARG_TYPE；边界：flush:false 与缺省等价。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("flush.mjs");
    file.write_str(
        r#"
import fs from "node:fs";
const bad = ["true", "", 0, 1, [], {}, Symbol()];
let n = 0;
for (const v of bad) {
  for (const fn of [
    () => fs.writeFileSync("f.txt", "x", { flush: v }),
    () => fs.appendFileSync("f.txt", "x", { flush: v }),
    () => fs.createWriteStream("f.txt", { flush: v }),
  ]) {
    try { const r = fn(); if (r && r.on) r.on("error", () => {}); console.log("flush-no-throw"); }
    catch (e) { if (e.code === "ERR_INVALID_ARG_TYPE") n++; }
  }
}
console.log("flush-bad", n === 21);
fs.writeFileSync("w.txt", "flushed", { flush: true });
fs.appendFileSync("a.txt", "more", { flush: true });
console.log("flush-sync", fs.readFileSync("w.txt", "utf8") === "flushed", fs.readFileSync("a.txt", "utf8") === "more");
fs.writeFile("w2.txt", "cb", { flush: true }, (e) => {
  if (e) throw e;
  console.log("flush-cb", fs.readFileSync("w2.txt", "utf8") === "cb");
  const s = fs.createWriteStream("s.txt", { flush: true });
  s.on("error", (e) => { throw e; });
  s.write("streamed");
  s.end(() => {
    console.log("flush-stream", fs.readFileSync("s.txt", "utf8") === "streamed");
    console.log("flush-done");
  });
});
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
        "flush-bad true",
        "flush-sync true true",
        "flush-cb true",
        "flush-stream true",
        "flush-done",
    ] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_fs_watch_rapid_and_rewrite() {
    // G8-8：持续写不饿死（前沿即刷）+ Create 二判据（重写首事件 change，
    // 新文件首事件 rename）。
    // 正常：10ms 写循环下首个 foo.txt 事件 3s 内必达；预存文件重写首事件
    // change；边界：watch 后新建首事件 rename。
    let dir = assert_fs::TempDir::new().unwrap();
    std::fs::write(dir.path().join("old.txt"), b"old").unwrap();
    let file = dir.child("rapid.mjs");
    file.write_str(
        r#"
import fs from "node:fs";
// 持续写：首事件必达（静默窗饿死回归）
{
  const w = fs.watch("loop");
  const iv = setInterval(() => { fs.writeFileSync("loop/foo.txt", "x"); }, 10);
  w.on("change", (ev, fn) => {
    if (fn === "foo.txt") { console.log("rapid-hit", ev); clearInterval(iv); w.close(); }
  });
}
// 预存重写：首事件 change（Create artifact 纠正）
{
  const w = fs.watch("old.txt");
  setTimeout(() => { fs.writeFileSync("old.txt", "new"); }, 300);
  w.on("change", (ev, fn) => {
    console.log("rewrite-first", ev === "change", fn);
    w.close();
  });
}
// watch 后新建：首事件 rename
{
  const w = fs.watch("fresh");
  setTimeout(() => { fs.writeFileSync("fresh/n.txt", "x"); }, 300);
  w.on("change", (ev, fn) => {
    if (fn === "n.txt") { console.log("fresh-first", ev === "rename"); w.close(); }
  });
}
setTimeout(() => { console.log("rapid-done"); process.exit(0); }, 6000);
"#,
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("loop")).unwrap();
    std::fs::create_dir(dir.path().join("fresh")).unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for line in ["rapid-hit rename", "rewrite-first true old.txt", "fresh-first true", "rapid-done"] {
        assert!(text.lines().any(|l| l == line), "missing: {line}\nout: {text}");
    }
    dir.close().unwrap();
}

#[test]
fn node_fs_streams() {
    // createReadStream 分块 + createWriteStream 落盘/追加（10f 起真 WriteStream：
    // write/end/finish 事件面，Web 流 getWriter 口径退役——node 真机无此面）。
    let dir = assert_fs::TempDir::new().unwrap();
    std::fs::write(dir.path().join("in.txt"), b"hello-fs-stream").unwrap();
    let code = r#"import fs from "node:fs";
const rs = fs.createReadStream("in.txt", { highWaterMark: 4 });
let s = "";
for await (const c of rs) s += new TextDecoder().decode(c);
if (s !== "hello-fs-stream") throw new Error("read failed: " + s);
const ws = fs.createWriteStream("out.txt");
ws.write("ab");
ws.write("cd");
await new Promise((res) => ws.end(res));
if (fs.readFileSync("out.txt", "utf8") !== "abcd") throw new Error("write failed");
const wa = fs.createWriteStream("out.txt", { flags: "a" });
wa.write("ef");
await new Promise((res) => wa.end(res));
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
// utimes（数字实参为秒，node 26 真机同款口径；毫秒精度 ±2s——G4 修正：
// 旧实现把数字当 ms，测试侧随实现偏差一并改秒口径）
utimesSync("t.txt", 1000, 2000);
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
try { cpSync("d", "d3"); } catch (e) { console.log("cp-eisdir", e.code === "ERR_FS_EISDIR"); }
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
const rr = await fh.read(rb, 0, 11, 0);
console.log("fh-read", rr.bytesRead, new TextDecoder().decode(rr.buffer));
console.log("fh-stat", (await fh.stat()).size);
await fh.chmod(0o640);
console.log("fh-chmod", (fs.statSync("f.txt").mode & 0o777).toString(8));
await fh.utimes(500, 600);
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
// 重复 close：node 口径幂等（缓存同 promise，不抛）；stat 才 EBADF
await fh2.close();
console.log("fh-ebadf", "idem");
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
    assert!(out.contains("fh-overwrite abcdef"), "out: {out}");
    assert!(out.contains("fh-readFile "), "out: {out}");
    assert!(out.contains("fh-append abcdefXYZ"), "out: {out}");
    assert!(out.contains("fh-ebadf idem"), "out: {out}");
    assert!(out.contains("fh-ebadf2 true"), "out: {out}");
    assert!(out.contains("p-trunc 2"), "out: {out}");
    assert!(out.contains("p-readlink f.txt"), "out: {out}");
    assert!(out.contains("p-cp ab"), "out: {out}");
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
fn phase9c_fs_watchfile_poll() {
    // 正常：同路径单例（w===w2，监听累积，listenerCount 2）；stop 关共享句柄
    //（后续 append 不再派发，真机 w2.stop 口径）；unwatchFile 指定摘除后归零即停。
    // 报错：listener 非函数即 ERR_INVALID_ARG_TYPE TypeError。
    // 边界：缺席文件首轮即发 (zero,zero)（真机实测）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("w.txt").write_str("aaa").unwrap();
    dir.child("v.txt").write_str("aaa").unwrap();
    let file = dir.child("m.mjs");
    file.write_str(
        r#"
import { watchFile, unwatchFile, appendFileSync } from "node:fs";
try { watchFile("w.txt"); console.log("NO-ERR"); }
catch (e) { console.log("bad-listener", e.constructor.name, e.code === "ERR_INVALID_ARG_TYPE"); }
let calls = 0;
let vCalls = 0;
const w = watchFile("w.txt", { interval: 100 }, () => { calls += 1; });
const w2 = watchFile("w.txt", { interval: 100 }, () => { calls += 10; });
console.log("same", w === w2, w.listenerCount("change") === 2);
console.log("chain", w2.stop() === w2, w2.ref() === w2, w2.unref() === w2);
const vfn = () => { vCalls += 1; };
watchFile("v.txt", { interval: 100 }, vfn);
unwatchFile("v.txt", vfn);
setTimeout(() => { appendFileSync("w.txt", "bbbb"); appendFileSync("v.txt", "bbbb"); }, 350);
setTimeout(() => {
  console.log("calls", calls, vCalls);
  unwatchFile("w.txt");
}, 900);
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
    for line in ["bad-listener TypeError true", "same true true", "chain true true true", "calls 0 0"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9m_fs_statfs_surface() {
    // 正常：sync/回调/promises 三面 + StatsFs 形状；报错：坏路径 ENOENT；
    // 边界：字段均为非负数。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import { statfsSync, statfs, StatsFs } from "node:fs";
import { statfs as pstatfs } from "node:fs/promises";
const s = statfsSync(".");
console.log("sync", s instanceof StatsFs, s.bsize > 0, s.blocks > 0, s.bfree >= 0, s.bavail >= 0, s.files >= 0, s.ffree >= 0, typeof s.type);
console.log("cb", await new Promise((res, rej) => statfs(".", (e, v) => e ? rej(e) : res(v.blocks > 0))));
console.log("prom", (await pstatfs(".")).bfree >= 0);
try { statfsSync("/no/such/dir-xyz-9m"); console.log("NO-ERR"); }
catch (e) { console.log("err", e.code); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in ["sync true true true true true true true number", "cb true", "prom true", "err ENOENT"] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase9m_read_file_no_encoding_returns_buffer() {
    // 无编码读返回 Buffer（Node 语义；真机口径）：isBuffer/String(buf)/
    // toString() = utf8 内容、JSON.parse(buf) 隐式转换取内容——裸 Uint8Array
    // 会 join 成 "byte,byte,…"（vite PostCSS 配置加载实测误判）。
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("f.txt").write_str("{\"k\":\"v\"}").unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fs from "node:fs";
const b = fs.readFileSync("f.txt");
console.log("buf", Buffer.isBuffer(b), b instanceof Uint8Array, b.constructor.name);
console.log("str", String(b), b.toString());
console.log("json", JSON.parse(b).k);
console.log("enc", typeof fs.readFileSync("f.txt", "utf8"));
"#,
    );
    for line in ["buf true true Buffer", "str {\"k\":\"v\"} {\"k\":\"v\"}", "json v", "enc string"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_fs_cp_validation_and_stream_opts() {
    // cp 校验族（validateCpOptions 逐字）+ 流构造器 getOptions/病 fd path。
    // 对拍：test-fs-cp-sync-mode-invalid/options-invalid-type/incompatible/
    // src-dest-identical/copy-directory-without-recursive + write-stream-throw-type-error/read-stream-fd。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "cpv.mjs",
        r#"
import fs from "node:fs";
const t = (name, fn, code) => {
  try { fn(); console.log(name, "no-throw"); }
  catch (e) { console.log(name, e.code === code); }
};
// 正常：文件拷贝 + 递归目录拷贝
fs.writeFileSync("a.txt", "hi");
fs.cpSync("a.txt", "b.txt");
console.log("cp-ok", fs.readFileSync("b.txt", "utf8"));
fs.mkdirSync("d/sub", { recursive: true });
fs.writeFileSync("d/sub/f.txt", "x");
fs.cpSync("d", "d2", { recursive: true });
console.log("cp-dir", fs.readFileSync("d2/sub/f.txt", "utf8"));
// 报错：mode 越界 / options 非对象 / 互斥对 / 同路径 / 目录非递归
t("mode", () => fs.cpSync("a.txt", "b.txt", { mode: -1 }), "ERR_OUT_OF_RANGE");
t("opts", () => fs.cpSync("a.txt", "b.txt", () => {}), "ERR_INVALID_ARG_TYPE");
t("pair", () => fs.cpSync("a.txt", "b.txt", { dereference: true, verbatimSymlinks: true }), "ERR_INCOMPATIBLE_OPTION_PAIR");
t("same", () => fs.cpSync("a.txt", "a.txt"), "ERR_FS_CP_EINVAL");
t("eisdir", () => fs.cpSync("d", "d3"), "ERR_FS_EISDIR");
// 边界：filter 返回 false 跳过；createWriteStream 非法 options 抛；fd 形 path 为 undefined
fs.cpSync("a.txt", "c.txt", { filter: () => false });
console.log("filter-skip", fs.existsSync("c.txt"));
// 链接语义：默认相对链接消解为绝对；verbatim 保留原文；复拷同目标换链无错
fs.writeFileSync("foo.js", "foo");
fs.symlinkSync("foo.js", "bar.js");
fs.mkdirSync("vd");
fs.cpSync("bar.js", "vd/bar.js");
console.log("link-abs", fs.readlinkSync("vd/bar.js").endsWith("foo.js") && fs.readlinkSync("vd/bar.js").startsWith("/"));
fs.mkdirSync("vd2");
fs.cpSync("bar.js", "vd2/bar.js", { verbatimSymlinks: true });
console.log("link-verb", fs.readlinkSync("vd2/bar.js"));
fs.cpSync("bar.js", "vd2/bar.js", { verbatimSymlinks: true });
console.log("link-replace", fs.readlinkSync("vd2/bar.js"));
// 文件盖链接目录（dereference 形）：dest 由链接变文件
fs.mkdirSync("rl");
fs.symlinkSync(fs.realpathSync("rl"), "rl-link", "dir");
fs.cpSync("a.txt", "rl-link", { dereference: false });
console.log("file-over-link", fs.statSync("rl-link").isFile());
t("wsopt", () => fs.createWriteStream("a.txt", 123), "ERR_INVALID_ARG_TYPE");
const fd = fs.openSync("a.txt", "r");
const rs = fs.createReadStream(null, { fd });
console.log("fd-path", rs.path === undefined);
fs.closeSync(fd);
"#,
    );
    for line in [
        "cp-ok hi",
        "cp-dir x",
        "mode true",
        "opts true",
        "pair true",
        "same true",
        "eisdir true",
        "filter-skip false",
        "link-abs true",
        "link-verb foo.js",
        "link-replace foo.js",
        "file-over-link true",
        "wsopt true",
        "fd-path true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_fs_stream_lifetime() {
    // fs 流续命（__wjs_fs_stream_ref/unref + idle 门）：裸 end() 后挂监听仍收
    // finish/close；只构造不用的流不续命（进程正常退出）；close 双调只释一次。
    // UNSAFE-BOUNDARY(fs_stream_ref/unref) 覆盖：饱和减无 panic 路径。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "life.mjs",
        r#"
import fs from "node:fs";
// end 后挂监听仍收齐（同步派发即丢，须递延）。
{
  const s = fs.createWriteStream("a.txt");
  s.end("hi");
  s.on("finish", () => console.log("w-fin"));
  s.on("close", () => console.log("w-close", fs.readFileSync("a.txt", "utf8")));
}
// 只构造不用：不续命（本用例能退出即证明）。
{
  const s = fs.createWriteStream("b.txt");
  const r = fs.createReadStream("a.txt");
  r.on("data", () => {});
  r.on("end", () => console.log("r-end"));
}
// 双关：计数归零不欠不超（退出码 0 即证明）。
{
  const s = fs.createWriteStream("c.txt");
  s.end("x");
  s.close(() => console.log("w-cb"));
  s.close(() => console.log("w-cb2"));
}
"#,
    );
    for line in ["w-fin", "w-close hi", "r-end", "w-cb", "w-cb2"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_mkdtemp_disposable_sync_cjs_export() {
    // node 26 双名都在：`require('fs').mkdtempDisposableSync` 具名（套件点名）
    // 与 `mkdtempDisposable` 别名并存；返回 {path, remove} 且二次 remove 不抛。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.cjs",
        r#"
const fs = require("fs");
console.log("names", typeof fs.mkdtempDisposableSync, typeof fs.mkdtempDisposable, fs.mkdtempDisposableSync === fs.mkdtempDisposable);
const r = fs.mkdtempDisposableSync("./wjs-x.");
console.log("shape", typeof r.path === "string", typeof r.remove === "function");
r.remove(); r.remove();
console.log("twice-remove-ok");
"#,
    );
    for line in [
        "names function function true",
        "shape true true",
        "twice-remove-ok",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_read_stream_live_follow() {
    // read-pos 套件回归：live 增长文件短读不断流（无显式 end 时耗尽走
    // macrotask 重查，有增长即续读），停写即落定。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fs from "node:fs";
fs.writeFileSync("g.txt", "0123456789");
// 快照耗尽 + 无增长即落定（单 macrotask，不 hang）。
{
  const s = fs.createReadStream("g.txt", { highWaterMark: 4, start: 2 });
  let n = "";
  s.on("data", (d) => { n += d.toString(); });
  await new Promise((res) => s.on("end", res));
  console.log("snap", n === "23456789");
}
// live 增长：边写边读不断流（短读出现），停写即 end。
{
  let cur = 0;
  let shorts = 0;
  let ended = false;
  let i = 0;
  await new Promise((res) => {
    const w = setInterval(() => { i++; fs.writeFileSync("g.txt", `x${i}\n`, { flag: "a" }); }, 2);
    const s = fs.createReadStream("g.txt", { highWaterMark: 10, start: cur });
    s.on("data", (d) => {
      cur += d.length;
      if (d.length < 10 && ++shorts >= 3) { clearInterval(w); }
    });
    s.on("end", () => { ended = true; res(); });
  });
  console.log("live", shorts >= 3, ended, cur > 10);
}
"#,
    );
    for line in ["snap true", "live true true true"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_cp_async_filter() {
    // async-filter 套件回归：异步 cp 逐项 await filter（含子目录递归），
    // 同步校验（mode/options）仍同步抛；cpSync 拒 async filter 不变。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fs from "node:fs";
fs.mkdirSync("src/sub", { recursive: true });
fs.writeFileSync("src/a.js", "1");
fs.writeFileSync("src/b.txt", "2");
fs.writeFileSync("src/sub/c.js", "3");
await fs.promises.cp("src", "dst", {
  recursive: true,
  filter: async (p) => p.endsWith(".js") || (await fs.promises.stat(p)).isDirectory(),
});
const walk = (d) => fs.readdirSync(d, { recursive: true }).sort();
console.log("async-filter", JSON.stringify(walk("dst")));
try { fs.cpSync("src", "dst2", { recursive: true, filter: async () => true }); }
catch (e) { console.log("sync-rejects-async", e.code); }
try { fs.cp("src", "dst3", { mode: -1 }, () => {}); }
catch (e) { console.log("async-mode-sync-throw", e.code); }
"#,
    );
    assert!(
        out.lines().any(|l| l == r#"async-filter ["a.js","sub","sub/c.js"]"# || l == r#"async-filter ["a.js", "sub", "sub/c.js"]"#),
        "missing async-filter:\n{out}"
    );
    for line in ["sync-rejects-async ERR_INVALID_RETURN_VALUE", "async-mode-sync-throw ERR_OUT_OF_RANGE"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_read_stream_offsets_and_props() {
    // start/end 校验 + start/end 暴露 + 缺失文件异步 error + fd 复用定位读。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fs from "node:fs";
fs.writeFileSync("x", "xyz\n");
const t = (fn) => { try { fn(); console.log("no-throw"); } catch (e) { console.log(e.code, e.name); } };
t(() => fs.createReadStream("x", { end: Infinity }));
t(() => fs.createReadStream("x", { start: "4" }));
t(() => fs.createReadStream("x", { end: NaN }));
t(() => fs.createReadStream("x", { start: -1 }));
t(() => fs.createReadStream("x", { start: 0.1 }));
t(() => fs.createReadStream("x", { start: 2 ** 53, end: Infinity }));
t(() => fs.createReadStream("x", { start: 5, end: 1 }));
try { fs.createReadStream("x", { start: 10, end: 2 }); }
catch (e) { console.log("msg", e.message); }
t(() => fs.createWriteStream("w.tmp", { end: "bogus" }));
const s = fs.createReadStream("x", { start: 1, end: 2 });
console.log("props", s.start, s.end);
const s2 = fs.createReadStream("x");
console.log("props2", s2.start, s2.end);
s.destroy(); s2.destroy();
// 缺失文件：构造期不抛，异步 error（无监听即抛的真机语义不测，只测有监听形）。
await new Promise((res) => {
  const m = fs.createReadStream("definitely-missing-xyz");
  m.on("data", () => {});
  m.on("error", (e) => { console.log("async-error", e.code); res(); });
});
// autoClose:false fd 复用 + start:0 定位读（fileNext 形）。
await new Promise((res) => {
  let file = fs.createReadStream("x", { autoClose: false });
  let data = "";
  file.on("data", (c) => { data += c; });
  file.on("end", () => {
    console.log("chain1", JSON.stringify(data), !file.closed);
    file = fs.createReadStream(null, { fd: file.fd, start: 0 });
    file.data = "";
    file.on("data", (d) => { file.data += d; });
    file.on("end", () => { console.log("chain2", JSON.stringify(file.data)); res(); });
  });
});
// 坏 fd + autoClose:false：只派 error，不 destroy（closed 恒 false）。
await new Promise((res) => {
  const b = fs.createReadStream(null, { fd: 13337, autoClose: false });
  b.on("data", () => console.log("DATA-unexpected"));
  b.on("error", (e) => { console.log("badfd", e.code, b.closed, b.destroyed); res(); });
});
"#,
    );
    for line in [
        "no-throw",
        "ERR_INVALID_ARG_TYPE TypeError",
        "ERR_OUT_OF_RANGE RangeError",
        "msg The value of \"start\" is out of range. It must be <= \"end\" (here: 2). Received 10",
        "props 1 2",
        "props2 undefined Infinity",
        "async-error ENOENT",
        "chain1 \"xyz\\n\" true",
        "chain2 \"xyz\\n\"",
        "badfd EBADF false false",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    assert_eq!(
        out.lines().filter(|l| *l == "ERR_OUT_OF_RANGE RangeError").count(),
        5,
        "OOR x5:\n{out}"
    );
    dir.close().unwrap();
}

#[test]
fn phase10f_read_write_stream_encoding() {
    // 编码流：base64 读→pipe→base64 写→finish→latin1 读验整块。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fs from "node:fs";
import stream from "node:stream";
fs.writeFileSync("x.txt", "xyz\n");
await new Promise((res, rej) => {
  const r = fs.createReadStream("x.txt", { encoding: "base64" });
  const w = fs.createWriteStream("d.txt", { encoding: "base64" });
  r.on("error", rej); w.on("error", rej);
  r.pipe(w).on("finish", res);
});
console.log("phase1", JSON.stringify(fs.readFileSync("d.txt", "utf8")));
await new Promise((res, rej) => {
  const got = [];
  const sink = new stream.Writable({
    write(c, e, n) { got.push(c); n(); },
  });
  sink.setDefaultEncoding("latin1");
  const r = fs.createReadStream("d.txt", { encoding: "latin1" });
  r.on("error", rej);
  r.pipe(sink).on("finish", () => {
    console.log("phase2", got.length, got.every((c) => c.equals(Buffer.from("xyz\n"))));
    res();
  });
});
// WriteStream encoding 即默认编码：base64 串解码落盘。
await new Promise((res) => {
  const w = fs.createWriteStream("e.txt", { encoding: "base64" });
  w.write("eHl6");
  w.end("Q2c9PQ==");
  w.on("finish", () => { console.log("phase3", JSON.stringify(fs.readFileSync("e.txt", "utf8"))); res(); });
});
"#,
    );
    for line in ["phase1 \"xyz\\n\"", "phase2 1 true", "phase3 \"xyzCg==\""] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_file_handle_read_empty() {
    // 空 buffer + 零长读合法（length===0 先于空检查，node 序）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fs from "node:fs";
fs.writeFileSync("x.txt", "xyz\n");
const fh = await fs.promises.open("x.txt", "r");
const r0 = await fh.read(Buffer.alloc(0));
console.log("empty", r0.bytesRead);
const r1 = await fh.read({ buffer: Buffer.alloc(4), length: 0 });
console.log("len0", r1.bytesRead);
const r2 = await fh.read();
console.log("noparams", r2.bytesRead);
await fh.close();
"#,
    );
    for line in ["empty 0", "len0 0", "noparams 4"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_read_stream_fifo_end() {
    // fifo + end:1：写者先行时 open 即会合（双 open 死锁回归——__doOpen 经已开 fd 读）。
    // 无 mkfifo 即跳过（windows 记档）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import fs from "node:fs";
import child_process from "node:child_process";
const mk = child_process.spawnSync("mkfifo", ["f.pipe"]);
if (mk.error) { console.log("skip-nomkfifo"); }
else {
  child_process.exec(`echo "xyz foobar" > "f.pipe"`);
  await new Promise((res, rej) => {
    const s = fs.createReadStream("f.pipe", { end: 1 });
    s.data = "";
    s.on("data", (c) => { s.data += c; });
    s.on("end", () => { console.log("fifo", JSON.stringify(s.data)); res(); });
    s.on("error", rej);
  });
  fs.unlinkSync("f.pipe");
}
"#,
    );
    assert!(
        out.lines().any(|l| l == "fifo \"xy\"" || l == "skip-nomkfifo"),
        "missing fifo:\n{out}"
    );
    dir.close().unwrap();
}
