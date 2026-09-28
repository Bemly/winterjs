//! 交互式 REPL 的纯逻辑侧（plan Phase 7-e3；2026-09-28 C 档换 reedline）。
//!
//! - 引擎胶水（会话初始化/求值/事件泵）在 `runtime/repl.rs`（需其私有项）；本模块
//!   只放无引擎依赖的逻辑 + reedline 接线（prompt/补全/高亮/校验/历史），可独立单测。
//! - 行编辑：`reedline`（default 特性；禁 `sqlite`/`system_clipboard`，见
//!   `docs/dependencies.md` §4）+ `IdeMenu` 浮窗补全（Tab 开菜单，右侧文档 pane）。
//!   提示符保持 `❄> `（左 `❄` + indicator `> `），无编号（用户口径）。
//! - 高亮为手写扫描器（关键字/字符串（含模板整段）/数字/注释四类；`oxc` 的
//!   `Lexer::new` 非公开，走不通，见 §13 偏差）：同一 `tokenize` 产出两种渲染——
//!   ANSI 串（`highlight_line`，单测/文档用）与 `StyledText`（TTY 实时渲染）。
//!   模板 `${}` 内不高亮细分；跨行块注释退化为普通文本（文档记录）。
//! - 多行：`SnowValidator` 以 `brace_balance` 判括号平衡（字符串/注释感知），
//!   `Incomplete` 即续行（multiline indicator 同 `> `）。
//! - 补全：`JsCompleter` 双源——静态表（点命令/关键字 + 一行签名）与
//!   **真上下文动态**（Tab 请求经通道投递 JS 线程，调 `node:repl` 的
//!   `cliComplete`：成员链逐步求值/fs 路径/bare 上下文键/大小写不敏感，
//!   R3 同源；`JSContext` 是 `!Send` 故只在 JS 线程求值）；超时降级静态。
//!   同名时动态优先，静态描述补充。
//!   空前缀不炸菜单（返回空集）。
//! - 非 TTY（stdin 管道，黑盒即此）：退化 stdin 逐行读，不调 highlighter/menu，
//!   输出无 ANSI（`tests/repl.rs` 钉住）。

use std::borrow::Cow;
use std::io::IsTerminal as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Ctrl-C 连击退出窗口：两次 `CtrlC` 间隔内即退出，超窗重置
/// （与 `.help` 文案同值；纯函数 `ctrl_c_should_exit` 单测覆盖）。
pub const CTRL_C_WINDOW: Duration = Duration::from_secs(2);

/// REPL TTY 输出旗（REPL 会话且 stdin 为终端时置位，进程级只增不减）。
/// 读行线程阻塞在 `read_line` 时终端处于 raw mode（crossterm 关 OPOST/ONLCR），
/// 主循环的**异步窗口输出**（timer 回调里 `console.log` 等）裸 `\n` 不回车，
/// 真终端阶梯右移——该旗开着时用户输出统一 CRLF 化（ONLCR 开时多出的
/// `\r` 光标已在行首，视觉无害）。行处理期间的输出不走此旗：哨兵协议下
/// 读行线程不进 `read_line`（raw 已退），终端自然正常换行。
static REPL_TTY_OUTPUT: AtomicBool = AtomicBool::new(false);

/// REPL 会话开工置位（`runtime/repl::repl`，仅 TTY 会话调用）。
pub fn set_tty_output(on: bool) {
    REPL_TTY_OUTPUT.store(on, Ordering::Relaxed);
}

/// 用户输出写点查询（`builtins/console::emit`）。
pub fn tty_output_enabled() -> bool {
    REPL_TTY_OUTPUT.load(Ordering::Relaxed)
}

/// 裸 `\n` → `\r\n`（已带 `\r` 的不动；raw mode 终端 LF 不回车，见上）。
pub fn crlf(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let mut prev_cr = false;
    for ch in s.chars() {
        if ch == '\n' && !prev_cr {
            out.push('\r');
        }
        out.push(ch);
        prev_cr = ch == '\r';
    }
    out
}

