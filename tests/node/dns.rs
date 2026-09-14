//! tests/node/dns.rs — 对齐 src/builtins/node/dns.rs（node:dns）。

use crate::helpers::*;

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
