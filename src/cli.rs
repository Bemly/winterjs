use std::path::PathBuf;
use std::sync::LazyLock;

use clap::{CommandFactory, Parser};

pub static VERSION_TEXT: LazyLock<String> = LazyLock::new(|| {
    let sha = option_env!("VERGEN_GIT_SHA").unwrap_or("unknown");
    let describe = option_env!("VERGEN_GIT_DESCRIBE").unwrap_or("unknown");
    let timestamp = option_env!("VERGEN_BUILD_TIMESTAMP").unwrap_or("unknown");
    format!(
        "{} ({} {}, built {})",
        env!("CARGO_PKG_VERSION"),
        describe,
        &sha[..sha.len().min(12)],
        timestamp
    )
});

/// 全 flag 规范（AGENTS §0.8）：无裸子命令、无裸位置参数（`--run` 的脚本透传除外）；
/// 一次恰好一个动作 flag；修饰 flag 只在对应动作下生效。
#[derive(Parser, Debug)]
#[command(
    name = "winterjs",
    version = &**VERSION_TEXT,
    about = "Bun-like JS runtime on SpiderMonkey",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Increase log verbosity (-v: info, -vv: debug, -vvv: trace)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Language for help text (en/zh; defaults to the system language)
    #[arg(short = 'l', long = "lang", value_name = "LANG",
        value_parser = clap::builder::PossibleValuesParser::new(["en", "zh"]))]
    pub lang: Option<String>,

    // ── 动作（恰好其一） ──────────────────────────────────────────────
    /// Run a JS file (with a script extension) or a package.json script (bare name), and print its completion value
    #[arg(short = 'r', long = "run", value_name = "FILE")]
    pub run: Option<PathBuf>,

    /// Evaluate inline JS code and print its completion value
    #[arg(short = 'e', long = "eval", value_name = "CODE")]
    pub eval: Option<String>,

    /// Show resolved settings (or their JSON Schema with --schema)
    #[arg(short = 'c', long = "config")]
    pub config: bool,

    /// Print a shell completion script for the given shell
    #[arg(long = "completions", value_name = "SHELL", value_enum)]
    pub completions: Option<clap_complete::Shell>,

    /// Print roff manual pages to stdout
    #[arg(short = 'm', long = "man")]
    pub man: bool,

    /// Add packages to the current project (local node_modules)
    #[arg(short = 'a', long = "add", value_name = "PKG", num_args = 1..)]
    pub add: Vec<String>,

    /// Install packages globally (shared data directory)
    #[arg(short = 'i', long = "install", value_name = "PKG", num_args = 1..)]
    pub install: Vec<String>,

    /// Remove packages from the current project (local node_modules)
    #[arg(short = 'R', long = "remove", value_name = "PKG", num_args = 1..)]
    pub remove: Vec<String>,

    /// Uninstall packages installed globally (shared data directory)
    #[arg(short = 'U', long = "uninstall", value_name = "PKG", num_args = 1..)]
    pub uninstall: Vec<String>,

    /// Publish the current package (dry-run validates only)
    #[arg(short = 'p', long = "publish")]
    pub publish: bool,

    /// Log in to a registry (stores a token in ~/.npmrc)
    #[arg(long = "login")]
    pub login: bool,

    /// Self-upgrade winterjs (needs WINTERJS_UPDATE_GITHUB=owner/repo)
    #[arg(short = 'u', long = "upgrade")]
    pub upgrade: bool,

    /// Scaffold a new package (installs package.json dependencies when present)
    #[arg(short = 'I', long = "init", value_name = "NAME", num_args = 0..=1, default_missing_value = "")]
    pub init: Option<String>,

    /// Start an interactive REPL
    #[arg(long = "repl")]
    pub repl: bool,

    /// Run test files (no paths: discover them from the current directory)
    #[arg(short = 't', long = "test", value_name = "PATH", num_args = 0..)]
    pub test: Option<Vec<String>>,

    /// Forward to oxlint (found in node_modules/.bin or PATH; args pass through verbatim)
    #[arg(long = "lint", value_name = "ARGS", num_args = 0.., allow_hyphen_values = true)]
    pub lint: Option<Vec<String>>,

    /// Forward to oxfmt (found in node_modules/.bin or PATH; args pass through verbatim)
    #[arg(short = 'f', long = "fmt", value_name = "ARGS", num_args = 0.., allow_hyphen_values = true)]
    pub fmt: Option<Vec<String>>,

    /// Serve a directory or a JS handler over HTTP (H1/H2/H3; TLS via --cert/--key or ACME)
    #[arg(short = 's', long = "serve", value_name = "DIR", num_args = 0..=1, default_missing_value = ".")]
    pub serve: Option<String>,

    // ── 修饰（只在对应动作下生效） ────────────────────────────────────
    /// Script arguments for --run (as `process.argv.slice(2)`)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Resolve/validate only and print, do not write anything (add/install/remove/uninstall/publish/init/upgrade, --serve ACME)
    #[arg(long)]
    pub dry_run: bool,

    /// Registry base URL for package actions (add/install/publish/login/init; default https://registry.npmjs.org)
    #[arg(long)]
    pub registry: Option<String>,

    /// Dist-tag to publish under (publish only)
    #[arg(long, default_value = "latest")]
    pub tag: String,

    /// Auth token to store (login only; prompted on TTY when omitted)
    #[arg(long)]
    pub token: Option<String>,

    /// Print an OAuth authorization URL instead (login only; code exchange deferred)
    #[arg(long)]
    pub oauth: bool,

    /// Package name for --init (default: current directory name)
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Skip the confirmation prompt (init only)
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Overwrite conflicting files instead of failing (init only)
    #[arg(long)]
    pub force: bool,

    /// Only run files matching this glob (test only; matched against relative path or file name)
    #[arg(long)]
    pub filter: Option<String>,

    /// Only run tests whose full name matches (test only; substring or /regex/flags)
    #[arg(long, value_name = "PATTERN")]
    pub test_name_pattern: Option<String>,

    /// Re-run when watched files change (test/run/serve only; Ctrl-C to stop)
    #[arg(long)]
    pub watch: bool,

    /// Directory to serve (serve only)
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,

    /// Interface to bind (serve only)
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to bind (serve only; 0 = ephemeral, actual port printed on stdout)
    #[arg(long, default_value_t = 3000)]
    pub port: u16,

    /// JS handler file for --serve (export default { fetch } or export function fetch)
    #[arg(long, value_name = "FILE")]
    pub handler: Option<PathBuf>,

    /// Requests per second limit (serve only; 0 = unlimited)
    #[arg(long, default_value_t = 0)]
    pub limit_rps: u32,

    /// TLS certificate (serve only; PEM, must come with --key)
    #[arg(long)]
    pub cert: Option<PathBuf>,

    /// TLS private key (serve only; PEM, must come with --cert)
    #[arg(long)]
    pub key: Option<PathBuf>,

    /// ACME domain for automatic certificates (serve only; default winterjs.bemly.moe; needs --acme-email)
    #[arg(long, value_name = "DOMAIN")]
    pub acme_domain: Option<String>,

    /// ACME account email (serve only; mailto contact for Let's Encrypt)
    #[arg(long, value_name = "EMAIL")]
    pub acme_email: Option<String>,

    /// ACME cache directory (serve only; default system cache)
    #[arg(long, value_name = "DIR")]
    pub acme_cache: Option<PathBuf>,

    /// Use Let's Encrypt production (serve only; default staging, safe against rate limits)
    #[arg(long)]
    pub acme_production: bool,

    /// Print the JSON Schema of the settings file instead (config only)
    #[arg(long)]
    pub schema: bool,

    #[command(flatten)]
    pub perms: PermissionArgs,
}