/// 连击判定（纯函数）：上次中断在窗口内即退出。
pub fn ctrl_c_should_exit(last: Option<Instant>, now: Instant) -> bool {
    matches!(last, Some(t) if now.duration_since(t) <= CTRL_C_WINDOW)
}

/// 点命令（行首 `.`；其余一律当代码）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dot {
    Exit,
    Help,
    /// `.doc <topic>`（整篇 MDN 文档；CLI 本体面，`node:repl` 不动）。
    Doc(String),
    Code,
}

/// 点命令分类（纯函数，单测覆盖）。
pub fn dot_command(line: &str) -> Dot {
    let t = line.trim();
    match t {
        ".exit" | ".quit" => Dot::Exit,
        ".help" => Dot::Help,
        ".doc" => Dot::Doc(String::new()),
        _ if t.starts_with(".doc ") || t.starts_with(".doc\t") => {
            Dot::Doc(t[4..].trim().to_string())
        }
        s if s.starts_with('.') => Dot::Code, // 未知点命令当代码求值（自然报错）
        _ => Dot::Code,
    }
}

/// 历史文件（`$HOME/.winterjs_history`；拿不到则 `None`，不存档）。
pub fn history_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".winterjs_history"))
}

/// 括号平衡（字符串/模板/行块注释感知；`}` 超前即 false）。
/// 纯函数，单测覆盖；`SnowValidator` 与黑盒多行行为都以它为准。
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

/// JS 关键字表（高亮 + 补全共用；保留字全收，不过度求全）。
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

/// 高亮 token 种类（`tokenize` 产出，双渲染共用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    Kw,
    Str,
    Num,
    Com,
    Plain,
}

/// 行切分（字节区间；注释/字符串/数字/标识符优先，余下逐字符透传）。
fn tokenize(line: &str) -> Vec<(Tok, &str)> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    while i < bytes.len() {
        let c = bytes[i];
        // 行注释。
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            out.push((Tok::Com, &line[i..]));
            break;
        }
        // 块注释（单行内；跨行块注释退化为普通文本，不求全）。
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let end = line[i..].find("*/").map(|e| i + e + 2).unwrap_or(line.len());
            out.push((Tok::Com, &line[i..end]));
            i = end;
            continue;
        }
        // 字符串（含模板整段；`${}` 内不细分，文档记录）。
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
            out.push((Tok::Str, &line[i..j.min(line.len())]));
            i = j;
            continue;
        }
        // 数字（digit 开头；`0x`/小数/指数一律 consume 到词边）。
        if c.is_ascii_digit() {
            let mut j = i + 1;
            while j < bytes.len() && (ident(bytes[j]) || bytes[j] == b'.') {
                j += 1;
            }
            out.push((Tok::Num, &line[i..j]));
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
            out.push((if is_kw(word) { Tok::Kw } else { Tok::Plain }, word));
            i = j;
            continue;
        }
        // 余下逐字符透传（UTF-8 安全：非 ASCII 按字符步进）。
        let ch_len = line[i..].chars().next().map(|ch| ch.len_utf8()).unwrap_or(1);
        out.push((Tok::Plain, &line[i..i + ch_len]));
        i += ch_len;
    }
    out
}

/// 行高亮（ANSI 串；单测钉 token 分类用；运行时走 `highlight_styled`）。
#[cfg(test)]
pub fn highlight_line(line: &str) -> String {
    use console::Style;
    let kw = Style::new().cyan();
    let st = Style::new().green();
    let num = Style::new().yellow();
    let com = Style::new().dim();
    let mut out = String::with_capacity(line.len() + 16);
    for (kind, text) in tokenize(line) {
        match kind {
            Tok::Kw => out.push_str(&kw.apply_to(text).to_string()),
            Tok::Str => out.push_str(&st.apply_to(text).to_string()),
            Tok::Num => out.push_str(&num.apply_to(text).to_string()),
            Tok::Com => out.push_str(&com.apply_to(text).to_string()),
            Tok::Plain => out.push_str(text),
        }
    }
    out
}

