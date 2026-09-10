use std::path::PathBuf;
use std::sync::LazyLock;

use clap::{Parser, Subcommand};

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
    /// Install packages from the npm registry (Phase 5; dry-run first)
    Install {
        /// Package specs (`name[@range]`, `@scope/name[@range]`); empty reads package.json (5b)
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