/// `--allow-*` 权限旗标（任一出现即进沙箱，Bun 同款 opt-in）。
/// 旗标无值 = 该类全开；`=a,b` 或重复出现 = 允许清单。
#[derive(clap::Args, Clone, Debug, Default)]
pub struct PermissionArgs {
    /// Allow filesystem reads (run/eval/test/repl only; optionally: --allow-read=<path>[,<path>...])
    #[arg(long, value_name = "PATH", num_args = 0..=1, require_equals = true, value_delimiter = ',')]
    pub allow_read: Option<Vec<String>>,
    /// Allow filesystem writes (run/eval/test/repl only; optionally: --allow-write=<path>[,<path>...])
    #[arg(long, value_name = "PATH", num_args = 0..=1, require_equals = true, value_delimiter = ',')]
    pub allow_write: Option<Vec<String>>,
    /// Allow environment variable access (run/eval/test/repl only; optionally: --allow-env=<VAR>[,<VAR>...])
    #[arg(long, value_name = "VAR", num_args = 0..=1, require_equals = true, value_delimiter = ',')]
    pub allow_env: Option<Vec<String>>,
    /// Allow spawning child processes (run/eval/test/repl only; optionally: --allow-run=<cmd>[,<cmd>...])
    #[arg(long, value_name = "CMD", num_args = 0..=1, require_equals = true, value_delimiter = ',')]
    pub allow_run: Option<Vec<String>>,
    /// Allow FFI (run/eval/test/repl only; dlopen of native libraries)
    #[arg(long)]
    pub allow_ffi: bool,
    /// Allow everything (run/eval/test/repl only; no sandbox)
    #[arg(long)]
    pub allow_all: bool,
}

