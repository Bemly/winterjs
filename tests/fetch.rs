//! fetch/streams/abort 黑盒测试(对齐 src/builtins/fetch.rs + prelude streams)。

mod common;

use common::*;

use assert_fs::prelude::*;

#[test]
fn phase3_fetch_http_get() {
    let port = serve_http(1, |_head, _body| {
        (200, vec![("x-echo", "yes".into())], b"hello-http".to_vec())
    });
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/p?q=1"); console.log(r.status, r.ok, r.url, await r.text(), r.headers.get("x-echo"));"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
        format!("200 true http://127.0.0.1:{port}/p?q=1 hello-http yes\n")
    );
}

#[test]
fn phase3_fetch_http_post_echo() {
    let port = serve_http(1, |head, body| {
        let ct = head
            .lines()
            .find(|l| l.to_lowercase().starts_with("content-type:"))
            .unwrap_or("")
            .to_owned();
        let mut echo = b"got:".to_vec();
        echo.extend_from_slice(&body);
        (200, vec![("x-ct", ct)], echo)
    });
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/echo", {{method: "POST", body: "a=1&b=2", headers: {{"content-type": "text/plain"}}}}); console.log(r.status, await r.text(), r.headers.get("x-ct"));"#
    );
    let out = stdout_of(&mut winterjs().args(["--eval", &code]));
    assert!(
        out.starts_with("200 got:a=1&b=2 content-type: text/plain"),
        "post: {out}"
    );
}

#[test]
fn phase3_fetch_data_and_file() {
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval",
            r#"const r = await fetch("data:text/plain,hello-fetch"); console.log(r.status, r.ok, await r.text());"#])),
        "200 true hello-fetch\n"
    );
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("f.txt").write_str("hello-file").unwrap();
    let url = format!("file://{}", dir.child("f.txt").path().display());
    let code = format!(r#"console.log(await (await fetch("{url}")).text());"#);
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "hello-file\n"
    );
    dir.close().unwrap();
}

#[test]
fn phase3_fetch_errors_are_rejections() {
    // 不支持的 scheme 与连不上的地址都以 rejection 呈现（catch 可接住）
    let out = stdout_of(&mut winterjs().args([
        "--eval",
        r#"console.log(await fetch("blob:xyz").then(() => "no", () => "blob-err"))"#,
    ]));
    assert_eq!(out, "blob-err\n", "blob: {out}");
    // 保证关闭的端口：bind 后立刻 drop
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let code = format!(
        r#"console.log(await fetch("http://127.0.0.1:{port}/").then(() => "no", (e) => String(e).includes("fetch failed") ? "net-err" : "other:" + e))"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "net-err\n"
    );
}

#[test]
fn phase3_headers_request_response_classes() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const h = new Headers([["X-A", "1"], ["x-a", "2"]]); console.log(h.get("x-a"), [...h.keys()].join(",")); const r = new Response("hi", { status: 201 }); console.log(r.status, r.ok, await r.text()); const q = new Request("https://ex.com/a", { method: "post", body: "x" }); console.log(q.method, q.url, await q.text());"#]));
    assert_eq!(
        out,
        "1, 2 x-a,x-a
201 true hi
POST https://ex.com/a x
",
        "classes: {out}"
    );
}

#[test]
fn phase3_abort_signal_pre_abort() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const c = new AbortController(); c.abort(); console.log(await fetch("http://127.0.0.1:9/x", { signal: c.signal }).then(() => 'no', () => 'abort-ok'));"#]));
    assert_eq!(
        out,
        "abort-ok
",
        "abort: {out}"
    );
}

#[test]
fn phase3_streams_basic() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const rs = new ReadableStream({ start(c) { c.enqueue("a"); c.enqueue("b"); c.close(); } }); const out = []; for await (const x of rs) out.push(x); console.log(out.join(",")); const t = new TransformStream({ transform(c, ctl) { ctl.enqueue(String(c).toUpperCase()); } }); const w = t.writable.getWriter(); w.write("hi"); w.close(); const r = t.readable.getReader(); console.log((await r.read()).value, (await r.read()).done);"#]));
    assert_eq!(out, "a,b\nHI true\n", "streams: {out}");
}

#[test]
fn phase3_streams_pipe_tee_body() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } }); const ts = new TransformStream({ transform(c, ctl) { ctl.enqueue(c + "!"); } }); const out = []; await rs.pipeThrough(ts).pipeTo(new WritableStream({ write(c) { out.push(c); } })); console.log(out.join(",")); const [a, b] = new ReadableStream({ start(c) { c.enqueue(1); c.close(); } }).tee(); console.log(await a.getReader().read().then((x) => x.value), await b.getReader().read().then((x) => x.value)); const r = new Response("stream-me"); console.log(r.body === r.body, (await r.body.getReader().read()).value.length);"#]));
    assert_eq!(out, "x!\n1 1\ntrue 9\n", "pipe: {out}");
}