/// 行高亮（`StyledText`，TTY 实时渲染用；配色与 `highlight_line` 同族）。
pub fn highlight_styled(line: &str) -> reedline::StyledText {
    use nu_ansi_term::{Color, Style};
    let mut out = reedline::StyledText::new();
    for (kind, text) in tokenize(line) {
        let style = match kind {
            Tok::Kw => Style::new().fg(Color::Cyan),
            Tok::Str => Style::new().fg(Color::Green),
            Tok::Num => Style::new().fg(Color::Yellow),
            Tok::Com => Style::new().dimmed(),
            Tok::Plain => Style::new(),
        };
        out.push((style, text.to_owned()));
    }
    out
}

/// 静态补全表（点命令/关键字 + 一行签名文档；bare 全局名由动态补全给出，
/// 静态表同名项的描述优先——见 `merge_dynamic_static`）。
fn candidates() -> Vec<(&'static str, &'static str)> {
    let mut out = vec![
        (".exit", ".exit — quit the repl"),
        (".help", ".help — show repl help"),
        (".doc", ".doc <topic> — show MDN docs (e.g. .doc console.log)"),
        ("exit", "exit() — quit the repl"),
        ("quit", "quit() — quit the repl"),
        ("q", "q() — quit the repl"),
        ("console", "console.log/info/warn/error/dir/…"),
        ("process", "process — argv/env/exit()/on()"),
        ("Buffer", "Buffer.from/alloc/isBuffer (class)"),
        ("globalThis", "globalThis — the global object"),
        ("JSON", "JSON.parse(str) / stringify(v) / rawJSON()"),
        ("Math", "Math.floor/abs/random/… (namespace)"),
        ("Object", "Object.keys/assign/freeze/… (class)"),
        ("Array", "Array.from/of/isArray (class)"),
        ("String", "String.raw/fromCharCode/… (class)"),
        ("Number", "Number.isInteger/parseFloat/… (class)"),
        ("Boolean", "Boolean(value?) (class)"),
        ("BigInt", "BigInt(v) → bigint"),
        ("Symbol", "Symbol(desc?) → symbol"),
        ("Promise", "Promise.all/race/allSettled/any/withResolvers"),
        ("Map", "new Map() — set/get/has/delete"),
        ("Set", "new Set() — add/has/delete"),
        ("WeakMap", "new WeakMap() — set/get/has"),
        ("WeakSet", "new WeakSet() — add/has"),
        ("URL", "new URL(url, base?) — href/origin/…"),
        ("URLSearchParams", "new URLSearchParams(init?) — get/set/…"),
        ("TextEncoder", "new TextEncoder().encode(str) → Uint8Array"),
        ("TextDecoder", "new TextDecoder(label?).decode(buf) → string"),
        ("Blob", "new Blob(parts, opts?) — text()/arrayBuffer()"),
        ("fetch", "fetch(url, init?) → Promise<Response>"),
        ("structuredClone", "structuredClone(value, opts?) → clone"),
        ("setTimeout", "setTimeout(cb, ms?, ...args) → Timeout"),
        ("clearTimeout", "clearTimeout(t?: Timeout) → undefined"),
        ("setInterval", "setInterval(cb, ms?, ...args) → Timeout"),
        ("clearInterval", "clearInterval(t?: Timeout) → undefined"),
        ("queueMicrotask", "queueMicrotask(cb)"),
        ("require", "require(id) → exports (CJS)"),
        ("module", "module — exports/require/id (CJS)"),
        ("exports", "exports — alias of module.exports (CJS)"),
        ("__dirname", "__dirname — script directory (CJS)"),
        ("__filename", "__filename — script file path (CJS)"),
    ];
    for kw in KEYWORDS {
        out.push((kw, "keyword"));
    }
    out
}

/// CLI 补全跨线程投递（readline 线程 ↔ JS 线程；`JSContext` 是 `!Send`，
/// 真上下文枚举只在 JS 线程做——`runtime/repl` 桥调 `node:repl` 的
/// `cliComplete`）。id 配对防超时后迟到的旧回包错配。
#[derive(Debug)]
pub(crate) struct CompReq {
    pub id: u64,
    pub line: String,
}