impl Cli {
    /// 命中的动作名（恰好其一由 dispatch 强制；0 个时 `arg_required_else_help`
    /// 已提前打印 help，只有透传 `args` 残留能到这里）。
    pub fn actions_present(&self) -> Vec<&'static str> {
        let mut v = Vec::with_capacity(1);
        if self.run.is_some() {
            v.push("--run");
        }
        if self.eval.is_some() {
            v.push("--eval");
        }
        if self.config {
            v.push("--config");
        }
        if self.completions.is_some() {
            v.push("--completions");
        }
        if self.man {
            v.push("--man");
        }
        if !self.add.is_empty() {
            v.push("--add");
        }
        if !self.install.is_empty() {
            v.push("--install");
        }
        if !self.remove.is_empty() {
            v.push("--remove");
        }
        if !self.uninstall.is_empty() {
            v.push("--uninstall");
        }
        if self.publish {
            v.push("--publish");
        }
        if self.login {
            v.push("--login");
        }
        if self.upgrade {
            v.push("--upgrade");
        }
        if self.init.is_some() {
            v.push("--init");
        }
        if self.repl {
            v.push("--repl");
        }
        if self.test.is_some() {
            v.push("--test");
        }
        if self.lint.is_some() {
            v.push("--lint");
        }
        if self.fmt.is_some() {
            v.push("--fmt");
        }
        if self.serve.is_some() {
            v.push("--serve");
        }
        v
    }
}

/// 双语 Command 构造器：derive 生成的是英文骨架，这里按当前 locale
///（`i18n::init_from_argv` 已在 `main` 起点定好）把 about/help/value_name
/// 换成 `t!` 查表。key 规则：`app.about` / `arg.{id}.help`（+`.value`）；
/// 缺 key（`t!` 回显 key 本身）则保留原文 → 英文输出逐字节不变。
/// 已知局限：clap 自带词（Usage/Options）与 clap 自动报错保持英文。
pub fn localized_command() -> clap::Command {
    let cmd = Cli::command();
    let cmd = match cmd.get_about().map(|s| s.to_string()) {
        Some(orig) => cmd.about(tr_or("app.about", &orig)),
        None => cmd,
    };
    with_localized_args(cmd)
}

