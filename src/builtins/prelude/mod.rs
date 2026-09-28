//! Web prelude（一次性全局脚本；§0.9 分域拆分：按 JS 语义域切分文件，concat 顺序不可动）。
//! vendor（acorn）不进 PRELUDE——仅 REPL 会话注入（runtime/repl；全会话加载
//! 拖慢 worker/child 等小窗口时序测试的启动，R6b 实测）。
mod blob;
mod bootstrap;
mod namespace;
mod buffer_api;
mod buffer_class;
mod buffer_core;
mod buffer_int;
mod buffer_ops;
mod crypto;
mod events;
mod fetch;
mod http;
pub mod repl_complete;
pub mod vendor;
mod serve;
mod storage;
mod streams;
mod url;

/// 引擎启动时在全局对象上求值的一次性脚本（§1 路线 Phase 1）。
/// parts 运行时一次拼接（`concat!` 只收字面量，不收 const 路径；LazyLock 进程级单例）。
pub static PRELUDE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    [
        bootstrap::BOOTSTRAP_JS,
        url::URL_JS,
        buffer_core::BUFFER_CORE_JS,
        buffer_int::BUFFER_INT_JS,
        buffer_ops::BUFFER_OPS_JS,
        buffer_class::BUFFER_CLASS_JS,
        buffer_api::BUFFER_API_JS,
        crypto::CRYPTO_JS,
        events::EVENTS_JS,
        http::HTTP_JS,
        serve::SERVE_JS,
        storage::STORAGE_JS,
        streams::STREAMS_JS,
        blob::BLOB_JS,
        fetch::FETCH_JS,
        namespace::NAMESPACE_JS,
        repl_complete::REPL_COMPLETE_JS,
    ]
    .concat()
});
