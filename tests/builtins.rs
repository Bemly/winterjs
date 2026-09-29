//! 内建全局黑盒测试(对齐 src/builtins/*:console/timers/clone/url/encoding + prelude)。
//! Phase 1 的 runtime/jobqueue 语义用例(微任务序/TLA/rejection)以 console/timers 为面,暂居此。

mod common;

use common::*;

#[test]
fn phase1_microtask_order_before_timer() {
    // 规范顺序：同步 → 微任务（FIFO）→ 宏任务
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "console.log('1'); setTimeout(()=>console.log('4'),0); Promise.resolve().then(()=>console.log('3')); queueMicrotask(()=>console.log('2'))"]));
    assert_eq!(out, "1\n3\n2\n4\n", "microtask/timer ordering: {out}");
}

#[test]
fn phase1_promise_chain_three_hops() {
    let out = stdout_of(&mut winterjs2().args([
        "--eval",
        "Promise.resolve(1).then(v=>v+1).then(v=>v+1).then(v=>console.log('chain:',v))",
    ]));
    assert!(out.contains("chain: 3"), "chain: {out}");
}

#[test]
fn phase1_top_level_await_acceptance() {
    // docs/plan.md Phase 1 验收样例
    assert_eq!(
        stdout_of(
            &mut winterjs2().args(["--eval", "await new Promise(r=>setTimeout(()=>r(1),10))"])
        ),
        "1\n"
    );
}

#[test]
fn phase1_interval_until_cleared() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "let n=0; const id=setInterval(()=>{n++; console.log('tick',n); if(n>=3) clearInterval(id)},5)"]));
    assert_eq!(out, "tick 1\ntick 2\ntick 3\n", "interval: {out}");
}

#[test]
fn phase1_nested_microtasks() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "async function f(){ for(let i=0;i<3;i++){ await Promise.resolve(); console.log('micro',i);} } f()"]));
    assert_eq!(
        out, "micro 0\nmicro 1\nmicro 2\n[object Promise]\n",
        "nested: {out}"
    );
}

#[test]
fn phase1_structured_clone_json_values() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "const a={x:1,y:[1,2,{z:'s'}]}; const b=structuredClone(a); console.log(JSON.stringify(b), b===a)"]));
    assert_eq!(
        out, "{\"x\":1,\"y\":[1,2,{\"z\":\"s\"}]} false\n",
        "clone object: {out}"
    );
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "console.log(JSON.stringify([structuredClone(42), structuredClone('s'), structuredClone(null)]))"]));
    assert_eq!(out, "[42,\"s\",null]\n");
}

#[test]
fn phase1_unhandled_rejection_is_fatal() {
    let out = winterjs2()
        .args(["--eval", "Promise.reject(new Error('nope'))"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "unhandled rejection must be fatal"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unhandled rejection"), "stderr: {stderr}");
}

#[test]
fn phase1_console_count_and_time() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "console.count('a'); console.count('a'); console.time('t'); console.timeLog('t'); console.timeEnd('t')"]));
    assert!(out.contains("a: 1") && out.contains("a: 2"), "count: {out}");
    assert!(
        out.contains("t: ") && out.matches("t: ").count() == 2,
        "time: {out}"
    );
}

// ── Phase 2 切片 a：ESM loader ────────────────────────────────────────────

#[test]
fn phase3_url_components() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const u = new URL("https://user:pass@example.com:8080/p?q=1#h"); console.log([u.href, u.protocol, u.host, u.hostname, u.port, u.pathname, u.search, u.hash, u.origin].join("|"))"#]));
    assert_eq!(
        out,
        "https://user:pass@example.com:8080/p?q=1#h|https:|example.com:8080|example.com|8080|/p|?q=1|#h|https://example.com:8080
",
        "url: {out}"
    );
}

#[test]
fn phase3_url_relative_and_can_parse() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"console.log(new URL("/p", "https://h.org/x").href, URL.canParse(':::'), URL.canParse('https://a.b'))"#]));
    assert_eq!(out, "https://h.org/p false true\n", "url base: {out}");
}

#[test]
fn phase3_url_invalid_throws() {
    let out = winterjs2()
        .args(["--eval", "new URL(':::')"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Invalid URL"), "stderr: {stderr}");
}

#[test]
fn phase3_usp_live_view() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const u = new URL("https://ex.com/?b=2"); const sp = u.searchParams; sp.append("c", "3"); console.log(u.search, sp === u.searchParams); u.search = "?x=9"; console.log(sp.toString())"#]));
    assert_eq!(
        out,
        "?b=2&c=3 true
x=9
",
        "live view: {out}"
    );
}

#[test]
fn phase3_usp_ops() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const s = new URLSearchParams("z=1&a=2&a=3"); s.sort(); console.log(s.toString(), s.get("a"), s.getAll("a").length, s.size)"#]));
    assert_eq!(
        out,
        "a=2&a=3&z=1 2 2 3
",
        "usp: {out}"
    );
}

