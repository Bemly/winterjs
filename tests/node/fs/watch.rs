//! tests/node/fs/watch.rs — watch 族（对齐 src/builtins/node/fs.rs）。

use crate::common::*;
use assert_fs::prelude::*;

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

