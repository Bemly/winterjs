#!/usr/bin/env python3
# §4.202-② flake 先分类再动手：新红先自动跑 N 遍，flaky 与必现分流——
# flaky 走定级法（§4.197），必现才 instrument。禁把 flake 当回归深挖。
#
# 用法:
#   scripts/flake-classify.py [--runs 3] [--node-runs 2] [--timeout 25]
#       [--thread-id 3601] [--port-base 29999]
#       [--dir /tmp/wjs-node-test/test/parallel] [--wjs BIN] [--node BIN]
#       [--json] <suite.js|绝对路径> [repro.js ...]
#
# - <suite.js>：套件名（--dir 下解析）或绝对路径；对它整文件跑 --runs 遍
#   （"整文件×3"）。若与 node 基线不一致再判 flaky/必现。
# - [repro.js ...]：手工抽出的单块复现文件（任意路径，同待遇×N——
#   "单块×3"）。复现文件自备：切块=instrument，属必现件的下一步，
#   分类阶段不做（§4.202-② 原文口径）。
# - 分类（看 wjs 侧；node 侧作基线并在行尾注明）：
#     GREEN               wjs N/0 红 + node 基线绿
#     FLAKY(k/N)          wjs 侧不稳——走定级法，勿当回归深挖
#     RED-DETERMINISTIC   wjs 必红——必现，可 instrument
#     NODE-FLAKY          node 基线不稳——先提高 --node-runs 复核再下结论
# - 结论只进 stdout（--json 另出一行机器可读）；退出码 0=完成分类，
#   2=工具错误（二进制缺失/路径不存在）。红绿不是退出码。
# 纪律：exec or die + glob 绝对路径（§4.145）；单套件跑分用 subprocess
# timeout（alarm 级，§4.143 只禁全量套 alarm）；与常驻 sweep 并跑时
# --thread-id/--port-base 错开（§4.122 互踩防线，sweep 默认 3599/29999）。
import argparse
import glob as _glob
import json
import os
import shutil
import subprocess
import sys

TOOL = "flake-classify"


def die(msg):
    print(f"{TOOL}: {msg}", file=sys.stderr)
    sys.exit(2)


def resolve_wjs(explicit):
    if explicit:
        p = os.path.abspath(explicit)
        if not (os.path.isfile(p) and os.access(p, os.X_OK)):
            die(f"--wjs 不可执行: {p}")
        return p
    hits = [
        h for h in _glob.glob("/Volumes/*/Projects/winterjs/target/debug/winterjs")
        if os.access(h, os.X_OK)
    ]
    if not hits:
        die("glob /Volumes/*/Projects/winterjs 未命中可执行 winterjs——先 cargo build（§4.145）")
    if len(hits) > 1:
        die("glob 命中多份二进制，用 --wjs 显式指定:\n  " + "\n  ".join(hits))
    return hits[0]


def resolve_suite(arg, suite_dir):
    p = arg if os.path.isabs(arg) else os.path.join(suite_dir, arg)
    if not os.path.isfile(p):
        near = sorted(
            f for f in os.listdir(suite_dir) if arg.split("/")[-1] in f
        )[:5] if os.path.isdir(suite_dir) else []
        die(f"套件不存在: {p}" + (f"\n  近名: {' '.join(near)}" if near else ""))
    return p


def run_one(binary, path, cwd, env, timeout, prefix=("--run",)):
    # rc 142 = 看门超时（sweep 家族口径）；FileNotFoundError 前置 die 兜底。
    # prefix：wjs 用 ("--run",)，node 用 ()——node 22+ 的 `--run` 是
    # "跑 package.json scripts"（2026-09-25 实测坑：给 node 也带 --run
    # 会报 `Can't find package.json for directory` 假红全表）。
    try:
        return subprocess.run(
            [binary, *prefix, path], cwd=cwd, env=env,
            capture_output=True, timeout=timeout,
        ).returncode
    except subprocess.TimeoutExpired:
        return 142
    except OSError as e:
        die(f"exec 失败 {binary}: {e}（§4.145 exec or die）")


def run_n(binary, path, cwd, env, timeout, n, prefix=("--run",)):
    rcs = []
    for _ in range(n):
        rcs.append(run_one(binary, path, cwd, env, timeout, prefix))
    return rcs