#[test]
fn phase3_text_encoder_decoder() {
    let out = stdout_of(&mut winterjs2().args(["--eval",
        r#"const e = new TextEncoder(); console.log(e.encoding, e.encode("hi").length, JSON.stringify(new TextEncoder().encodeInto("hello", new Uint8Array(3)))); console.log(new TextDecoder().decode(new Uint8Array([104, 105])), new TextDecoder("utf-16le").decode(new Uint8Array([104, 0, 105, 0])));"#]));
    assert_eq!(
        out, "utf-8 2 {\"read\":3,\"written\":3}\nhi hi\n",
        "codec: {out}"
    );
}

#[test]
fn phase3_text_decoder_fatal() {
    let out = winterjs2()
        .args([
            "--eval",
            "new TextDecoder('utf-8', {fatal:true}).decode(new Uint8Array([0xff]))",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = stdout_of(&mut winterjs2().args([
        "--eval",
        "console.log(new TextDecoder('utf-8').decode(new Uint8Array([0xff])).length)",
    ]));
    assert_eq!(
        out,
        "1
"
    );
}

#[test]
fn phase3_base64_roundtrip() {
    let out =
        stdout_of(&mut winterjs2().args(["--eval", "console.log(btoa('hello'), atob('aGVsbG8='))"]));
    assert_eq!(
        out,
        "aGVsbG8= hello
",
        "base64: {out}"
    );
    let out = winterjs2().args(["--eval", "btoa('€')"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn phase1_gc_pressure_keeps_rooted_targets() {
    // §4.39 回归：回调内的 nursery GC 不得收集 RootedState 的 Heap 目标。
    // 修前：2 万小对象分配触发 minor GC，读到 Vec 搬运后悬垂的 store-buffer 边，
    // 进程 SIGSEGV/SIGBUS（exit=138/139），t2 永不打印。
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "setTimeout(()=>{let acc=0; for(let i=0;i<20000;i++){acc+=({x:i,s:'pad-'+i}).x;} console.log('t1',acc)},50); setTimeout(()=>console.log('t2-ok'),400)"]));
    assert!(out.contains("t1 199990000"), "gc pressure t1: {out}");
    assert!(out.contains("t2-ok"), "gc pressure t2: {out}");
}

#[test]
fn phase9m_global_dom_exception_file_message_channel_sab() {
    // jsdom/vitest 生态全局面（真机 26.8.2 对拍）：DOMException（legacy code
    // getter + 常量族）/File（Blob 子类）/MessageChannel·MessagePort 与
    // worker_threads 同一性/SharedArrayBuffer + Atomics。
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "const de = new DOMException('boom', 'AbortError');\n\
         console.log('domex', de.name, de.message, de instanceof Error, DOMException.ABORT_ERR, de.code, String(de));\n\
         console.log('codedef', new DOMException('x', 'NopeError').code);\n\
         const f = new File(['ab'], 'a.txt', { type: 'text/plain' });\n\
         console.log('file', f.name, f instanceof Blob, f.size, f.type, typeof f.lastModified);\n\
         try { new File(['x']); console.log('NO-ERR'); } catch (e) { console.log('fileerr', e.constructor.name); }\n\
         const sab = new SharedArrayBuffer(8); const ta = new Int32Array(sab);\n\
         console.log('sab', typeof SharedArrayBuffer, Atomics.add(ta, 0, 5), ta[0]);\n\
         import('node:worker_threads').then((wt) => {\n\
           console.log('ident', globalThis.MessageChannel === wt.MessageChannel, globalThis.MessagePort === wt.MessagePort);\n\
           const { port1, port2 } = new MessageChannel();\n\
           port1.onmessage = (e) => { console.log('msg', e.data); port1.close(); port2.close(); };\n\
           port2.postMessage('ping');\n\
         });"]));
    for line in [
        "domex AbortError boom true 20 20 AbortError: boom",
        "codedef 0",
        "file a.txt true 2 text/plain number",
        "fileerr TypeError",
        "sab function 0 5",
        "ident true true",
        "msg ping",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
}

#[test]
fn phase10a_immediate_and_timeout_class() {
    // 10a：全局 setImmediate/clearImmediate + Timeout/Immediate 真类。
    // 近似口径：setImmediate ≈ setTimeout(0)，同 delay(0) 队列 FIFO；
    // check 分层上线时改 order 断言。
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "console.log('noleak', typeof Timeout, typeof Immediate);\n\
         const order = [];\n\
         setTimeout(() => order.push('timeout'), 0);\n\
         setImmediate(() => order.push('immediate'));\n\
         setImmediate((a, b) => console.log('args', a, b), 'x', 7);\n\
         const t = setTimeout(() => {}, 50);\n\
         console.log('cls', t.constructor.name, typeof t.unref, typeof t.ref, typeof t.hasRef, typeof t.refresh);\n\
         console.log('prim', (t + 0) === t.__wjs2_id, typeof (t + 0));\n\
         console.log('chain', t.unref() === t, t.ref() === t, t.refresh() === t, t.hasRef());\n\
         const im = setImmediate(() => {});\n\
         console.log('imm', im.constructor.name, im.hasRef());\n\
         clearImmediate(im);\n\
         let cancelled = false;\n\
         clearImmediate(setImmediate(() => { cancelled = true; }));\n\
         clearTimeout(t);\n\
         setTimeout(() => console.log('order', order.join(','), 'cancelled', cancelled), 30);"]));
    for line in [
        "noleak undefined undefined",
        "args x 7",
        "cls Timeout function function function function",
        "prim true number",
        "chain true true true true",
        "imm Immediate true",
        "order timeout,immediate cancelled false",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
}

#[test]
fn phase10f_timer_face_unref_uncaught() {
    // 10f 对拍定案面：this 绑定/_destroyed 生命周期/dispose+close/字符串 id/
    // validateCallback 码/三态警告/uncaughtException 路由/ALS 传播/域路由/
    // unref 不续命/node:timers delete-proof。套件断言原文逐项对拍
    // （test-timers{,-this,-unref,-destroyed,-to-primitive,-throw-when-cb,
    //   -api-refs,-uncaught-exception,-immediate-queue-throw}/
    //  test-timers-clearImmediate-als/-nan-duration-warning 等）。
    let out = stdout_of(&mut winterjs2().args(["--eval",
        "const tthis = await new Promise((res) => {\n\
         \x20 setTimeout(function () { res(this !== undefined && typeof this.hasRef === 'function' && this._destroyed === false); }, 1);\n\
         });\n\
         console.log('this', tthis === true);\n\
         const tf = setTimeout(() => {}, 1);\n\
         await new Promise((r) => setTimeout(r, 30));\n\
         console.log('destroyed-fired', tf._destroyed === true);\n\
         const tc = setTimeout(() => {}, 1000);\n\
         clearTimeout(tc);\n\
         console.log('destroyed-cleared', tc._destroyed === true, tc.hasRef() === true);\n\
         const ivd = setInterval(() => {}, 1000);\n\
         clearInterval(ivd);\n\
         console.log('destroyed-interval', ivd._destroyed === true);\n\
         const tdisp = setTimeout(() => {}, 1000);\n\
         console.log('dispose', typeof tdisp[Symbol.dispose] === 'function', typeof tdisp.close === 'function');\n\
         tdisp[Symbol.dispose]();\n\
         console.log('disposed', tdisp._destroyed === true);\n\
         globalThis.__bad = false;\n\
         clearTimeout(`${+setTimeout(() => { globalThis.__bad = true; }, 5)}`);\n\
         const codes = [];\n\
         try { setTimeout('x', 1); } catch (e) { codes.push(e.code, e instanceof TypeError); }\n\
         try { setInterval(null, 1); } catch (e) { codes.push(e.code); }\n\
         try { setImmediate({}); } catch (e) { codes.push(e.code); }\n\
         console.log('validate', codes.join(','));\n\
         const warned = [];\n\
         process.on('warning', (w) => warned.push(w.name));\n\
         setTimeout(() => {}, NaN); setTimeout(() => {}, -1); setTimeout(() => {}, -2);\n\
         setTimeout(() => {}, 3e9); setTimeout(() => {}, 4e9);\n\
         await new Promise((r) => setTimeout(r, 40));\n\
         console.log('warn', JSON.stringify(warned));\n\
         console.log('string-clear', globalThis.__bad === false);\n\
         let caught = '';\n\
         let origins = '';\n\
         process.on('uncaughtException', (e, origin) => { caught += e.message; origins += origin; });\n\
         setTimeout(() => { throw new Error('boom1'); }, 1);\n\
         setTimeout(() => {}, 2);\n\
         await new Promise((r) => setTimeout(r, 50));\n\
         console.log('uncaught', caught === 'boom1', origins === 'uncaughtException');\n\
         const { AsyncLocalStorage } = await import('node:async_hooks');\n\
         const als = new AsyncLocalStorage();\n\
         let alsv = '';\n\
         als.run(new Map([['k', 'v']]), () => {\n\
         \x20 setTimeout(() => { alsv = als.getStore() ? als.getStore().get('k') : ''; }, 5);\n\
         });\n\
         await new Promise((r) => setTimeout(r, 40));\n\
         console.log('als', alsv === 'v');\n\
         const domain = await import('node:domain');\n\
         let domOk = false;\n\
         const d = domain.create();\n\
         d.on('error', (e) => { domOk = e.domain === d; });\n\
         d.run(() => setImmediate(() => { throw new Error('dom-err'); }));\n\
         await new Promise((r) => setTimeout(r, 40));\n\
         console.log('domain', domOk === true, process.domain === null);\n\
         const spin = setInterval(() => {}, 1); spin.unref();\n\
         await new Promise((r) => setTimeout(r, 20));\n\
         const timers = await import('node:timers');\n\
         delete globalThis.setTimeout; delete globalThis.clearTimeout;\n\
         delete globalThis.setInterval; delete globalThis.clearInterval;\n\
         delete globalThis.setImmediate; delete globalThis.clearImmediate;\n\
         let apicount = 0;\n\
         timers.setTimeout(() => {\n\
         \x20 apicount++;\n\
         \x20 timers.clearInterval(timers.setInterval(() => {}, 1000));\n\
         \x20 timers.clearImmediate(timers.setImmediate(() => {}));\n\
         }, 1);\n\
         await new Promise((r) => timers.setTimeout(r, 40));\n\
         console.log('api-refs', apicount === 1, typeof globalThis.setTimeout === 'undefined');"]));
    for line in [
        "this true",
        "destroyed-fired true",
        "destroyed-cleared true true",
        "destroyed-interval true",
        "dispose true true",
        "disposed true",
        "validate ERR_INVALID_ARG_TYPE,true,ERR_INVALID_ARG_TYPE,ERR_INVALID_ARG_TYPE",
        "string-clear true",
        "warn [\"TimeoutNaNWarning\",\"TimeoutNegativeWarning\",\"TimeoutOverflowWarning\",\"TimeoutOverflowWarning\"]",
        "uncaught true true",
        "als true",
        "domain true true",
        "api-refs true true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing: {line}\nout: {out}");
    }
}

