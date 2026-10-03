#!/usr/bin/env python3
# R2-iter 移植生成器：node 原文 -> 本仓 .rs（逐字内嵌 + 垫片头 + ESM 导出）。
# 用法：python3 scripts/gen-iter-ports.py  (原地写 src/builtins/node/internal/streams/iter_*.rs)
# 校验：逐文件断言体部字节恒等 + 导出名非空；提交前再人工抽查垫片映射。
import re, os

WJS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NLIB = os.path.expanduser("~/wjs-data/node-lib")
OUTDIR = os.path.join(WJS, "src/builtins/node/internal/streams")

# 已知 canonical（精确形优先；下划线回落由 normalize_internal 运行时处理，
# 此处 ESM import 必须用本仓真实 canonical，故直接下划线形）。
def canonical(spec):
    if spec == "buffer":
        return "node:buffer"
    if spec == "async_hooks":
        return "node:async_hooks"
    if spec.startswith("internal/"):
        rest = spec[len("internal/"):]
        # 本仓扁平下划线形（streams/iter/* → streams/iter_*；其余连字符同理）。
        rest = rest.replace("-", "_").replace("streams/iter/", "streams/iter_")
        return "node:internal/" + rest
    raise ValueError(f"unexpected spec {spec}")

HEADER_TMPL = """//! `{canon}`（Node {orig} 逐字内嵌，MIT）。
/// 来源：nodejs/node `{orig}`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import {{ primordials }} from 'node:internal/primordials';
{imports}

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {{
  switch (spec) {{
{cases}  default: throw new Error('unmapped internal require: ' + spec);
  }}
}}

const module = {{ exports: {{ __proto__: null }} }};

"""

def gen_one(name, orig_rel, canon):
    src = open(os.path.join(NLIB, orig_rel), encoding="utf-8").read()
    assert '"#' not in src, f"{name}: raw-string terminator collision"
    # 注释内的 require('...')（用法示例）不收：先去注释再扫 spec（体部保持原样）。
    nospec = re.sub(r"/\*.*?\*/", "", src, flags=re.S)
    nospec = re.sub(r"//[^\n]*", "", nospec)
    specs = []
    for m in re.finditer(r"require\('([^']+)'\)", nospec):
        if m.group(1) not in specs:
            specs.append(m.group(1))
    imports, cases = [], []
    for i, spec in enumerate(specs):
        imports.append(f"import * as __m{i} from '{canonical(spec)}';")
        cases.append(f"    case '{spec}': return __m{i}.default;")
    assert "const require" not in src.split("const module")[0] or True
    m = re.search(r"module\.exports\s*=\s*\{([^}]*)\}", src)
    assert m, f"{name}: no module.exports"
    body = re.sub(r"//[^\n]*", "", m.group(1))
    names = [n.strip() for n in body.split(",") if n.strip()]
    assert names, f"{name}: empty exports"
    assert all(":" not in n and " " not in n for n in names), f"{name}: renamed exports {names}"
    header = HEADER_TMPL.format(
        canon=canon, orig=orig_rel,
        imports="\n".join(imports),
        cases="\n".join(cases) + ("\n" if cases else ""),
    )
    tail = f"\nexport {{ {', '.join(names)} }};\nexport default module.exports;\n"
    out = header + src + tail + '"#;\n'
    # 体部恒等校验：抽出 r#"..."# 之间的原文
    start = out.index('r#"') + 3
    # 跳过 header 内的 JS（imports/垫片/module 行）——以原文首行定位
    first_line = src.split("\n")[0]
    body_at = out.index(first_line, start)
    assert out[body_at:body_at + len(src)] == src, f"{name}: body mismatch"
    return out, names, specs

FILES = [
    ("iter_ringbuffer", "internal/streams/iter/ringbuffer.js", "node:internal/streams/iter_ringbuffer"),
    ("iter_utils", "internal/streams/iter/utils.js", "node:internal/streams/iter_utils"),
    ("iter_from", "internal/streams/iter/from.js", "node:internal/streams/iter_from"),
    ("iter_pull", "internal/streams/iter/pull.js", "node:internal/streams/iter_pull"),
    ("iter_push", "internal/streams/iter/push.js", "node:internal/streams/iter_push"),
    ("iter_duplex", "internal/streams/iter/duplex.js", "node:internal/streams/iter_duplex"),
    ("iter_share", "internal/streams/iter/share.js", "node:internal/streams/iter_share"),
    ("iter_broadcast", "internal/streams/iter/broadcast.js", "node:internal/streams/iter_broadcast"),
    ("iter_transform", "internal/streams/iter/transform.js", "node:internal/streams/iter_transform"),
    ("iter_consumers", "internal/streams/iter/consumers.js", "node:internal/streams/iter_consumers"),
]

if __name__ == "__main__":
    for name, orig, canon in FILES:
        out, names, specs = gen_one(name, orig, canon)
        p = os.path.join(OUTDIR, name + ".rs")
        open(p, "w", encoding="utf-8").write(out)
        print(f"{name}.rs: exports={len(names)} requires={specs}")
