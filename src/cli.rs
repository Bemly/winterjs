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
    /// Run a JS file and print its completion value
    #[arg(short = 'r', long = "run", value_name = "FILE")]
    pub run: Option<PathBuf>,

    /// Evaluate inline JS code
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

    /// Publish the current package (Phase 5d; dry-run validates only)
    #[arg(short = 'p', long = "publish")]
    pub publish: bool,

    /// Log in to a registry (Phase 5d; stores a token in ~/.npmrc)
    #[arg(long = "login")]
    pub login: bool,

    /// Self-upgrade winterjs (Phase 5d; needs WINTERJS_UPDATE_GITHUB=owner/repo)
    #[arg(short = 'u', long = "upgrade")]
    pub upgrade: bool,

    /// Scaffold a new package (Phase 7)
    #[arg(long = "init", value_name = "NAME", num_args = 0..=1, default_missing_value = "")]
    pub init: Option<String>,

    /// Start an interactive REPL (Phase 7)
    #[arg(long = "repl")]
    pub repl: bool,

    /// Run test files (Phase 7)
    #[arg(short = 't', long = "test", value_name = "PATH", num_args = 0..)]
    pub test: Option<Vec<String>>,

    /// Forward to the project's oxlint (passthrough; args go to oxlint verbatim, npx fallback)
    #[arg(long = "lint", value_name = "ARGS", num_args = 0.., allow_hyphen_values = true)]
    pub lint: Option<Vec<String>>,

    /// Forward to the project's oxfmt (passthrough; args go to oxfmt verbatim, npx fallback)
    #[arg(short = 'f', long = "fmt", value_name = "ARGS", num_args = 0.., allow_hyphen_values = true)]
    pub fmt: Option<Vec<String>>,

    /// Serve a directory over HTTP (Phase 6)
    #[arg(short = 's', long = "serve", value_name = "DIR", num_args = 0..=1, default_missing_value = ".")]
    pub serve: Option<String>,

    // ── 修饰（只在对应动作下生效） ────────────────────────────────────
    /// Script arguments for --run (as `process.argv.slice(2)`)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Only resolve and print the tree, do not write anything (add/install/upgrade)
    #[arg(long)]
    pub dry_run: bool,

    /// Registry base URL (default https://registry.npmjs.org)
    #[arg(long)]
    pub registry: Option<String>,

    /// Dist-tag to publish under
    #[arg(long, default_value = "latest")]
    pub tag: String,

    /// Auth token to store (prompted on TTY when omitted)
    #[arg(long)]
    pub token: Option<String>,

    /// Print an OAuth authorization URL instead (code exchange deferred)
    #[arg(long)]
    pub oauth: bool,

    /// Package name for --init (default: current directory name)
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Skip the confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Overwrite conflicting files instead of failing
    #[arg(long)]
    pub force: bool,

    /// Only run files matching this glob (matched against relative path or file name)
    #[arg(long)]
    pub filter: Option<String>,

    /// Only run tests whose full name matches (substring or /regex/flags)
    #[arg(long, value_name = "PATTERN")]
    pub test_name_pattern: Option<String>,

    /// Re-run tests when watched files change (Ctrl-C to stop)
    #[arg(long)]
    pub watch: bool,

    /// Directory to serve
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,

    /// Interface to bind
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to bind (0 = ephemeral, actual port printed on stdout)
    #[arg(long, default_value_t = 3000)]
    pub port: u16,

    /// Requests per second limit, 0 = unlimited
    #[arg(long, default_value_t = 0)]
    pub limit_rps: u32,

    /// TLS certificate (PEM, must come with --key)
    #[arg(long)]
    pub cert: Option<PathBuf>,

    /// TLS private key (PEM, must come with --cert)
    #[arg(long)]
    pub key: Option<PathBuf>,

    /// ACME domain for automatic certificates (default winterjs.bemly.moe; needs --acme-email)
    #[arg(long, value_name = "DOMAIN")]
    pub acme_domain: Option<String>,

    /// ACME account email (mailto contact for Let's Encrypt)
    #[arg(long, value_name = "EMAIL")]
    pub acme_email: Option<String>,

    /// ACME cache directory (default system cache)
    #[arg(long, value_name = "DIR")]
    pub acme_cache: Option<PathBuf>,

    /// Use Let's Encrypt production (default staging, safe against rate limits)
    #[arg(long)]
    pub acme_production: bool,

    /// Print the JSON Schema of the settings file instead
    #[arg(long)]
    pub schema: bool,

    #[command(flatten)]
    pub perms: PermissionArgs,
}

/// `--allow-*` 权限旗标（Phase 8-b；任一出现即进沙箱，Bun 同款 opt-in）。
/// 旗标无值 = 该类全开；`=a,b` 或重复出现 = 允许清单。
#[derive(clap::Args, Clone, Debug, Default)]
pub struct PermissionArgs {
    /// Allow filesystem reads (optionally: --allow-read=<path>[,<path>...])
    #[arg(long, value_name = "PATH", num_args = 0..=1, require_equals = true, value_delimiter = ',')]
    pub allow_read: Option<Vec<String>>,
    /// Allow filesystem writes (optionally: --allow-write=<path>[,<path>...])
    #[arg(long, value_name = "PATH", num_args = 0..=1, require_equals = true, value_delimiter = ',')]
    pub allow_write: Option<Vec<String>>,
    /// Allow environment variable access (optionally: --allow-env=<VAR>[,<VAR>...])
    #[arg(long, value_name = "VAR", num_args = 0..=1, require_equals = true, value_delimiter = ',')]
    pub allow_env: Option<Vec<String>>,
    /// Allow spawning child processes (optionally: --allow-run=<cmd>[,<cmd>...])
    #[arg(long, value_name = "CMD", num_args = 0..=1, require_equals = true, value_delimiter = ',')]
    pub allow_run: Option<Vec<String>>,
    /// Allow FFI (dlopen of native libraries)
    #[arg(long)]
    pub allow_ffi: bool,
    /// Allow everything (no sandbox)
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
