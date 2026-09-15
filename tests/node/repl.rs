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
