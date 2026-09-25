#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

mod acme;
mod alloc;
mod builtins;
mod cli;
mod cli_node_flags;
mod dispatch;
mod error;
mod i18n;
mod jobqueue;
mod jsapi_glue;
mod lintfmt;
mod loader;
mod logging;
mod modules;
mod napi;
mod initpkg;
mod pm;
mod permissions;
mod repl;
mod runtime;
mod scripts;
mod sentry_report;
mod serve;
mod serve_bridge;
mod testrun;
mod settings;
mod state;

// 双语 help 文案：`locales/*.yml` 编译期打进二进制，缺译文回英文（`src/i18n.rs`）。
rust_i18n::i18n!("locales", fallback = "en");

use clap::FromArgMatches;
use cli::Cli;
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
    // Node 兼容旗预处理（测试套件 common.js 自举 respawn / 子进程自举透传）：
    // 剥除 + 记录（execArgv 保真），裸文件条件补 --run。见 cli::strip_node_compat_args。
    let compat_argv = {
        let raw: Vec<std::ffi::OsString> = std::env::args_os().collect();
        let (filtered, stripped) = cli::strip_node_compat_args(&raw);
        // node 同款：非法旗值启动即拒（exit 9）——剥除后照跑同一文件会让自 spawn
        // 套件无限递归（pitfalls 4.209）。
        for f in &stripped {
            if let Err(msg) = cli_node_flags::validate_node_flag(f) {
                eprintln!("winterjs: {msg}");
                std::process::exit(9);
            }
        }
        builtins::node::process_::record_node_compat(stripped);
        filtered
    };
    // 自 spawn 深度闸（pitfalls 4.209 防线二）：子进程链经 `WINTERJS_SPAWN_DEPTH` 逐层 +1
    //（child_process 起自身时设置，见 node::child::tag_self_depth），超限即拒，
    // 任何未知的自递归形都止于有限深度而非吃光系统。
    if builtins::node::child::self_spawn_depth() > builtins::node::child::SELF_SPAWN_LIMIT {
        eprintln!(
            "winterjs: self-spawn depth limit ({}) exceeded — recursive self-spawn aborted",
            builtins::node::child::SELF_SPAWN_LIMIT
        );
        std::process::exit(9);
    }
    let cli = {
        let m = cli::localized_command()
            .try_get_matches_from(compat_argv)
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
    error::set_render_color(settings.log.color);
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
    permissions::install(p.clone());
    permissions::remember_cli(&p);
}

