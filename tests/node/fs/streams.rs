//! tests/node/fs/streams.rs — 流/cp/偏移（对齐 src/builtins/node/fs.rs）。

use crate::common::*;
use crate::helpers::*;
use assert_fs::prelude::*;

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