#[test]
fn phase11_console_node_format() {
    // 2026-09-25：全局 console 走 util.format（修前原生 sink 只 ToString：`[object Object]`、
    // `%s` 原样）。正常：对象/数组/Map/Symbol/BigInt inspect；占位符；console.dir 深度。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "console.log({x:1}, [1,2], new Map([[1,2]]), Symbol('s'), 10n, -0);\
         console.log('a %s b %d c %i %j %%', 'S', 4.5, 4.5, {x:1});\
         console.dir({a:{b:{c:{d:1}}}});\
         console.log('50% plain', 1, true, null, undefined);",
    ]));
    for line in [
        "{ x: 1 } [ 1, 2 ] Map(1) { 1 => 2 } Symbol(s) 10n -0",
        "a S b 4.5 c 4 {\"x\":1} %",
        "{ a: { b: { c: [Object] } } }",
        // 边界：首参含 `%` 但无占位符按字面，原始值快路径与 node 同形。
        "50% plain 1 true null undefined",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line:?}; out: {out}");
    }
    // 报错流：console.error 同样格式化（stderr）。
    let o = winterjs2().args(["--eval", "console.error('e %s', {k: 2})"]).output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("e { k: 2 }"));
}

#[test]
fn phase11_console_global_unified() {
    // 2026-09-28 §7-②：全局 console 统一收尾（assert/trace 原文语义 + 8 缺失方法）。
    // 正常：方法表齐 + assert 格式化 + 多参/裸参 + trace 首行 + 别名/存根。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "console.log(typeof console.table, typeof console.dirxml, typeof console.groupCollapsed,\
         typeof console.context, typeof console.createTask, typeof console.profile,\
         typeof console.timeStamp, typeof console.Console);\
         console.table(42); console.table(null); console.dirxml('dx');\
         console.groupCollapsed('gc'); console.log('in-gc'); console.groupEnd();\
         console.log(typeof console.context().log, typeof console.createTask().run);\
         import('node:console').then(m=>console.log(m.Console===console.Console));",
    ]));
    for line in [
        "function function function function function function function function",
        "42",
        "null",
        "dx",
        "gc",
        "  in-gc",
        "function function",
        "true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line:?}; out: {out}");
    }
    // 报错流（stderr）：assert 按 constructor.js 原文（首参字符串前缀/多参格式化/
    // 裸参/真值静默）+ trace 首行 `Trace: msg`。
    let o = winterjs2()
        .args(["--eval", "console.assert(false, '%s=%d', 'a', 1); console.assert(false, 'x', {k:1}); console.assert(false); console.assert(true, 'silent'); console.trace('%s', 't');"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    for line in ["Assertion failed: a=1", "Assertion failed: x { k: 1 }", "Assertion failed"] {
        assert!(err.lines().any(|l| l == line), "missing {line:?}; err: {err}");
    }
    assert!(!err.contains("silent"), "assert(true) must be silent; err: {err}");
    assert!(err.lines().any(|l| l == "Trace: t"), "trace head; err: {err}");
    // 边界：trace 无参 → 裸 `Trace`（空消息 V8 省略 `: `，真机同形）。
    let o2 = winterjs2().args(["--eval", "console.trace()"]).output().unwrap();
    let err2 = String::from_utf8_lossy(&o2.stderr);
    assert!(err2.lines().any(|l| l == "Trace"), "trace() head; err: {err2}");
}

#[test]
fn phase11_repl_sig_js_docs() {
    // 2026-09-28 用户裁定：pane 只放签名，不搬运文档句（文档只读语料，经 `.doc`）。
    // 正常：R3 签名表命中 + 原生 toString 真形参；边界：用户自有同名方法显示自身。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "const j = (o) => console.log(JSON.stringify(o));\
         j(globalThis.__wjs2_cli_complete('console.')[0].filter(p=>p[0]==='console.log'||p[0]==='console.trace'));\
         j(globalThis.__wjs2_cli_complete('fet')[0]);\
         const u = new URLSearchParams('a=1'); globalThis.u = u;\
         j(globalThis.__wjs2_cli_complete('u.')[0].filter(p=>p[0]==='u.get'));\
         globalThis.o = { assign(a, b) { return a; } };\
         j(globalThis.__wjs2_cli_complete('o.')[0].filter(p=>p[0]==='o.assign'));",
    ]));
    // 桥对子 `[全文, 描述]`：候选框干净名，右盒纯文档（前两段 + 调用形状
    // + Parameters；`.doc` 同源）；缺页（用户自有）回签名。
    for line in [
        "console.log\",\"The console.log() static method outputs a message",
        "console.log(val1)",
        "val1 … valN:",
        "console.trace\",\"The console.trace() static method outputs a stack trace",
        "fetch\",\"The fetch() method of the Window interface starts the process",
        "fetch(resource",
        "u.get\",\"The get() method of the URLSearchParams interface returns the first value",
        "[\"o.assign\",\"assign(a, b)\"]",
    ] {
        assert!(out.contains(line), "missing {line:?}; out: {out}");
    }
}