async fn dispatch(cli: Cli, settings: &settings::Settings) -> i32 {
    let r = dispatch_inner(cli, settings).await;
    match &r {
        Ok(()) => 0,
        // 管道下游提前关闭（如 `winterjs --man | head`）静默退出，不刷错误
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
    // 全 flag 规范（AGENTS §0.8）：一次恰好一个动作。0 个时 `arg_required_else_help`
    // 已提前打印 help，只有透传 `args` 残留能到这里，照样报错指路。
    let actions = cli.actions_present();
    if actions.len() > 1 {
        return Err(Error::Other(format!(
            "specify exactly one action, got: {} (see --help)",
            actions.join(", ")
        )));
    }
    if let Some(target) = cli.run {
        install_permissions(&cli.perms);
        let target = target.to_string_lossy().into_owned();
        // 9i-10：带脚本后缀 → 文件直跑；裸名 → package.json scripts 优先、同名文件回落。
        match scripts::resolve(&target)? {
            scripts::RunTarget::File(path) => {
                let source = std::fs::read_to_string(&path).map_err(|source| Error::IoRead {
                    path: path.clone(),
                    source,
                })?;
                let filename = path.to_string_lossy().into_owned();
                return runtime::run(&source, &filename, runtime::Mode::Script, &cli.args).await;
            }
            scripts::RunTarget::Script(pkg_dir, script) => {
                let code = scripts::run(&pkg_dir, &script, &cli.args)?;
                return Err(Error::Exit(code));
            }
        }
    }
    if let Some(code) = cli.eval {
        install_permissions(&cli.perms);
        return runtime::run(&code, "eval.js", runtime::Mode::Eval, &[]).await;
    }
    if cli.config {
        if cli.schema {
            let schema = schemars::schema_for!(settings::Settings);
            println!("{}", serde_json::to_string_pretty(&schema)?);
        } else {
            println!("{}", serde_json::to_string_pretty(settings)?);
        }
        return Ok(());
    }
    if let Some(shell) = cli.completions {
        let mut cmd = cli::localized_command();
        clap_complete::generate(shell, &mut cmd, "winterjs", &mut std::io::stdout().lock());
        return Ok(());
    }
    if cli.man {
        use std::io::Write as _;

        let cmd = cli::localized_command();
        let man = clap_mangen::Man::new(cmd).title("WINTERJS");
        let mut buf = Vec::new();
        man.render(&mut buf)?;
        std::io::stdout().write_all(&buf)?;
        return Ok(());
    }
    if !cli.add.is_empty() {
        let cwd = std::env::current_dir()
            .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
        return pm::install_to(&cwd, &cli.add, cli.dry_run, cli.registry.as_deref()).await;
    }
    if !cli.install.is_empty() {
        let root = pm::global_root()?;
        pm::install_to(&root, &cli.install, cli.dry_run, cli.registry.as_deref()).await?;
        if !cli.dry_run {
            println!("global root: {}", root.display());
            println!(
                "add {} to PATH to use installed bins",
                root.join("node_modules").join(".bin").display()
            );
        }
        return Ok(());
    }
    if cli.publish {
        let cwd = std::env::current_dir()
            .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
        let reg = pm::effective_registry(&cwd, cli.registry.as_deref());
        return pm::publish::publish(&cwd, cli.dry_run, &reg, &cli.tag).await;
    }
    if cli.login {
        let cwd = std::env::current_dir()
            .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
        let reg = pm::effective_registry(&cwd, cli.registry.as_deref());
        return pm::publish::login(&reg, cli.token.as_deref(), cli.oauth).await;
    }
    if cli.upgrade {
        return pm::upgrade::upgrade(cli.dry_run).await;
    }
    if let Some(args) = cli.lint {
        return lintfmt::run("oxlint", &args);
    }
    if let Some(args) = cli.fmt {
        return lintfmt::run("oxfmt", &args);
    }
    if let Some(name) = cli.init {
        let cwd = std::env::current_dir()
            .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
        // `--init` 裸 flag（无值，经 default_missing_value 得空串）取目录名
        let name = if name.is_empty() { None } else { Some(name) };
        return initpkg::init(&cwd, name.as_deref(), cli.yes, cli.force, cli.dry_run, cli.registry.as_deref()).await;
    }
    if cli.repl {
        return runtime::repl().await;
    }
    if let Some(paths) = cli.test {
        install_permissions(&cli.perms);
        let cwd = std::env::current_dir()
            .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
        let paths = paths.into_iter().map(std::path::PathBuf::from).collect();
        let opts = testrun::TestOpts {
            paths,
            filter: cli.filter,
            test_name_pattern: cli.test_name_pattern,
            watch: cli.watch,
        };
        return testrun::run_tests(&cwd, &opts).await;
    }
    // 修饰 flag 只在对应动作下生效（§0.8）：`--handler` 无 `--serve` 即错。
    if cli.handler.is_some() && cli.serve.is_none() {
        return Err(Error::Other("--handler only works with --serve (see --help)".into()));
    }
    if let Some(dir) = cli.serve {
        // `--serve` 裸 flag 走 default_missing_value(".")；`--dir` 显式给则覆盖
        let dir = if dir != "." { std::path::PathBuf::from(dir) } else { cli.dir };
        // ACME 自动证书（与 --cert/--key 互斥；--dry-run 只校验打印，见 acme）。
        let acme = acme::AcmeOpts {
            domain: cli.acme_domain,
            email: cli.acme_email,
            cache_dir: cli.acme_cache,
            production: cli.acme_production,
        };
        if acme.enabled() {
            if cli.cert.is_some() || cli.key.is_some() {
                return Err(Error::Other("--acme-* cannot be combined with --cert/--key".into()));
            }
            if cli.dry_run {
                let root = acme::cache_root(acme.cache_dir.as_deref())?;
                println!("acme dry-run: domain={} email={} directory={} cache={}",
                    acme.effective_domain(),
                    acme.email.as_deref().unwrap_or("(none)"),
                    acme.directory_url(),
                    root.display());
                return Ok(());
            }
        }
        return serve::serve(&serve::ServeOpts {
            dir,
            host: cli.host,
            port: cli.port,
            limit_rps: cli.limit_rps,
            cert: cli.cert,
            key: cli.key,
            acme: Some(acme).filter(|a| a.enabled()),
            handler: cli.handler,
        })
        .await;
    }
    Err(Error::Other("specify an action (see --help)".into()))
}
