//! vendored acorn（node 26.8.2 deps/acorn 同款，MIT；§0.5 拍板 2026-09-28）。
//! 包装 IIFE 提供局部 module/exports 强制 UMD 走 CJS 分支，尾挂 globalThis
//! （acorn/acornWalk），vendored 内容字节不动（split-js.py 切片断言恒等）。

/// acorn 本体（node `internal/deps/acorn/acorn/dist/acorn.js`）。
pub const ACORN_JS: &str = concat!(
    "var __wjs2AcornMod = { exports: {} };\n(function (module, exports) {\n",
    include_str!("acorn.js"),
    include_str!("acorn_if.js"),
    include_str!("acorn_if2.js"),
    include_str!("acorn_if3.js"),
    include_str!("acorn_elts.js"),
    include_str!("acorn_if5.js"),
    include_str!("acorn_curcontext.js"),
    "\nglobalThis.acorn = module.exports;\n})(__wjs2AcornMod, __wjs2AcornMod.exports);\n",
);

/// acorn-walk（node `internal/deps/acorn/acorn-walk/dist/walk.js`）。
pub const ACORN_WALK_JS: &str = concat!(
    "var __wjsAcornWalkMod = { exports: {} };\n(function (module, exports) {\n",
    include_str!("acorn_walk.js"),
    "\nglobalThis.acornWalk = module.exports;\n})(__wjsAcornWalkMod, __wjsAcornWalkMod.exports);\n",
);