#[test]
fn phase11_set_immediate_not_clamped() {
    // 2026-09-26：setImmediate 不走 setTimeout 的 1ms 钳（修前每个 immediate ≥1ms）；
    // 顺序与真机 26.8.2 一致：I/O 回调内 immediate 先于 setTimeout(0)。
    use assert_fs::prelude::*;
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("i.js")
        .write_str(
            "const t = Date.now(); let n = 0;\n\
             const f = () => { if (++n < 1000) setImmediate(f); else console.log('fast', Date.now() - t < 300); };\n\
             f();\n\
             require('fs').readFile(__filename, () => { setTimeout(() => console.log('io-timeout'), 0); setImmediate(() => console.log('io-immediate')); });\n\
             const im = setImmediate(() => console.log('BAD')); clearImmediate(im);\n",
        )
        .unwrap();
    let (ok, out, _) = wjs(&["--run", "i.js"], &dir);
    assert!(ok, "out: {out}");
    // 正常：1000 条链式 immediate 远低于 1s；边界：clearImmediate 生效；顺序对齐真机。
    assert!(out.contains("fast true") && !out.contains("BAD"), "out: {out}");
    let a = out.find("io-immediate").unwrap();
    let b = out.find("io-timeout").unwrap();
    assert!(a < b, "out: {out}");
}

