#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

mod acme;
mod alloc;
mod banner;
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
mod timing;
mod settings;
mod state;
mod watch;

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
        // 破例单横杠 `-hide_banner` 改写（`--` 之后不动，见 cli::rewrite_banner_flag）。
        cli::rewrite_banner_flag(&filtered)
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
    let (cli, matches) = {
        let m = cli::localized_command()
            .try_get_matches_from(compat_argv)
            .unwrap_or_else(|e| e.exit());
        let cli = Cli::from_arg_matches(&m).unwrap_or_else(|e| e.exit());
        (cli, m)
    };

    // F4 纯动作懒初始化：--completions/--man 不读 settings、不建 tokio、不打日志
    //（--version/--help 已由 clap 提前 exit，同款）。仅单动作且修饰归属通过才走
    // 快路径，否则落回 dispatch 走原错误口径（保持文案/退出码逐字节一致）。
    if (cli.completions.is_some() || cli.man)
        && cli.actions_present().len() == 1
        && cli_modifier_scope(&cli, &matches).is_none()
    {
        if let Some(shell) = cli.completions {
            let mut cmd = cli::localized_command();
            clap_complete::generate(shell, &mut cmd, "winterjs", &mut std::io::stdout().lock());
            std::process::exit(0);
        }
        if cli.man {
            use std::io::Write as _;
            let cmd = cli::localized_command();
            let man = clap_mangen::Man::new(cmd).title("WINTERJS");
            let mut buf = Vec::new();
            match man.render(&mut buf) {
                Ok(()) => match std::io::stdout().write_all(&buf) {
                    Ok(()) => std::process::exit(0),
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                        std::process::exit(0)
                    }
                    Err(_) => std::process::exit(1),
                },
                Err(_) => std::process::exit(1),
            }
        }
    }

    let settings = match settings::Settings::load() {
        Ok(settings) => settings,
        Err(source) => {
            let _ = Error::Config { source }.render(ColorChoice::Auto);
            std::process::exit(1);
        }
    };
    // F4（续）：--config 只需 settings，无需 logging/tokio/sentry。同上仅单动作
    // 快路径，否则落回 dispatch。
    if cli.config
        && cli.actions_present().len() == 1
        && cli_modifier_scope(&cli, &matches).is_none()
    {
        let code = match (cli.schema, serde_json::to_string_pretty(&settings)) {
            (true, _) => match serde_json::to_string_pretty(&schemars::schema_for!(
                settings::Settings
            )) {
                Ok(s) => {
                    println!("{s}");
                    0
                }
                Err(e) => {
                    let _ = Error::Other(e.to_string()).render(settings.log.color);
                    1
                }
            },
            (false, Ok(s)) => {
                println!("{s}");
                0
            }
            (false, Err(e)) => {
                let _ = Error::Other(e.to_string()).render(settings.log.color);
                1
            }
        };
        std::process::exit(code);
    }
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
    let code = tokio_rt.block_on(dispatch(cli, &matches, &settings));
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

