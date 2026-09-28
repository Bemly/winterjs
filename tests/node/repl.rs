//! tests/node/repl.rs — 对齐 src/builtins/node/repl.rs（node:repl）。

use crate::helpers::*;

#[test]
fn phase10c_repl_eval_print() {
    // 求值/打印/错误行/跨行持久 + exit 事件 + 形状面。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "r.mjs",
        r#"
import repl, { start, writer, REPLServer, REPL_MODE_SLOPPY, REPL_MODE_STRICT, Recoverable, isValidSyntax } from "node:repl";
import { PassThrough } from "node:stream";
console.log("shape", typeof start, typeof writer, typeof REPLServer, typeof REPL_MODE_SLOPPY, typeof REPL_MODE_STRICT, typeof Recoverable);
console.log("syntax", isValidSyntax("1+1"), isValidSyntax("1+"), isValidSyntax(""), isValidSyntax(42));
console.log("writer", writer({ x: 1 }));
console.log("recov", new Recoverable(new SyntaxError("x")) instanceof SyntaxError);
const input = new PassThrough(), output = new PassThrough();
let out = "";
output.on("data", (c) => (out += c));
const r = start({ input, output, prompt: "rs> ", terminal: false });
console.log("isRepl", r instanceof REPLServer, JSON.stringify(r.getPrompt()));
r.on("exit", () => console.log("exit-ev"));
input.write("40 + 2\n");
setTimeout(() => {
  input.write("let qqq = 41\n");
  setTimeout(() => {
    input.write("qqq + 1\n");
    setTimeout(() => {
      input.write("throw new Error(\"boom\")\n");
      setTimeout(() => {
        input.write("undeclared_xyz\n");
        setTimeout(() => {
          console.log("out", JSON.stringify(out));
          r.close();
        }, 50);
      }, 50);
    }, 50);
  }, 50);
}, 50);
"#,
    );
    for line in [
        "shape function function function symbol symbol function",
        "syntax true false true true",
        "writer { x: 1 }",
        "recov true",
        "isRepl true \"rs> \"",
        "out \"rs> 42\\nrs> undefined\\nrs> 42\\nrs> Uncaught Error: boom\\nrs> Uncaught ReferenceError: undeclared_xyz is not defined\\nrs> \"",
        "exit-ev",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10c_repl_multiline_commands() {
    // 续行（Recoverable 启发式）+ .break/.help/.exit + 自定义 eval。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "m.mjs",
        r#"
import { start } from "node:repl";
import { PassThrough } from "node:stream";
const input = new PassThrough(), output = new PassThrough();
let out = "";
output.on("data", (c) => (out += c));
const r = start({ input, output, prompt: "> ", terminal: false });
input.write("function foo() {\n");
setTimeout(() => {
  console.log("cont", JSON.stringify(out));
  input.write("return 7;\n}\n");
  setTimeout(() => {
    input.write("foo()\n");
    setTimeout(() => {
      console.log("call", JSON.stringify(out));
      input.write(".break\n");
      input.write("function(\n");
      setTimeout(() => {
        console.log("bad", JSON.stringify(out));
        input.write(".help\n");
        setTimeout(() => {
          console.log("help", out.includes(".exit") && out.includes(".help"));
          input.write(".exit\n");
          setTimeout(() => console.log("after-exit closed", r.rli.closed), 50);
        }, 50);
      }, 50);
    }, 50);
  }, 50);
}, 50);
"#,
    );
    for line in [
        "cont \"> | \"",
        "call \"> | | undefined\\n> 7\\n> \"",
        "bad \"> | | undefined\\n> 7\\n> > Uncaught SyntaxError: function statement requires a name\\n> \"",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    assert!(out.lines().any(|l| l == "help true"), "out: {out}");
    assert!(out.lines().any(|l| l == "after-exit closed true"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn p2_repl_legacy_positional() {
    // P2-repl：legacy 位置形 start(prompt, stream, eval) + writer.options 面。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "l.mjs",
        r#"
import { start } from "node:repl";
import { PassThrough } from "node:stream";
// 入出分离（同流自回显即真机亦无限递归，recoverable 套件靠 noop-write  duplex 避开）。
const input = new PassThrough(), output = new PassThrough();
let out = "";
output.on("data", (c) => (out += c));
// 位置形 duplex 取 stdin/stdout（node 299 行口径）。
const r = start("leg> ", { stdin: input, stdout: output }, (cmd, context, filename, cb) => cb(null, cmd.trim()));
console.log("prompt", JSON.stringify(r.getPrompt()));
console.log("wopts", typeof r.writer.options);
input.write("hi\n");
setTimeout(() => {
  console.log("out", JSON.stringify(out));
  r.close();
}, 100);
"#,
    );
    for line in [
        "prompt \"leg> \"",
        "wopts object",
        "out \"leg> 'hi'\\nleg> \"",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn p2_repl_methods_define_help_editor_complete() {
    // P2-repl 方法面：defineCommand 函数形 + help 版式 + editor 收尾 + complete 空回。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "m.mjs",
        r#"
import { start } from "node:repl";
import { PassThrough } from "node:stream";
const input = new PassThrough(), output = new PassThrough();
let out = "";
output.on("data", (c) => (out += c));
const r = start({ input, output, prompt: "> ", terminal: true });
r.defineCommand("say", function (t) { this.output.write(`hi ${t}\n`); this.displayPrompt(); });
input.write(".help\n");
input.write(".say yo\n");
r.complete("foo", (err, res) => console.log("comp", err, JSON.stringify(res)));
setTimeout(() => {
  console.log("help-ed", out.includes(".editor   Enter editor mode"));
  console.log("help-break", /\.break {4}Abort/.test(out));
  console.log("say", out.includes("hi yo\n"));
  const i2 = new PassThrough(), o2 = new PassThrough();
  let o = "";
  o2.on("data", (c) => (o += c));
  const e = start({ input: i2, output: o2, prompt: "> ", terminal: true });
  i2.write(".editor\n");
  i2.write("21 + 21\n");
  e.write("", { ctrl: true, name: "d" });
  setTimeout(() => {
    console.log("ed", o.includes("Entering editor mode") && o.includes("42"));
    r.close();
    e.close();
  }, 100);
}, 100);
"#,
    );
    for line in [
        "comp null [[],\"foo\"]",
        "help-ed true",
        "help-break true",
        "say true",
        "ed true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn p2_repl_subset_complete() {
    // P2-repl R3：子集补全（成员/拒答面；正常 + 报错边界）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "c.mjs",
        r#"
import { start } from "node:repl";
import { PassThrough } from "node:stream";
const input = new PassThrough(), output = new PassThrough();
const r = start({ input, output, prompt: "> ", terminal: false });
input.write('const o = { one: 1, nest: { two: 2 } };\n');
setTimeout(() => {
  r.complete("o.n", (e, d) => console.log("m1", JSON.stringify(d)));
  r.complete("o.nest.t", (e, d) => console.log("m2", JSON.stringify(d)));
  r.complete("o.missing.", (e, d) => console.log("m3", JSON.stringify(d)));
  r.complete("f().x", (e, d) => console.log("m4", JSON.stringify(d)));
  r.complete("o['nest'].t", (e, d) => console.log("m5", JSON.stringify(d)));
  setTimeout(() => r.close(), 50);
}, 100);
"#,
    );
    for line in [
        "m1 [[\"o.nest\"],\"o.n\"]",
        "m3 [[],\"o.missing.\"]",
        "m4 [[],\"f().x\"]",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    // 原型成员同列（真机同款；includes 断关键项）。
    assert!(out.contains("\"o.nest.two\""), "out: {out}");
    assert!(out.contains("\"o['nest'].two\""), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn p2_repl_cli_complete_bridge() {
    // 本体拥有补全核心（prelude/repl_complete）：`__wjs_cli_complete` 与
    // `__wjs_repl_default_complete` 开箱即有，不依赖 `node:repl` 加载；
    // `node:repl` 仅薄包反向复用（注入 vm 求值器），公开面保持 node 同形
    // （无 cliComplete）。bare 真上下文键/成员链/大小写不敏感/调用形拒答/签名描述。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "o.mjs",
        r#"
console.log("pre", typeof globalThis.__wjs_cli_complete === "function" && typeof globalThis.__wjs_repl_default_complete === "function");
import repl from "node:repl";
console.log("clean", repl.cliComplete === undefined && typeof globalThis.__wjs_repl_default_complete === "function");
const b = __wjs_cli_complete("gl");
console.log("bare", b[0].some((e) => e[0] === "global") && b[0].some((e) => e[0] === "globalThis") && b[1] === "gl");
const m = __wjs_cli_complete("globalThis.Array.fr");
console.log("member", m[0].some((e) => e[0] === "globalThis.Array.from"));
const sig = __wjs_cli_complete("globalThis.Object.assign");
// 右盒纯文档（有语料页的内建不再贴签名；签名表仅缺页回落）。
console.log("sig", sig[0][0][0] === "globalThis.Object.assign" && sig[0][0][1].includes("copies all enumerable own properties"));
const ci = __wjs_cli_complete("globalThis.arraybuf");
console.log("ci", ci[0].some((e) => e[0] === "globalThis.ArrayBuffer"));
const call = __wjs_cli_complete("globalThis.Array().");
console.log("call", call[0].length === 0);
const e = __wjs_cli_complete("console.");
// 右盒纯文档（候选框干净名；签名不进任何格）。
console.log("dot-empty", e[0].some((p) => p[0] === "console.log" && (p[1] ?? "").includes("outputs a message")) && e[0].some((p) => p[0] === "console.trace" && (p[1] ?? "").includes("stack trace")) && e[1] === "console.");
const g = __wjs_cli_complete("global.");
console.log("global-dot", g[0].length > 0 && g[0].every((p) => p[0].startsWith("global.")) && g[1] === "global.");
const v = __wjs_cli_complete("Object.p");
console.log("value-sig", v[0].some((p) => p[0] === "Object.prototype" && p[1] === ": {}"));
"#,
    );
    for line in [
        "pre true",
        "clean true",
        "bare true",
        "member true",
        "sig true",
        "ci true",
        "call true",
        "dot-empty true",
        "global-dot true",
        "value-sig true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn p2_repl_options_surface() {
    // P2-repl R4：options 面（访问器/旗/校验/废弃表；standalone 另案）。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "o.mjs",
        r#"
import repl from "node:repl";
import { PassThrough } from "node:stream";
console.log("norepl", repl.repl === undefined);
// 入出必须分离（4.221：同流 + terminal:true 自回显递归，真机同挂）。
const sin = new PassThrough();
const sout = new PassThrough();
const r1 = repl.start({ input: sin, output: sout, terminal: true });
console.log("r1", r1.input === sin && r1.output === sout && r1.input === r1.inputStream
  && r1.output === r1.outputStream && r1.terminal === true && r1.useColors === false
  && r1.useGlobal === false && r1.ignoreUndefined === false
  && r1.replMode === repl.REPL_MODE_SLOPPY && r1.historySize === 30);
const r2 = repl.start({ input: sin, output: sout, terminal: false, historySize: 50, useGlobal: true });
console.log("r2", r2.historySize === 50 && r2.useGlobal === true && r2.terminal === false);
try {
  repl.start({ breakEvalOnSigint: true, eval: true });
  console.log("evalcfg FAIL");
} catch (e) {
  console.log("evalcfg", e.code === "ERR_INVALID_REPL_EVAL_CONFIG");
}
console.log("mods", Array.isArray(repl.builtinModules) && repl.builtinModules.length > 0
  && Array.isArray(repl._builtinLibs));
r1.close();
r2.close();
"#,
    );
    for line in [
        "norepl true",
        "r1 true",
        "r2 true",
        "evalcfg true",
        "mods true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    assert!(!out.contains("FAIL"), "out: {out}");
    dir.close().unwrap();
}
