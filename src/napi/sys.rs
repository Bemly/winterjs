//! bindgen 生成（build.rs `emit_napi_sys`；勿手改，改生成器或 vendored 头）。
//! 只含类型 + 常量：`napi_*` 函数由本 crate `api.rs` 定义并导出（plan-napi §2）。

#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    clippy::all,
    improper_ctypes_definitions
)]

include!(concat!(env!("OUT_DIR"), "/napi_sys.rs"));