async fn dispatch(cli: Cli, matches: &clap::ArgMatches, settings: &settings::Settings) -> i32 {
    let r = dispatch_inner(cli, matches, settings).await;
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

async fn dispatch_inner(cli: Cli, matches: &clap::ArgMatches, settings: &settings::Settings) -> Result<(), Error> {
    // 全 flag 规范（AGENTS §0.8）：一次恰好一个动作。0 个时 `arg_required_else_help`
    // 已提前打印 help，只有透传 `args` 残留能到这里，照样报错指路。
    let actions = cli.actions_present();
    if actions.len() > 1 {
        return Err(Error::Other(format!(
            "specify exactly one action, got: {} (see --help)",
            actions.join(", ")
        )));
    }
    // 修饰 flag 只在对应动作下生效（§0.8）：错配即错，不静默吞掉。
    // 归属表见 `cli_modifier_scope`（与 --help 文案括号注一致）。
    // 注：放所有动作分支之前——各分支提前返回，迟了够不着。
    if let Some(msg) = cli_modifier_scope(&cli, matches) {
        return Err(Error::Other(format!("{msg} (see --help)")));
    }
    // 启动 banner：每次运行首行走 stderr（非 TTY 自动跳过；机器输出动作除外）。
    if cli.completions.is_none() && !cli.man {
        crate::banner::print_startup(cli.hide_banner);
    }
    // S1：WinterCG 存储默认库（修饰 flag，归属已由 scope 保证）。
    if cli.run.is_some() || cli.eval.is_some() || cli.test.is_some() || cli.repl || cli.serve.is_some() {
        crate::builtins::storage::set_default_path(
            cli.storage_path.clone().map(|p| p.to_string_lossy().into_owned()),
        );
    }
    if let Some(target) = cli.run {
        install_permissions(&cli.perms);
        let target = target.to_string_lossy().into_owned();
        // 9i-10：带脚本后缀 → 文件直跑；裸名 → package.json scripts 优先、同名文件回落。
        match scripts::resolve(&target)? {
            scripts::RunTarget::File(path) => {
                if cli.watch {
                    return scripts::run_file_watch(&path, &cli.args, settings.log.color).await;
                }
                let source = std::fs::read_to_string(&path).map_err(|source| Error::IoRead {
                    path: path.clone(),
                    source,
                })?;
                let filename = path.to_string_lossy().into_owned();
                return runtime::run(&source, &filename, runtime::Mode::Script, &cli.args).await;
            }
            scripts::RunTarget::Script(pkg_dir, script) => {
                if cli.watch {
                    return Err(Error::Other(
                        "--watch only works with file targets (package.json scripts are not watchable)".into(),
                    ));
                }
                let code = scripts::run(&pkg_dir, &script, &cli.args).await?;
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
    if !cli.remove.is_empty() {
        let cwd = std::env::current_dir()
            .map_err(|e| Error::Other(format!("cannot get cwd: {e}")))?;
        return pm::remove::remove_from(&cwd, &cli.remove, cli.dry_run).await;
    }
    if !cli.uninstall.is_empty() {
        let root = pm::global_root()?;
        return pm::remove::remove_from(&root, &cli.uninstall, cli.dry_run).await;
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
        install_permissions(&cli.perms);
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
        let opts = serve::ServeOpts {
            dir,
            host: cli.host,
            port: cli.port,
            limit_rps: cli.limit_rps,
            cert: cli.cert,
            key: cli.key,
            acme: Some(acme).filter(|a| a.enabled()),
            handler: cli.handler,
        };
        if cli.watch {
            return serve::serve_watch(&opts, cli.verbose).await;
        }
        return serve::serve(&opts).await;
    }
    if let Some(path) = cli.db {
        install_permissions(&cli.perms);
        return db_inspect(&path, cli.exec.as_deref(), cli.dry_run).await;
    }
    Err(Error::Other("specify an action (see --help)".into()))
}

/// `--db` turso 透传查库（S1）：只读 SQL 走 query 打印 `{columns, rows}`，
/// 写 SQL 走 execute 打印 `{changes}`；缺省 SQL 列出全部表。沙箱内走
/// `check_read`（读）/`check_write`（写）门控。
async fn db_inspect(path: &std::path::Path, exec: Option<&str>, dry_run: bool) -> Result<(), Error> {
    use base64::Engine as _;
    let path_s = path.to_string_lossy().into_owned();
    let sql = exec
        .unwrap_or("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .to_string();
    if dry_run {
        println!("db dry-run: file={path_s} sql={sql}");
        return Ok(());
    }
    if let Err(msg) = permissions::check_read(&path_s) {
        return Err(Error::Other(msg));
    }
    let head = sql.trim_start().to_ascii_uppercase();
    let read_only = ["SELECT", "WITH", "EXPLAIN", "PRAGMA", "VALUES"]
        .iter()
        .any(|p| head.starts_with(p));
    if !read_only {
        if let Err(msg) = permissions::check_write(&path_s) {
            return Err(Error::Other(msg));
        }
    }
    tracing::debug!(target: "winterjs::storage", path_len = path_s.len(), sql_len = sql.len(), "db inspect");
    let db = turso::Builder::new_local(&path_s)
        .build()
        .await
        .map_err(|e| Error::Other(format!("db open failed: {e}")))?;
    let conn = db
        .connect()
        .map_err(|e| Error::Other(format!("db connect failed: {e}")))?;
    if read_only {
        let mut rows = conn
            .query(&sql, turso::params::Params::Positional(Vec::new()))
            .await
            .map_err(|e| Error::Other(format!("db query failed: {e}")))?;
        let columns = rows.column_names();
        let mut out = Vec::new();
        loop {
            match rows.next().await {
                Ok(None) => break,
                Ok(Some(row)) => {
                    let mut vals = Vec::with_capacity(row.column_count());
                    for i in 0..row.column_count() {
                        let v = row
                            .get_value(i)
                            .map_err(|e| Error::Other(format!("db row failed: {e}")))?;
                        vals.push(match v {
                            turso::Value::Null => serde_json::Value::Null,
                            turso::Value::Integer(n) => serde_json::json!(n),
                            turso::Value::Real(f) => serde_json::json!(f),
                            turso::Value::Text(s) => serde_json::json!(s),
                            turso::Value::Blob(b) => serde_json::json!({
                                "$blob": base64::engine::general_purpose::STANDARD.encode(b)
                            }),
                        });
                    }
                    out.push(serde_json::Value::Array(vals));
                }
                Err(e) => return Err(Error::Other(format!("db row failed: {e}"))),
            }
        }
        println!("{}", serde_json::json!({ "columns": columns, "rows": out }));
    } else {
        let n = conn
            .execute(&sql, turso::params::Params::Positional(Vec::new()))
            .await
            .map_err(|e| Error::Other(format!("db execute failed: {e}")))?;
        println!("{}", serde_json::json!({ "changes": n }));
    }
    Ok(())
}

/// 修饰 flag → 归属动作校验（AGENTS §0.8）。
/// 返回首个错配的 `"--flag only works with --action"`，全对回 None。
/// 归属（与 `--help` 括号注同源）：
/// dry-run→add/install/remove/uninstall/publish/init/upgrade/serve/db；registry→add/install/publish/login/init；
/// tag→publish；token/oauth→login；name/yes/force→init；filter/test-name-pattern→test；
/// watch→test/run/serve；dir/host/port/handler/limit-rps/cert/key/acme-*→serve；schema→config；
/// exec→db；storage-path→run/eval/test/repl/serve；
/// allow-*→run/eval/test/repl/db。-v/-l 全局，不校验。
fn cli_modifier_scope(cli: &Cli, matches: &clap::ArgMatches) -> Option<String> {
    use clap::parser::ValueSource;
    let has_add = !cli.add.is_empty();
    let has_install = !cli.install.is_empty();
    let has_remove = !cli.remove.is_empty();
    let has_uninstall = !cli.uninstall.is_empty();
    let fail = |flag: &str, scope: &str| Some(format!("{flag} only works with {scope}"));
    // 带 clap 默认值的 flag 按值判会漏掉显式给默认值（`--port 3000` 与缺省同值，
    // 按值比较即漏判）；一律按解析来源判显式。
    let explicit = |id: &str| {
        matches
            .value_source(id)
            .is_some_and(|s| s == ValueSource::CommandLine)
    };
    if cli.dry_run
        && !(has_add || has_install || has_remove || has_uninstall || cli.publish || cli.init.is_some() || cli.upgrade || cli.serve.is_some() || cli.db.is_some())
    {
        return fail("--dry-run", "--add/--install/--remove/--uninstall/--publish/--init/--upgrade/--serve/--db");
    }
    if cli.registry.is_some()
        && !(has_add || has_install || cli.publish || cli.login || cli.init.is_some())
    {
        return fail("--registry", "--add/--install/--publish/--login/--init");
    }
    if explicit("tag") && !cli.publish {
        return fail("--tag", "--publish");
    }
    if cli.token.is_some() && !cli.login {
        return fail("--token", "--login");
    }
    if cli.oauth && !cli.login {
        return fail("--oauth", "--login");
    }
    if cli.name.is_some() && cli.init.is_none() {
        return fail("--name", "--init");
    }
    if cli.yes && cli.init.is_none() {
        return fail("--yes", "--init");
    }
    if cli.force && cli.init.is_none() {
        return fail("--force", "--init");
    }
    if cli.filter.is_some() && cli.test.is_none() {
        return fail("--filter", "--test");
    }
    if cli.test_name_pattern.is_some() && cli.test.is_none() {
        return fail("--test-name-pattern", "--test");
    }
    if cli.watch && cli.test.is_none() && cli.run.is_none() && cli.serve.is_none() {
        return fail("--watch", "--test/--run/--serve");
    }
    // --serve 修饰：带默认值的（dir/host/port/limit-rps）按解析来源判显式，
    // 按值比会漏掉显式给默认值（`--port 3000` 与缺省同值）。
    if cli.serve.is_none() {
        if cli.handler.is_some() {
            return fail("--handler", "--serve");
        }
        if explicit("dir") {
            return fail("--dir", "--serve");
        }
        if explicit("host") {
            return fail("--host", "--serve");
        }
        if explicit("port") {
            return fail("--port", "--serve");
        }
        if explicit("limit_rps") {
            return fail("--limit-rps", "--serve");
        }
        if cli.cert.is_some() {
            return fail("--cert", "--serve");
        }
        if cli.key.is_some() {
            return fail("--key", "--serve");
        }
        if cli.acme_domain.is_some() {
            return fail("--acme-domain", "--serve");
        }
        if cli.acme_email.is_some() {
            return fail("--acme-email", "--serve");
        }
        if cli.acme_cache.is_some() {
            return fail("--acme-cache", "--serve");
        }
        if cli.acme_production {
            return fail("--acme-production", "--serve");
        }
    }
    if cli.schema && !cli.config {
        return fail("--schema", "--config");
    }
    if cli.exec.is_some() && cli.db.is_none() {
        return fail("--exec", "--db");
    }
    if cli.storage_path.is_some()
        && !(cli.run.is_some() || cli.eval.is_some() || cli.test.is_some() || cli.repl || cli.serve.is_some())
    {
        return fail("--storage-path", "--run/--eval/--test/--repl/--serve");
    }
    let perms = &cli.perms;
    let perm_given = perms.allow_read.is_some()
        || perms.allow_write.is_some()
        || perms.allow_env.is_some()
        || perms.allow_run.is_some()
        || perms.allow_ffi
        || perms.allow_all;
    if perm_given && !(cli.run.is_some() || cli.eval.is_some() || cli.test.is_some() || cli.repl || cli.db.is_some()) {
        return fail("--allow-*", "--run/--eval/--test/--repl/--db");
    }
    // 尾部透传只归 --run：`args` 是 trailing 收集，未知旗形（如 `--env-file=x`）
    // 会落进来；非 --run 动作携带来即未知 flag 误写，不静默吞掉（§0.8）。
    // --run 的脚本参数走 `--` 之后（§4.61/§4.63），未知旗形同理透传无碍。
    if !cli.args.is_empty() && cli.run.is_none() {
        return fail(
            &format!("unexpected argument '{}'", cli.args[0]),
            "--run (script arguments go after `--`: `--run FILE -- ARGS`)",
        );
    }
    // --test 的 paths 是位置值，clap 会把 `--cov` 类未知旗形吞成路径；
    // `--` 打头的位置值几乎必为 flag 误写，指路 --help 而非报"无此路径"。
    if let Some(paths) = &cli.test
        && let Some(flaggy) = paths.iter().find(|p| p.starts_with("--"))
    {
        return fail(&format!("unknown flag '{flaggy}'"), "--help");
    }
    None
}
