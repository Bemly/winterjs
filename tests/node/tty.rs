//! tests/node/tty.rs — 对齐 src/builtins/node/tty.rs（node:tty）。

use crate::common::*;
use assert_fs::prelude::*;

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
