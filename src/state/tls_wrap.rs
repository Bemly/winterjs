//! state/tls_wrap：TLSSocket 包裹引擎登记表（PlainState.tls_engines；无 GC 指针，
//! natives 同步进出、仅 JS 线程触碰——不经事件通道，§4.24）。

use super::*;
use crate::builtins::node::tls_wrap::TlsWrapEngine;

/// 登记引擎（id 与 net 面共用计数器，两表独立互不冲突）。
pub fn tls_wrap_add(engine: TlsWrapEngine) -> u64 {
    with_plain(|p| {
        p.net_next_id += 1;
        let id = p.net_next_id;
        p.tls_engines.insert(id, engine);
        id
    })
}

/// 引擎闭包操作（引擎表在 PlainState，出借不可跨闭包——操作整体在表内完成）。
pub fn tls_wrap_with_engine<R>(id: u64, f: impl FnOnce(&mut TlsWrapEngine) -> R) -> Option<R> {
    with_plain(|p| p.tls_engines.get_mut(&id).map(f))
}

/// 摘表（kill / destroy 收尾）。
pub fn tls_wrap_del(id: u64) {
    with_plain(|p| p.tls_engines.remove(&id));
}
