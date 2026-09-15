//! tests/node/path.rs — 对齐 src/builtins/node/path.rs（node:path）。

use crate::helpers::*;

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
fn node_path_to_namespaced_path() {
    // toNamespacedPath（真机 26.8.2 对拍）：posix 平台恒等；win32 盘符前缀
    // `\\?\`；null 原样穿透。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import path from "node:path";
console.log("posix", path.posix.toNamespacedPath("/a/b"), path.toNamespacedPath("/a/b"));
console.log("win32", path.win32.toNamespacedPath("C:\\a\\b"), path.win32.toNamespacedPath(null));
console.log("pnull", path.posix.toNamespacedPath(null));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    for line in ["posix /a/b /a/b", "win32 \\\\?\\C:\\a\\b null", "pnull null"] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_path_trailing_sep_boundary() {
    // 边界（真机 26.8.2 逐字节对码）：多尾分隔符全剥、后缀整吞回退、
    // UNC 设备前导双条保留、`//a` dirname 保 `//`、`..` 无 ext。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import path from "node:path";
const out = [
  path.win32.basename("basename.ext\\\\"),
  path.posix.basename("basename.ext//"),
  path.posix.basename("aaa/bbb//", "bbb"),
  path.posix.basename("a", "a"),
  path.win32.basename("aaa\\bbb\\\\", "bbb"),
  path.win32.dirname("\\\\unc\\share"),
  path.win32.dirname("\\\\unc\\share\\foo"),
  path.posix.dirname("//a"),
  path.posix.dirname("////"),
  path.posix.extname("/path/to/.."),
  path.win32.extname("C:\\path\\to\\.."),
  path.win32.dirname("/a/b/"),
  path.win32.dirname("/"),
];
console.log(JSON.stringify(out));
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "[\"basename.ext\",\"basename.ext\",\"bbb\",\"\",\"bbb\",\"\\\\\\\\unc\\\\share\",\"\\\\\\\\unc\\\\share\\\\\",\"//\",\"/\",\"\",\"\",\"/a\",\"/\"]\n",
    );
    dir.close().unwrap();
}

#[test]
fn phase10f_path_posix_port() {
    // posix 直译真值表（真机 26.8.2 逐字节对码；resolve-cwd 走动态比对）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import path from "node:path";
const p = path.posix;
const out = [
  p.join("./"), p.join(".", "./"), p.join("", "."), p.join("", "foo"),
  p.join("a", "b", "..", "c"), p.join("/a", "/b"),
  p.normalize("a//b/./c/"), p.normalize("./"), p.normalize(""),
  p.normalize("a/../../b"), p.normalize("/a/../../b"),
  p.resolve("/a", "b"), p.resolve("/a", "/b"),
  p.resolve("") === process.cwd(), p.relative("/a/b/c", "/a/d"),
  p.relative("/", "/foo"), p.relative("/a", "/a"),
  JSON.stringify(p.parse("/home/user/dir/file.txt")),
  JSON.stringify(p.parse("file")), JSON.stringify(p.parse("..")),
  p.format({ name: "x", ext: "png" }), p.format({ dir: "some/dir" }),
];
console.log(JSON.stringify(out));
for (const v of [null, undefined, 1, true, false, "str"]) {
  try { p.format(v); console.log("NO-THROW"); }
  catch (e) { console.log("THROW", e.name, e.code, JSON.stringify(e.message)); }
}
try { p.parse(null); console.log("NO-THROW parse"); }
catch (e) { console.log("THROW", e.name, e.code); }
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "[\"./\",\"./\",\".\",\"foo\",\"a/c\",\"/a/b\",\"a/b/c/\",\"./\",\".\",\"../b\",\"/b\",\"/a/b\",\"/b\",true,\"../../d\",\"foo\",\"\",\"{\\\"root\\\":\\\"/\\\",\\\"dir\\\":\\\"/home/user/dir\\\",\\\"base\\\":\\\"file.txt\\\",\\\"ext\\\":\\\".txt\\\",\\\"name\\\":\\\"file\\\"}\",\"{\\\"root\\\":\\\"\\\",\\\"dir\\\":\\\"\\\",\\\"base\\\":\\\"file\\\",\\\"ext\\\":\\\"\\\",\\\"name\\\":\\\"file\\\"}\",\"{\\\"root\\\":\\\"\\\",\\\"dir\\\":\\\"\\\",\\\"base\\\":\\\"..\\\",\\\"ext\\\":\\\"\\\",\\\"name\\\":\\\"..\\\"}\",\"x.png\",\"some/dir/\"]\n".to_string()
            + "THROW TypeError ERR_INVALID_ARG_TYPE \"The \\\"pathObject\\\" argument must be of type object. Received null\"\n"
            + "THROW TypeError ERR_INVALID_ARG_TYPE \"The \\\"pathObject\\\" argument must be of type object. Received undefined\"\n"
            + "THROW TypeError ERR_INVALID_ARG_TYPE \"The \\\"pathObject\\\" argument must be of type object. Received type number (1)\"\n"
            + "THROW TypeError ERR_INVALID_ARG_TYPE \"The \\\"pathObject\\\" argument must be of type object. Received type boolean (true)\"\n"
            + "THROW TypeError ERR_INVALID_ARG_TYPE \"The \\\"pathObject\\\" argument must be of type object. Received type boolean (false)\"\n"
            + "THROW TypeError ERR_INVALID_ARG_TYPE \"The \\\"pathObject\\\" argument must be of type object. Received type string ('str')\"\n"
            + "THROW TypeError ERR_INVALID_ARG_TYPE\n",
    );
    dir.close().unwrap();
}

