#!/usr/bin/env python3
# 按域汇总 sweep 结果（plan3 §0.3 回填用）：域 = `test-<域>-…` 首段。
# 用法：scripts/sweep-report.py <tag> [--md]
import os, re, sys, collections
tag = sys.argv[1]
root = os.environ.get("WJS_SWEEP_ROOT") or os.path.expanduser("~/wjs-data/sweep")
here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
scope_p = os.path.join(here, "docs", "bun-scope.txt")
node_dir = os.path.expanduser("~/wjs-data/node-test/test/parallel")
dom = lambda f: f[5:].split("-")[0].split(".")[0]
scope = {l.strip() for l in open(scope_p) if l.strip() and not l.startswith("#")}
files = [f for f in os.listdir(node_dir) if f.startswith("test-") and f.endswith((".js", ".mjs")) and f in scope]
total = collections.Counter(dom(f) for f in files)
red = collections.defaultdict(collections.Counter)
for line in open(os.path.join(root, tag, "results.log")):
    kind = line.split()[0].split("(")[0]
    m = re.search(r"\b(test-[\w.-]+\.m?js)\b", line)
    if m:
        red[dom(m.group(1))][kind] += 1
rows = []
for d, n in total.items():
    r = red[d]
    # 欠账口径：DIFF + TIMEOUT（SAME1 双红不计）。
    bad = r["DIFF"] + r["TIMEOUT"]
    rows.append((bad, d, n, n - bad - r["SAME1"], r["DIFF"], r["TIMEOUT"], r["SAME1"]))
rows.sort(reverse=True)
md = "--md" in sys.argv
if md:
    print("| 域 | 清单件 | 绿 | DIFF | TIMEOUT | 双红 | 绿率 |\n|---|---|---|---|---|---|---|")
for bad, d, n, g, df, to, s1 in rows:
    pct = f"{g * 100 // max(1, n - s1)}%"
    print(f"| {d} | {n} | {g} | {df} | {to} | {s1} | {pct} |" if md else f"{d:16} n={n:4} green={g:4} diff={df:3} timeout={to:3} same1={s1:3} {pct}")
