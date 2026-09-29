//! state/ports：MessagePort 路由/转交 + BroadcastChannel 注册。

use super::*;

use mozjs::gc::Traceable;
use mozjs::jsapi::{Heap, JSTracer};
use mozjs::jsval::JSVal;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, OnceLock};


/// 建直连端口对（同会话回环；初始未监听不计数，`port_listen` 后续命）。
pub fn port_pair() -> Option<(u64, u64)> {
    with_plain(|p| {
        let tx = p.worker_tx.clone()?;
        p.worker_next_id += 1;
        let a = p.worker_next_id;
        p.worker_next_id += 1;
        let b = p.worker_next_id;
        with_rooted(|s| {
            s.worker_ports.push(WorkerPort { id: a, peer: b, peer_tx: tx.clone(), target: None, open: true, refed: true, listening: false, counted: false, peer_closed: false, peer_is_worker: false, forward: None, moved: false, via: None, pending: Vec::new() });
            s.worker_ports.push(WorkerPort { id: b, peer: a, peer_tx: tx, target: None, open: true, refed: true, listening: false, counted: false, peer_closed: false, peer_is_worker: false, forward: None, moved: false, via: None, pending: Vec::new() });
        });
        Some((a, b))
    })
}

/// 建跨会话端口（worker parentPort 用；对端地址是 worker id，走 `WMsg`）。
pub fn port_alloc_cross(
    peer_worker: u64,
    peer_tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>,
) -> u64 {
    let id = with_plain(|p| {
        p.worker_next_id += 1;
        p.worker_next_id
    });
    with_rooted(|s| {
        s.worker_ports.push(WorkerPort { id, peer: peer_worker, peer_tx, target: None, open: true, refed: true, listening: false, counted: false, peer_closed: false, peer_is_worker: true, forward: None, moved: false, via: None, pending: Vec::new() });
    });
    id
}

/// 按 `open && refed && listening` 重算，返回计数净变化（+1/0/-1）。
/// 对端已关（peer_closed）不再续命——node 口径：对端关后本端口无法再收新
/// 消息，receive-message 套件靠它收尾退出。
fn port_recount(id: u64) -> i64 {
    with_rooted(|s| match s.worker_ports.iter_mut().find(|p| p.id == id) {
        Some(p) => {
            let want = p.open && p.refed && p.listening && !p.moved && p.forward.is_none() && !p.peer_closed;
            if want == p.counted {
                0
            } else {
                p.counted = want;
                if want { 1 } else { -1 }
            }
        }
        None => 0,
    })
}

fn port_bump(delta: i64) {
    with_plain(|p| {
        if delta > 0 {
            p.worker_open += delta as usize;
        } else {
            p.worker_open = p.worker_open.saturating_sub((-delta) as usize);
        }
    });
}

/// 监听装上（有 message 监听即续命）。
pub fn port_listen(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.listening = true;
        }
    });
    port_bump(port_recount(id));
}

/// 监听卸完（无 message 监听即不续命；JS 侧末个移除时调）。
pub fn port_unlisten(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.listening = false;
        }
    });
    port_bump(port_recount(id));
}

pub fn port_has_ref(id: u64) -> bool {
    with_rooted(|s| s.worker_ports.iter().find(|p| p.id == id).is_some_and(|p| p.refed))
}

/// 摘目标（`__wjs2_port_detach` 用）：迁移排空后/静默摘除，不通知对端，不碰路由。
pub fn port_detach(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.target = None;
            if p.counted {
                p.counted = false;
                port_bump(-1);
            }
        }
    });
}

/// 取转发路由（`PortMsg` 分发回退用：`forwarded` 排空摘 target 后到达的迟到
/// 消息改道新址，不丢）。
pub fn port_forward_route(id: u64) -> Option<(PortTx, u64)> {
    with_rooted(|s| {
        s.worker_ports.iter().find(|p| p.id == id).and_then(|p| {
            p.forward.as_ref().map(|f| (f.tx.clone(), f.to))
        })
    })
}

/// 登记端口 JS 目标（MessagePort 构造时 attach；dispatch 经 `__ev` 回调）。
pub fn port_attach(id: u64, target: JSVal) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.target = Some(Heap::boxed(target));
        }
    });
}

pub fn port_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| {
        s.worker_ports.iter().find(|p| p.id == id).and_then(|p| p.target.as_ref().map(|t| t.get()))
    })
}

