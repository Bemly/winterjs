//! tests/node/url.rs — 对齐 src/builtins/node/url.rs（node:url）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase9j_url_file_convert() {
    // 真机逐项对过（node 26.8.2）：往返/编解码/三码三文案。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("u.mjs");
    file.write_str(
        r#"
import { URL as U, URLSearchParams as USP, fileURLToPath, pathToFileURL } from "node:url";
console.log("u-re", U === globalThis.URL && USP === globalThis.URLSearchParams);
console.log("u-f2p", fileURLToPath("file:///a/b%20c"));
console.log("u-f2purl", fileURLToPath(new URL("file:///x/y")));
console.log("u-p2f", pathToFileURL("/a/b c").href);
try { fileURLToPath(42); } catch (e) { console.log("u-t", e.code, e.message); }
try { fileURLToPath("https://x/y"); } catch (e) { console.log("u-s", e.code, e.message); }
try { fileURLToPath("/a/b"); } catch (e) { console.log("u-i", e.code, e.message); }
try { pathToFileURL(42); } catch (e) { console.log("u-pt", e.code, e.message); }
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
    for line in [
        "u-re true",
        "u-f2p /a/b c",
        "u-f2purl /x/y",
        "u-p2f file:///a/b%20c",
        "u-t ERR_INVALID_ARG_TYPE The \"path\" argument must be of type string or an instance of URL. Received type number (42)",
        "u-s ERR_INVALID_URL_SCHEME The URL must be of scheme file",
        "u-i ERR_INVALID_URL Invalid URL",
        "u-pt ERR_INVALID_ARG_TYPE The \"path\" argument must be of type string. Received type number (42)",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