#[test]
fn namespace_three_globals_present() {
    // 正常：Deno/Bun/WinterJS2 三命名空间存在 + 版本同源 + toStringTag。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "console.log(JSON.stringify([typeof Deno, typeof Bun, typeof WinterJS2]));\
         console.log(JSON.stringify([Deno.version.deno, Bun.version, WinterJS2.version]));\
         console.log(JSON.stringify([Object.prototype.toString.call(Deno), Object.prototype.toString.call(Bun), Object.prototype.toString.call(WinterJS2)]));",
    ]));
    assert!(out.contains(r#"["object","object","object"]"#), "out: {out}");
    assert!(out.contains(r#"["26.9.27","26.9.27","26.9.27"]"#), "out: {out}");
    assert!(out.contains("[object Deno]"), "out: {out}");
    assert!(out.contains("[object Bun]"), "out: {out}");
    assert!(out.contains("[object WinterJS2]"), "out: {out}");
}

#[test]
fn namespace_frozen_vs_mutable() {
    // 报错/语义：Deno 冻结（赋值不生效），Bun/WinterJS2 可改（setter 生效）。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "Deno.version = 1; console.log('deno-mut:' + (Deno.version === 1));\
         Bun.version = 'x'; WinterJS2.version = 'y';\
         console.log(JSON.stringify([Bun.version, WinterJS2.version]));\
         console.log(JSON.stringify(Object.isFrozen(Deno)));",
    ]));
    assert!(out.contains("deno-mut:false"), "out: {out}");
    assert!(out.contains(r#"["x","y"]"#), "out: {out}");
    assert!(out.contains("true"), "out: {out}");
}

#[test]
fn namespace_full_surface_types() {
    // 全量别名存在性：一行多断言分参打印（§4.42）。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "const d = ['readFile','writeFile','readTextFile','writeTextFile','open','stat','lstat','mkdir','remove','rename','copyFile','symlink','readLink','realPath','readDir','makeTempDir','truncate','chmod','chown','utime','watchFs','test','serve','connect','listen','listenDatagram','resolveDns','Command','permissions','errors','env','cwd','chdir','exit','hostname','osRelease','args','pid','version','build'];\n\
         const b = ['file','write','spawnSync','$','sleep','sleepSync','nanoseconds','randomUUIDv7','sha','serve','listen','connect','udpSocket','fileURLToPath','pathToFileURL','which','version','revision','argv','main','env'];\n\
         const w = ['version','versions','args','env','cwd','pid','storage','localStorage','CompressionStream','DecompressionStream','Deno','Bun'];\n\
         console.log('deno-missing:' + JSON.stringify(d.filter((k) => typeof Deno[k] === 'undefined')));\n\
         console.log('bun-missing:' + JSON.stringify(b.filter((k) => typeof Bun[k] === 'undefined')));\n\
         console.log('wjs-missing:' + JSON.stringify(w.filter((k) => typeof WinterJS2[k] === 'undefined')));",
    ]));
    assert!(out.contains("deno-missing:[]"), "out: {out}");
    assert!(out.contains("bun-missing:[]"), "out: {out}");
    assert!(out.contains("wjs-missing:[]"), "out: {out}");
}