fn with_localized_args(mut cmd: clap::Command) -> clap::Command {
    // 先收 id（`mut_arg` 是 by-value builder，不能边遍历边改，见 AGENTS §4.25）
    let ids: Vec<String> = cmd.get_arguments().map(|a| a.get_id().to_string()).collect();
    for id in &ids {
        // 不可变借用下算好译文；与原文相同则回 None（英文 locale 零改动，
        // 保证英文输出逐字节不变，也省掉 `value_name` 的泄漏）。
        let (new_help, new_value): (Option<String>, Option<String>) = match cmd
            .get_arguments()
            .find(|a| a.get_id().to_string() == *id)
        {
            Some(a) => {
                let h = a.get_help().and_then(|s| {
                    let o = s.to_string();
                    let t = tr_or(&format!("arg.{id}.help"), &o);
                    (t != o).then_some(t)
                });
                let v = a.get_value_names().and_then(|names| names.first()).and_then(|s| {
                    let o = s.to_string();
                    let t = tr_or(&format!("arg.{id}.value"), &o);
                    (t != o).then_some(t)
                });
                (h, v)
            }
            None => (None, None),
        };
        if new_help.is_none() && new_value.is_none() {
            continue;
        }
        cmd = cmd.mut_arg(id.as_str(), |a| {
            let a = match new_help {
                // `help` 收 owned String，无泄漏
                Some(h) => a.help(h),
                None => a,
            };
            match new_value {
                // `value_name` 只收 `&'static str`（`Str` 无 `From<String>`）：
                // 译文泄漏一次，进程生命周期内有效（中文 locale 下约 60 个短串）。
                Some(v) => {
                    let leaked: &'static str = Box::leak(v.into_boxed_str());
                    a.value_name(leaked)
                }
                None => a,
            }
        });
    }
    cmd
}

/// 查表命中即译文；缺 key（`t!` 回显 key 本身）则回原文。
fn tr_or(key: &str, original: &str) -> String {
    let hit = rust_i18n::t!(key).to_string();
    if hit == key { original.to_string() } else { hit }
}

/// Node 兼容旗（node 测试套件 `common.js` 自举 respawn / 子进程自举透传的
/// node 运行时旗）。winterjs 无同名动作：解析前剥除（`--flag=value` 形整项剥），
/// 剥下的名单交调用方记录（execArgv 保真 + 语义旗按需生效，见 `process_::record_node_compat`）。
/// §0.8 不受影响：不新增动作/位置参数；裸文件补 `--run` 仅在剥除发生时
/// （node-spawn 上下文的证据，与 `__selfArgv`/`__selfCmd` 同款"自举翻译"），
/// 纯 `winterjs file.js`（无兼容旗）照旧报错。
pub const NODE_COMPAT_FLAGS: &[&str] = &[
    "--expose-internals",
    "--expose-gc",
    "--expose_gc",
    "--insecure-http-parser",
    "--allow_natives_syntax",
    "--allow-natives-syntax",
    // 语义旗（max-header-size 套件：值参与默认头限，需透传值；见下 VALUE_FLAGS）。
    "--max-http-header-size",
];

/// 是否 node 运行时兼容旗：本表（语义旗/兼容旗）或 `cli_node_flags::NODE_RUNTIME_FLAGS`
/// 精确名单（按 `--k` 基名；**不收前缀族**，见 pitfalls 4.209）。
pub fn is_node_compat_flag(arg: &str) -> bool {
    let base = arg.split('=').next().unwrap_or("");
    if NODE_COMPAT_FLAGS.contains(&base) {
        return true;
    }
    // winterjs 自有同名旗（如 `--allow-ffi`）永远归 winterjs，不当 node 旗剥除。
    if winterjs_longs().contains(base) {
        return false;
    }
    // 必须带值的旗只认 `--k=v` 整项（空格分隔值会吞脚本名）。
    if crate::cli_node_flags::VALUE_REQUIRED.contains(&base) && !arg.contains('=') {
        return false;
    }
    crate::cli_node_flags::NODE_RUNTIME_FLAGS.contains(&base)
}

/// winterjs 自身全部长旗名（`--xxx`，含子结构 flatten 的）。
fn winterjs_longs() -> &'static std::collections::HashSet<String> {
    static SET: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    SET.get_or_init(|| {
        <Cli as clap::CommandFactory>::command()
            .get_arguments()
            .filter_map(|a| a.get_long().map(|l| format!("--{l}")))
            .collect()
    })
}

/// 取值形兼容旗（空格分隔值也一并剥除/记录；其余旗只认 `--k=v` 整项）。
const COMPAT_VALUE_FLAGS: &[&str] = &["--max-http-header-size"];