/// 同步收信口（10f：`receiveMessageOnPort` 底座）——取本端口 pending 队首
/// wire；空即 None。纯 Rust 串进出，无 GC 值（§4.40 n/a）。
pub fn port_try_recv(id: u64) -> Option<String> {
    with_rooted(|s| {
        s.worker_ports
            .iter_mut()
            .find(|p| p.id == id)
            .and_then(|p| {
                if p.pending.is_empty() { None } else { Some(p.pending.remove(0)) }
            })
    })
}

/// pump 逐轮派发用：取走全端口 pending（(port_id, wire) 平化，按表序）。
pub fn take_port_pending() -> Vec<(u64, String)> {
    with_rooted(|s| {
        let mut out = Vec::new();
        for p in s.worker_ports.iter_mut() {
            if !p.pending.is_empty() {
                out.extend(p.pending.drain(..).map(|w| (p.id, w)));
            }
        }
        out
    })
}

/// 发往对端（本端已关/对端已关即丢弃，Node 同款静默；parentPort 走 `WMsg`；
/// 转发器表项改道新址，发送失败即自拆）。
pub fn port_post(id: u64, json: String) -> bool {    enum Route {
        Direct { peer: u64, tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>, is_worker: bool },
        Forward { tx: tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>, to: u64 },
        /// 本地 pair：对端 pending 表直投（pump 逐轮派发；10f）。
        Local { peer: u64 },
    }
    let route = with_rooted(|s| {
        s.worker_ports.iter().find(|p| p.id == id).and_then(|p| {
            if !p.open || p.peer_closed {
                return None;
            }
            if let Some(f) = p.forward.as_ref() {
                return Some(Route::Forward { tx: f.tx.clone(), to: f.to });
            }
            // 10f：本地 pair 且对端表项在本会话 → 直入对端 pending（同步收信
            // 口 + pump 逐轮派发；跨会话口仍走通道）。
            if !p.peer_is_worker
                && s.worker_ports.iter().any(|q| q.id == p.peer && q.open && !q.moved)
            {
                return Some(Route::Local { peer: p.peer });
            }
            Some(Route::Direct { peer: p.peer, tx: p.peer_tx.clone(), is_worker: p.peer_is_worker })
        })
    });
    match route {
        Some(Route::Local { peer }) => {
            with_rooted(|s| {
                if let Some(q) = s.worker_ports.iter_mut().find(|q| q.id == peer) {
                    q.pending.push(json);
                    true
                } else {
                    false
                }
            })
        }
        Some(Route::Direct { peer, tx, is_worker: true }) => tx.send(crate::builtins::node::worker::WorkerEvent::WMsg { worker_id: peer, json }).is_ok(),
        Some(Route::Direct { peer, tx, is_worker: false }) => tx.send(crate::builtins::node::worker::WorkerEvent::PortMsg { to: peer, json }).is_ok(),
        Some(Route::Forward { tx, to }) => {
            if tx.send(crate::builtins::node::worker::WorkerEvent::PortMsg { to, json }).is_ok() {
                true
            } else {
                // 新址已死：自拆转发器，后续直落。
                with_rooted(|s| {
                    if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
                        p.open = false;
                        p.forward = None;
                    }
                });
                false
            }
        }
        None => false,
    }
}

/// 本端关闭（首次 true；摘 target；计过数即减；尽力通知对端记 peer_closed；
/// 承接表项另发 `PortDrop` 拆源转发器）。
pub fn port_close(id: u64) -> bool {
    let route = with_rooted(|s| {
        let mut out = None;
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            if !p.open {
                return None;
            }
            p.open = false;
            p.target = None;
            p.forward = None;
            let was = p.counted;
            p.counted = false;
            out = Some((p.peer, p.peer_tx.clone(), was, p.via.clone()));
        }
        out
    });
    match route {
        Some((peer, tx, was, via)) => {
            if was {
                port_bump(-1);
            }
            // 原语义保持：`PortClose{to: peer}`（cross 表项的 peer 是 worker_id，
            // 对端查不到即无视，静默；见旧实现）。
            let _ = tx.send(crate::builtins::node::worker::WorkerEvent::PortClose { to: peer });
            if let Some((via_tx, via_id)) = via {
                let _ = via_tx.send(crate::builtins::node::worker::WorkerEvent::PortDrop { to: via_id });
            }
            true
        }
        None => false,
    }
}

