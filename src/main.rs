#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

mod cli;
mod runner;

use anyhow::{Context as _, Result};
use clap::Parser;
use cli::{Cli, Cmd};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { path } => {
            let source = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let filename = path.to_string_lossy().into_owned();
            runner::run(&source, &filename)
        }
        Cmd::Eval { code } => runner::run(&code, "eval.js"),
    }
}
