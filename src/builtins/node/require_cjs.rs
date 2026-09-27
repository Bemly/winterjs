//! CJS 互操作的纯静态侧（自 `require.rs` 纯搬移，调用方经原位重导出；见 F2）：
//! 具名导出发现（cjs-module-lexer 核心子集，零求值）+ 最近 `package.json`
//! `type` 判定。F2-a 性能：正则进程级预编译、pkgtype 目录缓存（mtime 失效）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use url::Url;

use crate::loader::resolve::resolve_require;

// ── CJS 具名导出静态发现（cjs-module-lexer 核心子集；零求值）──────────────
//
// require(esm)（Node ≥22.12）落地后，M5 的运行时键快照（求值取键）在
// CJS↔ESM 环上会无限递归：发现期求值 CJS → require ESM → 编译其子图 →
// 环上 CJS 再进发现期。正解同 Node cjs-module-lexer：静态词法为主（零求值、
// 循环天然安全），运行时快照降级为回退并加在飞护栏。
//
// 静态面（近似口径，偏差记本节）：`exports.NAME =` / `module.exports.NAME =`
// （标识符键）、`exports["NAME"] =`、`Object.defineProperty(exports, "NAME", …)`、
// `module.exports = require("<相对>")` 与 `__exportStar(require("<相对>"), exports)`
// 转出跟随（深度 ≤ 8，路径集合去重；多分支 if/else 取并集）。
// `__esModule` 互操作标记剔除。

// ── CJS 具名发现正则（F2-a：进程级预编译；此前每次调用现场编译 8 个，
// vite build 顶层 48 次调用烧 ~1688ms，递归层层再编译）───────────
macro_rules! precompiled {
    ($name:ident, $pat:literal) => {
        fn $name() -> &'static regex::Regex {
            static CELL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
            CELL.get_or_init(|| regex::Regex::new($pat).expect("cjs static pattern"))
        }
    };
}

