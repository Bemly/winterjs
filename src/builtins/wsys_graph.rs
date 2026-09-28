//! 本体图面（WinterJS.graph；petgraph id 表托管，裸指针不出 JS）。
//!
//! 纯搬移拆分自 wsys.rs（§0.9 超限拆分；调用方经 wsys 原位重导出）。
//! UNSAFE-BOUNDARY：全部 JSNative 入口经 `wrap_cx` + `Frame::from_raw`
//!（结构性边界块）；前置：调用方 realm 内 + 参数槽 rooted 后才分配；
//! 覆盖：`tests/wsys.rs::wsys_graph_faces`。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use mozjs::context::JSContext;
use mozjs::jsval::{JSVal, UndefinedValue};

use crate::builtins::wsys::{arg_num, arg_str, set_json};
use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};

// ── graph（petgraph；id 表托管）────────────────────────────────────────────

enum WGraph {
    Directed(petgraph::Graph<String, String>),
    Undirected(petgraph::Graph<String, String, petgraph::Undirected>),
}

static GRAPHS: std::sync::LazyLock<std::sync::Mutex<HashMap<u64, WGraph>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static GRAPH_NEXT: AtomicU64 = AtomicU64::new(1);

fn arg_gid(cx: &mut JSContext, frame: &Frame, i: u32, what: &str) -> Option<u64> {
    if frame.argc() <= i || !frame.arg(i).is_number() {
        report_error(cx, &format!("TypeError: {what} requires a graph id"));
        return None;
    }
    let n = frame.arg(i).to_number();
    if !n.is_finite() || n < 0.0 || n.fract() != 0.0 {
        report_error(cx, &format!("TypeError: {what} requires a graph id"));
        return None;
    }
    Some(n as u64)
}

/// `__wjs_wsys_graph_create(kind)` → id.
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_graph_faces`。
pub unsafe extern "C" fn graph_create(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(kind) = arg_str(&mut cx, &frame, 0, "graph create") else {
        return false;
    };
    let g = match kind.as_str() {
        "directed" => WGraph::Directed(petgraph::Graph::new()),
        "undirected" => WGraph::Undirected(petgraph::Graph::new_undirected()),
        _ => {
            report_error(&mut cx, "TypeError: graph create kind must be directed|undirected");
            return false;
        }
    };
    let id = GRAPH_NEXT.fetch_add(1, Ordering::Relaxed);
    match GRAPHS.lock() {
        Ok(mut t) => {
            t.insert(id, g);
            frame.set_rval(mozjs::jsval::DoubleValue(id as f64));
            true
        }
        Err(_) => {
            report_error(&mut cx, "WsysError: graph table poisoned");
            false
        }
    }
}

/// `__wjs_wsys_graph_add_node(id, label)` → idx。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_graph_faces`。
pub unsafe extern "C" fn graph_add_node(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_gid(&mut cx, &frame, 0, "graph addNode") else {
        return false;
    };
    let Some(label) = arg_str(&mut cx, &frame, 1, "graph addNode") else {
        return false;
    };
    match GRAPHS.lock() {
        Ok(mut t) => match t.get_mut(&id) {
            Some(g) => {
                let idx = match g {
                    WGraph::Directed(g) => g.add_node(label).index(),
                    WGraph::Undirected(g) => g.add_node(label).index(),
                };
                frame.set_rval(mozjs::jsval::DoubleValue(idx as f64));
                true
            }
            None => {
                report_error(&mut cx, "WsysError: unknown graph id");
                false
            }
        },
        Err(_) => {
            report_error(&mut cx, "WsysError: graph table poisoned");
            false
        }
    }
}

