#!/usr/bin/env python3
# §0.9 内嵌 JS 拆分器：按 ≤950 行在顶层声明 / 类方法边界切片，新片插进 rs 的
# `concat!(include_str!(…))` 原位之后；断言新片拼接与原文件字节恒等。
# 用法：scripts/split-js.py <path.js> <含 include_str! 的 rs 文件>
import os, re, sys
path, rs = sys.argv[1], sys.argv[2]
src = open(path, encoding="utf-8").read()
keep_nl = src.endswith("\n")
body = src.split("\n")[:-1] if keep_nl else src.split("\n")
n, MAX = len(body), 950
top = re.compile(r"^(export |function |class |const |let |var |async function |// )")
meth = re.compile(r"^  (//|static |async |get |set |#?[A-Za-z_$][\w$]*\s*\()")

def score(i):
    if i <= 0 or i >= n:
        return -1
    blank = body[i - 1].strip() == ""
    if top.match(body[i]) and blank:
        return 3
    if top.match(body[i]):
        return 2
    if meth.match(body[i]) and blank:
        return 1
    m0 = re.match(r"^( {2}| {4})\}\s*$", body[i - 1])
    if m0 and re.match("^" + m0.group(1) + r"(//|static |async |get |set |#?[A-Za-z_$][\w$]*\s*\()", body[i]):
        return 0
    return -1

cuts, start = [], 0
while n - start > MAX:
    lo, hi = start + MAX - 250, start + MAX
    best = None
    for want in (3, 2, 1, 0):
        cand = [i for i in range(hi, lo, -1) if score(i) == want]
        if cand:
            best = cand[0]
            break
    if best is None:
        sys.exit(f"no cut in {lo}-{hi}")
    while best - 1 > start and body[best - 1].lstrip().startswith("//"):
        best -= 1
    cuts.append(best)
    start = best

def ident(i):
    for line in body[i:i + 12]:
        if line.lstrip().startswith("//"):
            continue
        m = re.search(r"(?:class|function)\s+([A-Za-z_$][\w$]*)|^\s*(?:static |async |get |set )?#?([A-Za-z_$][\w$]*)\s*\(|(?:const|let|var)\s+([A-Za-z_$][\w$]*)", line)
        if m:
            return re.sub(r"[^a-z0-9]+", "", next(g for g in m.groups() if g).lower()) or "part"
    return "part"

bounds = [0] + cuts + [n]
base, ext = os.path.splitext(path)
parts = []
for k in range(len(bounds) - 1):
    a, b = bounds[k], bounds[k + 1]
    chunk = "\n".join(body[a:b]) + ("\n" if (b < n or keep_nl) else "")
    fn = path if k == 0 else f"{base}_{ident(a)}{ext}"
    if any(fn == p for p, _ in parts) or (k and os.path.exists(fn)):
        fn = f"{base}_{ident(a)}{k}{ext}"
    parts.append((fn, chunk))
assert "".join(c for _, c in parts) == src, "concat mismatch"
for fn, c in parts:
    open(fn, "w", encoding="utf-8").write(c)
r = open(rs, encoding="utf-8").read()
old = f'include_str!("{os.path.basename(path)}"),'
i = r.index(old)
indent = r[r.rfind("\n", 0, i) + 1:i]
add = "".join(f'\n{indent}include_str!("{os.path.relpath(fn, os.path.dirname(rs))}"),' for fn, _ in parts[1:])
open(rs, "w", encoding="utf-8").write(r[:i + len(old)] + add + r[i + len(old):])
for fn, c in parts:
    print(fn, c.count("\n"))
