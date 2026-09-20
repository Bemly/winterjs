//! state/fetch：fetch 回调/任务计数 + 流式 body 泵（§0.9 拆分）。

use super::*;
use mozjs::jsapi::Heap;
use mozjs::jsval::JSVal;


/// 分配 fetch id 并计数（调用方随后 spawn 任务；失败路径须配套 `fetch_unpend`）。
pub fn fetch_alloc() -> Option<(u64, tokio::sync::mpsc::UnboundedSender<crate::builtins::fetch::FetchMsg>)> {
    with_plain(|p| {
        let tx = p.fetch_tx.clone()?;
        p.fetch_next_id += 1;
        let id = p.fetch_next_id;
        p.fetch_pending += 1;
        Some((id, tx))
    })
}

/// fetch 计数减一（结算/取消时调用；与 `fetch_alloc` 严格配对，见 `fetch_abort` 注释）。
pub fn fetch_unpend() {
    with_plain(|p| p.fetch_pending = p.fetch_pending.saturating_sub(1));
}

/// 登记任务句柄（spawn 后调用；建任务失败路径不调，调用方配套 `fetch_unpend`）。
pub fn fetch_task(id: u64, handle: tokio::task::AbortHandle) {
    with_plain(|p| {
        p.fetch_tasks.insert(id, handle);
    });
}

/// 任务完成记账（句柄移除；计数由回调摘除侧负责，见下）。
pub fn fetch_task_done(id: u64) {
    with_plain(|p| {
        p.fetch_tasks.remove(&id);
    });
}

/// 取消未决 fetch（幂等；未知 id 静默成功）。
/// 计数规则：`fetch_pending` 与回调摘除严格配对——`settle` 取走回调时减、
/// 此处取走回调时减；`settle` 的 None 分支（回调已被此处取走）不再减。
/// 残留消息（取消时已在途）落到 None 分支即丢弃，无副作用。
/// 流等待者不归此处：调用方（`fetch::fetch_abort`）经 `stream_abort` 取走并拒绝。
pub fn fetch_abort(id: u64) {
    if let Some(handle) = with_plain(|p| p.fetch_tasks.remove(&id)) {
        handle.abort();
    }
    let removed = with_rooted(|s| {
        s.fetch_callbacks
            .iter()
            .position(|c| c.id == id)
            .map(|i| s.fetch_callbacks.remove(i))
            .is_some()
    });
    if removed {
        fetch_unpend();
    }
}

// ── 流式 body 状态（事件循环 + pull 两侧）──────────────────────────────────
// 背压说明：channel 无界 + Rust 侧 VecDeque 无界（消费慢即堆内存；与 prelude
// streams 的 tee 简化同哲学，文档记录，不静默丢数据）。

/// 响应头到达时建流（`deliver` 的 stream 分支；重复建幂等覆盖）。
pub fn stream_add(id: u64) {
    with_rooted(|s| {
        if !s.fetch_streams.iter().any(|st| st.id == id) {
            s.fetch_streams.push(FetchStreamState {
                id,
                chunks: std::collections::VecDeque::new(),
                waiters: Vec::new(),
                done: false,
                error: None,
            });
        }
    });
}

/// pull 动作（纯状态操作；JS 调用由 native 侧执行，避开 TLS 嵌套借用 §4.14）。
pub enum StreamPull {
    /// 立即给 chunk。
    Chunk(Vec<u8>),
    /// 流已终结（done/未知 id/cancel 后）→ resolve null。
    Done,
    /// 流已失败 → reject。
    Failed(String),
    /// 已排队（native 不再调用）。
    Queued,
}

/// prelude `pull` 进（resolve/reject 为 JS 函数值；未知 id 按 Done 处理）。
/// 终结后全消费的流此处摘除（内存有界；见 `stream_on_finish` 语义注释）。
pub fn stream_pull(id: u64, resolve: JSVal, reject: JSVal) -> StreamPull {
    with_rooted(|s| {
        let Some(pos) = s.fetch_streams.iter().position(|st| st.id == id) else {
            return StreamPull::Done;
        };
        // 先给缓冲 chunk（终结与否皆然；终结态的残留缓冲照常交付）。
        if let Some(chunk) = s.fetch_streams[pos].chunks.pop_front() {
            return StreamPull::Chunk(chunk);
        }
        if s.fetch_streams[pos].done {
            // 全消费即摘除；后继 pull 走未知 id → Done。
            if s.fetch_streams[pos].waiters.is_empty() {
                s.fetch_streams.remove(pos);
            }
            return StreamPull::Done;
        }
        if let Some(err) = s.fetch_streams[pos].error.clone() {
            return StreamPull::Failed(err);
        }
        s.fetch_streams[pos].waiters.push(StreamWaiter {
            resolve: Heap::boxed(resolve),
            reject: Heap::boxed(reject),
        });
        StreamPull::Queued
    })
}