#[test]
fn namespace_delegation_spot() {
    // 委派抽查：Deno.cwd/Bun.file+write/env 三方互通/Deno.Command/Bun.$。
    use assert_fs::prelude::*;
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("ns.js")
        .write_str(
            "console.log('cwd:' + (Deno.cwd() === process.cwd() && WinterJS2.cwd() === process.cwd()));\n\
             await Bun.write('a.txt', 'bun');\n\
             console.log('file:' + (await Bun.file('a.txt').text() === 'bun' && Bun.file('a.txt').exists()));\n\
             Deno.env.set('WJS_NS_X', '9');\n\
             console.log('env:' + (Deno.env.get('WJS_NS_X') === '9' && WinterJS2.env.WJS_NS_X === '9' && Bun.env.WJS_NS_X === '9'));\n\
             const r = await new Deno.Command('echo', { args: ['hi'] }).output();\n\
             console.log('cmd:' + (r.code === 0 && String(r.stdout).trim() === 'hi'));\n\
             const q = await Bun.$`echo hi`;\n\
             console.log('dollar:' + (q.exitCode === 0 && String(q.stdout).trim() === 'hi'));\n\
             console.log('which:' + (Bun.which('sh') === '/bin/sh'));\n",
        )
        .unwrap();
    let (ok, out, _) = wjs(&["--run", "ns.js"], &dir);
    assert!(ok, "out: {out}");
    for line in ["cwd:true", "file:true", "env:true", "cmd:true", "dollar:true", "which:true"] {
        assert!(out.lines().any(|l| l == line), "missing {line:?}; out: {out}");
    }
}

#[test]
fn namespace_member_completion_after_sync() {
    // R3 getter 拒入纪律下值型成员须为数据属性：`Deno.version.` 等二级补全非空
    //（含 `global.` 前缀形）；活值与 process 同源。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "const j = (x) => console.log(x, JSON.stringify(globalThis.__wjs2_cli_complete(x)[0].map((p) => p[0]).slice(0, 4)));\n\
         j('Deno.version.');\n\
         j('global.Deno.version.');\n\
         j('Bun.argv.');\n\
         j('WinterJS2.versions.');\n\
         console.log('live:' + JSON.stringify([Deno.pid === process.pid, WinterJS2.pid === process.pid, Bun.main === String(process.argv[1] || '')]));",
    ]));
    assert!(out.contains("\"Deno.version.deno\""), "out: {out}");
    assert!(out.contains("\"global.Deno.version.deno\""), "out: {out}");
    assert!(out.contains("\"Bun.argv.length\""), "out: {out}");
    assert!(out.contains("\"WinterJS2.versions.winterjs2\""), "out: {out}");
    assert!(out.contains("live:[true,true,true]"), "out: {out}");
}

