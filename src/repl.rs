//! 交互式 REPL 的纯逻辑侧（plan Phase 7-e3）：点命令、高亮、括号验证、历史。
//!
//! - 引擎胶水（会话初始化/求值/事件泵）在 `runtime.rs`（需其私有项）；本模块
//!   只放无引擎依赖的逻辑 + `rustyline` helper，可独立单测。
//! - 高亮为手写扫描器（约 60 行）：`oxc` 的 `Lexer::new` 非公开（`pub(super)`），
//!   词法结果直染走不通（见 §13 偏差，`docs/dependencies.md` 附记）；关键字 /
//!   字符串（含模板 `${}`）/ 数字 / 注释四类，`console` 上色，仅 TTY 生效
//!   （非 TTY 下 rustyline 退化逐行读，不调 highlighter，黑盒钉住无 ANSI）。
//! - 多行：`Validator` 判括号平衡（字符串/注释感知），`Incomplete` 即续行；
//!   非 TTY 逐行直求值（`1 +` 这类报 SyntaxError 后继续，行为差异文档记录）。

use std::borrow::Cow;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Ctrl-C 连击退出窗口：两次 `Interrupted` 间隔内即退出，超窗重置
/// （与 `.help` 文案同值；纯函数 `ctrl_c_should_exit` 单测覆盖）。
pub const CTRL_C_WINDOW: Duration = Duration::from_secs(2);

/// 连击判定（纯函数）：上次中断在窗口内即退出。
pub fn ctrl_c_should_exit(last: Option<Instant>, now: Instant) -> bool {
    matches!(last, Some(t) if now.duration_since(t) <= CTRL_C_WINDOW)
}

/// 点命令（行首 `.`；其余一律当代码）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dot {
    Exit,
    Help,
    Code,
}

/// 点命令分类（纯函数，单测覆盖）。
pub fn dot_command(line: &str) -> Dot {
    match line.trim() {
        ".exit" | ".quit" => Dot::Exit,
        ".help" => Dot::Help,
        s if s.starts_with('.') => Dot::Code, // 未知点命令当代码求值（自然报错）
        _ => Dot::Code,
    }
}

/// 历史文件（`$HOME/.winterjs_history`；拿不到则 `None`，不存档）。
pub fn history_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".winterjs_history"))
}

/// 括号平衡（字符串/模板/行块注释感知；`}` 超前即 false）。
/// 纯函数，单测覆盖；`Validator` 与黑盒多行行为都以它为准。
pub fn brace_balance(src: &str) -> bool {
    let mut stack: Vec<char> = Vec::new();
    let mut chars = src.chars().peekable();
    // 模板 `${}` 嵌套：遇到 `${` 压入的 `{` 与普通 `{` 同计数即可（只关心平衡）。
    let mut line_comment = false;
    let mut block_depth = 0u32;
    while let Some(c) = chars.next() {
        if line_comment {
            if c == '\n' {
                line_comment = false;
            }
            continue;
        }
        if block_depth > 0 {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                block_depth -= 1;
            } else if c == '/' && chars.peek() == Some(&'*') {
                chars.next();
                block_depth += 1;
            }
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                line_comment = true;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                block_depth = 1;
            }
            '\'' | '"' => {
                // 未闭合引号即不完整（多行字符串续读）。
                let q = c;
                let mut closed = false;
                while let Some(ch) = chars.next() {
                    if ch == '\\' {
                        chars.next();
                    } else if ch == q {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return false;
                }
            }
            '`' => {
                // 模板：`${` 进，配对 `}` 出；未闭合即不完整（多行模板续读）。
                let mut depth = 0u32;
                let mut closed = false;
                while let Some(ch) = chars.next() {
                    if ch == '\\' {
                        chars.next();
                    } else if ch == '`' && depth == 0 {
                        closed = true;
                        break;
                    } else if ch == '$' && chars.peek() == Some(&'{') {
                        chars.next();
                        depth += 1;
                        stack.push('{');
                    } else if ch == '}' && depth > 0 {
                        depth -= 1;
                        if stack.pop() != Some('{') {
                            return false;
                        }
                    }
                }
                if !closed {
                    return false;
                }
            }
            '(' | '[' | '{' => stack.push(c),
            ')' | ']' | '}' => {
                let want = match c {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if stack.pop() != Some(want) {
                    return false;
                }
            }
            _ => {}
        }
    }
    stack.is_empty()
}

