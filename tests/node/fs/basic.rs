//! tests/node/fs/basic.rs — 读写/目录/promises 基础（对齐 src/builtins/node/fs.rs）。

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

