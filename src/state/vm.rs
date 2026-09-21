//! state/vm：vm 上下文/模块表。

use super::*;
use mozjs::jsapi::JSObject;


// ── vm 上下文（同 Runtime 内多 global，各占新 compartment）────────────────
///
/// 登记新 global（`Box` 定址，`Heap::set` 后禁移动 §4.40），返回单调 id。

pub fn vm_add(global: *mut JSObject) -> u64 {
    let id = with_plain(|p| {
        p.vm_next_id += 1;
        p.vm_next_id
    });
    with_rooted(|s| {
        s.vm_contexts.push(VmCtx { id, global: Heap::boxed(global) });
    });
    id
}

/// 取上下文 global 裸指针（调用方必须立即重 root，中间无 GC 间隙，见 runtime 约定）。
pub fn vm_global(id: u64) -> Option<*mut JSObject> {
    with_rooted(|s| s.vm_contexts.iter().find(|c| c.id == id).map(|c| c.global.get()))
}

/// 摘除上下文（JS 侧 FinalizationRegistry/显式释放用；不在即 false）。
pub fn vm_release(id: u64) -> bool {
    let n0 = with_rooted(|s| {
        let n0 = s.vm_contexts.len();
        s.vm_contexts.retain(|c| c.id != id);
        n0
    });
    // 上下文摘除时顺带摘其名下未释放模块（record 随 global 走，无悬垂）。
    with_rooted(|s| s.vm_mods.retain(|m| m.ctx != id));
    with_rooted(|s| s.vm_contexts.len() != n0)
}

// ── vm 模块（9i-1；SourceTextModule 编译产物，归属其 ctx 的 compartment）───

/// 登记模块记录，返回单调 id（`Box` 定址 §4.40）。
pub fn vm_mod_add(
    ctx: u64,
    identifier: String,
    record: *mut JSObject,
    has_imports: bool,
    deps: Vec<String>,
) -> u64 {
    let id = with_plain(|p| {
        p.vm_next_id += 1;
        p.vm_next_id
    });
    with_rooted(|s| {
        s.vm_mods.push(VmMod {
            id,
            ctx,
            identifier,
            record: Heap::boxed(record),
            has_imports,
            deps,
            linked: false,
            evaluated: false,
        });
    });
    id
}

/// 取模块（record 裸指针 + 状态快照；调用方立即重 root，中间无 GC 间隙）。
pub fn vm_mod_get(id: u64) -> Option<(*mut JSObject, u64, bool, bool, bool)> {
    with_rooted(|s| {
        s.vm_mods.iter().find(|m| m.id == id).map(|m| {
            (m.record.get(), m.ctx, m.has_imports, m.linked, m.evaluated)
        })
    })
}

/// 取模块标识（报错信息用）。
pub fn vm_mod_identifier(id: u64) -> Option<String> {
    with_rooted(|s| s.vm_mods.iter().find(|m| m.id == id).map(|m| m.identifier.clone()))
}

/// 取静态依赖表 JSON（`dependencySpecifiers` 面）。
pub fn vm_mod_deps_json(id: u64) -> Option<String> {
    with_rooted(|s| {
        s.vm_mods.iter().find(|m| m.id == id).map(|m| {
            serde_json::Value::Array(m.deps.iter().map(|d| serde_json::Value::String(d.clone())).collect())
                .to_string()
        })
    })
}

/// 置 link 位（重复 link 由 JS 壳按 status 机拦截，此处幂等）。
pub fn vm_mod_set_linked(id: u64) {
    with_rooted(|s| {
        if let Some(m) = s.vm_mods.iter_mut().find(|m| m.id == id) {
            m.linked = true;
        }
    });
}

/// 置 evaluate 位（幂等）。
pub fn vm_mod_set_evaluated(id: u64) {
    with_rooted(|s| {
        if let Some(m) = s.vm_mods.iter_mut().find(|m| m.id == id) {
            m.evaluated = true;
        }
    });
}

/// 摘除模块（重复释放 false）。
pub fn vm_mod_release(id: u64) -> bool {
    let n0 = with_rooted(|s| {
        let n0 = s.vm_mods.len();
        s.vm_mods.retain(|m| m.id != id);
        n0
    });
    with_rooted(|s| s.vm_mods.len() != n0)
}
