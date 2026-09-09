#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

mod cli;
mod error;
mod logging;
mod runner;
mod settings;

use std::process::ExitCode;

use clap::Parser;
use cli::{Cli, Cmd};
use error::Error;
use settings::ColorChoice;

fn main() -> ExitCode {
    // panic 美化：release 下 panic 走 human-panic 报告（debug 下该宏自动 no-op，
    // RUST_BACKTRACE=1 时自动回退标准 panic 输出，不吞调试信息）
    human_panic::setup_panic!();

    let cli = Cli::parse();

    let settings = match settings::Settings::load() {
        Ok(settings) => settings,
        Err(source) => return Error::Config { source }.render(ColorChoice::Auto),
    };
    logging::init(logging::LogOptions {
        verbosity: cli.verbose,
        filter: settings.log.filter.clone(),
        color: settings.log.color,
        file: settings.log.file.clone(),
    });
    let version = &*cli::VERSION_TEXT;
    tracing::debug!(target: "winterjs", %version, "starting");

    let result = dispatch(cli, &settings);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => err.render(settings.log.color),
    }
}

fn dispatch(cli: Cli, settings: &settings::Settings) -> Result<(), Error> {
    match cli.cmd {
        Cmd::Run { path } => {
            let source = std::fs::read_to_string(&path).map_err(|source| Error::IoRead {
                path: path.clone(),
                source,
            })?;
            let filename = path.to_string_lossy().into_owned();
            runner::run(&source, &filename)
        }
        Cmd::Eval { code } => runner::run(&code, "eval.js"),
        Cmd::Config { schema } => {
            if schema {
                let schema = schemars::schema_for!(settings::Settings);
                println!("{}", serde_json::to_string_pretty(&schema)?);
            } else {
                println!("{}", serde_json::to_string_pretty(settings)?);
            }
            Ok(())
        }
    }
}
