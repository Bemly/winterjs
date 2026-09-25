//! 异常栈 sourcemap 回映射（D4 node 形渲染用；2026-09-25）。
//! SM 栈帧 `fn@file:L:C` 中 file 命中 `module_debug`（TS 等转译产物）即把 L:C
//! 映射回原文，与报错头行的 `remap_location` 同口径。

use super::with_plain;
use crate::loader::sourcemap::remap_location;

/// `module_debug` 键：CJS 脚本以绝对路径编译（node 栈帧口径），表按 URL 登记——路径转 URL。
pub fn debug_key(file: &str) -> String {
    if file.starts_with('/') {
        if let Ok(u) = url::Url::from_file_path(file) {
            return u.as_str().to_owned();
        }
    }
    file.to_owned()
}

/// 逐帧回映射；无 map 的帧原样保留。
pub fn remap_stack(stack: &str) -> String {
    stack
        .lines()
        .map(|l| {
            let (name, loc) = match l.rfind('@') {
                Some(i) => (&l[..=i], &l[i + 1..]),
                None => ("", l),
            };
            let mut it = loc.rsplitn(3, ':');
            let (Some(c), Some(ln), Some(file)) = (it.next(), it.next(), it.next()) else {
                return l.to_owned();
            };
            let (Ok(line), Ok(col)) = (ln.parse::<u32>(), c.parse::<u32>()) else {
                return l.to_owned();
            };
            let map = with_plain(|p| p.module_debug.get(&debug_key(file)).and_then(|d| d.map.clone()));
            if map.is_none() {
                return l.to_owned();
            }
            let (line, col) = remap_location(map.as_deref(), line, col);
            format!("{name}{file}:{line}:{col}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
