#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

mod alloc;
mod builtins;
mod cli;
mod error;
mod i18n;
mod jobqueue;
mod jsapi_glue;
mod lintfmt;
mod loader;
mod logging;
mod modules;
mod initpkg;
mod pm;
mod permissions;
mod repl;
mod runtime;
mod sentry_report;
mod serve;
mod testrun;
mod settings;
mod state;

// 双语 help 文案：`locales/*.yml` 编译期打进二进制，缺译文回英文（`src/i18n.rs`）。
rust_i18n::i18n!("locales", fallback = "en");

use clap::FromArgMatches;
use cli::{Cli, Cmd};
use error::Error;
use settings::ColorChoice;

fn main() {
    // panic 美化：release 下 panic 走 human-panic 报告（debug 下该宏自动 no-op，
    // RUST_BACKTRACE=1 时自动回退标准 panic 输出，不吞调试信息）
    human_panic::setup_panic!();

    // 双语：先定 locale（-l > WINTERJS_LANG > 系统 > en），再解析本地化 Command。
    // 两遍 argv 扫描（`i18n::prescan` 定语言 + clap 正式解析）是刻意设计：
    // clap 的 help 文本在解析前就要定死，不存在单遍解法。
    i18n::init_from_argv();
    let cli = {
        let m = cli::localized_command()
            .try_get_matches_from(std::env::args_os())
            .unwrap_or_else(|e| e.exit());
        Cli::from_arg_matches(&m).unwrap_or_else(|e| e.exit())
    };

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

    // 崩溃上报 opt-in（WINTERJS_SENTRY_DSN；未设零成本，plan Phase 8-c）
    sentry_report::init();

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
    // §4.8：process::exit 跳过 teardown；上报事件先排空（panic 路径自身已 flush）
    sentry_report::flush();
    std::process::exit(code);
}

/// CLI 旗标 → 权限集（沙箱 opt-in：任一 `--allow-*` 出现即启用）。
fn install_permissions(perms: &cli::PermissionArgs) {
    let p = permissions::Permissions {
        read: permissions::grant_from_values(perms.allow_read.clone()),
        write: permissions::grant_from_values(perms.allow_write.clone()),
        env: permissions::grant_from_values(perms.allow_env.clone()),
        run: permissions::grant_from_values(perms.allow_run.clone()),
        ffi: perms.allow_ffi,
        allow_all: perms.allow_all,
    };
    tracing::debug!(target: "winterjs::permissions", sandbox = p.sandboxed(), "installed");
    permissions::install(p);
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
        Cmd::Run { path, args, perms } => {
            install_permissions(&perms);
            let source = std::fs::read_to_string(&path).map_err(|source| Error::IoRead {
                path: path.clone(),
                source,
            })?;
            let filename = path.to_string_lossy().into_owned();
            runtime::run(&source, &filename, runtime::Mode::Script, &args).await
        }
        Cmd::Eval { code, perms } => {
            install_permissions(&perms);
            runtime::run(&code, "eval.js", runtime::Mode::Eval, &[]).await
        }
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
            let mut cmd = cli::localized_command();
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
        Cmd::Lint { args } => lintfmt::run("oxlint", &args),
        Cmd::Fmt { args } => lintfmt::run("oxfmt", &args),
        Cmd::Init { name, yes } => {
            let cwd = std::env::current_dir()
                .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
            initpkg::init(&cwd, name.as_deref(), yes).await
        }
        Cmd::Repl => runtime::repl().await,
        Cmd::Test { paths, filter, watch, perms } => {
            install_permissions(&perms);
            let cwd = std::env::current_dir()
                .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
            testrun::run_tests(&cwd, &testrun::TestOpts { paths, filter, watch }).await
        }
        Cmd::Serve { dir, host, port, limit_rps, cert, key } => {
            serve::serve(&serve::ServeOpts { dir, host, port, limit_rps, cert, key }).await
        }
        Cmd::Man => {            use std::io::Write as _;

            let mut cmd = cli::localized_command();
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