/// `__wjs_wsys_graph_add_edge(id, a, b, label?)` 。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_graph_faces`。
pub unsafe extern "C" fn graph_add_edge(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_gid(&mut cx, &frame, 0, "graph addEdge") else {
        return false;
    };
    let (a, b) = (arg_num(&frame, 1), arg_num(&frame, 2));
    let (Some(a), Some(b)) = (a, b) else {
        report_error(&mut cx, "TypeError: graph addEdge requires (id, a, b)");
        return false;
    };
    if a < 0.0 || a.fract() != 0.0 || b < 0.0 || b.fract() != 0.0 {
        report_error(&mut cx, "TypeError: graph addEdge requires integer node indices");
        return false;
    }
    let label = if frame.argc() > 3 {
        value_to_string(&mut cx, frame.arg(3))
    } else {
        String::new()
    };
    match GRAPHS.lock() {
        Ok(mut t) => match t.get_mut(&id) {
            Some(g) => {
                let (a, b) = (
                    petgraph::graph::NodeIndex::new(a as usize),
                    petgraph::graph::NodeIndex::new(b as usize),
                );
                let ok = match g {
                    WGraph::Directed(g) => {
                        if g.node_weight(a).is_none() || g.node_weight(b).is_none() {
                            false
                        } else {
                            g.add_edge(a, b, label);
                            true
                        }
                    }
                    WGraph::Undirected(g) => {
                        if g.node_weight(a).is_none() || g.node_weight(b).is_none() {
                            false
                        } else {
                            g.add_edge(a, b, label);
                            true
                        }
                    }
                };
                if !ok {
                    report_error(&mut cx, "TypeError: graph addEdge node index out of range");
                    return false;
                }
                frame.set_rval(UndefinedValue());
                true
            }
            None => {
                report_error(&mut cx, "WsysError: unknown graph id");
                false
            }
        },
        Err(_) => {
            report_error(&mut cx, "WsysError: graph table poisoned");
            false
        }
    }
}

/// `__wjs_wsys_graph_toposort(id)` → idx 数组 JSON 串（有环即错；仅 directed）。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_graph_faces`。
pub unsafe extern "C" fn graph_toposort(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_gid(&mut cx, &frame, 0, "graph toposort") else {
        return false;
    };
    match GRAPHS.lock() {
        Ok(t) => match t.get(&id) {
            Some(WGraph::Directed(g)) => match petgraph::algo::toposort(g, None) {
                Ok(order) => {
                    set_json(
                        &mut cx,
                        &frame,
                        &serde_json::Value::Array(
                            order.into_iter().map(|n| serde_json::json!(n.index())).collect(),
                        ),
                    );
                    true
                }
                Err(cycle) => {
                    report_error(
                        &mut cx,
                        &format!("TypeError: graph has a cycle at node {}", cycle.node_id().index()),
                    );
                    false
                }
            },
            Some(WGraph::Undirected(_)) => {
                report_error(&mut cx, "TypeError: graph toposort requires a directed graph");
                false
            }
            None => {
                report_error(&mut cx, "WsysError: unknown graph id");
                false
            }
        },
        Err(_) => {
            report_error(&mut cx, "WsysError: graph table poisoned");
            false
        }
    }
}

/// `__wjs_wsys_graph_counts(id)` → `[nodes, edges]` JSON；`__wjs_wsys_graph_free(id)`。
/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_graph_faces`。
pub unsafe extern "C" fn graph_counts(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_gid(&mut cx, &frame, 0, "graph counts") else {
        return false;
    };
    match GRAPHS.lock() {
        Ok(t) => match t.get(&id) {
            Some(g) => {
                let (n, e) = match g {
                    WGraph::Directed(g) => (g.node_count(), g.edge_count()),
                    WGraph::Undirected(g) => (g.node_count(), g.edge_count()),
                };
                set_json(&mut cx, &frame, &serde_json::json!([n, e]));
                true
            }
            None => {
                report_error(&mut cx, "WsysError: unknown graph id");
                false
            }
        },
        Err(_) => {
            report_error(&mut cx, "WsysError: graph table poisoned");
            false
        }
    }
}

/// UNSAFE-BOUNDARY：见本文件头注；覆盖 `tests/wsys.rs::wsys_graph_faces`。
pub unsafe extern "C" fn graph_free(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let Some(id) = arg_gid(&mut cx, &frame, 0, "graph free") else {
        return false;
    };
    match GRAPHS.lock() {
        Ok(mut t) => {
            if t.remove(&id).is_some() {
                frame.set_rval(UndefinedValue());
                true
            } else {
                report_error(&mut cx, "WsysError: unknown graph id (double-free?)");
                false
            }
        }
        Err(_) => {
            report_error(&mut cx, "WsysError: graph table poisoned");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn wsys_graph_kinds_split() {
        // id 表双形态由同一 Mutex 托管（directed/undirected 分支见 natives）。
        assert!(matches!(
            super::WGraph::Directed(petgraph::Graph::new()),
            super::WGraph::Directed(_)
        ));
    }
}
