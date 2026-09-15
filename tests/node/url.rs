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

#[test]
fn phase10a_url_legacy() {
    // 10a：legacy 面（lib/url.js 口径移植）——正常 + 报错 + 边界。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("l.mjs");
    file.write_str(
        r##"
import url, { parse, format, resolve, resolveObject, Url, domainToASCII, domainToUnicode, urlToHttpOptions } from "node:url";
// parse 正常
const p = parse("https://user:pass@example.com:8080/a/b?x=1&y=2#frag");
console.log("p-proto", p.protocol, p.slashes, p.auth, p.host, p.port, p.hostname);
console.log("p-tail", p.hash, p.search, p.pathname, p.path, p.href);
console.log("p-qobj", JSON.stringify(parse("http://h/?a=1&b=2", true).query));
console.log("p-ipv6", parse("http://[::1]:3000/x").hostname);
console.log("p-auth", parse("http://a%20b:c@h/").auth);
try { parse(42); } catch (e) { console.log("p-t", e.code); }
console.log("p-def", url.parse("http://h/a").href === parse("http://h/a").href);
const m = parse("mailto:foo@bar.com");
console.log("p-mailto", m.auth, m.host, m.hostname, m.pathname, m.path);
const e = parse("http://example.com");
console.log("p-empty", e.pathname, e.path, e.href);
console.log("p-rt6", format(parse("http://[::1]:3000/x")));
console.log("p-fmt6", format({ protocol: "http:", slashes: true, hostname: "::1", port: "3000", pathname: "/x" }));
// format 正常
console.log("f-full", format({ protocol: "https:", slashes: true, auth: "u:p", hostname: "h.com", port: "8443", pathname: "/a", search: "?x=1", hash: "f" }));
console.log("f-qobj", format({ pathname: "/s", query: { a: "1", b: "2" } }));
console.log("f-str", format("http://h/a?x=1"));
try { format(42); } catch (e) { console.log("f-t", e.code); }
// resolve 电池（Node test-url-resolve.js 子集口径）
const R = [
  ["http://a/b/c/d;p?q", "g", "http://a/b/c/g"],
  ["http://a/b/c/d;p?q", "./g", "http://a/b/c/g"],
  ["http://a/b/c/d;p?q", "../g", "http://a/b/g"],
  ["http://a/b/c/d;p?q", "../../g", "http://a/g"],
  ["http://a/b/c/d;p?q", "/g", "http://a/g"],
  ["http://a/b/c/d;p?q", "//h/g", "http://h/g"],
  ["http://a/b/c/d;p?q", "?y", "http://a/b/c/d;p?y"],
  ["http://a/b/c/d;p?q", "#s", "http://a/b/c/d;p?q#s"],
  ["http://a/b/c/d;p?q", "", "http://a/b/c/d;p?q"],
  ["http://a/b/c/g", ".", "http://a/b/c/"],
  ["http://a/b/c/g", "..", "http://a/b/"],
  ["foo:a/b", "c", "foo:a/c"],
];
let rok = true;
for (const [from, to, want] of R) {
  const got = resolve(from, to);
  if (got !== want) { rok = false; console.log("r-miss", from, to, got, want); }
}
console.log("r-all", rok);
console.log("r-obj", resolveObject("http://a/b/c", "../d").href);
// Url 类
const u = new Url();
u.parse("http://h:80/a?x=1");
console.log("c-props", u.hostname, u.port, u.path, u.href);
console.log("c-fmt", u.format());
console.log("c-res", u.resolve("b"));
// domainTo* / urlToHttpOptions
console.log("d-a", domainToASCII("münchen.de"), domainToUnicode("xn--mnchen-3ya.de"));
try { domainToASCII(42); } catch (e) { console.log("d-t", e.code); }
const o = urlToHttpOptions(new URL("https://user:pw@h.com:8443/a?x=1#f"));
console.log("o-opts", o.protocol, o.hostname, o.port, o.path, o.auth, o.hash);
const o2 = urlToHttpOptions(new URL("http://[::1]:8080/"));
console.log("o-ipv6", o2.hostname, o2.port);
const o3 = urlToHttpOptions(parse("http://h:80/a?x=1"));
console.log("o-leg", o3.hostname, o3.port, o3.path);
try { urlToHttpOptions("https://h.com/a"); } catch (e) { console.log("o-str", e.code); }
try { urlToHttpOptions(42); } catch (e) { console.log("o-t", e.code, e.message); }
"##,
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
        "p-proto https: true user:pass example.com:8080 8080 example.com",
        "p-tail #frag ?x=1&y=2 /a/b /a/b?x=1&y=2 https://user:pass@example.com:8080/a/b?x=1&y=2#frag",
        "p-qobj {\"a\":\"1\",\"b\":\"2\"}",
        "p-ipv6 ::1",
        "p-auth a b:c",
        "p-t ERR_INVALID_ARG_TYPE",
        "p-def true",
        "p-mailto foo bar.com bar.com null null",
        "p-empty / / http://example.com/",
        "p-rt6 http://[::1]:3000/x",
        "p-fmt6 http://[::1]:3000/x",
        "f-full https://u:p@h.com:8443/a?x=1#f",
        "f-qobj /s?a=1&b=2",
        "f-str http://h/a?x=1",
        "f-t ERR_INVALID_ARG_TYPE",
        "r-all true",
        "r-obj http://a/d",
        "c-props h 80 /a?x=1 http://h:80/a?x=1",
        "c-fmt http://h:80/a?x=1",
        "c-res http://h:80/b",
        "d-a xn--mnchen-3ya.de münchen.de",
        "d-t ERR_INVALID_ARG_TYPE",
        "o-opts https: h.com 8443 /a?x=1 user:pw #f",
        "o-ipv6 ::1 8080",
        "o-leg h 80 /a?x=1",
        "o-str ERR_INVALID_ARG_TYPE",
        "o-t ERR_INVALID_ARG_TYPE The \"url\" argument must be of type object. Received type number (42)",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_url_parity_suite() {
    // 10f 对拍定案面（node lib/url.js 逐字口径）：parse 校验族（消息逐字/URIError
    // 无码/ERR_INVALID_URL+input/IDNA NFKC/软连字符/evil 端口）、resolveObject
    // 空源短路 + 非斜杠协议爬升、format auth 表（noEscapeAuth/代理对）、
    // pathToFileURL windows 全套（UNC/盘符/^~[] 编码）+ posix 尾分隔符与
    // %5C 合法性 + %2F 抛、DEP0169 异步 once、autoEscape 表。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"
import url, { parse, format, resolve, resolveObject, pathToFileURL, fileURLToPath } from "node:url";
const L = [];
const chk = (tag, got, want) => L.push(`${tag} ${got === want}`);
try { parse(undefined); } catch (e) { chk("v-undef", e.message, 'The "url" argument must be of type string. Received undefined'); }
try { parse([1]); } catch (e) { chk("v-arr", e.message, 'The "url" argument must be of type string. Received an instance of Array'); }
try { parse("http://%E0%A4%A@fail"); } catch (e) { chk("v-uri", e instanceof URIError && e.code === undefined, true); }
try { parse("http://[127.0.0.1\x00c8763]:8000/"); } catch (e) { chk("v-ipv6", e.code === "ERR_INVALID_URL" && e.input === "http://[127.0.0.1\x00c8763]:8000/", true); }
try { parse("http://fail\u2100fail.com/"); } catch (e) { chk("v-idna", e.code === "ERR_INVALID_URL", true); }
try { parse("http://\u00AD/bad.com/"); } catch (e) { chk("v-shy", e.code === "ERR_INVALID_URL", true); }
try { parse("https://evil.com:.example.com"); } catch (e) { chk("v-evil", e.code === "ERR_INVALID_ARG_VALUE", true); }
chk("ro-empty", resolveObject("", "foo"), "foo");
chk("ro-type", typeof resolveObject(null, "http://a/b"), "string");
chk("r-crawl", resolve("foo:a/b", "../c"), "foo:c");
chk("r-https1", resolve("http://example.com/b//c//d;p?q#blarg", "https:/p/a/t/h?s#hash2"), "https://p/a/t/h?s#hash2");
chk("f-auth", format("http://atpass:foo%40bar@127.0.0.1/"), "http://atpass:foo%40bar@127.0.0.1/");
chk("f-emoji", format("http://%F0%9F%98%80@www.example.com/"), "http://%F0%9F%98%80@www.example.com/");
chk("p-unc", pathToFileURL("\\\\host\\share\\file.txt", { windows: true }).href, "file://host/share/file.txt");
chk("p-drive", pathToFileURL("C:\\foo", { windows: true }).href, "file:///C:/foo");
chk("p-caret", pathToFileURL("C:\\foo^bar", { windows: true }).href, "file:///C:/foo%5Ebar");
chk("p-tilde", pathToFileURL("/foo~").href, "file:///foo%7E");
chk("p-trail", pathToFileURL("/tmp/a/").href, "file:///tmp/a/");
chk("p-bslash", pathToFileURL("/foo\\bar").href, "file:///foo%5Cbar");
try { fileURLToPath("file:///a%2F/"); } catch (e) { chk("p-f2p", e.code === "ERR_INVALID_FILE_URL_PATH" && e.input instanceof URL && e.input.href === "file:///a%2F/", true); }
chk("p-f2p5c", fileURLToPath("file:///foo%5Cbar"), "/foo\\bar");
const warns = [];
process.on("warning", (w) => warns.push(w.code + "|" + w.message.slice(0, 13)));
url.parse("foo"); url.parse("bar");
await new Promise((r) => setTimeout(r, 30));
// node 口径：emitWarning nextTick 异步 + 每进程一次（先 parse 后挂监听仍可收）。
chk("w-dep", warns.length === 1 && warns[0] === "DEP0169|`url.parse()`", true);
chk("n-puny", parse("http://example.Bücher.com/").hostname, "example.xn--bcher-kva.com");
chk("a-esc", parse("http://x:1/' <>\"`/{}|\\^~`/").pathname, "/%27%20%3C%3E%22%60/%7B%7D%7C/%5E~%60/");
console.log(L.join("\n"));
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
        "v-undef true",
        "v-arr true",
        "v-uri true",
        "v-ipv6 true",
        "v-idna true",
        "v-shy true",
        "v-evil true",
        "ro-empty true",
        "ro-type true",
        "r-crawl true",
        "r-https1 true",
        "f-auth true",
        "f-emoji true",
        "p-unc true",
        "p-drive true",
        "p-caret true",
        "p-tilde true",
        "p-trail true",
        "p-bslash true",
        "p-f2p true",
        "p-f2p5c true",
        "w-dep true",
        "n-puny true",
        "a-esc true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
