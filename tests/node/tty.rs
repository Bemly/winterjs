//! tests/node/tty.rs — 对齐 src/builtins/node/tty.rs（node:tty）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9a_tty_thin_surface() {
    // test-tty* 命名子集。10c-1 起构造器真机口径：fd 非 TTY 即 ERR_TTY_INIT_FAILED
    //（hermetic 管道下 0/1/2 全非 TTY，构造必抛；真 TTY 路径经 pty 手工验）。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("y.mjs");
    file.write_str(
        r#"import tty, { isatty, ReadStream, WriteStream, getColorDepth } from "node:tty";
import net from "node:net";
console.log("isatty", isatty(1), isatty(0), isatty(-1), isatty(1.5), isatty("1"), isatty(99));
try { new ReadStream(0); } catch (e) { console.log("rs-throw", e.code); }
try { new WriteStream(1); } catch (e) { console.log("ws-throw", e.code); }
try { new WriteStream(99); } catch (e) { console.log("ws99-throw", e.code); }
console.log("base", Object.getPrototypeOf(ReadStream.prototype) === net.Socket.prototype);
console.log("static", ReadStream.isatty === isatty, WriteStream.isatty(2));
// 形状探针（Object.create 绕构造，直测方法面；真 TTY 下字段见 pty 手工验）：
const rs = Object.create(ReadStream.prototype);
rs.fd = 0; rs.isRaw = false; rs.rawMode = false;
try { rs.setRawMode("bogus"); } catch (e) { console.log("e2", e.code); }
const ws = Object.create(WriteStream.prototype);
ws.fd = 1; ws._writable = null;
ws._refreshSize = WriteStream.prototype._refreshSize;
ws._refreshSize();
console.log("size", ws.columns, ws.rows, JSON.stringify(ws.getWindowSize()));
console.log("ctl", ws.clearLine(0), ws.clearScreenDown(), ws.cursorTo(5), ws.cursorTo(5, 6), ws.moveCursor(1, 0));
console.log("depth", [1, 8, 24].includes(getColorDepth()), getColorDepth({ isTTY: false }) === 1);
try { new WriteStream(-5); } catch (e) { console.log("e1", e.code, e.constructor.name); }
"#,
    )
    .unwrap();
    let out = winterjs2().args(["--run", file.path().to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = String::from_utf8(out.stdout).unwrap();
    assert!(out.contains("isatty false false false false false false"), "out: {out}");
    assert!(out.contains("rs-throw ERR_TTY_INIT_FAILED"), "out: {out}");
    assert!(out.contains("ws-throw ERR_TTY_INIT_FAILED"), "out: {out}");
    assert!(out.contains("ws99-throw ERR_TTY_INIT_FAILED"), "out: {out}");
    assert!(out.contains("base true"), "out: {out}");
    assert!(out.contains("static true"), "out: {out}");
    assert!(out.contains("e2 ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("size undefined undefined [null,null]"), "out: {out}");
    assert!(out.contains("ctl true true true true true"), "out: {out}");
    assert!(out.contains("depth true true"), "out: {out}");
    assert!(out.contains("e1 ERR_INVALID_FD RangeError"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9b-1：node:buffer 模块面 + 全局 Blob ──────────────────────────────