/// chunk 到达的泵动作（调用方执行 JS 回调）。
pub enum StreamPump {
    /// 唤醒首个等待者给 chunk。
    Wake(JSVal, Vec<u8>),
    /// 终结交付：各等待者按序拿残留 chunk，拿不到的 resolve-null。
    DoneAll(Vec<(JSVal, Option<Vec<u8>>)>),
    /// 全部等待者 reject（失败终结）。
    FailAll(Vec<JSVal>, String),
    /// 缓存/丢弃（无动作）。
    Buffered,
}

/// chunk 到达（事件循环；未知/已终结流即丢弃——cancel 后残留或 Done 后多发）。
pub fn stream_on_chunk(id: u64, chunk: Vec<u8>) -> StreamPump {
    with_rooted(|s| {
        let Some(st) = s.fetch_streams.iter_mut().find(|st| st.id == id) else {
            return StreamPump::Buffered;
        };
        if st.done || st.error.is_some() {
            return StreamPump::Buffered;
        }
        if !st.waiters.is_empty() {
            let w = st.waiters.remove(0);
            return StreamPump::Wake(w.resolve.get(), chunk);
        }
        st.chunks.push_back(chunk);
        StreamPump::Buffered
    })
}

/// 终态语义（事件循环）：
/// - Done：等待者按序分残留 chunk，分不到的 resolve-null；残留缓冲保留给后继 pull；
///   流标 done 保留（后继 pull 消费完即摘，见 `stream_pull`）。
/// - Failed：等待者全 reject；流标 error 保留（后继 pull 照常 reject）。
/// 存活计数（`stream_pending`）只看未终结流：终结流不续命事件循环（类比 Node
/// 的 EOF socket：无人消费的残留数据随进程退出丢弃，文档记录）。
pub fn stream_on_finish(id: u64, err: Option<String>) -> StreamPump {
    with_rooted(|s| {
        let Some(pos) = s.fetch_streams.iter().position(|st| st.id == id) else {
            return StreamPump::Buffered;
        };
        if let Some(e) = err {
            let st = &mut s.fetch_streams[pos];
            st.error = Some(e.clone());
            let rejects: Vec<JSVal> = st.waiters.drain(..).map(|w| w.reject.get()).collect();
            return StreamPump::FailAll(rejects, e);
        }
        let st = &mut s.fetch_streams[pos];
        st.done = true;
        let mut out = Vec::with_capacity(st.waiters.len());
        for w in st.waiters.drain(..) {
            let chunk = st.chunks.pop_front();
            out.push((w.resolve.get(), chunk));
        }
        StreamPump::DoneAll(out)
    })
}

/// 中止流并取走全部等待者（调用方负责 reject；残留缓冲一并丢弃）。
/// 返回 `(found, waiters)`。cancel 与 `fetch::fetch_abort` 共用。
pub fn stream_abort(id: u64) -> (bool, Vec<(JSVal, JSVal)>) {
    with_rooted(|s| {
        let Some(pos) = s.fetch_streams.iter().position(|st| st.id == id) else {
            return (false, Vec::new());
        };
        let mut st = s.fetch_streams.remove(pos);
        let waiters: Vec<(JSVal, JSVal)> =
            st.waiters.drain(..).map(|w| (w.resolve.get(), w.reject.get())).collect();
        (true, waiters)
    })
}

/// 存活流数（任务未终结；事件循环退出条件用，语义见 `stream_on_finish`）。
pub fn stream_pending() -> usize {
    with_rooted(|s| s.fetch_streams.iter().filter(|st| !st.done && st.error.is_none()).count())
}

/// 未决 fetch 数（事件循环退出条件用）。
pub fn fetch_pending() -> usize {
    with_plain(|p| p.fetch_pending)
}