/// JS 关键字表（高亮用；保留字全收，不过度求全）。
const KEYWORDS: &[&str] = &[
    "await", "break", "case", "catch", "class", "const", "continue", "debugger",
    "default", "delete", "do", "else", "export", "extends", "finally", "for",
    "from", "function", "if", "import", "in", "instanceof", "let", "new",
    "of", "return", "static", "super", "switch", "this", "throw", "try",
    "typeof", "var", "void", "while", "with", "yield", "async",
];

fn is_kw(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

/// 行高亮（纯函数，单测覆盖；调用方只在 TTY 下用）。
pub fn highlight_line(line: &str) -> String {
    use console::Style;
    let kw = Style::new().cyan();
    let st = Style::new().green();
    let num = Style::new().yellow();
    let com = Style::new().dim();
    let mut out = String::with_capacity(line.len() + 16);
    let bytes = line.as_bytes();
    let mut i = 0;
    // 标识符字符（含 `$`/`_`，Unicode 退化为逐字节透传）。
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    while i < bytes.len() {
        let c = bytes[i];
        // 行注释。
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            out.push_str(&com.apply_to(&line[i..]).to_string());
            break;
        }
        // 块注释（单行内；跨行块注释退化为普通文本，不求全）。
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let end = line[i..].find("*/").map(|e| i + e + 2).unwrap_or(line.len());
            out.push_str(&com.apply_to(&line[i..end]).to_string());
            i = end;
            continue;
        }
        // 字符串（含模板整段染绿；`${}` 内不高亮细分，文档记录）。
        if c == b'\'' || c == b'"' || c == b'`' {
            let q = c;
            let mut j = i + 1;
            while j < bytes.len() {
                if bytes[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if bytes[j] == q {
                    j += 1;
                    break;
                }
                j += 1;
            }
            out.push_str(&st.apply_to(&line[i..j.min(line.len())]).to_string());
            i = j;
            continue;
        }
        // 数字（digit 开头；`0x`/小数/指数一律 consume 到词边）。
        if c.is_ascii_digit() {
            let mut j = i + 1;
            while j < bytes.len() && (ident(bytes[j]) || bytes[j] == b'.') {
                j += 1;
            }
            out.push_str(&num.apply_to(&line[i..j]).to_string());
            i = j;
            continue;
        }
        // 标识符/关键字。
        if ident(c) {
            let mut j = i + 1;
            while j < bytes.len() && ident(bytes[j]) {
                j += 1;
            }
            let word = &line[i..j];
            if is_kw(word) {
                out.push_str(&kw.apply_to(word).to_string());
            } else {
                out.push_str(word);
            }
            i = j;
            continue;
        }
        out.push(c as char);
        i += 1;
    }
    out
}

/// rustyline helper（高亮 + 括号续行；补全/提示用默认空实现）。
pub struct ReplHelper;

impl rustyline::completion::Completer for ReplHelper {
    type Candidate = String;
}

impl rustyline::hint::Hinter for ReplHelper {
    type Hint = String;
}

impl rustyline::highlight::Highlighter for ReplHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        Cow::Owned(highlight_line(line))
    }
}

impl rustyline::validate::Validator for ReplHelper {
    fn validate(
        &self,
        ctx: &mut rustyline::validate::ValidationContext,
    ) -> rustyline::Result<rustyline::validate::ValidationResult> {
        use rustyline::validate::ValidationResult;
        if brace_balance(ctx.input()) {
            Ok(ValidationResult::Valid(None))
        } else {
            Ok(ValidationResult::Incomplete)
        }
    }
}

impl rustyline::Helper for ReplHelper {}