#[test]
fn namespace_args_flow_to_run() {
    // `--run file -- args` 透传进三命名空间（NODE_PRELUDE 尾同步）。
    use assert_fs::prelude::*;
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("args.js")
        .write_str("console.log(JSON.stringify([Deno.args, WinterJS2.args]))\n")
        .unwrap();
    let (ok, out, _) = wjs(&["--run", "args.js", "--", "a", "b"], &dir);
    assert!(ok, "out: {out}");
    assert!(out.contains(r#"[["a","b"],["a","b"]]"#), "out: {out}");
}

#[test]
fn image_roundtrip_and_params() {
    // 正常：多格式往返 + jpeg quality/png 压缩/gif repeat/pnm 子集/svg 矢量。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "const px = new Uint8Array([255,0,0,255, 0,255,0,255, 0,0,255,255, 255,255,0,255]);\n\
         const img = { data: px, width: 2, height: 2 };\n\
         const rt = (f, o) => { const e = WinterJS2.image.encode(img, f, o); const d = WinterJS2.image.decode(e, f); return d.format + ':' + d.width + 'x' + d.height + ':' + d.data.length; };\n\
         console.log('r:' + ['png','jpeg','gif','webp','tiff','bmp','qoi','pnm','farbfeld','tga','hdr','exr','ico'].map((f) => rt(f)).join(','));\n\
         console.log('q:' + (WinterJS2.image.encode(img, 'jpeg', { quality: 100 }).length > 0));\n\
         console.log('c:' + (WinterJS2.image.encode(img, 'png', { compression: 'best', filter: 'paeth' }).length > 0));\n\
         console.log('g:' + (WinterJS2.image.encode(img, 'gif', { speed: 10, repeat: 0 }).length > 0));\n\
         console.log('p:' + (WinterJS2.image.encode(img, 'pnm', { subtype: 'pgm', encoding: 'ascii' }).length > 0));\n\
         const svg = new TextEncoder().encode('<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"3\"><rect width=\"4\" height=\"3\" fill=\"red\"/></svg>');\n\
         const s = WinterJS2.image.decode(svg);\n\
         console.log('s:' + s.format + ':' + s.width + 'x' + s.height + ':' + Array.from(s.data.slice(0, 4)).join(','));\n\
         const s2 = WinterJS2.image.decode(svg, 'svg', 2);\n\
         console.log('s2:' + s2.width + 'x' + s2.height + ':' + (s2.data.length === 8 * 6 * 4));\n\
         console.log('i:' + JSON.stringify(WinterJS2.image.info(svg)));",
    ]));
    assert!(out.contains("r:png:2x2:16,jpeg:2x2:16,gif:2x2:16,webp:2x2:16,tiff:2x2:16,bmp:2x2:16,qoi:2x2:16,pnm:2x2:16,farbfeld:2x2:16,tga:2x2:16,hdr:2x2:16,exr:2x2:16,ico:2x2:16"), "out: {out}");
    assert!(out.contains("q:true"), "out: {out}");
    assert!(out.contains("c:true"), "out: {out}");
    assert!(out.contains("g:true"), "out: {out}");
    assert!(out.contains("p:true"), "out: {out}");
    assert!(out.contains("s:svg:4x3:255,0,0,255"), "out: {out}");
    assert!(out.contains("s2:8x6:true"), "out: {out}");
    assert!(out.contains(r#""format":"svg","width":4,"height":3"#), "out: {out}");
}

#[test]
fn image_errors_and_bounds() {
    // 报错：垃圾字节/未知格式/dds 无编解码/svg 编码/jxl 编码/质量越界/像素长度错。
    // 边界：1x1 最小；dds 行标 unsupported。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "const t = (f) => { try { f(); return 'NO-THROW'; } catch (e) { return e.message; } };\n\
         console.log('e1:' + t(() => WinterJS2.image.decode(new Uint8Array([1,2,3]))));\n\
         console.log('e2:' + t(() => WinterJS2.image.decode(new Uint8Array([1,2,3]), 'nope')));\n\
         console.log('e3:' + t(() => WinterJS2.image.encode({ data: new Uint8Array(16), width: 2, height: 2 }, 'dds')));\n\
         console.log('e4:' + t(() => WinterJS2.image.encode({ data: new Uint8Array(16), width: 2, height: 2 }, 'svg')));\n\
         console.log('e5:' + t(() => WinterJS2.image.encode({ data: new Uint8Array(16), width: 2, height: 2 }, 'jxl')));\n\
         console.log('e6:' + t(() => WinterJS2.image.encode({ data: new Uint8Array(16), width: 2, height: 2 }, 'jpeg', { quality: 101 })));\n\
         console.log('e7:' + t(() => WinterJS2.image.encode({ data: new Uint8Array(15), width: 2, height: 2 }, 'png')));\n\
         console.log('e8:' + t(() => WinterJS2.image.encode({ data: new Uint8Array(16), width: 0, height: 2 }, 'png')));\n\
         console.log('e9:' + t(() => WinterJS2.image.encode({ data: new Uint8Array(16), width: 2, height: 2 }, 'png', { compression: 'turbo' })));\n\
         const one = WinterJS2.image.decode(WinterJS2.image.encode({ data: new Uint8Array([9,9,9,255]), width: 1, height: 1 }, 'qoi'));\n\
         console.log('b:' + (one.width === 1 && one.data[0] === 9));\n\
         console.log('f:' + JSON.stringify(WinterJS2.image.formats().find((r) => r.name === 'dds')));",
    ]));
    for line in [
        "e1:TypeError: unsupported image format",
        "e2:TypeError: unsupported image format",
        "e3:WinterJS2.image.encode: 'dds' has no encoder",
        "e4:WinterJS2.image.encode: 'svg' has no encoder",
        "e5:WinterJS2.image.encode: 'jxl' has no encoder",
        "e6:RangeError: jpeg quality must be an integer within 1..100",
        "e7:RangeError: pixel data length must equal width*height*4",
        "e8:RangeError: image dimensions must be at least 1x1",
        "e9:RangeError: png compression must be",
        "b:true",
        r#"f:{"name":"dds","mime":"image/vnd-ms.dds","decode":false,"encode":false}"#,
    ] {
        assert!(out.contains(line), "missing {line:?}; out: {out}");
    }
}

