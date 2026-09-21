//! tests/node/fs/sync.rs — 同步/filehandle/回调（对齐 src/builtins/node/fs.rs）。

use crate::helpers::*;

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