/// 取消引用（端口不再续命事件循环；幂等）。
pub fn port_unref(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.refed = false;
        }
    });
    port_bump(port_recount(id));
}

/// 重新引用（unref 的逆操作；关闭/无监听即无操作）。
pub fn port_ref(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.refed = true;
        }
    });
    port_bump(port_recount(id));
}

/// 对端关闭到达：记 peer_closed（后续 post 静默丢弃；本端不派 close，Node 口径；
/// 到达转发器则递往新址后自拆）。
pub fn port_peer_closed(id: u64) {
    let (fwd, recount) = with_rooted(|s| {
        let mut out = None;
        let mut needs_recount = false;
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            if p.forward.is_some() {
                out = p.forward.as_ref().map(|f| (f.tx.clone(), f.to));
                p.open = false;
                p.forward = None;
            } else {
                // 10f：对端关后本端口不再续命（receive-message 收尾）；recount
                // 在 with_rooted 外补调（§4.14 禁嵌套）。
                p.peer_closed = true;
                needs_recount = true;
            }
        }
        (out, needs_recount)
    });
    if recount {
        port_bump(port_recount(id));
    }
    if let Some((tx, to)) = fwd {
        let _ = tx.send(crate::builtins::node::worker::WorkerEvent::PortClose { to });
    }
}

// ── 端口迁移（9i-2；`transferList` 的 MessagePort 件）───────────────────────
//
// 表是会话级的，id 只在源会话有效；跨会话迁移走进程级邀约槽：offer 侧快照
// 路由并摘 target（转 silent：对端不感知），accept 侧建表并回发 `PortForward`
// 把源表项升为转发器（对端永远只认原路由，多一跳）。同会话迁移走同一路径。

type PortTx = tokio::sync::mpsc::UnboundedSender<crate::builtins::node::worker::WorkerEvent>;

/// 迁移邀约（offer 会话存，accept 会话取走即删）。
struct PortOffer {
    source_tx: PortTx,
    source_id: u64,
    peer: u64,
    peer_tx: PortTx,
    peer_is_worker: bool,
}

/// 分配并记下本会话序号（`init_session` 内调一次）。
pub fn session_seq_init() -> u64 {
    let n = SESSION_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    with_plain(|p| p.worker_session_seq = n);
    n
}

fn port_xfer() -> &'static Mutex<HashMap<String, PortOffer>> {
    PORT_XFER.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 邀约迁移（`__wjs2_port_offer` 用）：表项须 open 且未迁移；成功保留 target 收
/// 竞态消息（`forwarded` 到达排空），置 moved 停计数，回 nonce 串；失败回 None。
static PORT_XFER: OnceLock<Mutex<HashMap<String, PortOffer>>> = OnceLock::new();
static PORT_XFER_NEXT: AtomicU64 = AtomicU64::new(1);
/// 会话序号分配（进程级单调；`init_session` 内调）。
static SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

/// 取本会话序号（0 表未初始化，调用方须在 `init_session` 后用）。
pub fn session_seq() -> u64 {
    with_plain(|p| p.worker_session_seq)
}

// ── BroadcastChannel（9i-2；同名跨会话扇出）──────────────────────────────
// 落法：订阅是进程级注册表（名 → 订阅者收件箱），投递走各会话收件箱的
// `BcMsg` 事件；会话侧 `bc_targets` 持 JS 目标（`Box` 定址 §4.40）。
// 计数抄端口口径（`open && refed && listening`），`worker_open` 同一池子。

/// 一个 BC 订阅的 JS 目标。
pub struct BcTarget {
    pub id: u64,
    pub name: String,
    pub target: Option<Box<Heap<JSVal>>>,
    pub open: bool,
    pub refed: bool,
    pub listening: bool,
    pub counted: bool,
}

// SAFETY: 只追踪 target（其余无 GC 指针）。
unsafe impl Traceable for BcTarget {
    unsafe fn trace(&self, trc: *mut JSTracer) { unsafe {
        self.target.trace(trc);
    }}
}

static BC_REGISTRY: OnceLock<Mutex<HashMap<String, Vec<BcSub>>>> = OnceLock::new();