#[test]
fn media_mp4_fixture() {
    // 正常：mp4 demux（tracks/codec/samples/首字节）+ 经 demux 取出的 flac 全链解码。
    // fixture：mp4-rs 自带 beep-flac（Apache-2.0，tests/fixtures/media/）。
    use assert_fs::prelude::*;
    let dir = assert_fs::TempDir::new().unwrap();
    let fx = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/media/beep-flac-audio.mp4");
    dir.child("m.js")
        .write_str(&format!(
            "const fs = require('node:fs');\n\
             const b = new Uint8Array(fs.readFileSync('{fx}'));\n\
             const info = WinterJS2.media.mp4Info(b);\n\
             console.log('tracks:' + JSON.stringify(info.tracks.map((t) => [t.id, t.kind, t.codec, t.sampleCount])));\n\
             const ss = WinterJS2.media.mp4Samples(b, 1, 2);\n\
             console.log('ss:' + (ss.length === 2 && ss[0].index === 0 && ss[0].size > 0));\n\
             const s0 = WinterJS2.media.mp4Sample(b, 1, 0);\n\
             console.log('s0:' + (s0.length === ss[0].size));\n\
             const d = WinterJS2.media.decodeAudio(b);\n\
             console.log('dec:' + [d.format, d.codec, d.sampleRate, d.channels, d.data.length > 40000].join(','));\n"
        ))
        .unwrap();
    let (ok, out, _) = wjs(&["--run", "m.js"], &dir);
    assert!(ok, "out: {out}");
    assert!(out.contains("tracks:[[1,\"audio\",\"flac\",10]]"), "out: {out}");
    assert!(out.contains("ss:true"), "out: {out}");
    assert!(out.contains("s0:true"), "out: {out}");
    assert!(out.contains("dec:mp4,flac,44100,1,true"), "out: {out}");
}

#[test]
fn media_errors_and_bounds() {
    // 报错：垃圾音频/空字节/坏音量/坏视频维/坏帧数/越界样本/未知 id stop=false。
    // 边界：volume 0 可放（有设备）或干净无设备错（CI）。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "const t = (f) => { try { f(); return 'NO-THROW'; } catch (e) { return e.message; } };\n\
         console.log('e1:' + t(() => WinterJS2.media.decodeAudio(new Uint8Array([1,2,3]))));\n\
         console.log('e2:' + t(() => WinterJS2.media.decodeAudio(new Uint8Array(0))));\n\
         console.log('e3:' + t(() => WinterJS2.media.play({ data: new Float32Array(8), sampleRate: 8000, channels: 1 }, { volume: -1 })));\n\
         console.log('e4:' + t(() => WinterJS2.media.videoEncode({ data: new Uint8Array(16), width: 3, height: 2, count: 1 })));\n\
         console.log('e5:' + t(() => WinterJS2.media.videoEncode({ data: new Uint8Array(16), width: 2, height: 2, count: 0 })));\n\
         console.log('e6:' + t(() => WinterJS2.media.mp4Sample(new Uint8Array([1,2,3]), 1, 0)));\n\
         console.log('e7:' + t(() => WinterJS2.media.videoEncode({ data: new Uint8Array(16), width: 2, height: 2, count: 1 }, { speed: 11 })));\n\
         console.log('b:' + (WinterJS2.media.stop(424242) === false));",
    ]));
    for line in [
        "e1:TypeError: unsupported audio format",
        "e2:TypeError",
        "e3:RangeError: WinterJS2.media.play volume",
        "e4:RangeError",
        "e5:RangeError",
        "e6:TypeError",
        "e7:RangeError",
        "b:true",
    ] {
        assert!(out.contains(line), "missing {line:?}; out: {out}");
    }
}

#[test]
fn namespace_user_predefine_kept() {
    // 边界：用户在 prelude 后覆盖三命名空间不炸，会话继续。
    let out = stdout_of(winterjs2().args([
        "--eval",
        "Bun.foo = 42; WinterJS2.bar = 's';\
         console.log(JSON.stringify([Bun.foo, WinterJS2.bar, typeof Deno.args]));",
    ]));
    assert!(out.contains(r#"[42,"s","object"]"#), "out: {out}");
}