precompiled!(re_exports_eq, r#"(?:module\.)?exports\.([A-Za-z_$][\w$]*)\s*=[^=]"#);
precompiled!(
    re_exports_sub,
    r#"(?:module\.)?exports\[["']([^"']+)["']\]\s*=[^=]"#
);
precompiled!(
    re_define_prop,
    r#"Object\.defineProperty\(\s*(?:\(0,\s*)?(?:module\.)?exports\s*,\s*["']([^"']+)["']"#
);
precompiled!(
    re_require_bind,
    r#"(?:var|const|let)\s+([A-Za-z_$][\w$]*)\s*=\s*require\(\s*['"]([^'"]+)['"]\s*\)"#
);
precompiled!(
    re_module_exports_require,
    r#"module\.exports\s*=\s*require\(\s*['"]([^'"]+)['"]\s*\)"#
);
precompiled!(
    re_keys_require_foreach,
    r#"Object\.keys\(\s*require\(\s*['"]([^'"]+)['"]\s*\)\s*\)\.forEach"#
);
precompiled!(
    re_keys_ident_foreach,
    r#"Object\.keys\(\s*([A-Za-z_$][\w$]*)\s*\)\.forEach"#
);
precompiled!(
    re_export_star,
    r#"__exportStar\(\s*require\(\s*['"]([^'"]+)['"]\s*\)\s*,\s*(?:module\.)?exports\s*\)"#
);

pub(crate) fn cjs_static_names(
    path: &Path,
    depth: usize,
    seen: &mut HashSet<PathBuf>,
) -> Vec<String> {
    if depth > 8 || !seen.insert(path.to_path_buf()) {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    let mut push = |n: &str| {
        if n != "__esModule" && n != "default" && !names.iter().any(|x| x == n) {
            names.push(n.to_owned());
        }
    };
    // exports.NAME = … / module.exports.NAME = …（链式 `exports.a = exports.b =`
    // 全捕获；`=[^=]` 排除 ==/===；无行锚——`(0 && (exports.x = …))` 死代码形
    // 也是真导出名）。babel 的 `exports.version = exports.types = void 0` 落此。
    for caps in re_exports_eq().captures_iter(&text) {
        push(&caps[1]);
    }
    // exports["NAME"] = …
    for caps in re_exports_sub().captures_iter(&text) {
        push(&caps[1]);
    }
    // Object.defineProperty(exports|module.exports|(0, exports), "NAME", …)
    // babel 系 TS 编译产物的 `(0, exports)` 接收器同捕获。
    for caps in re_define_prop().captures_iter(&text) {
        push(&caps[1]);
    }
    // require 绑定表（var/const/let X = require("spec")）：Object.keys(X).forEach
    // 动态转出的跟随依据。裸说明符经本仓 resolver 解析（真机 vue.cjs.js 的
    // Object.keys(runtimeDom).forEach 即此形，实测 173 键全出）。
    let mut bindings: Vec<(String, String)> = Vec::new();
    for caps in re_require_bind().captures_iter(&text) {
        bindings.push((caps[1].to_owned(), caps[2].to_owned()));
    }
    // 整包转出跟随：module.exports = require("rel") / Object.keys(X|require(..)).forEach
    // / __exportStar(require("rel"), exports)。多分支取并集。
    let dir = path.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    let mut follow_spec = |spec: &str, depth: usize, seen: &mut HashSet<PathBuf>| {
        if spec.starts_with("./") || spec.starts_with("../") {
            let resolved = cjs_follow_spec(&dir, spec);
            for name in cjs_static_names(&resolved, depth + 1, seen) {
                push(&name);
            }
            return;
        }
        // 裸说明符：base = 本文件 URL，走本仓 resolver（package exports 感知）。
        let Ok(base) = Url::from_file_path(path) else {
            return;
        };
        if let Ok(u) = resolve_require(spec, Some(&base)) {
            if let Ok(p) = u.to_file_path() {
                for name in cjs_static_names(&p, depth + 1, seen) {
                    push(&name);
                }
            }
        }
    };
    for caps in re_module_exports_require().captures_iter(&text) {
        follow_spec(&caps[1], depth, seen);
    }
    for caps in re_keys_require_foreach().captures_iter(&text) {
        follow_spec(&caps[1], depth, seen);
    }
    for caps in re_keys_ident_foreach().captures_iter(&text) {
        if let Some((_, spec)) = bindings.iter().find(|(n, _)| n == &caps[1]) {
            follow_spec(spec, depth, seen);
        }
    }
    for caps in re_export_star().captures_iter(&text) {
        let resolved = cjs_follow_spec(&dir, &caps[1]);
        for name in cjs_static_names(&resolved, depth + 1, seen) {
            push(&name);
        }
    }
    names
}

/// 转出跟随的相对说明符解析（仅 `./`/`../` 形；扩展名缺失探测 .js/.cjs/index 双形）。
fn cjs_follow_spec(dir: &Path, spec: &str) -> PathBuf {
    let rel = spec.trim_start_matches("./");
    if let Some(p) = dir.join(rel).canonicalize().ok() {
        return p;
    }
    for suffix in [".js", ".cjs", "/index.js", "/index.cjs"] {
        let cand = dir.join(format!("{rel}{suffix}"));
        if cand.is_file() {
            return cand;
        }
    }
    dir.join(rel)
}

/// 最近 `package.json` 的 `type` 字段（`module`/`commonjs`/缺省）。
/// 纯 fs + 宽容 JSON：坏文件/无清单一律当缺省（`None`）；找到最近一份即停。
/// 纯函数（除 fs 外），单测覆盖判定表（用 tempfile 搭清单树）。
/// `pub(crate)`：入口 ESM 判定（`runtime::sniff_module`）复用同一口径。
// ── 最近 package.json `type` 缓存（F2-a）：`package.json` 路径 → 结论。
// 同进程多次 run（test runner/watch）复用；watch 下文件变更靠 mtime 失效。
// Miss（文件不存在）同样缓存——有效条件是"文件仍不存在"，每次一次 stat 校验。
// 原语义逐字保留：读到但 JSON 非法 → None（停）；读错（缺文件等）→ 继续向上。
#[derive(Clone)]
enum PkgTypeEntry {
    Hit { ty: Option<String>, mtime: SystemTime },
    Miss,
}

static PKGTYPE_CACHE: std::sync::LazyLock<parking_lot::Mutex<HashMap<PathBuf, PkgTypeEntry>>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(HashMap::new()));

pub(crate) fn nearest_pkg_type(path: &std::path::Path) -> Option<String> {
    enum Step {
        Return(Option<String>),
        Up,
        Rewalk,
    }
    let mut dir = path.parent();
    while let Some(d) = dir {
        let cand = d.join("package.json");
        let step: Step = {
            let map = PKGTYPE_CACHE.lock();
            match map.get(&cand) {
                Some(PkgTypeEntry::Hit { ty, mtime }) => {
                    let cur = std::fs::metadata(&cand).and_then(|m| m.modified()).ok();
                    if cur.as_ref() == Some(mtime) {
                        Step::Return(ty.clone())
                    } else {
                        Step::Rewalk
                    }
                }
                Some(PkgTypeEntry::Miss) => {
                    if cand.exists() {
                        Step::Rewalk
                    } else {
                        Step::Up
                    }
                }
                None => Step::Rewalk,
            }
        };
        match step {
            Step::Return(ty) => return ty,
            Step::Up => {
                dir = d.parent();
                continue;
            }
            Step::Rewalk => {}
        }
        match std::fs::read_to_string(&cand) {
            Ok(text) => {
                let ty = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| {
                        v.get("type").and_then(|t| t.as_str()).map(|s| s.to_string())
                    });
                if let Ok(mtime) = std::fs::metadata(&cand).and_then(|m| m.modified()) {
                    PKGTYPE_CACHE
                        .lock()
                        .insert(cand, PkgTypeEntry::Hit { ty: ty.clone(), mtime });
                }
                return ty;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                PKGTYPE_CACHE.lock().insert(cand, PkgTypeEntry::Miss);
                dir = d.parent();
            }
            Err(_) => {
                dir = d.parent();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::nearest_pkg_type;

    #[test]
    fn nearest_pkg_type_table() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub/deep");
        std::fs::create_dir_all(&sub).unwrap();
        let f = sub.join("a.js");
        // 无清单 → 缺省
        assert_eq!(nearest_pkg_type(&f), None);
        // 最近清单赢（子目录覆盖根）
        std::fs::write(dir.path().join("package.json"), r#"{"type":"commonjs"}"#).unwrap();
        std::fs::write(sub.join("package.json"), r#"{"type":"module"}"#).unwrap();
        assert_eq!(nearest_pkg_type(&f).as_deref(), Some("module"));
        assert_eq!(
            nearest_pkg_type(&dir.path().join("b.js")).as_deref(),
            Some("commonjs")
        );
        // 坏 JSON 当缺省且不再上找（最近清单即决）
        std::fs::write(sub.join("package.json"), r#"{"type": "#).unwrap();
        assert_eq!(nearest_pkg_type(&f), None);
    }
}