/// readline 线程主函数（阻塞 IO 独占线程；行经 `tx` 发主循环，`None` 表 EOF）。
/// 历史加载失败忽略；退出时尽力存档。
pub fn readline_loop(
    tx: tokio::sync::mpsc::UnboundedSender<Option<String>>,
    history: Option<PathBuf>,
) {
    let mut rl = match rustyline::Editor::<ReplHelper, rustyline::history::DefaultHistory>::new() {
        Ok(rl) => rl,
        Err(_) => {
            // TTY 初始化失败（如无终端）：退化为 stdin 逐行读（仍无 ANSI）。
            let stdin = std::io::stdin();
            for line in stdin.lines() {
                let Ok(line) = line else { break };
                if tx.send(Some(line)).is_err() {
                    break;
                }
            }
            let _ = tx.send(None);
            return;
        }
    };
    rl.set_helper(Some(ReplHelper));
    if let Some(h) = &history {
        let _ = rl.load_history(h);
    }
    // 上次 Ctrl-C 时刻（连击窗口判定用；首击只提示）。
    let mut last_interrupt: Option<Instant> = None;
    loop {
        match rl.readline("❄> ") {
            Ok(line) => {
                let _ = rl.add_history_entry(line.as_str());
                if tx.send(Some(line)).is_err() {
                    break;
                }
            }
            // Ctrl-C：窗内连击即退出（`None` 表 EOF，同 Ctrl-D 路径收尾）；
            // 首击只提示，不断会话。
            Err(rustyline::error::ReadlineError::Interrupted) => {
                let now = Instant::now();
                if ctrl_c_should_exit(last_interrupt, now) {
                    let _ = tx.send(None);
                    break;
                }
                last_interrupt = Some(now);
                println!("(To exit, press Ctrl+C again or Ctrl+D)");
            }
            // Ctrl-D / EOF：退出。
            Err(_) => {
                let _ = tx.send(None);
                break;
            }
        }
    }
    if let Some(h) = &history {
        let _ = rl.save_history(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_table() {
        assert_eq!(dot_command(".exit"), Dot::Exit);
        assert_eq!(dot_command(".quit"), Dot::Exit);
        assert_eq!(dot_command("  .help  "), Dot::Help);
        assert_eq!(dot_command("1 + 1"), Dot::Code);
        assert_eq!(dot_command(".nope"), Dot::Code);
    }

    #[test]
    fn brace_table() {
        for good in [
            "1 + 1",
            "function f() { return [1, (2)]; }",
            "if (x) { y(); }",
            "const s = '(oops)';",
            "const t = `a${b}c`;",
            "// ( unbalanced in comment",
            "/* ( unbalanced in block */ x",
            "",
        ] {
            assert!(brace_balance(good), "{good}");
        }
        for bad in ["function f() {", "const a = [1,", "if (x {", "`unterminated", "const s = 'x;", "}"] {
            assert!(!brace_balance(bad), "{bad}");
        }
    }

    #[test]
    fn ctrl_c_window() {
        let now = Instant::now();
        // 首击（无上次）不退；窗内连击退；超窗重置不退；边界恰窗退。
        assert!(!ctrl_c_should_exit(None, now));
        assert!(ctrl_c_should_exit(Some(now - Duration::from_millis(100)), now));
        assert!(!ctrl_c_should_exit(Some(now - CTRL_C_WINDOW - Duration::from_millis(100)), now));
        assert!(ctrl_c_should_exit(Some(now - CTRL_C_WINDOW), now));
    }

    #[test]
    fn highlight_marks_kinds() {
        // 强制开色后：四类 token 各带 ANSI，且剥码后与原文一致。
        console::set_colors_enabled(true);
        let line = "const s = 'hi' + 42; // c";
        let out = highlight_line(line);
        assert_eq!(console::strip_ansi_codes(&out), line);
        assert!(out.len() > line.len(), "no ANSI emitted:\n{out}");
        console::set_colors_enabled(false);
        assert_eq!(highlight_line(line), line);
    }
}
