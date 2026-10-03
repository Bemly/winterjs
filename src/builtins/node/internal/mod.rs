//! `node:internal/*` 共享小件（plan2 §9a 前置，Bun `src/js/internal/` 词典命名）：
//! errors / validators / fixed_queue / util / util:inspect / util:types /
//! events:abort_listener / events:symbols / event_target。
//!
//! 口径：
//! - 源为 Node `lib/internal/*` 的 ESM 移植（MIT 头保留在模块头注），primordials
//!   解构一律还原为直接调用（无防篡改硬化，bun-compat.md §4.1 记录的偏差）。
//! - `node:internal/*` 只供内建模块互引（绝对 `node:` URL 不依赖 base，见
//!   loader/resolve.rs）；不进 `available()` 报错列表（mod.rs 过滤），
//!   用户直引行为未定义（Node 直接拒绝，此处宽松，偏差记录）。
//! - 引擎能力差异处（栈整形、Promise 内态）以偏差注释就地记录。

pub mod abort_listener;
pub mod assert;
pub mod async_context_frame;
pub mod async_hooks_int;
pub mod binding;
pub mod blob;
pub mod abort_controller;
pub mod buffer;
pub mod debuglog;
pub mod encoding;
pub mod http2_util;
pub mod http_aliases;
pub mod errors;
pub mod event_target;
pub mod fixed_queue;
pub mod http_framing;
pub mod inspect;
pub mod options;
pub mod primordials;
pub mod process_task_queues;
pub mod querystring;
pub mod registry;
pub mod snapshot;
pub mod streams;
pub mod symbols;
pub mod test_mock;
pub mod timers;
pub mod types;
pub mod util;
pub mod validators;
pub mod webstream_adapters;
pub mod zip;

/// internal 表（`node:internal/*` → 源；顺序即 `available()` 过滤无关）。
pub const INTERNALS: &[(&str, &str)] = &[
    ("node:internal/errors", errors::SOURCE),
    ("node:internal/validators", validators::SOURCE),
    ("node:internal/fixed_queue", fixed_queue::SOURCE),
    ("node:internal/util", util::SOURCE),
    ("node:internal/util/inspect", inspect::SOURCE),
    ("node:internal/util/types", types::SOURCE),
    ("node:internal/querystring", querystring::SOURCE),
    ("node:internal/events/abort_listener", abort_listener::SOURCE),
    ("node:internal/events/symbols", symbols::SOURCE),
    ("node:internal/event_target", event_target::SOURCE),
    // Phase 9b：streams 系共享件
    ("node:internal/primordials", primordials::SOURCE),
    ("node:internal/registry", registry::SOURCE),
    ("node:internal/assert", assert::SOURCE),
    ("node:internal/options", options::SOURCE),
    ("node:internal/timers", timers::SOURCE),
    ("node:internal/snapshot", snapshot::SOURCE),
    ("node:internal/blob", blob::SOURCE),
    ("node:internal/process/task_queues", process_task_queues::SOURCE),
    ("node:internal/abort_controller", abort_controller::SOURCE),
    ("node:internal/async_context_frame", async_context_frame::SOURCE),
    ("node:internal/async_hooks_int", async_hooks_int::SOURCE),
    ("node:internal/test/binding", binding::SOURCE),
    // `internal/async_hooks` 经门面与公开实例同源（分实例即 ALS/enable 状态分叉，
    // eos 分支与套件直调读空即假；immediate-error 套件直引）。
    ("node:internal/async_hooks", super::async_hooks::INTERNAL_ASYNC_HOOKS_FACADE_SOURCE),
    ("node:internal/debuglog", debuglog::SOURCE),
    ("node:internal/encoding", encoding::SOURCE),
    ("node:internal/buffer", buffer::SOURCE),
    ("node:internal/webstream_adapters", webstream_adapters::SOURCE),
    ("node:internal/zip/constants", zip::constants::SOURCE),
    ("node:internal/zip/binary", zip::binary::SOURCE),
    ("node:internal/zip/content-size", zip::content_size::SOURCE),
    ("node:internal/zip/dos", zip::dos::SOURCE),
    ("node:internal/zip/extra-fields", zip::extra_fields::SOURCE),
    ("node:internal/zip/compression", zip::compression::SOURCE),
    ("node:internal/zip/header-builders", zip::header_builders::SOURCE),
    ("node:internal/zip/fs-util", zip::fs_util::SOURCE),
    ("node:internal/zip/headers", zip::headers::SOURCE),
    ("node:internal/zip/entry", zip::entry::SOURCE),
    ("node:internal/zip/archive", zip::archive::SOURCE),
    ("node:internal/zip/buffer", zip::buffer::SOURCE),
    ("node:internal/zip/file", zip::file::SOURCE),
    ("node:internal/streams/legacy", streams::legacy::SOURCE),
    ("node:internal/streams/state", streams::state::SOURCE),
    ("node:internal/streams/utils", streams::utils::SOURCE),
    ("node:internal/streams/destroy", streams::destroy::SOURCE),
    ("node:internal/streams/end_of_stream", streams::end_of_stream::SOURCE),
    ("node:internal/streams/from", streams::from::SOURCE),
    ("node:internal/streams/readable", streams::readable::SOURCE),
    ("node:internal/streams/writable", streams::writable::SOURCE),
    ("node:internal/streams/duplex", streams::duplex::SOURCE),
    ("node:internal/streams/duplexify", streams::duplexify::SOURCE),
    ("node:internal/streams/transform", streams::transform::SOURCE),
    ("node:internal/streams/lazy_transform", streams::lazy_transform::SOURCE),
    ("node:internal/streams/passthrough", streams::passthrough::SOURCE),
    ("node:internal/streams/duplexpair", streams::duplexpair::SOURCE),
    ("node:internal/streams/add_abort_signal", streams::add_abort_signal::SOURCE),
    ("node:internal/streams/pipeline", streams::pipeline::SOURCE),
    ("node:internal/streams/compose", streams::compose::SOURCE),
    ("node:internal/streams/operators", streams::operators::SOURCE),
    ("node:internal/streams/iter_classic", streams::iter_classic::SOURCE),
    ("node:internal/streams/iter_types", streams::iter_types::SOURCE),
    ("node:internal/streams/iter_ringbuffer", streams::iter_ringbuffer::SOURCE),
    ("node:internal/streams/iter_utils", streams::iter_utils::SOURCE),
    ("node:internal/streams/iter_from", streams::iter_from::SOURCE),
    ("node:internal/streams/iter_pull", streams::iter_pull::SOURCE),
    ("node:internal/streams/iter_push", streams::iter_push::SOURCE),
    ("node:internal/streams/iter_duplex", streams::iter_duplex::SOURCE),
    ("node:internal/streams/iter_share", streams::iter_share::SOURCE),
    ("node:internal/streams/iter_broadcast", streams::iter_broadcast::SOURCE),
    ("node:internal/streams/iter_transform", streams::iter_transform::SOURCE),
    ("node:internal/streams/iter_consumers", streams::iter_consumers::SOURCE),
    // R3a：transform 用的缓冲式 zlib 句柄 shim（体部经局部 internalBinding 映射）。
    ("node:internal/streams/iter_zlib_binding", streams::iter_zlib_binding::SOURCE),
    // Phase 9d-6：http/https 共享帧层
    ("node:internal/http_framing", http_framing::SOURCE),
    // 10g：http2 内部 util（套件直引 `internal/http2/util`；kSocket 符号跨模块同源）
    ("node:internal/http2/util", http2_util::SOURCE),
    // 10g：node 内部模块遗留别名（套件 require('_http_agent') 直引）
    ("node:_http_agent", http_aliases::AGENT_SOURCE),
    ("node:_http_common", http_aliases::COMMON_SOURCE),
    ("node:_http_server", http_aliases::SERVER_SOURCE),
    ("node:_http_outgoing", http_aliases::OUTGOING_SOURCE),
    // 残件轮：node:internal/http（套件直引 internal/http，kOutHeaders）。
    ("node:internal/http", http_aliases::HTTP_SOURCE),
    // plan3 test Slice B1：MockTracker 核心（node:test 经此接线）。
    ("node:internal/test/mock", test_mock::SOURCE),
];