def classify(wrcs, nrcs):
    wred = sum(1 for r in wrcs if r != 0)
    nred = sum(1 for r in nrcs if r != 0)
    if nred and nred < len(nrcs):
        return "NODE-FLAKY", f"node 基线不稳 {nred}/{len(nrcs)} 红——先提高 --node-runs 复核"
    if wred == 0:
        if nred:
            return "GREEN", f"wjs 全绿 + node 必红 {nred}/{len(nrcs)}——node 侧独有红（罕见，人工看）"
        return "GREEN", ""
    if wred < len(wrcs):
        return "FLAKY", f"wjs {wred}/{len(wrcs)} 红——flaky 走定级法（§4.197），勿当回归深挖"
    if nred == 0:
        return "RED-DETERMINISTIC", f"wjs 必红 {wred}/{len(wrcs)} + node 全绿——必现，可 instrument"
    return "RED-DETERMINISTIC", (
        f"wjs 必红 + node 必红 {nred}/{len(nrcs)}——先比 rc 是否相等"
        f"（相等=引擎同缺，不等=真 DIFF）"
    )


def orphan_warning():
    # 实测坑（2026-09-25）：上一轮 sweep 挂死留下的孤儿 winterjs 进程
    # （ps 见 test-http-server-keepalive-req-gc 等滞留数小时）占着端口/
    # 状态，把 node 基线弄红——分类前先预警，结论可疑时先 pkill 再复跑。
    try:
        out = subprocess.run(
            ["pgrep", "-f", "winterjs"], capture_output=True, timeout=5
        ).stdout.split()
    except (OSError, subprocess.TimeoutExpired):
        return
    if out:
        print(
            f"WARNING: 检测到 {len(out)} 个残留 winterjs 进程 (pgrep pid:"
            f" {','.join(p.decode() for p in out[:5])})——孤儿会污染基线，"
            "建议 pkill -f winterjs 后复跑",
            flush=True,
        )


def main():
    ap = argparse.ArgumentParser(prog=TOOL, description="§4.202-② flake 分类：整文件×N + 单块 repro×N")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--node-runs", type=int, default=2)
    ap.add_argument("--timeout", type=int, default=25)
    ap.add_argument("--thread-id", default="3601")
    ap.add_argument("--port-base", default="29999")
    ap.add_argument("--dir", default="/tmp/wjs-node-test/test/parallel")
    ap.add_argument("--wjs", default=None)
    ap.add_argument("--node", default="node")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("suites", nargs="+")
    args = ap.parse_args()

    if args.runs < 1 or args.node_runs < 1:
        die("--runs/--node-runs 至少 1")
    if not os.path.isdir(args.dir):
        die(
            f"套件目录不存在: {args.dir}——重建 node 套件 sparse 检出"
            "（/tmp 随时会被清，§4.144；缺 fixtures 会污染基线，§4.158）"
        )
    wjs = resolve_wjs(args.wjs)
    # 裸名走 PATH 查找；带分隔符才当路径解析（abspath 相对 CWD，误吞裸名）。
    if os.path.sep in args.node:
        node = os.path.abspath(args.node)
    else:
        node = shutil.which(args.node) or ""
    if not (node and os.path.isfile(node) and os.access(node, os.X_OK)):
        die(f"--node 不可执行: {args.node}")

    print(f"wjs={wjs}", flush=True)
    orphan_warning()
    print(f"node={node} dir={args.dir} runs={args.runs}x wjs / {args.node_runs}x node "
          f"timeout={args.timeout}s thread-id={args.thread_id}", flush=True)

    for arg in args.suites:
        path = resolve_suite(arg, args.dir)
        env = dict(
            os.environ,
            NODE_SKIP_FLAG_CHECK="1",
            TEST_THREAD_ID=args.thread_id,
            TEST_SERIAL_ID=args.thread_id,
            NODE_COMMON_PORT=args.port_base,
        )
        wrcs = run_n(wjs, path, args.dir, env, args.timeout, args.runs, ("--run",))
        nrcs = run_n(node, path, args.dir, env, args.timeout, args.node_runs, ())
        verdict, hint = classify(wrcs, nrcs)
        fmt = lambda rcs: ",".join(str(r) for r in rcs)
        print(f"[{verdict}] {os.path.basename(path)}  wjs={fmt(wrcs)}  node={fmt(nrcs)}"
              + (f"  # {hint}" if hint else ""), flush=True)
        if args.json:
            print(json.dumps({
                "file": path, "wjs": wrcs, "node": nrcs, "verdict": verdict,
            }), flush=True)


if __name__ == "__main__":
    main()