#[test]
fn phase10f_path_win32_port() {
    // win32 直译真值表（真机 26.8.2 逐字节对码；JS 内自比对，22 行全 ok）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "p.mjs",
        r#"
import path from "node:path";
const w = path.win32;
const eq = (a, b) => console.log(a === b ? "ok" : "FAIL " + JSON.stringify(a));
eq(w.join(".\\"), ".\\");
eq(w.join(".", ".\\"), ".\\");
eq(w.join("", "."), ".");
eq(w.join("C:\\a", "b"), "C:\\a\\b");
eq(w.join("//server", "share"), "\\\\server\\share\\");
eq(w.normalize("C:\\a\\.\\b\\"), "C:\\a\\b\\");
eq(w.normalize("C:..\\abc"), "C:..\\abc");
eq(w.normalize(""), ".");
eq(w.resolve("c:/ignore", "C:/a/b"), "C:\\a\\b");
eq(w.resolve("C:\\a", "b"), "C:\\a\\b");
eq(w.relative("c:/AaAa/bbbb", "c:/aaaa/bbbb"), "");
eq(w.relative("C:\\orandea\\test\\aaa", "C:\\orandea\\impl\\bbb"), "..\\..\\impl\\bbb");
eq(w.relative("\\\\foo\\bar", "\\\\foo\\bar\\baz"), "baz");
eq(JSON.stringify(w.parse("C:\\path\\dir\\index.html")), "{\"root\":\"C:\\\\\",\"dir\":\"C:\\\\path\\\\dir\",\"base\":\"index.html\",\"ext\":\".html\",\"name\":\"index\"}");
eq(JSON.stringify(w.parse("C:")), "{\"root\":\"C:\",\"dir\":\"C:\",\"base\":\"\",\"ext\":\"\",\"name\":\"\"}");
eq(w.format({ dir: "some\\dir" }), "some\\dir\\");
eq(w.format({ root: "C:\\" }), "C:\\");
eq(w.toNamespacedPath("C:\\a\\b"), "\\\\?\\C:\\a\\b");
for (const [m, a] of [["parse", [null]], ["format", [""]], ["join", [1]], ["resolve", [null]]]) {
  try { w[m](...a); console.log("NO-THROW"); }
  catch (e) { console.log(e.name === "TypeError" && e.code === "ERR_INVALID_ARG_TYPE" ? "ok" : "FAIL " + e.name + " " + e.code); }
}
"#,
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "ok\n".repeat(22),);
    dir.close().unwrap();
}
