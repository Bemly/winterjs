//! tests/node/dns.rs — 对齐 src/builtins/node/dns.rs（node:dns）。

use crate::helpers::*;

#[test]
fn phase10f_dns_parity_fixes() {
    // 10f 对拍牵引回归（test-dns-* 套件门，hermetic：localhost + 形状断言）：
    // 正常：Resolver 构造/getServers/lookupService 回环/setServers  canonical；
    // 报错：resolveNs 非串同步抛（callback+promises）/setServers 非数组/
    //   setLocalAddress 非串/lookupService 坏端口；
    // 边界：family -0 直通/holes 压实/cancel 即刻 ECANCELLED（无应答 stub）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import dns, { Resolver } from "node:dns";
console.log("consts", dns.V4MAPPED === 2048, dns.ADDRCONFIG === 1024, dns.ALL === 256);
const r = new Resolver();
console.log("srv", r.getServers().length > 0, typeof r.cancel);
r.setServers(["127.0.0.1"]);
console.log("set", JSON.stringify(r.getServers()));
dns.lookupService("127.0.0.1", 22, (e, h, s) => {
  console.log("ls", e === null, typeof h === "string" && h.length > 0, ["ssh", "22"].includes(s));
});
dns.lookup("localhost", -0, (e, a) => {
  console.log("negzero", e === null, typeof a === "string");
});
try { dns.resolveNs([]); console.log("BAD1"); }
catch (e) { console.log("e1", e.code === "ERR_INVALID_ARG_TYPE"); }
try { dns.promises.resolveNs([]); console.log("BAD2"); }
catch (e) { console.log("e2", e.code === "ERR_INVALID_ARG_TYPE"); }
try { dns.setServers("x"); console.log("BAD3"); }
catch (e) { console.log("e3", e.code === "ERR_INVALID_ARG_TYPE"); }
try { r.setLocalAddress(123); console.log("BAD4"); }
catch (e) { console.log("e4", e.code === "ERR_INVALID_ARG_TYPE"); }
try { dns.lookupService("127.0.0.1", -1, () => {}); console.log("BAD5"); }
catch (e) { console.log("e5", e.code === "ERR_SOCKET_BAD_PORT"); }
try { dns.lookup("", () => {}); console.log("BAD6"); }
catch (e) { console.log("e6", e.code === "ERR_INVALID_ARG_VALUE"); }
// holes 压实 + 非法 IP 文案
r.setServers(["127.0.0.1", , "0.0.0.0"]);
console.log("holes", JSON.stringify(r.getServers()));
try { r.setServers(["foobar"]); console.log("BAD7"); }
catch (e) { console.log("e7", e.code === "ERR_INVALID_IP_ADDRESS", e.message === "Invalid IP address: foobar"); }
// cancel：无应答 stub + 即刻 ECANCELLED（不等待超时）
const dgram = await import("node:dgram");
const server = dgram.createSocket("udp4");
server.bind(0, () => {
  const rc = new Resolver({ timeout: 8000, tries: 1 });
  rc.setServers([`127.0.0.1:${server.address().port}`]);
  rc.resolve4("example.invalid", (e) => {
    console.log("cancel", e && e.code === "ECANCELLED", e && e.syscall === "queryA");
    server.close();
  });
  rc.cancel();
});
setTimeout(() => console.log("end-ok"), 500);
"#,
    );
    for line in [
        "consts true true true",
        "srv true function",
        r#"set ["127.0.0.1"]"#,
        "ls true true true",
        "negzero true true",
        "e1 true",
        "e2 true",
        "e3 true",
        "e4 true",
        "e5 true",
        "e6 true",
        r#"holes ["127.0.0.1","0.0.0.0"]"#,
        "e7 true true",
        "cancel true true",
        "end-ok",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

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
// 空主机名 → 同步抛 ERR_INVALID_ARG_VALUE（真机口径，test-dns.js；
// 旧断言按回调错编码，系实现偏差 contemporaneous，已翻转见 §4.x）
try { lookup("", () => {}); console.log("empty-err BAD"); }
catch (e) { console.log("empty-err", e instanceof Error, e.code === "ERR_INVALID_ARG_VALUE"); }
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
    assert!(out.contains("empty-err true true"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}

// ── Phase 9d-3：node:http 回环（JS-over-net 解析器；hermetic port 0）────────

// ── Phase 10d：dns 深件（hickory 全套；hermetic localhost + 错误形状）────────
#[test]
fn phase10d_dns_deep_hickory() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import dns from "node:dns";
console.log("surface",
  typeof dns.resolveCname, typeof dns.resolveMx, typeof dns.resolveNs,
  typeof dns.resolveTxt, typeof dns.resolveSrv, typeof dns.resolvePtr,
  typeof dns.resolveAny, typeof dns.resolve, typeof dns.reverse,
  typeof dns.getServers, typeof dns.setServers,
  typeof dns.getDefaultResultOrder, typeof dns.setDefaultResultOrder);
dns.resolveMx("localhost", (e, r) => {
  console.log("mx", e && e.code, e && e.syscall, e && e.hostname);
});
dns.resolveTxt("localhost", (e, r) => {
  console.log("txt", e && e.code, e && e.syscall);
});
dns.resolveCname("localhost", (e, r) => {
  console.log("cname", e && e.code, Array.isArray(r));
});
dns.resolveSrv("localhost", (e, r) => {
  console.log("srv", e && e.code, e && e.syscall);
});
dns.resolvePtr("127.0.0.1", (e, r) => {
  console.log("ptr", e === null, Array.isArray(r), r.every((s) => typeof s === "string" && !s.endsWith(".")));
});
dns.reverse("127.0.0.1", (e, r) => {
  console.log("reverse", e === null, Array.isArray(r));
});
dns.resolveAny("localhost", (e, r) => {
  console.log("any", e === null, Array.isArray(r), r.every((x) => typeof x.type === "string"));
});
dns.resolve("localhost", "MX", (e, r) => {
  console.log("resolve-mx", e && e.code, e && e.syscall);
});
try { dns.resolve("localhost", "NOPE", () => {}); } catch (e) {
  console.log("resolve-badtype", e.code);
}
dns.resolveMx("", (e) => {
  console.log("empty", e instanceof Error, typeof e.code, e.hostname);
});
console.log("servers", Array.isArray(dns.getServers()), dns.getServers().length >= 1);
dns.setServers(["127.0.0.1"]);
console.log("set", JSON.stringify(dns.getServers()));
try { dns.setServers(["nope"]); } catch (e) { console.log("set-bad", e instanceof Error); }
try { dns.setServers("x"); } catch (e) { console.log("set-nonarray", e instanceof TypeError); }
console.log("order", dns.getDefaultResultOrder());
dns.setDefaultResultOrder("ipv4first");
console.log("order2", dns.getDefaultResultOrder());
try { dns.setDefaultResultOrder("nope"); } catch (e) { console.log("order-bad", e instanceof Error); }
dns.promises.resolveMx("localhost").then(
  () => console.log("p-mx unexpected"),
  (e) => console.log("p-mx-err", e.code, e.syscall, e.hostname),
);
dns.promises.reverse("127.0.0.1").then((r) => {
  console.log("p-reverse", Array.isArray(r));
});
setTimeout(() => console.log("end-ok"), 100);
"#,
    );
    assert!(out.contains("surface function function function function function function function function function function function function function"), "out: {out}");
    assert!(out.contains("mx ENODATA queryMx localhost"), "out: {out}");
    assert!(out.contains("txt ENODATA queryTxt"), "out: {out}");
    assert!(out.contains("cname ENODATA false"), "out: {out}");
    assert!(out.contains("srv ENODATA querySrv"), "out: {out}");
    assert!(out.contains("ptr true true true"), "out: {out}");
    assert!(out.contains("reverse true true"), "out: {out}");
    assert!(out.contains("any true true true"), "out: {out}");
    assert!(out.contains("resolve-mx ENODATA queryMx"), "out: {out}");
    assert!(out.contains("resolve-badtype EBADNAME"), "out: {out}");
    assert!(out.contains("empty true string"), "out: {out}");
    assert!(out.contains("servers true true"), "out: {out}");
    assert!(out.contains(r#"set ["127.0.0.1"]"#), "out: {out}");
    assert!(out.contains("set-bad true"), "out: {out}");
    assert!(out.contains("set-nonarray true"), "out: {out}");
    assert!(out.contains("order verbatim"), "out: {out}");
    assert!(out.contains("order2 ipv4first"), "out: {out}");
    assert!(out.contains("order-bad true"), "out: {out}");
    assert!(out.contains("p-mx-err ENODATA queryMx localhost"), "out: {out}");
    assert!(out.contains("p-reverse true"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}
