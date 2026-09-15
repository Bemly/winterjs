//! tests/node/readline.rs — 对齐 src/builtins/node/readline.rs（node:readline）。

use crate::helpers::*;

#[test]
fn phase10c_readline_lines_history() {
    // 行提交 + history（recent-first/去空去重/上限）+ question + prompt。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "l.mjs",
        r#"
import readline, { createInterface } from "node:readline";
import { PassThrough } from "node:stream";
const input = new PassThrough(), output = new PassThrough();
let shown = "";
output.on("data", (c) => (shown += c));
const rl = createInterface({ input, output, prompt: "P> ", historySize: 3 });
console.log("term", rl.terminal, JSON.stringify(rl.getPrompt()), JSON.stringify(rl.history));
const lines = [];
rl.on("line", (l) => lines.push(l));
rl.prompt();
rl.question("Q? ", (a) => console.log("answer", JSON.stringify(a)));
input.write("hello\n");
input.write("world\n");
setTimeout(() => {
  console.log("lines", JSON.stringify(lines));
  console.log("hist", JSON.stringify(rl.history));
  console.log("shown", JSON.stringify(shown));
  console.log("line", JSON.stringify(rl.line), rl.cursor);
  rl.close();
  console.log("closed", rl.closed);
}, 100);
"#,
    );
    for line in [
        "term false \"P> \" []",
        "answer \"hello\"",
        "lines [\"world\"]",
        "hist []",
        "shown \"P> Q? \"",
        "line \"\" undefined",
        "closed true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10c_readline_terminal_edit() {
    // 终端编辑 Emacs 子集 + history + question + prompt 回显。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "t.mjs",
        r#"
import { createInterface } from "node:readline";
import { PassThrough } from "node:stream";
const input = new PassThrough(), output = new PassThrough();
let shown = "";
output.on("data", (c) => (shown += c));
const rl = createInterface({ input, output, terminal: true, prompt: "> " });
console.log("term", rl.terminal, rl.cursor);
const lines = [];
rl.on("line", (l) => lines.push(l));
// 可打印插入 + 回车提交
input.write("helo");
input.write("\x7f"); // 删 o
input.write("lo");
input.write("\r");
setTimeout(() => {
  console.log("l1", JSON.stringify(lines), JSON.stringify(rl.line), rl.cursor);
  // C-a/C-f*2/C-k：行首 + 前进两格 + 杀至行尾得 "ab"
  input.write("abcdef");
  input.write("\x01");
  input.write("\x06");
  input.write("\x06");
  input.write("\x0b");
  input.write("\r");
  setTimeout(() => {
    console.log("l2", JSON.stringify(lines));
    // Up 历史 + C-u 整行杀
    input.write("first\r");
    setTimeout(() => {
      input.write("second\r");
      setTimeout(() => {
        input.write("\x1b[A"); // Up -> second
        input.write("\x15"); // C-u 杀掉
        input.write("third\r");
        setTimeout(() => {
          console.log("hist", JSON.stringify(rl.history));
          // question 走终端提交
          rl.question("Q? ", (a) => console.log("answer", JSON.stringify(a)));
          input.write("ans\r");
          setTimeout(() => {
            console.log("shown-has-prompt", shown.includes("> ") && shown.includes("Q? "));
            rl.close();
          }, 50);
        }, 50);
      }, 50);
    }, 50);
  }, 50);
}, 50);
"#,
    );
    for line in [
        "term true 0",
        "l1 [\"hello\"] \"\" 0",
        "l2 [\"hello\",\"ab\"]",
          "hist [\"third\",\"second\",\"first\",\"ab\",\"hello\"]",
        "answer \"ans\"",
        "shown-has-prompt true",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}

#[test]
fn phase10c_readline_keypress_validate() {
    // 按键形状 + 校验 + 迭代器 + pause/resume + 自关。
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "k.mjs",
        r#"
import readline, { createInterface, emitKeypressEvents } from "node:readline";
import { PassThrough } from "node:stream";
// 按键形状（对拍表转断言）
{
  const i = new PassThrough();
  emitKeypressEvents(i);
  const got = [];
  i.on("keypress", (s, k) => got.push([s, k.name, k.ctrl, k.meta, k.shift, k.code]));
  i.write("a");
  i.write("\r");
  i.write("\x7f");
  i.write("\x1b[A");
  i.write("\x03");
  i.write("\x1bx");
  i.write("\n");
  setTimeout(() => {
    console.log("keys", JSON.stringify(got));
  }, 50);
}
// 校验：无码 TypeError
try { createInterface({}); } catch (e) { console.log("v-obj", e.constructor.name, e.code); }
try { createInterface(); } catch (e) { console.log("v-none", e.constructor.name, e.code); }
console.log("v-pos", typeof createInterface(new PassThrough(), new PassThrough()).question);
// split 包按键（ESC 跨包）
{
  const i = new PassThrough();
  emitKeypressEvents(i);
  i.on("keypress", (s, k) => console.log("split-key", k.name));
  i.write("\x1b");
  setTimeout(() => i.write("[B"), 20);
}
// 迭代器 + 自关 + pause
{
  const i = new PassThrough(), o = new PassThrough();
  const rl = createInterface({ input: i, output: o });
  (async () => {
    const got = [];
    for await (const l of rl) { got.push(l); if (got.length === 2) rl.close(); }
    console.log("iter", JSON.stringify(got));
  })();
  i.write("x\ny\n");
  const i2 = new PassThrough(), o2 = new PassThrough();
  const rl2 = createInterface({ input: i2, output: o2 });
  rl2.on("close", () => console.log("auto-close yes"));
  i2.end("last\n");
  const i3 = new PassThrough(), o3 = new PassThrough();
  const rl3 = createInterface({ input: i3, output: o3 });
  const ls = [];
  rl3.on("line", (l) => ls.push(l));
  rl3.pause();
  i3.write("a\n");
  setTimeout(() => {
    console.log("paused", JSON.stringify(ls), rl3.paused);
    rl3.resume();
    setTimeout(() => console.log("resumed", JSON.stringify(ls)), 30);
  }, 60);
}
setTimeout(() => console.log("end-ok"), 300);
"#,
    );
    for line in [
        "keys [[\"a\",\"a\",false,false,false,null],[\"\\r\",\"return\",false,false,false,null],[\"\",\"backspace\",false,false,false,null],[null,\"up\",false,false,false,\"[A\"],[\"\\u0003\",\"c\",true,false,false,null],[\"\\u001bx\",\"x\",false,true,false,null],[\"\\n\",\"enter\",false,false,false,null]]",
        "v-obj TypeError undefined",
        "v-none TypeError undefined",
        "v-pos function",
        "split-key down",
        "iter [\"x\",\"y\"]",
        "auto-close yes",
        "paused [] true",
        "resumed [\"a\"]",
        "end-ok",
    ] {
        assert!(out.lines().any(|l| l == line), "missing line: {line}\nout: {out}");
    }
    dir.close().unwrap();
}
