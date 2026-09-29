//! tests/node/url.rs — 对齐 src/builtins/node/url.rs（node:url）。

use crate::common::*;
use crate::helpers::*;
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
    let out = winterjs2()
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
    let out = winterjs2()
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
    let out = winterjs2()
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

#[test]
fn phase11_urlpattern_surface() {
    // URLPattern 构造/属性/test/exec（plan3 §5 专项；真机 node 26.8.2 逐项对拍）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "up.mjs",
        r##"
import { URLPattern } from "node:url";
const show = (l, v) => console.log(l, JSON.stringify(v));
// 构造三形 + 缺省
const a = new URLPattern({ pathname: "/foo/:id" });
show("a-proto", a.protocol); show("a-path", a.pathname); show("a-host", a.hostname);
show("empty", [new URLPattern().protocol, new URLPattern().hostname, new URLPattern().pathname]);
show("str", [new URLPattern("https://ex.com/foo/*").hostname, new URLPattern("https://ex.com/foo/*").pathname]);
show("base", [new URLPattern("/foo/:id", "https://ex.com").hostname, new URLPattern("/foo/:id", "https://ex.com").pathname]);
// 全局与模块同构
show("same", URLPattern === globalThis.URLPattern);
// exec 命中/未命中 + groups + inputs 键序
const r = new URLPattern({ pathname: "/:value" }).exec("https://example.com/test");
show("keys", Object.keys(r)); show("compkeys", Object.keys(r.pathname));
show("vals", [r.hostname.input, r.pathname.input, r.pathname.groups.value]);
show("miss", a.exec("https://other.com/bar"));
show("hit", [a.test("https://ex.com/foo/1"), a.test("https://ex.com/nope")]);
// 可选组缺席：键在、值为 undefined（真机口径）
const g = new URLPattern({ pathname: "/foo/:id/:opt?" }).exec("https://ex.com/foo/1").pathname.groups;
show("opt", [Object.keys(g).sort(), "opt" in g, String(g.opt), g.id]);
// 匿名组 + 规范化 + hasRegExpGroups + search/hash
show("anon", new URLPattern({ hostname: "{*.}ex.com" }).exec("https://mail.ex.com/").hostname.groups);
show("norm", [new URLPattern({ protocol: "https:" }).protocol, new URLPattern({ username: ":u" }).username]);
show("reg", [new URLPattern({ pathname: "/foo/(bar|baz)" }).hasRegExpGroups, new URLPattern({ pathname: "/foo/:id" }).hasRegExpGroups]);
const f = new URLPattern({ search: "?q=:q", hash: "#frag" });
show("sh", [f.search, f.hash, f.exec("https://ex.com/?q=1#frag").search.groups]);
show("inputs", new URLPattern({ pathname: "/foo/:id" }).exec("https://ex.com/foo/1?x=2#h").inputs);
show("dict-in", new URLPattern({ pathname: "/foo/:id" }).exec({ pathname: "/foo/9" }).pathname.groups);
"##,
    );
    let out = String::from_utf8_lossy(&out.stdout).into_owned();
    for line in [
        r#"a-proto "*""#,
        r#"a-path "/foo/:id""#,
        r#"a-host "*""#,
        r#"empty ["*","*","*"]"#,
        r#"str ["ex.com","/foo/*"]"#,
        r#"base ["ex.com","/foo/:id"]"#,
        r#"same true"#,
        r#"keys ["hash","hostname","inputs","password","pathname","port","protocol","search","username"]"#,
        r#"compkeys ["groups","input"]"#,
        r#"vals ["example.com","/test","test"]"#,
        r#"miss null"#,
        r#"hit [true,false]"#,
        r#"opt [["id","opt"],true,"undefined","1"]"#,
        r#"anon {"0":"mail"}"#,
        r#"norm ["https",":u"]"#,
        r#"reg [true,false]"#,
        r#"sh ["q=:q","frag",{"q":"1"}]"#,
        r#"inputs ["https://ex.com/foo/1?x=2#h"]"#,
        r#"dict-in {"id":"9"}"#,
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase11_urlpattern_errors_boundary() {
    // 错误矩阵（test-urlpattern-types/invalidthis 全断言 + getter 透传 + 组序偏离钉档）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_node_file(
        &dir,
        "upe.mjs",
        r##"
import { URLPattern } from "node:url";
const show = (l, f) => { try { const r = f(); console.log(l, "OK", JSON.stringify(r)); } catch (e) { console.log(l, "THROW", e.name, e.code, JSON.stringify(e.message)); } };
show("no-new", () => URLPattern());
show("num", () => new URLPattern(1));
show("opts-num", () => new URLPattern({}, 1));
show("base3-num", () => new URLPattern({}, "", 1));
show("exec-num", () => new URLPattern().exec(1));
show("exec-base-num", () => new URLPattern().exec("", 1));
show("test-num", () => new URLPattern().test(1));
show("3null", () => new URLPattern("https://example.com", null, null));
show("dict-base", () => new URLPattern({}, "https://ex.com"));
show("dict-nullbase", () => new URLPattern().test(null, null));
show("getter", () => new URLPattern({ get protocol() { throw new Error("boom"); } }));
show("ignorecase", () => { const p = new URLPattern({}, { ignoreCase: "" }); return p.protocol; });
const proto = Object.getPrototypeOf(new URLPattern());
show("brand-get", () => Object.getOwnPropertyDescriptor(proto, "protocol").get.call({}));
const { test, exec } = new URLPattern();
show("brand-test", () => test({}));
show("brand-exec", () => exec({}));
"##,
    );
    let out = String::from_utf8_lossy(&out.stdout).into_owned();
    for line in [
        "no-new THROW TypeError ERR_CONSTRUCT_CALL_REQUIRED \"Cannot call constructor without `new`\"",
        "num THROW TypeError ERR_INVALID_ARG_TYPE \"Input must be an object or a string\"",
        "opts-num THROW TypeError ERR_INVALID_ARG_TYPE \"second argument must be a string or object\"",
        "base3-num THROW TypeError ERR_INVALID_ARG_TYPE \"options must be an object\"",
        "exec-num THROW TypeError ERR_INVALID_ARG_TYPE \"URLPattern input needs to be a string or an object\"",
        "exec-base-num THROW TypeError ERR_INVALID_ARG_TYPE \"baseURL must be a string\"",
        "test-num THROW TypeError ERR_INVALID_ARG_TYPE \"URLPattern input needs to be a string or an object\"",
        "3null THROW TypeError ERR_INVALID_URL_PATTERN \"Failed to construct URLPattern\"",
        "dict-base THROW TypeError ERR_INVALID_URL_PATTERN \"Failed to construct URLPattern\"",
        "dict-nullbase THROW TypeError ERR_OPERATION_FAILED \"Failed to test URLPattern\"",
        "getter THROW Error undefined \"boom\"",
        "ignorecase OK \"*\"",
        "brand-get THROW TypeError undefined \"Illegal invocation\"",
        "brand-test THROW TypeError undefined \"Illegal invocation\"",
        "brand-exec THROW TypeError undefined \"Illegal invocation\"",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