#[derive(Debug)]
pub(crate) struct CompResp {
    pub id: u64,
    /// 候选 `(全文, 描述)`——全文写入 span 段，描述进 IdeMenu 右侧 pane
    /// （底座桥 `__wjs_cli_complete` 产出：签名/类型摘要，可 `None`）。
    pub items: Vec<(String, Option<String>)>,
    /// 行尾被替换段（R3 `completeOn` 语义）。
    pub complete_on: String,
}

/// `completeOn` → reedline 替换区间：completeOn 必须是行尾段（R3 语义），
/// 即 `(行长 - completeOn 长, 行长)`；不是行尾段时回 None（拒映射，不乱替换）。
fn complete_span(line: &str, complete_on: &str) -> Option<(usize, usize)> {
    if complete_on.is_empty() || complete_on.len() > line.len() || !line.ends_with(complete_on) {
        return None;
    }
    Some((line.len() - complete_on.len(), line.len()))
}

/// 静态+动态补全器（点命令/关键字静态表；bare/成员形经 JS 线程真上下文——
/// 成员链逐步求值/fs 路径/大小写不敏感，与 `node:repl` 模块补全同源）。
/// 超时或主循环退出时降级为纯静态（不阻塞行编辑）。
pub struct JsCompleter {
    req_tx: tokio::sync::mpsc::UnboundedSender<CompReq>,
    resp_rx: tokio::sync::mpsc::UnboundedReceiver<CompResp>,
    next_id: u64,
    /// 动态回包总窗（单测注入缩短；缺省 150ms）。
    timeout: Duration,
}

impl JsCompleter {
    pub fn new(
        req_tx: tokio::sync::mpsc::UnboundedSender<CompReq>,
        resp_rx: tokio::sync::mpsc::UnboundedReceiver<CompResp>,
    ) -> Self {
        Self { req_tx, resp_rx, next_id: 0, timeout: Duration::from_millis(150) }
    }

    /// 动态补全请求（readline 线程阻塞等回包；超时/断链回 None）。
    /// `line` 为光标前文本（R3 按行尾处理）。`UnboundedReceiver` 无同步带
    /// 超时的 recv——`try_recv` 微步轮询（仅 Tab 触发，200µs 步进开销可忽略）。
    fn request_dynamic(&mut self, line: &str) -> Option<(Vec<(String, Option<String>)>, String)> {
        use tokio::sync::mpsc::error::TryRecvError;
        self.next_id += 1;
        let id = self.next_id;
        self.req_tx.send(CompReq { id, line: line.to_owned() }).ok()?;
        let start = Instant::now();
        loop {
            match self.resp_rx.try_recv() {
                Ok(resp) if resp.id == id => return Some((resp.items, resp.complete_on)),
                Ok(_) => continue, // 前次超时后迟到的旧回包，丢弃
                Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) => {
                    if start.elapsed() >= self.timeout {
                        return None;
                    }
                    std::thread::sleep(Duration::from_micros(200));
                }
            }
        }
    }

    /// 动态回包 → 候选（span 由 completeOn 换算；映射失败即空）。
    fn dynamic_suggestions(&mut self, line: &str) -> Vec<reedline::Suggestion> {
        let mut out = Vec::new();
        if let Some((items, complete_on)) = self.request_dynamic(line)
            && let Some((s0, s1)) = complete_span(line, &complete_on)
        {
            for (value, description) in items {
                out.push(reedline::Suggestion {
                    value,
                    display_override: None,
                    description,
                    style: None,
                    extra: None,
                    span: reedline::Span::new(s0, s1),
                    append_whitespace: false,
                    match_indices: None,
                });
            }
        }
        out
    }
}

impl JsCompleter {
    fn suggest(prefix: &str, dot_mode: bool) -> Vec<reedline::Suggestion> {
        let low = prefix.to_lowercase();
        let mut out = Vec::new();
        for (word, doc) in candidates() {
            if dot_mode && !word.starts_with('.') {
                continue;
            }
            if !dot_mode && word.starts_with('.') {
                continue;
            }
            let key = if dot_mode { &word[1..] } else { word };
            if !key.to_lowercase().starts_with(&low) {
                continue;
            }
            out.push(reedline::Suggestion {
                value: if dot_mode {
                    key.to_owned()
                } else {
                    word.to_owned()
                },
                display_override: if dot_mode { Some(word.to_owned()) } else { None },
                description: Some(doc.to_owned()),
                style: None,
                extra: None,
                // span 由调用方按前缀回填。
                span: reedline::Span::new(0, 0),
                append_whitespace: false,
                match_indices: None,
            });
        }
        out
    }
}

