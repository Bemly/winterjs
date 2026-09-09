#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

mod cli;
mod logging;
mod runner;
mod settings;

use anyhow::{Context as _, Result};
use clap::Parser;
use cli::{Cli, Cmd};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let settings = settings::Settings::load()
        .map_err(|e| anyhow::anyhow!("failed to load settings: {e}"))
        .with_context(|| "reading winterjs.toml / winterjs.json / winterjs.ini or WINTERJS_* env")?;
    logging::init(logging::LogOptions {
        verbosity: cli.verbose,
        filter: settings.log.filter.clone(),
        color: settings.log.color,
        file: settings.log.file.clone(),
    });
    let version = &*cli::VERSION_TEXT;
    tracing::debug!(target: "winterjs", %version, "starting");

    match cli.cmd {
        Cmd::Run { path } => {
            let source = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let filename = path.to_string_lossy().into_owned();
            runner::run(&source, &filename)
        }
        Cmd::Eval { code } => runner::run(&code, "eval.js"),
        Cmd::Config { schema } => {
            if schema {
                let schema = schemars::schema_for!(settings::Settings);
                println!("{}", serde_json::to_string_pretty(&schema)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&settings)?);
            }
            Ok(())
        }
    }
}