fn bc_registry() -> &'static Mutex<HashMap<String, Vec<BcSub>>> {
    BC_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn port_offer(id: u64) -> Option<String> {
    let own_tx = with_plain(|p| p.worker_tx.clone())?;
    let snap = with_rooted(|s| {
        let p = s.worker_ports.iter_mut().find(|p| p.id == id)?;
        if !p.open || p.moved || p.forward.is_some() {
            return None;
        }
        p.moved = true;
        if p.counted {
            p.counted = false;
            port_bump(-1);
        }
        Some((p.peer, p.peer_tx.clone(), p.peer_is_worker))
    });
    let (peer, peer_tx, peer_is_worker) = snap?;
    let n = PORT_XFER_NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let nonce = format!("px{n}");
    if port_xfer().lock().ok()?.insert(nonce.clone(), PortOffer { source_tx: own_tx, source_id: id, peer, peer_tx, peer_is_worker }).is_some() {
        return None; // 计数器回绕碰撞（实践不发生），邀约作废
    }
    Some(nonce)
}

/// 撤邀约（`__wjs2_port_withdraw` 用）：消息组装失败/未引用转让的槽回收；
/// 源表项保持摘 target（JS 侧已 neutered），对端无感知。
pub fn port_withdraw(nonce: &str) -> bool {
    port_xfer().lock().ok().is_some_and(|mut m| m.remove(nonce).is_some())
}

/// 承接迁移（`__wjs2_port_accept` 用）：nonce 有效即在当前会话建表（路由直连原
/// 对端），回发 `PortForward` 升源表项为转发器，回新 id；失败回 None。
pub fn port_accept(nonce: &str) -> Option<u64> {
    let offer = port_xfer().lock().ok()?.remove(nonce)?;
    let own_tx = with_plain(|p| p.worker_tx.clone())?;
    let id = with_plain(|p| {
        p.worker_next_id += 1;
        p.worker_next_id
    });
    with_rooted(|s| {
        s.worker_ports.push(WorkerPort {
            id,
            peer: offer.peer,
            peer_tx: offer.peer_tx,
            target: None,
            open: true,
            refed: true,
            listening: false,
            counted: false,
            peer_closed: false,
            peer_is_worker: offer.peer_is_worker,
            forward: None,
            moved: false,
            via: Some((offer.source_tx.clone(), offer.source_id)),
            pending: Vec::new(),
        });
    });
    let _ = offer.source_tx.send(crate::builtins::node::worker::WorkerEvent::PortForward {
        to: offer.source_id,
        dest_tx: own_tx,
        dest_id: id,
    });
    Some(id)
}

/// 源表项升为转发器（`PortForward` 派发用；表项已死即丢弃）。
pub fn port_forward_set(id: u64, dest_tx: PortTx, dest_id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            if !p.open {
                return;
            }
            p.forward = Some(PortForward { tx: dest_tx, to: dest_id });
        }
    });
}

/// 拆转发器（`PortDrop` 派发用；承接端已关）。
pub fn port_drop(id: u64) {
    with_rooted(|s| {
        if let Some(p) = s.worker_ports.iter_mut().find(|p| p.id == id) {
            p.open = false;
            p.forward = None;
            p.target = None;
            if p.counted {
                p.counted = false;
                port_bump(-1);
            }
        }
    });
}

/// 进程级订阅项（投递地址；`sub` 是归属会话内的本地 id，`sess` 排自发）。
#[derive(Clone)]
struct BcSub {
    sess: u64,
    tx: PortTx,
    sub: u64,
}

fn bc_recount(id: u64) {
    let delta = with_rooted(|s| match s.bc_targets.iter_mut().find(|b| b.id == id) {
        Some(b) => {
            let want = b.open && b.refed && b.listening;
            if want == b.counted {
                0
            } else {
                b.counted = want;
                if want { 1 } else { -1 }
            }
        }
        None => 0,
    });
    port_bump(delta);
}

/// 订阅（`__wjs2_bc_sub` 用）：进程注册 + 会话建表，回本地 sub id。
pub fn bc_sub(name: String) -> Option<u64> {
    let own_tx = with_plain(|p| p.worker_tx.clone())?;
    let id = with_plain(|p| {
        p.worker_next_id += 1;
        p.worker_next_id
    });
    with_rooted(|s| {
        s.bc_targets.push(BcTarget {
            id,
            name: name.clone(),
            target: None,
            open: true,
            refed: true,
            listening: false,
            counted: false,
        });
    });
    bc_registry().lock().ok()?.entry(name).or_default().push(BcSub { sess: session_seq(), tx: own_tx, sub: id });
    Some(id)
}

