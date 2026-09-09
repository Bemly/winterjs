//! mozjs 边界胶水：引擎初始化、Realm、脚本求值。
//! 本模块是 §6 允许 `unsafe` 的唯一区域（rooting / AutoRealm / FFI）。

use std::ffi::CString;
use std::ptr;

use anyhow::Result;
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

/// Evaluate `source` (named `filename`) and print its completion value.
pub fn run(source: &str, filename: &str) -> Result<()> {
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
