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

#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Run a JS file and print its completion value
    Run {
        /// Path to the JS file
        path: PathBuf,
        /// Script arguments (as `process.argv.slice(2)`)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Evaluate inline JS code
    Eval {
        /// The code to evaluate
        code: String,
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
}