/// 退订（`__wjs2_bc_unsub` 用）：双表摘除；计过数即减。
pub fn bc_unsub(id: u64) {
    let name = with_rooted(|s| {
        let mut out = None;
        if let Some(i) = s.bc_targets.iter().position(|b| b.id == id) {
            let b = s.bc_targets.remove(i);
            if b.counted {
                port_bump(-1);
            }
            out = Some(b.name);
        }
        out
    });
    if let Some(name) = name {
        let me = session_seq();
        if let Ok(mut reg) = bc_registry().lock() {
            if let Some(v) = reg.get_mut(&name) {
                v.retain(|e| !(e.sess == me && e.sub == id));
                if v.is_empty() {
                    reg.remove(&name);
                }
            }
        }
    }
}

/// 扇出（`__wjs2_bc_pub` 用）：同名订阅全发（除发送者自身 `(sess, sub)`），
/// 关闭/死亡即摘。同会话订阅走 pending 表（`receiveMessageOnPort` 同步收信
/// 口 + pump 逐轮派发双消费，端口 pending 同款；10f broadcastchannel 套件）。
pub fn bc_pub(name: &str, except_sub: u64, json: String) {
    let me = session_seq();
    let subs: Vec<BcSub> = bc_registry().lock().ok().and_then(|reg| reg.get(name).cloned()).unwrap_or_default();
    let mut dead: Vec<(u64, u64)> = Vec::new();
    for e in &subs {
        if e.sess == me && e.sub == except_sub {
            continue;
        }
        if e.sess == me {
            bc_pending_push(e.sub, json.clone());
            continue;
        }
        if e.tx.send(crate::builtins::node::worker::WorkerEvent::BcMsg { to: e.sub, json: json.clone() }).is_err() {
            dead.push((e.sess, e.sub));
        }
    }
    if !dead.is_empty() {
        if let Ok(mut reg) = bc_registry().lock() {
            if let Some(v) = reg.get_mut(name) {
                v.retain(|e| !dead.contains(&(e.sess, e.sub)));
                if v.is_empty() {
                    reg.remove(name);
                }
            }
        }
    }
}

/// BC 同会话 pending 入队（纯 Rust 串，无 GC 值）。
pub fn bc_pending_push(sub: u64, wire: String) {
    with_rooted(|s| s.bc_pending.push((sub, wire)));
}

/// 同步收信口（`receiveMessageOnPort` 对 BC 的底座）：取该 sub 队首 wire。
pub fn bc_try_recv(sub: u64) -> Option<String> {
    with_rooted(|s| {
        let i = s.bc_pending.iter().position(|(id, _)| *id == sub)?;
        Some(s.bc_pending.remove(i).1)
    })
}

/// pump 逐轮派发用：取走全部 BC pending，**按 sub id（端口创建序）分组**——
/// node 底层每端口独立队列、按端口序逐口排空（broadcastchannel-wpt 套件
/// 三端口事件序 `from c3, done, from c1, from c3, from c1, done` 逐字断言）。
pub fn take_bc_pending() -> Vec<(u64, String)> {
    with_rooted(|s| {
        let mut out = std::mem::take(&mut s.bc_pending);
        out.sort_by_key(|(id, _)| *id);
        // sort 稳定（Vec sort_by_key 稳定序）：同 sub 内保持投递序。
        out
    })
}

/// 旗变更（`__wjs2_bc_flags` 用）：`listen/unlisten/ref/unref` 四档。
pub fn bc_flags(id: u64, what: &str) {
    with_rooted(|s| {
        if let Some(b) = s.bc_targets.iter_mut().find(|b| b.id == id) {
            match what {
                "listen" => b.listening = true,
                "unlisten" => b.listening = false,
                "ref" => b.refed = true,
                "unref" => b.refed = false,
                _ => return,
            }
        } else {
            return;
        }
    });
    bc_recount(id);
}

/// 登记 BC JS 目标（构造时 attach）。
pub fn bc_attach(id: u64, target: JSVal) {
    with_rooted(|s| {
        if let Some(b) = s.bc_targets.iter_mut().find(|b| b.id == id) {
            b.target = Some(Heap::boxed(target));
        }
    });
}

/// 取 BC 目标（`BcMsg` 派发用）。
pub fn bc_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| {
        s.bc_targets.iter().find(|b| b.id == id).and_then(|b| b.target.as_ref().map(|t| t.get()))
    })
}
