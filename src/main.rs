#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use mozjs::conversions::{ConversionResult, FromJSValConvertible};
use mozjs::jsapi::OnNewGlobalHookOption;
use mozjs::jsval::UndefinedValue;
use mozjs::realm::AutoRealm;
use mozjs::rooted;
use mozjs::rust::wrappers2::JS_NewGlobalObject;
use mozjs::rust::{
    CompileOptionsWrapper, RealmOptions, SIMPLE_GLOBAL_CLASS, error_info_from_exception_stack,
    evaluate_script, JSEngine, Runtime,
};

#[derive(Parser, Debug)]
#[command(name = "winterjs", version, about = "Bun-like JS runtime on SpiderMonkey")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run a JS file and print its completion value
    Run {
        /// Path to the JS file
        path: PathBuf,
    },
    /// Evaluate inline JS code
    Eval {
        /// The code to evaluate
        code: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run { path } => {
            let source = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let filename = path.to_string_lossy().into_owned();
            run(&source, &filename)
        }
        Cmd::Eval { code } => run(&code, "eval.js"),
    }
}

fn run(source: &str, filename: &str) -> Result<()> {
    // JS engine handle must outlive every Runtime.
    let engine = JSEngine::init().map_err(|_| anyhow::anyhow!("failed to init JS engine"))?;
    let mut rt = Runtime::new(engine.handle());

    let options = RealmOptions::default();
    rooted!(&in(rt.cx()) let global = unsafe {
        JS_NewGlobalObject(
            rt.cx(),
            &SIMPLE_GLOBAL_CLASS,
            ptr::null_mut(),
            OnNewGlobalHookOption::FireOnNewGlobalHook,
            &*options,
        )
    });

    rooted!(&in(rt.cx()) let mut rval = UndefinedValue());

    let c_filename = CString::new(filename).unwrap_or_else(|_| CString::new("script.js").unwrap());
    let options = CompileOptionsWrapper::new(rt.cx_no_gc(), c_filename, 1);
    let res = evaluate_script(rt.cx(), global.handle(), source, rval.handle_mut(), options);

    match res {
        Ok(()) => {
            if !rval.get().is_undefined() {
                match String::from_jsval(rt.cx(), rval.handle(), ()) {
                    Ok(ConversionResult::Success(s)) => println!("{s}"),
                    _ => println!("<non-stringifiable result>"),
                }
            }
            Ok(())
        }
        Err(()) => {
            rooted!(&in(rt.cx()) let mut exc = UndefinedValue());
            // evaluate_script 退出时已离开 realm，取异常堆栈必须重新进入，
            // 否则 PendingExceptionStackInfo 内的 JSAPI 因无 current realm 直接 SEGV。
            let mut realm = AutoRealm::new_from_handle(rt.cx(), global.handle());
            match error_info_from_exception_stack(&mut realm, exc.handle_mut()) {
                Some(info) => anyhow::bail!(
                    "{}:{}:{}: {}",
                    info.filename,
                    info.line,
                    info.col,
                    info.message
                ),
                None => anyhow::bail!("uncaught JS exception (no stack info)"),
            }
        }
    }
}
