#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

mod alloc;
mod builtins;
mod cli;
mod error;
mod jobqueue;
mod jsapi_glue;
mod loader;
mod logging;
mod modules;
mod pm;
mod runtime;
mod serve;
mod settings;
mod state;

use clap::{CommandFactory, Parser};
use cli::{Cli, Cmd};
use error::Error;
use settings::ColorChoice;

fn main() {
    // panic 美化：release 下 panic 走 human-panic 报告（debug 下该宏自动 no-op，
    // RUST_BACKTRACE=1 时自动回退标准 panic 输出，不吞调试信息）
    human_panic::setup_panic!();

    let cli = Cli::parse();

    let settings = match settings::Settings::load() {
        Ok(settings) => settings,
        Err(source) => {
            let _ = Error::Config { source }.render(ColorChoice::Auto);
            std::process::exit(1);
        }
    };
    logging::init(logging::LogOptions {
        verbosity: cli.verbose,
        filter: settings.log.filter.clone(),
        color: settings.log.color,
        file: settings.log.file.clone(),
    });
    let version = &*cli::VERSION_TEXT;
    tracing::debug!(target: "winterjs", %version, "starting");

    // JS 跑在独占线程（AGENTS §6）：CLI 生命周期内主线程即 JS 线程，
    // tokio current-thread 只负责驱动 timers 的睡眠与 fetch 的 socket IO。
    let tokio_rt = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let err = Error::Other(format!("failed to start tokio runtime: {e}"));
            let _ = err.render(settings.log.color);
            std::process::exit(1);
        }
    };

    // 已知问题（AGENTS §4.8）：引擎/运行时析构期 StoreBuffer 悬垂边 SEGV。
    // 结果（含错误渲染）就绪后直接 process::exit 跳过 teardown，由 dispatch 返回退出码。
    let code = tokio_rt.block_on(dispatch(cli, &settings));
    tracing::debug!(target: "winterjs", code, "finished");
    std::process::exit(code);
}

async fn dispatch(cli: Cli, settings: &settings::Settings) -> i32 {
    let r = dispatch_inner(cli, settings).await;
    match &r {
        Ok(()) => 0,
        // 管道下游提前关闭（如 `winterjs man | head`）静默退出，不刷错误
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => 0,
        // process.exit/exitCode：静默以指定码退出
        Err(Error::Exit(code)) => *code,
        Err(err) => {
            let _ = err.render(settings.log.color);
            1
        }
    }
}

async fn dispatch_inner(cli: Cli, settings: &settings::Settings) -> Result<(), Error> {
    match cli.cmd {
        Cmd::Run { path, args } => {
            let source = std::fs::read_to_string(&path).map_err(|source| Error::IoRead {
                path: path.clone(),
                source,
            })?;
            let filename = path.to_string_lossy().into_owned();
            runtime::run(&source, &filename, runtime::Mode::Script, &args).await
        }
        Cmd::Eval { code } => runtime::run(&code, "eval.js", runtime::Mode::Eval, &[]).await,
        Cmd::Config { schema } => {
            if schema {
                let schema = schemars::schema_for!(settings::Settings);
                println!("{}", serde_json::to_string_pretty(&schema)?);
            } else {
                println!("{}", serde_json::to_string_pretty(settings)?);
            }
            Ok(())
        }
        Cmd::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "winterjs", &mut std::io::stdout().lock());
            Ok(())
        }
        Cmd::Install { packages, dry_run, registry } => {
            pm::install(&packages, dry_run, registry.as_deref()).await
        }
        Cmd::Publish { dry_run, registry, tag } => {
            let cwd = std::env::current_dir()
                .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
            let reg = pm::effective_registry(&cwd, registry.as_deref());
            pm::publish::publish(&cwd, dry_run, &reg, &tag).await
        }
        Cmd::Login { token, registry, oauth } => {
            let cwd = std::env::current_dir()
                .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
            let reg = pm::effective_registry(&cwd, registry.as_deref());
            pm::publish::login(&reg, token.as_deref(), oauth).await
        }
        Cmd::Upgrade { dry_run } => pm::upgrade::upgrade(dry_run).await,
        Cmd::Serve { dir, host, port, limit_rps } => {
            serve::serve(&serve::ServeOpts { dir, host, port, limit_rps }).await
        }
        Cmd::Man => {            use std::io::Write as _;

            let mut cmd = Cli::command();
            let main_man = clap_mangen::Man::new(cmd.clone()).title("WINTERJS");
            let mut buf = Vec::new();
            main_man.render(&mut buf)?;
            std::io::stdout().write_all(&buf)?;
            for sub in cmd.get_subcommands_mut() {
                let sub_name = sub.get_name().to_uppercase();
                let man = clap_mangen::Man::new(sub.clone()).title(format!("WINTERJS-{sub_name}"));
                let mut buf = Vec::new();
                man.render(&mut buf)?;
                std::io::stdout().write_all(&buf)?;
            }
            Ok(())
        }
    }
}