impl reedline::Completer for JsCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> reedline::CompletionResult {
        let upto = line.get(..pos.min(line.len())).unwrap_or(line);
        // 点命令形（行首 `.` + 无空白）：补 `.help`/`.exit`（纯静态）。
        let trimmed = upto.trim_start();
        if let Some(rest) = trimmed.strip_prefix('.')
            && !rest.contains(char::is_whitespace)
        {
            let start = upto.len() - rest.len();
            let mut items = Self::suggest(rest, true);
            for s in &mut items {
                s.span = reedline::Span::new(start, pos);
            }
            return reedline::CompletionResult::fresh(items);
        }
        // bare 词尾起点（成员/静态共用）。
        let end = upto.len();
        let mut start = end;
        for (idx, ch) in upto.char_indices().rev() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                start = idx;
            } else {
                break;
            }
        }
        let prefix = &upto[start..end];
        let member_form = start > 0 && upto.as_bytes()[start - 1] == b'.';
        if prefix.is_empty() {
            // `console.` 点后空前缀：动态全键枚举（R3 filter="" 口径）。
            if member_form {
                return reedline::CompletionResult::fresh(self.dynamic_suggestions(upto));
            }
            // 空 bare 前缀：空集（R3 `bm === null` 同形）。
            return reedline::CompletionResult::fresh(Vec::new());
        }
        // 动态优先：成员链逐步求值 / bare 上下文键（与 node:repl 模块同源）。
        let mut items = self.dynamic_suggestions(upto);
        // 静态表合并：仅 bare 面（静态表无成员形）；同 value 动态在前不重。
        if !member_form {
            let seen: std::collections::HashSet<String> =
                items.iter().map(|s| s.value.clone()).collect();
            for mut s in Self::suggest(prefix, false) {
                if seen.contains(&s.value) {
                    continue;
                }
                s.span = reedline::Span::new(start, pos);
                items.push(s);
            }
        }
        reedline::CompletionResult::fresh(items)
    }
}

/// 提示符（左 `❄` + indicator `> ` = `❄> `，无编号；用户口径）。
pub struct SnowPrompt;

impl reedline::Prompt for SnowPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed("❄")
    }
    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }
    fn render_prompt_indicator(&self, _mode: reedline::PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("> ")
    }
    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("> ")
    }
    fn render_prompt_history_search_indicator(
        &self,
        _search: reedline::PromptHistorySearch,
    ) -> Cow<'_, str> {
        Cow::Borrowed("❄> ")
    }
}

/// 括号续行校验（`brace_balance` 即真相）。
pub struct SnowValidator;

impl reedline::Validator for SnowValidator {
    fn validate(&self, line: &str) -> reedline::ValidationResult {
        if brace_balance(line) {
            reedline::ValidationResult::Complete
        } else {
            reedline::ValidationResult::Incomplete
        }
    }
}

/// 实时高亮（`highlight_styled` 即真相）。
pub struct SnowHighlighter;

impl reedline::Highlighter for SnowHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> reedline::StyledText {
        highlight_styled(line)
    }
}

