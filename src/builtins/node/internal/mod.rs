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
pub mod blob;
pub mod abort_controller;
pub mod buffer;
pub mod debuglog;
pub mod encoding;
pub mod errors;
pub mod event_target;
pub mod fixed_queue;
pub mod inspect;
pub mod options;
pub mod primordials;
pub mod querystring;
pub mod registry;
pub mod snapshot;
pub mod streams;
pub mod symbols;
pub mod types;
pub mod util;
pub mod validators;
pub mod webstream_adapters;

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
    ("node:internal/snapshot", snapshot::SOURCE),
    ("node:internal/blob", blob::SOURCE),
    ("node:internal/abort_controller", abort_controller::SOURCE),
    ("node:internal/async_context_frame", async_context_frame::SOURCE),
    ("node:internal/async_hooks_int", async_hooks_int::SOURCE),
    ("node:internal/debuglog", debuglog::SOURCE),
    ("node:internal/encoding", encoding::SOURCE),
    ("node:internal/buffer", buffer::SOURCE),
    ("node:internal/webstream_adapters", webstream_adapters::SOURCE),
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
];

/// internal 规范名（`internal/errors` 与 `node:internal/errors` 皆收 → `node:internal/errors`；
/// 非 internal 返回 None）。
pub fn normalize_internal(spec: &str) -> Option<&'static str> {
    let rest = spec
        .strip_prefix("node:")
        .and_then(|s| s.strip_prefix("internal/"))
        .or_else(|| spec.strip_prefix("internal/"))?;
    INTERNALS
        .iter()
        .find(|(name, _)| name.strip_prefix("node:internal/") == Some(rest))
        .map(|(name, _)| *name)
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
        assert_eq!(INTERNALS.len(), 43);
        for (name, src) in INTERNALS {
            assert!(source(name).is_some(), "{name} missing");
            assert!(!src.is_empty(), "{name} empty source");
            assert!(src.contains("export") || src.contains("module.exports"), "{name} no exports");
        }
    }
}
