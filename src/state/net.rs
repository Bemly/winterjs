//! state/net：网络驱动端点/socket 表/半关状态机 + 占位 listener。

use super::*;
use mozjs::jsval::JSVal;


// ── 网络驱动（node:net；task → channel → 事件循环，同 child 模型）───────────

/// 分配网络 id + 事件端点。
pub fn net_alloc() -> Option<(
    u64,
    tokio::sync::mpsc::UnboundedSender<crate::builtins::node::net::NetEvent>,
)> {
    with_plain(|p| {
        let tx = p.net_tx.clone()?;
        p.net_next_id += 1;
        Some((p.net_next_id, tx))
    })
}

/// 登记客户端/服务端 socket（native 侧；返回写端命令接收端交泵 task）。
pub fn net_socket_add(
    id: u64,
    target: JSVal,
) -> tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::net::NetCmd> {
    with_rooted(|s| s.net_targets.push(NetTarget { id, target: Heap::boxed(target) }));
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    with_plain(|p| {
        p.net_sockets.insert(id, NetEntry { cmd_tx: tx, half_read: false, half_write: false, close_sent: false, writer_alive: true, reader_done: false, refed: true });
        p.net_open += 1;
    });
    rx
}

/// server accept 出的连接：无 target 入表（JS 侧 attach 后补），返回命令接收端。
pub fn net_conn_add() -> (u64, tokio::sync::mpsc::UnboundedReceiver<crate::builtins::node::net::NetCmd>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let id = with_plain(|p| {
        p.net_next_id += 1;
        let id = p.net_next_id;
        p.net_sockets.insert(id, NetEntry { cmd_tx: tx, half_read: false, half_write: false, close_sent: false, writer_alive: true, reader_done: false, refed: true });
        p.net_open += 1;
        id
    });
    (id, rx)
}

/// 事后补登记 target（server 连接 attach）。
pub fn net_target_add(id: u64, target: JSVal) {
    with_rooted(|s| s.net_targets.push(NetTarget { id, target: Heap::boxed(target) }));
}

pub fn net_target(id: u64) -> Option<JSVal> {
    with_rooted(|s| s.net_targets.iter().find(|t| t.id == id).map(|t| t.target.get()))
}

/// 发命令（socket 已收尾即 false）。
pub fn net_cmd(id: u64, cmd: crate::builtins::node::net::NetCmd) -> bool {
    with_plain(|p| p.net_sockets.get(&id).is_some_and(|e| e.cmd_tx.send(cmd).is_ok()))
}

/// 置半关旗（读端）；返回是否两侧都已半关（旗已随 purge 清空不重发）。
pub fn net_half_read(id: u64) -> bool {
    with_plain(|p| {
        if let Some(e) = p.net_sockets.get_mut(&id) {
            e.half_read = true;
            e.half_read && e.half_write
        } else {
            false
        }
    })
}

/// 置半关旗（写端）。
pub fn net_half_write(id: u64) -> bool {
    with_plain(|p| {
        if let Some(e) = p.net_sockets.get_mut(&id) {
            e.half_write = true;
            e.half_read && e.half_write
        } else {
            false
        }
    })
}

/// 标记写端 task 退出（返回此前是否存活）。
pub fn net_writer_exit(id: u64) -> bool {
    with_plain(|p| {
        if let Some(e) = p.net_sockets.get_mut(&id) {
            std::mem::replace(&mut e.writer_alive, false)
        } else {
            false
        }
    })
}

/// 标记读端 task 退出（EOF/错后 break；写端已死则此处不补——写端退出侧补）。
pub fn net_reader_done(id: u64) {
    with_plain(|p| {
        if let Some(e) = p.net_sockets.get_mut(&id) {
            e.reader_done = true;
        }
    });
}

/// 读端是否已退出（写端退出时若读端已走则补发 Close，close_once 防双发）。
pub fn net_reader_gone(id: u64) -> bool {
    with_plain(|p| p.net_sockets.get(&id).is_some_and(|e| e.reader_done))
}

/// 写端是否已死（死则读端 EOF 需代行收尾，End 命令无人消费）。
pub fn net_writer_dead(id: u64) -> bool {
    with_plain(|p| p.net_sockets.get(&id).is_some_and(|e| !e.writer_alive))
}

/// Close 单次发送旗（task 侧防双发；purge 由 dispatch 完成后统一做）。
pub fn net_close_once(id: u64) -> bool {
    with_plain(|p| {
        if let Some(e) = p.net_sockets.get_mut(&id) {
            if e.close_sent {
                false
            } else {
                e.close_sent = true;
                true
            }
        } else {
            false
        }
    })
}

/// ref/unref 真计数（10a）：切换 refed 位并增减 net_open；entry 不在即 false。
/// unref 后 Close 派发不再减（purge 按位），ref 装回后恢复。
pub fn net_set_ref(id: u64, refed: bool) -> bool {
    with_plain(|p| {
        if let Some(e) = p.net_sockets.get_mut(&id) {
            if e.refed != refed {
                e.refed = refed;
                if refed {
                    p.net_open += 1;
                } else {
                    p.net_open = p.net_open.saturating_sub(1);
                }
            }
            true
        } else {
            false
        }
    })
}

/// 收尾清除（entry + target；Close 派发后调用；返回首次 true）。
pub fn net_purge(id: u64) -> bool {
    let entry = with_plain(|p| p.net_sockets.remove(&id));
    with_rooted(|s| s.net_targets.retain(|t| t.id != id));
    if entry.is_some_and(|e| e.refed) {
        with_plain(|p| p.net_open = p.net_open.saturating_sub(1));
        true
    } else {
        false
    }
}

/// 存活 socket/server 数（事件循环退出条件用）。
pub fn net_open() -> usize {
    with_plain(|p| p.net_open)
}

/// BoundSocket 占位保活：存入 listener 回 token；取出消费；丢弃释放。
pub fn net_hold_add() -> u64 {
    with_plain(|p| {
        p.net_hold_next += 1;
        p.net_hold_next
    })
}
pub fn net_hold_put(token: u64, l: std::net::TcpListener) {
    with_plain(|p| {
        p.net_held.insert(token, l);
    });
}
pub fn net_hold_take(token: u64) -> bool {
    with_plain(|p| p.net_held.remove(&token).is_some())
}

/// 占位 listener 的 fd（dup 出独立 fd；未知 token 回 -1）。
/// win 无 as_fd：回 -1（套件 win 侧不断 fd>=0，只断类型）。
pub fn net_hold_fd(token: u64) -> i32 {
    with_plain(|p| {
        let Some(l) = p.net_held.get(&token) else {
            return -1;
        };
        #[cfg(unix)]
        {
            use std::os::fd::AsFd as _;
            match l.as_fd().try_clone_to_owned() {
                Ok(owned) => {
                    use std::os::fd::IntoRawFd as _;
                    owned.into_raw_fd()
                }
                Err(_) => -1,
            }
        }
        #[cfg(not(unix))]
        {
            let _ = l;
            -1
        }
    })
}