/// readline 线程主函数（阻塞 IO 独占线程；行经 `tx` 发主循环，`None` 表 EOF）。
/// TTY 下走 reedline（IdeMenu/历史/高亮）；非 TTY（管道/黑盒）逐行直读无 ANSI。
/// 历史加载失败忽略；`FileBackedHistory` drop 时尽力存档。
///
/// 哨兵协议（`flush_rx`）：TTY 下发一行后先收干积压旧哨兵，再阻塞等主循环
/// 的"本轮输出完毕"哨兵——期间**不进 `read_line`**（终端 raw 已退，主循环的
/// 多行输出/错误框 ONLCR 正常换行，且下一轮 prompt 不与求值输出竞争渲染）。
/// 主循环退出（`flush_tx` drop）→ `blocking_recv` 返回 `None` → 线程收尾。
///
/// 补全投递（`comp_req_tx`/`comp_resp_rx`）：Tab 时经 JS 线程真上下文补全
/// （`JsCompleter` 注记），超时降级静态表。
#[allow(clippy::too_many_arguments)]
pub fn readline_loop(
    tx: tokio::sync::mpsc::UnboundedSender<Option<String>>,
    history: Option<PathBuf>,
    mut flush_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    comp_req_tx: tokio::sync::mpsc::UnboundedSender<CompReq>,
    comp_resp_rx: tokio::sync::mpsc::UnboundedReceiver<CompResp>,
) {
    if !std::io::stdin().is_terminal() {
        // 非 TTY：逐行直读（仍无 ANSI；`.exit` 等由主循环分类）。同 TTY 哨兵
        // 协议背压——行按序处理，TLA 挂起期间管道自然阻塞（node 语义：await
        // 期间不收新输入）。
        let stdin = std::io::stdin();
        for line in stdin.lines() {
            let Ok(line) = line else { break };
            if tx.send(Some(line)).is_err() {
                break;
            }
            while flush_rx.try_recv().is_ok() {}
            if flush_rx.blocking_recv().is_none() {
                break;
            }
        }
        let _ = tx.send(None);
        return;
    }
    use reedline::{
        MenuBuilder as _, default_emacs_keybindings, Emacs, FileBackedHistory, IdeMenu, KeyCode,
        KeyModifiers, Reedline, ReedlineEvent, ReedlineMenu, Signal,
    };
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    let edit_mode = Box::new(Emacs::new(keybindings));
    let mut rl = Reedline::create()
        .with_completer(Box::new(JsCompleter::new(comp_req_tx, comp_resp_rx)))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(
            IdeMenu::default().with_name("completion_menu"),
        )))
        .with_highlighter(Box::new(SnowHighlighter))
        .with_validator(Box::new(SnowValidator))
        .with_edit_mode(edit_mode);
    if let Some(h) = &history
        && let Ok(db) = FileBackedHistory::with_file(1000, h.clone())
    {
        rl = rl.with_history(Box::new(db));
    }
    let prompt = SnowPrompt;
    // 上次 Ctrl-C 时刻（连击窗口判定用；首击只提示）。
    let mut last_interrupt: Option<Instant> = None;
    loop {
        match rl.read_line(&prompt) {
            Ok(Signal::Success(line)) => {
                if tx.send(Some(line)).is_err() {
                    break;
                }
                // 哨兵协议：清 tick 轮积压的旧哨兵，再等本轮输出完毕
                // （`None` = 主循环退出，同 EOF 路径收尾）。
                while flush_rx.try_recv().is_ok() {}
                if flush_rx.blocking_recv().is_none() {
                    break;
                }
            }
            // Ctrl-C：窗内连击即退出（`None` 表 EOF，同 Ctrl-D 路径收尾）；
            // 首击只提示，不断会话。
            Ok(Signal::CtrlC) => {
                let now = Instant::now();
                if ctrl_c_should_exit(last_interrupt, now) {
                    let _ = tx.send(None);
                    break;
                }
                last_interrupt = Some(now);
                println!("(To exit, press Ctrl+C again or Ctrl+D)");
            }
            // Ctrl-D / EOF / 其他中止：退出。
            Ok(_) | Err(_) => {
                let _ = tx.send(None);
                break;
            }
        }
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
        assert_eq!(dot_command(".doc"), Dot::Doc(String::new()));
        assert_eq!(
            dot_command(".doc console.log"),
            Dot::Doc("console.log".to_string())
        );
        assert_eq!(dot_command(".doctrine"), Dot::Code);
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

    #[test]
    fn styled_roundtrip() {
        // StyledText 拼回即原文（TTY 渲染不断行不错位）。
        for line in ["const s = 'hi' + 42; // c", "❄> emoji", "`a${b}c`", ""] {
            let st = highlight_styled(line);
            assert_eq!(st.raw_string(), line, "{line}");
        }
    }

    #[test]
    fn crlf_table() {
        // CRLF 化：裸 LF 插 CR；已带 CR 不动；空串/无 LF 恒等。
        assert_eq!(crlf("a\nb"), "a\r\nb");
        assert_eq!(crlf("a\r\nb"), "a\r\nb");
        assert_eq!(crlf("\n"), "\r\n");
        assert_eq!(crlf("abc"), "abc");
        assert_eq!(crlf(""), "");
        assert_eq!(crlf("a\n\nb"), "a\r\n\r\nb");
    }

    #[test]
    fn prompt_shape() {
        // 提示符保持 ❄>（左 ❄ + indicator >），无编号无 irb 字样。
        let p = SnowPrompt;
        use reedline::Prompt as _;
        assert_eq!(p.render_prompt_left(), "❄");
        assert_eq!(p.render_prompt_indicator(reedline::PromptEditMode::Default), "> ");
        assert_eq!(p.render_prompt_right(), "");
    }

    #[test]
    fn validator_mirrors_brace() {
        // 校验器即 brace_balance 的镜像（正常/报错/边界）。
        use reedline::Validator as _;
        let v = SnowValidator;
        assert!(matches!(
            v.validate("1 + 1"),
            reedline::ValidationResult::Complete
        ));
        assert!(matches!(
            v.validate("function f() {"),
            reedline::ValidationResult::Incomplete
        ));
        assert!(matches!(v.validate("}"), reedline::ValidationResult::Incomplete));
    }

    #[test]
    fn completer_faces() {
        // 补全三件：点命令形（静态）+ bare 降级面（通道无回应 → 静态关键字）
        // + 边界（空前缀/超时窗）。动态真上下文面由黑盒盖（需 JS 线程桥）。
        use reedline::Completer as _;
        let (req_tx, _req_rx) = tokio::sync::mpsc::unbounded_channel::<CompReq>();
        let (resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel::<CompResp>();
        let mut c = JsCompleter { req_tx, resp_rx, next_id: 0, timeout: Duration::from_millis(1) };
        // 点命令：`.he` → 显示 `.help`（value 补剩余部分，不吞点）。
        let dots = match c.complete(".he", 3) {
            reedline::CompletionResult::Fresh { suggestions, .. } => suggestions.to_vec(),
            _ => panic!("expected fresh"),
        };
        let help = dots.iter().find(|s| s.value == "help").expect("help");
        assert_eq!(help.display_override.as_deref(), Some(".help"));
        // bare：动态无回应（空通道）→ 超时降级静态（keyword/CLI 名）。
        let items = match c.complete("con", 3) {
            reedline::CompletionResult::Fresh { suggestions, .. } => suggestions.to_vec(),
            _ => panic!("expected fresh"),
        };
        assert!(items.iter().any(|s| s.value == "console"), "degraded static: {items:?}");
        assert!(items.iter().any(|s| s.value == "const"), "keywords merged: {items:?}");
        // 报错/边界：空前缀空集；成员形动态无回应即空集（不炸菜单）。
        for (line, pos) in [("", 0), ("console.", 8), ("1 + ", 4)] {
            match c.complete(line, pos) {
                reedline::CompletionResult::Fresh { suggestions, .. } => {
                    assert!(suggestions.is_empty(), "{line:?}")
                }
                _ => panic!("expected fresh for {line:?}"),
            }
        }
        drop(resp_tx); // 断链路径：resp 通道关 → request_dynamic None。
    }

    #[test]
    fn complete_span_table() {
        // completeOn→span：行尾段正常映射；非行尾段/超长/空拒映射。
        assert_eq!(complete_span("const gl", "gl"), Some((6, 8)));
        assert_eq!(complete_span("globalThis.fr", "globalThis.fr"), Some((0, 13)));
        assert_eq!(complete_span("abc", "x"), None);
        assert_eq!(complete_span("abc", "abcd"), None);
        assert_eq!(complete_span("abc", ""), None);
    }
}