/// 动作旗（条件 `--run` 插入时判"已有显式动作"用；`-v/-l` 修饰旗不在内）。
const COMPAT_ACTION_FLAGS: &[&str] = &[
    "-r", "--run", "-e", "--eval", "-c", "--config", "--completions", "-m", "--man",
    "-a", "--add", "-i", "--install", "-R", "--remove", "-U", "--uninstall",
    "-p", "--publish", "--login", "-u", "--upgrade",
    "-I", "--init", "--repl", "-t", "--test", "--lint", "-f", "--fmt", "-s", "--serve",
];

/// 剥除 node 兼容旗；返回（过滤后 argv，含 bin；被剥旗原文，execArgv 保真）。
/// 条件 `--run` 插入：剥过旗、过滤后首个位置参数非旗形、且无显式动作时，
/// 在首个位置参数前补 `--run`（`node --flags file args...` 形）。
pub fn strip_node_compat_args(
    raw: &[std::ffi::OsString],
) -> (Vec<std::ffi::OsString>, Vec<String>) {
    use std::ffi::OsString;
    let mut out: Vec<OsString> = Vec::with_capacity(raw.len());
    let mut stripped: Vec<String> = Vec::new();
    let mut it = raw.iter().enumerate().peekable();
    let mut script_args = false;
    while let Some((i, a)) = it.next() {
        if i == 0 {
            out.push(a.clone());
            continue;
        }
        let s = a.to_string_lossy();
        // `--` 之后是脚本参数，原样透传（勿剥脚本自有旗）。
        if script_args {
            out.push(a.clone());
            continue;
        }
        if s == "--" {
            script_args = true;
            out.push(a.clone());
            continue;
        }
        let base = s.split('=').next().unwrap_or("");
        if is_node_compat_flag(&s) {
            // 记录原文（含值，execArgv 保真；语义旗按原文解析）。
            stripped.push(s.to_string());
            // 取值形旗的空格分隔值一并剥除（`--max-http-header-size 10`）。
            // 仅 VALUE_FLAGS 名单，防止吞脚本名。
            if !s.contains('=') && COMPAT_VALUE_FLAGS.contains(&base) {
                if let Some((_, nxt)) = it.peek() {
                    let ns = nxt.to_string_lossy();
                    if !ns.starts_with('-') {
                        stripped.push(ns.to_string());
                        it.next();
                    }
                }
            }
            continue;
        }
        out.push(a.clone());
    }
    if !stripped.is_empty() && out.len() > 1 {
        // node 自举翻译（max-header-size 套件：子进程即自身，`--flag -p expr`
        // 形；`-p`（node print）→ `--eval`（本仓 --eval 即打印完成值）。
        // 仅剥过兼容旗时（node-spawn 上下文证据），纯 `winterjs -p` 照旧 publish。
        // `-e` 本就同 `--eval`，统一改写无害。
        if out.len() > 2 && (out[1] == "-p" || out[1] == "-e") {
            out[1] = OsString::from("--eval");
        }
        let has_action = out[1..].iter().any(|a| {
            let s = a.to_string_lossy();
            COMPAT_ACTION_FLAGS.contains(&s.as_ref())
                || s.starts_with("--run=")
                || s.starts_with("--eval=")
        });
        if !has_action && !out[1].to_string_lossy().starts_with('-') {
            out.insert(1, OsString::from("--run"));
        }
    }
    (out, stripped)
}

#[cfg(test)]
mod node_compat_tests {
    use super::*;
    use std::ffi::OsString;

