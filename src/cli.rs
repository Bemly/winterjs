use std::path::PathBuf;
use std::sync::LazyLock;

use clap::{CommandFactory, Parser, Subcommand};

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

#[derive(Parser, Debug)]
#[command(
    name = "winterjs",
    version = &**VERSION_TEXT,
    about = "Bun-like JS runtime on SpiderMonkey"
)]
pub struct Cli {
    /// Increase log verbosity (-v: info, -vv: debug, -vvv: trace)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Language for help text (en/zh; defaults to the system language)
    #[arg(short = 'l', long = "lang", global = true, value_name = "LANG",
        value_parser = clap::builder::PossibleValuesParser::new(["en", "zh"]))]
    pub lang: Option<String>,

    #[command(subcommand)]
    pub cmd: Cmd,
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


#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Run a JS file and print its completion value
    Run {
        /// Path to the JS file
        path: PathBuf,
        /// Script arguments (as `process.argv.slice(2)`)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        #[command(flatten)]
        perms: PermissionArgs,
    },
    /// Evaluate inline JS code
    Eval {
        /// The code to evaluate
        code: String,
        #[command(flatten)]
        perms: PermissionArgs,
    },
    /// Show resolved settings (or their JSON Schema with --schema)
    Config {
        /// Print the JSON Schema of the settings file instead
        #[arg(long)]
        schema: bool,
    },
    /// Print a shell completion script for the given shell
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Print roff manual pages to stdout (main page, then one per subcommand)
    Man,
    /// Add packages to the current project (local node_modules)
    Add {
        /// Package specs (`name[@range]`, `@scope/name[@range]`)
        #[arg(short = 'a', long = "add", value_name = "PKG")]
        packages: Vec<String>,
        /// Only resolve and print the tree, do not write anything
        #[arg(long)]
        dry_run: bool,
        /// Registry base URL (default https://registry.npmjs.org)
        #[arg(long)]
        registry: Option<String>,
    },
    /// Install packages globally (shared data directory)
    Install {
        /// Package specs (`name[@range]`, `@scope/name[@range]`)
        #[arg(short = 'a', long = "add", value_name = "PKG")]
        packages: Vec<String>,
        /// Only resolve and print the tree, do not write anything
        #[arg(long)]
        dry_run: bool,
        /// Registry base URL (default https://registry.npmjs.org)
        #[arg(long)]
        registry: Option<String>,
    },
    /// Publish the current package (Phase 5d; dry-run validates only)
    Publish {
        /// Validate and print what would be published, do not upload
        #[arg(long)]
        dry_run: bool,
        /// Registry base URL (npmrc/env fallback, see `install`)
        #[arg(long)]
        registry: Option<String>,
        /// Dist-tag to publish under
        #[arg(long, default_value = "latest")]
        tag: String,
    },
    /// Log in to a registry (Phase 5d; stores a token in ~/.npmrc)
    Login {
        /// Auth token to store (prompted on TTY when omitted)
        #[arg(long)]
        token: Option<String>,
        /// Registry base URL (npmrc/env fallback, see `install`)
        #[arg(long)]
        registry: Option<String>,
        /// Print an OAuth authorization URL instead (code exchange deferred)
        #[arg(long)]
        oauth: bool,
    },
    /// Self-upgrade winterjs (Phase 5d; needs WINTERJS_UPDATE_GITHUB=owner/repo)
    Upgrade {
        /// Only report the current version and channel, do not upgrade
        #[arg(long)]
        dry_run: bool,
    },
    /// Scaffold a new package (Phase 7)
    Init {
        /// Package name (default: current directory name)
        name: Option<String>,
        /// Skip the confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Start an interactive REPL (Phase 7)
    Repl,
    /// Run test files (Phase 7)
    Test {
        /// Test files or directories (default: discover under cwd)
        paths: Vec<PathBuf>,
        /// Only run files matching this glob (matched against relative path or file name)
        #[arg(long)]
        filter: Option<String>,
        /// Re-run tests when watched files change (Ctrl-C to stop)
        #[arg(long)]
        watch: bool,
        #[command(flatten)]
        perms: PermissionArgs,
    },
    /// Forward to the project's oxlint (passthrough; args go to oxlint verbatim)
    Lint {
        /// Arguments forwarded to oxlint verbatim
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Forward to the project's oxfmt (passthrough; args go to oxfmt verbatim)
    Fmt {
        /// Arguments forwarded to oxfmt verbatim
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Serve a directory over HTTP (Phase 6)
    Serve {
        /// Directory to serve
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Interface to bind
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind (0 = ephemeral, actual port printed on stdout)
        #[arg(long, default_value_t = 3000)]
        port: u16,
        /// Requests per second limit, 0 = unlimited
        #[arg(long, default_value_t = 0)]
        limit_rps: u32,
        /// TLS certificate (PEM, must come with --key)
        #[arg(long)]
        cert: Option<PathBuf>,
        /// TLS private key (PEM, must come with --cert)
        #[arg(long)]
        key: Option<PathBuf>,
    },
}

/// 双语 Command 构造器：derive 生成的是英文骨架，这里按当前 locale
///（`i18n::init_from_argv` 已在 `main` 起点定好）把 about/help/value_name
/// 换成 `t!` 查表。key 规则：`app.about` / `cmd.{sub}.about` /
/// `arg.{scope}.{id}.help`（+`.value`）；scoped 缺 key 先回 `arg.app.{id}`
///（全局 flag 在各子命令 help 里复用文案），再缺保留原文 → 英文输出逐字节不变。
/// 已知局限：clap 自带词（Usage/Commands/Options）与 clap 自动报错保持英文。
pub fn localized_command() -> clap::Command {
    let cmd = Cli::command();
    let cmd = with_about(cmd, "app.about");
    let mut cmd = with_localized_args(cmd, "app");
    // 子命令只能 `&mut` 访问（无 by-value take），用 mem::replace 原位换回；
    // 占位 dummy 只活一个语句，零行为影响。
    for sub in cmd.get_subcommands_mut() {
        let name = sub.get_name().to_string();
        let tmp = std::mem::replace(sub, clap::Command::new("winterjs-placeholder"));
        let tmp = with_about(tmp, &format!("cmd.{name}.about"));
        *sub = with_localized_args(tmp, &name);
    }
    cmd
}

fn with_about(cmd: clap::Command, key: &str) -> clap::Command {
    match cmd.get_about().map(|s| s.to_string()) {
        Some(orig) => cmd.about(tr_or(key, &orig)),
        None => cmd,
    }
}

fn with_localized_args(mut cmd: clap::Command, scope: &str) -> clap::Command {
    // 先收 id（`mut_arg` 要 `&mut`，不能边遍历边改）
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
                    let t = tr_fallback(
                        &format!("arg.{scope}.{id}.help"),
                        &format!("arg.app.{id}.help"),
                        &o,
                    );
                    (t != o).then_some(t)
                });
                let v = a.get_value_names().and_then(|names| names.first()).and_then(|s| {
                    let o = s.to_string();
                    let t = tr_fallback(
                        &format!("arg.{scope}.{id}.value"),
                        &format!("arg.app.{id}.value"),
                        &o,
                    );
                    (t != o).then_some(t)
                });
                (h, v)
            }
            None => (None, None),
        };
        if new_help.is_none() && new_value.is_none() {
            continue;
        }
        // `mut_arg` 是 by-value builder（`mut self -> Self`），闭包内用 shadowing 串联。
        cmd = cmd.mut_arg(id.as_str(), |a| {
            let a = match new_help {
                // `help` 收 owned String，无泄漏
                Some(h) => a.help(h),
                None => a,
            };
            match new_value {
                // `value_name` 只收 `&'static str`（`Str` 无 `From<String>`）：
                // 译文泄漏一次，进程生命周期内有效（中文 locale 下约 60 个短串，可接受，见注释）。
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

/// scoped 命中即用，否则回 app 级（全局 flag 复用），再缺回原文。
fn tr_fallback(scoped: &str, app_level: &str, original: &str) -> String {
    for k in [scoped, app_level] {
        let hit = rust_i18n::t!(k).to_string();
        if hit != k {
            return hit;
        }
    }
    original.to_string()
}