#[test]
fn phase3_fetch_in_flight_abort() {
    // 5s 才回的服务，50ms abort：拒绝带 AbortError，进程不等 5s（超时即挂）。
    let port = serve_http(1, |_head, _body| {
        std::thread::sleep(std::time::Duration::from_secs(5));
        (200, vec![], b"too-late".to_vec())
    });
    let code = format!(
        r#"const c = new AbortController(); const p = fetch("http://127.0.0.1:{port}/slow", {{ signal: c.signal }}); setTimeout(() => c.abort(), 50); try {{ await p; console.log("no-throw"); }} catch (e) {{ console.log("aborted:" + String(e && e.message || e).includes("AbortError")); }}"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "aborted:true\n"
    );
}

#[test]
fn phase3_fetch_abort_reason_and_late_abort_noop() {
    // 自定义 reason 原样透出；已决议后 abort 不翻转结果。
    let port = serve_http(1, |_head, _body| {
        std::thread::sleep(std::time::Duration::from_secs(5));
        (200, vec![], b"too-late".to_vec())
    });
    let code = format!(
        r#"const c = new AbortController(); const p = fetch("http://127.0.0.1:{port}/slow", {{ signal: c.signal }}); setTimeout(() => c.abort(new Error("custom-stop")), 50); try {{ await p; console.log("no-throw"); }} catch (e) {{ console.log(e.message); }}"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "custom-stop\n"
    );
    let ok = stdout_of(&mut winterjs().args(["--eval",
        r#"const c = new AbortController(); const r = await fetch("data:text/plain,settled", { signal: c.signal }); c.abort(); console.log(await r.text());"#]));
    assert_eq!(ok, "settled\n", "late abort: {ok}");
}

#[test]
fn phase3_fetch_body_streams_chunks() {
    // 首个 read 在第二个半包到达前即返回 "abc"（整包缓冲实现会给出 "abcdef"）。
    let port = serve_split();
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/split"); const rd = r.body.getReader(); const a = await rd.read(); const b = await rd.read(); const c = await rd.read(); console.log(new TextDecoder().decode(a.value), new TextDecoder().decode(b.value), c.done);"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "abc def true\n"
    );
}

#[test]
fn phase3_fetch_body_stream_text_and_cancel() {
    // text() 照常拼装流式 body；读一半 cancel 照常退出。
    let port = serve_split();
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/split"); console.log(await r.text());"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "abcdef\n"
    );
    let port = serve_split();
    let code = format!(
        r#"const r = await fetch("http://127.0.0.1:{port}/split"); const rd = r.body.getReader(); const a = await rd.read(); console.log(new TextDecoder().decode(a.value)); await rd.cancel(); console.log("cancelled");"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "abc\ncancelled\n"
    );
}

#[test]
fn phase3_fetch_body_mid_stream_abort() {
    // 流中 abort：已读 chunk 保留，后继 read 以 AbortError 拒绝（非静默 done）。
    let port = serve_split();
    let code = format!(
        r#"const c = new AbortController(); const r = await fetch("http://127.0.0.1:{port}/split", {{ signal: c.signal }}); const rd = r.body.getReader(); const a = await rd.read(); console.log(new TextDecoder().decode(a.value)); c.abort(); try {{ await rd.read(); console.log("no-throw"); }} catch (e) {{ console.log("stream-aborted:" + String(e.message || e).includes("Abort")); }}"#
    );
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", &code])),
        "abc\nstream-aborted:true\n"
    );
}

#[test]
fn streams_text_decoder_streaming() {
    // 正常：多字节跨片（€ 切两半不提前 FFFD）+ 多片 hello + BOM 跨片 + 空片 flush；
    // 报错：fatal 非法字节；边界：gbk 跨片 + 一次性路径不受影响。
    let code = r#"const d = new TextDecoder();
if (d.decode(new Uint8Array([0xE2]), { stream: true }) !== "") throw new Error("split head must buffer");
if (d.decode(new Uint8Array([0x82, 0xAC]), { stream: true }) !== "€") throw new Error("split tail must emit €");
if (d.decode() !== "") throw new Error("flush must be empty");
const h = new TextDecoder();
const parts = ["hel", "lo, ", "stream"].map(s => new TextEncoder().encode(s));
if (parts.map(p => h.decode(p, { stream: true })).join("") + h.decode() !== "hello, stream") throw new Error("hello split failed");
const b = new TextDecoder();
if (b.decode(new Uint8Array([0xEF]), { stream: true }) !== "") throw new Error("BOM head must buffer");
if (b.decode(new Uint8Array([0xBB, 0xBF, 0x68, 0x69])) !== "hi") throw new Error("BOM split failed");
const g = new TextDecoder("gbk");
if (g.decode(new Uint8Array([0xC4]), { stream: true }) !== "" || g.decode(new Uint8Array([0xE3])) !== "你") throw new Error("gbk split failed");
const f = new TextDecoder("utf-8", { fatal: true });
f.decode(new Uint8Array([0xE2]), { stream: true });
try { f.decode(new Uint8Array([0x28])); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("not valid")) throw e; }
// 一次性路径不变：截断直接 FFFD
if (new TextDecoder().decode(new Uint8Array([0xE2])) !== "�") throw new Error("one-shot changed");
if (new TextEncoder().encode("hi", { stream: true }).length !== 2) throw new Error("encoder stream opt");
console.log("td-stream-ok");
"#;
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", code])),
        "td-stream-ok\n"
    );
}