    fn argv(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }
    fn strs(v: &[OsString]) -> Vec<String> {
        v.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn strip_and_rerun() {
        // respawn 形：剥旗 + 补 --run（记录原文）。
        let (f, s) = strip_node_compat_args(&argv(&["w", "--expose-internals", "a.js", "child"]));
        assert_eq!(strs(&f), ["w", "--run", "a.js", "child"]);
        assert_eq!(s, ["--expose-internals"]);
        // 多旗 + =值形（原文记录）。
        let (f, s) = strip_node_compat_args(&argv(&["w", "--expose-gc", "--allow_natives_syntax=1", "a.js"]));
        assert_eq!(strs(&f), ["w", "--run", "a.js"]);
        assert_eq!(s, ["--expose-gc", "--allow_natives_syntax=1"]);
        // 取值形空格分隔（值一并剥除记录；-p 随自举翻译走 --eval）。
        let (f, s) = strip_node_compat_args(&argv(&["w", "--max-http-header-size", "10", "-p", "x"]));
        assert_eq!(strs(&f), ["w", "--eval", "x"]);
        assert_eq!(s, ["--max-http-header-size", "10"]);
        // `--` 后脚本旗不动。
        let (f, s) = strip_node_compat_args(&argv(&["w", "--run", "a.js", "--", "--expose-gc"]));
        assert_eq!(strs(&f), ["w", "--run", "a.js", "--", "--expose-gc"]);
        assert!(s.is_empty());
        // node 自举 `-p` 翻译（仅剥过旗时；纯 -p 不动，见 keeps_publish_bare）。
        let (f, _) = strip_node_compat_args(&argv(&["w", "--max-http-header-size=10", "-p", "1+1"]));
        assert_eq!(strs(&f), ["w", "--eval", "1+1"]);
    }

    #[test]
    fn node_runtime_flags_by_rule() {
        // D1：node 运行时旗（精确名单 + 前缀族）剥除并补 --run / -e→--eval。
        let (f, s) = strip_node_compat_args(&argv(&["w", "--pending-deprecation", "a.js", "x"]));
        assert_eq!(strs(&f), ["w", "--run", "a.js", "x"]);
        assert_eq!(s, ["--pending-deprecation"]);
        let (f, s) = strip_node_compat_args(&argv(&["w", "--experimental-stream-iter", "-e", "1"]));
        assert_eq!(strs(&f), ["w", "--eval", "1"]);
        assert_eq!(s, ["--experimental-stream-iter"]);
        let (f, s) = strip_node_compat_args(&argv(&["w", "--stack-trace-limit=3", "--no-warnings", "a.mjs"]));
        assert_eq!(strs(&f), ["w", "--run", "a.mjs"]);
        assert_eq!(s, ["--stack-trace-limit=3", "--no-warnings"]);
        // 边界（4.209）：未知的前缀形旗不收——交 clap 报错，绝不剥除后重跑。
        assert!(!is_node_compat_flag("--experimental-foo-bar"));
        assert!(!is_node_compat_flag("--trace-bogus"));
    }

    #[test]
    fn node_rule_spares_winterjs_flags() {
        // 边界：winterjs 同名/修饰旗（--watch/--test/--dry-run）与取值形空格值不被吞。
        for a in ["--watch", "--test", "--dry-run", "--port", "--stack-trace-limit"] {
            assert!(!is_node_compat_flag(a), "{a}");
        }
        let (f, s) = strip_node_compat_args(&argv(&["w", "--test", "--watch"]));
        assert_eq!(strs(&f), ["w", "--test", "--watch"]);
        assert!(s.is_empty());
        // `--` 之后的脚本旗原样透传。
        let (f, _) = strip_node_compat_args(&argv(&["w", "--run", "a.js", "--", "--no-warnings"]));
        assert_eq!(strs(&f), ["w", "--run", "a.js", "--", "--no-warnings"]);
    }

    #[test]
    fn keeps_publish_bare() {
        // 无兼容旗的裸 `-p` 照旧是 publish（§0.8）。
        let (f, s) = strip_node_compat_args(&argv(&["w", "-p"]));
        assert_eq!(strs(&f), ["w", "-p"]);
        assert!(s.is_empty());
    }

    #[test]
    fn keeps_explicit_action() {
        // 已有显式动作不补 --run。
        let (f, s) = strip_node_compat_args(&argv(&["w", "--expose-gc", "--eval", "1+1"]));
        assert_eq!(strs(&f), ["w", "--eval", "1+1"]);
        assert_eq!(s, ["--expose-gc"]);
        // 无兼容旗的裸文件不动（§0.8：纯裸形照旧报错）。
        let (f, s) = strip_node_compat_args(&argv(&["w", "a.js"]));
        assert_eq!(strs(&f), ["w", "a.js"]);
        assert!(s.is_empty());
        // 仅旗无文件：不过补（交 clap 按无动作报错）。
        let (f, _) = strip_node_compat_args(&argv(&["w", "--expose-gc"]));
        assert_eq!(strs(&f), ["w"]);
    }
}