/// internal 规范名（`internal/errors` 与 `node:internal/errors` 皆收 → `node:internal/errors`；
/// 非 internal 返回 None）。
/// R9-stream：连字符回落下划线——本仓表内 `streams/*` 等用下划线
/// （`end_of_stream`），真机全连字符（`end-of-stream`）；精确命中优先，
/// 失配再试下划线形（zip 系原生连字符不受影响）。
pub fn normalize_internal(spec: &str) -> Option<&'static str> {
    let rest = spec
        .strip_prefix("node:")
        .and_then(|s| s.strip_prefix("internal/"))
        .or_else(|| spec.strip_prefix("internal/"))?;
    if let Some(hit) = INTERNALS
        .iter()
        .find(|(name, _)| name.strip_prefix("node:internal/") == Some(rest))
        .map(|(name, _)| *name)
    {
        return Some(hit);
    }
    if rest.contains('-') {
        let under: String = rest.replace('-', "_");
        return INTERNALS
            .iter()
            .find(|(name, _)| name.strip_prefix("node:internal/") == Some(under.as_str()))
            .map(|(name, _)| *name);
    }
    None
}

/// internal 源。
pub fn source(canonical: &str) -> Option<&'static str> {
    INTERNALS.iter().find(|(name, _)| *name == canonical).map(|(_, src)| *src)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_spec_table() {
        assert_eq!(normalize_internal("internal/errors"), Some("node:internal/errors"));
        assert_eq!(normalize_internal("internal/util"), Some("node:internal/util"));
        assert_eq!(normalize_internal("internal/util/inspect"), Some("node:internal/util/inspect"));
        assert_eq!(normalize_internal("internal/util/types"), Some("node:internal/util/types"));
        assert_eq!(
            normalize_internal("internal/events/abort_listener"),
            Some("node:internal/events/abort_listener")
        );
        assert_eq!(normalize_internal("internal/nope"), None);
        assert_eq!(normalize_internal("errors"), None);
        assert_eq!(normalize_internal("node:internal/errors"), Some("node:internal/errors"));
        // R9-stream：连字符回落（套件直引真机形）。
        assert_eq!(
            normalize_internal("internal/streams/add-abort-signal"),
            Some("node:internal/streams/add_abort_signal")
        );
        assert_eq!(
            normalize_internal("internal/streams/end-of-stream"),
            Some("node:internal/streams/end_of_stream")
        );
        // 表长度随注册增减（G11 +4 http 别名 + http2_util +1 + test/mock +1 + internal/http +1 + timers +1 + test/binding +1 + async_hooks +1；R2-iter + task_queues +1 + iter 系 +10；R3a + zlib_binding +1；增删同步改此数）。
        assert_eq!(INTERNALS.len(), 79);
        for (name, src) in INTERNALS {
            assert!(source(name).is_some(), "{name} missing");
            assert!(!src.is_empty(), "{name} empty source");
            assert!(src.contains("export") || src.contains("module.exports"), "{name} no exports");
        }
    }
}