#[test]
fn streams_abort_events() {
    // 正常：listener 收事件对象（type/target）+ onabort + dispatchEvent；
    // timeout/any；边界：abort 后再加监听不触发（已消费）。
    let code = r#"const c = new AbortController();
let got = null;
c.signal.addEventListener("abort", function (e) { got = e.type + ":" + (e.target === c.signal) + ":" + (this === c.signal); });
let on = null;
c.signal.onabort = (e) => { on = e.type; };
c.abort();
if (got !== "abort:true:true" || on !== "abort") throw new Error("event object failed: " + got + "/" + on);
let late = 0;
c.signal.addEventListener("abort", () => { late++; });
if (late !== 0) throw new Error("late listener must not fire");
const c2 = new AbortController();
if (c2.signal.dispatchEvent({ type: "abort" }) !== true || !c2.signal.aborted) throw new Error("dispatchEvent failed");
if (c2.signal.dispatchEvent({ type: "click" }) !== true) throw new Error("dispatchEvent non-abort");
const t = AbortSignal.timeout(5);
await new Promise(r => setTimeout(r, 30));
if (!t.aborted) throw new Error("timeout failed");
const pre = AbortSignal.abort("early");
const a1 = AbortSignal.any([pre]);
if (!a1.aborted || String(a1.reason) !== "early") throw new Error("any pre-aborted failed");
const c3 = new AbortController();
const a2 = AbortSignal.any([c3.signal]);
c3.abort("zzz");
if (!a2.aborted || String(a2.reason) !== "zzz") throw new Error("any follow failed");
try { AbortSignal.any([{}]); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("AbortSignal")) throw e; }
console.log("abort-ev-ok");
"#;
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", code])),
        "abort-ev-ok\n"
    );
}

#[test]
fn streams_byob() {
    // 正常：enqueue 跨片 + read(view) 部分填充 + 读空 done + byobRequest/respond；
    // 报错：非字节流开 BYOB、非 view、空 view；边界：default reader 照读字节流。
    let code = r#"const rs = new ReadableStream({ type: "bytes", start(c) {
  c.enqueue(new Uint8Array([1, 2, 3]));
  c.enqueue(new Uint8Array([4, 5]));
  c.close();
} });
const r = rs.getReader({ mode: "byob" });
const r1 = await r.read(new Uint8Array(4));
if (r1.done || [...r1.value].join(",") !== "1,2,3,4") throw new Error("byob1 failed");
const r2 = await r.read(new Uint8Array(4));
if (r2.done || [...r2.value].join(",") !== "5") throw new Error("byob2 failed");
const r3 = await r.read(new Uint8Array(4));
if (!r3.done) throw new Error("byob done failed");
// pull + byobRequest/respond
const rs2 = new ReadableStream({ type: "bytes", pull(c) {
  const q = c.byobRequest;
  if (q) { const v = new Uint8Array(q.view.buffer, q.view.byteOffset, 2); v[0] = 7; v[1] = 8; q.respond(2); }
} });
const rr = rs2.getReader({ mode: "byob" });
const x = await rr.read(new Uint8Array(8));
if ([...x.value].join(",") !== "7,8") throw new Error("byobRequest failed");
rr.releaseLock(); await rs2.cancel();
// 报错面
try { new ReadableStream().getReader({ mode: "byob" }); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("byte stream")) throw e; }
const rs3 = new ReadableStream({ type: "bytes" });
const r4 = rs3.getReader({ mode: "byob" });
try { await r4.read([1, 2]); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("view")) throw e; }
try { await r4.read(new Uint8Array(0)); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("empty")) throw e; }
try { new ReadableStream({ type: "stream" }); throw new Error("must throw"); }
catch (e) { if (!String(e.message).includes("'bytes'")) throw e; }
r4.releaseLock(); await rs3.cancel();
// 边界：default reader 照读字节流（整块）
const rs5 = new ReadableStream({ type: "bytes", start(c) { c.enqueue(new Uint8Array([9])); c.close(); } });
const d5 = await rs5.getReader().read();
if (d5.done || [...d5.value].join(",") !== "9") throw new Error("default-on-bytes failed");
console.log("byob-ok");
"#;
    assert_eq!(
        stdout_of(&mut winterjs().args(["--eval", code])),
        "byob-ok\n"
    );
}

/// 分半写的 hang 服务器（先吐 "abc"，200ms 后吐 "def"，content-length 6）。
fn serve_split() -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for _ in 0..8 {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            let mut head = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let Ok(k) = s.read(&mut buf) else { break };
                if k == 0 {
                    break;
                }
                head.extend_from_slice(&buf[..k]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let _ = s
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 6\r\nconnection: close\r\n\r\nabc");
            let _ = s.flush();
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = s.write_all(b"def");
        }
    });
    port
}
