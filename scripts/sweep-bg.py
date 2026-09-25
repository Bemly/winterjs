#!/usr/bin/env python3
# §4.202-③ sweep 常驻后台：全量 serial 对拍后台直跑、结果落盘、轮询进度，
# 动手与等数解耦（§4.143：全量禁套 alarm——只有单套件 subprocess timeout）。
#
# 用法:
#   scripts/sweep-bg.py start [--prefix test-http-] [--dir DIR] [--timeout 25]
#       [--thread-id 3599] [--port-base 29999] [--tag TAG] [--wjs BIN] [--node BIN]
#       [--scope docs/bun-scope.txt|none] [--rerun-red TAG] [--no-node-cache]
#
# 提速三件（2026-09-25，plan3 §0.8）：
#   --scope      只跑 Bun 清单内的件（plan3 §0.2 范围；缺省 docs/bun-scope.txt）
#   node 缓存    node 侧结果按 (node 版本, 文件, mtime, size) 缓存——node 结果不随
#                本仓变化，二跑起 node 侧零开销（--no-node-cache 关闭）
#   --rerun-red  只重跑某 tag 的红件（DIFF/TIMEOUT/SAME1），修完即验
# 套件头 `// Flags: …` 两侧都透传（node 真跑旗；本仓 CLI 按规则剥除记录，§D1）。
#   scripts/sweep-bg.py status [--tag TAG] [--json]
#   scripts/sweep-bg.py tail   [--tag TAG] [-n 20]
#   scripts/sweep-bg.py wait   [--tag TAG] [--interval 30]
#   scripts/sweep-bg.py stop   [--tag TAG]
#
# 工件落 ~/.wjs-sweep/<tag>/{status.json,results.log,worker.log}——家目录，
# 不随 /tmp 清理（§4.144）。results.log 行格式与 sweep4 家族一致
# （TIMEOUT/SAME1/DIFF 前缀），存量 grep 习惯不变；SAME0 不落 results
# （计数在 status.json）。同一 tag 同时只允许一个 sweep；不同 prefix
# 并跑请用不同 tag + 错开 --thread-id/--port-base（§4.122）。
import argparse
import glob as _glob
import json
import os
import shutil
import signal
import subprocess
import sys
import time

TOOL = "sweep-bg"
# 工件根：$WJS_SWEEP_ROOT > ~/wjs-data/sweep（外置盘软链，plan3 §0.5）> ~/.wjs-sweep。
_DATA = os.path.join(os.path.expanduser("~"), "wjs-data")
HOME_ROOT = os.environ.get("WJS_SWEEP_ROOT") or (
    os.path.join(_DATA, "sweep") if os.path.isdir(_DATA)
    else os.path.join(os.path.expanduser("~"), ".wjs-sweep")
)
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_SCOPE = os.path.join(REPO, "docs", "bun-scope.txt")
DEFAULT_DIR = (
    os.path.join(_DATA, "node-test", "test", "parallel")
    if os.path.isdir(os.path.join(_DATA, "node-test", "test", "parallel"))
    else "/tmp/wjs-node-test/test/parallel"
)


def suite_flags(path):
    # node 套件头 `// Flags: --a --b`（前 40 行内；多行 Flags 合并）。
    flags = []
    try:
        with open(path, errors="replace") as f:
            for _, line in zip(range(40), f):
                if line.startswith("// Flags:"):
                    flags += line[len("// Flags:"):].split()
    except OSError:
        pass
    return flags


def load_scope(spec):
    if not spec or spec == "none":
        return None
    with open(spec) as f:
        return {l.strip() for l in f if l.strip() and not l.startswith("#")}


def node_version(node):
    try:
        return subprocess.run([node, "--version"], capture_output=True, timeout=10).stdout.decode().strip()
    except (OSError, subprocess.TimeoutExpired):
        return "?"


def cache_path():
    return os.path.join(HOME_ROOT, "node-cache.json")


def load_cache():
    try:
        with open(cache_path()) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def save_cache(c):
    os.makedirs(HOME_ROOT, exist_ok=True)
    tmp = cache_path() + ".tmp"
    with open(tmp, "w") as f:
        json.dump(c, f)
    os.replace(tmp, cache_path())


def die(msg):
    print(f"{TOOL}: {msg}", file=sys.stderr)
    sys.exit(2)


def tag_dir(tag):
    return os.path.join(HOME_ROOT, tag)


def status_path(tag):
    return os.path.join(tag_dir(tag), "status.json")


def load_status(tag):
    p = status_path(tag)
    if not os.path.isfile(p):
        die(f"未找到 sweep 工件: {p}（先 start）")
    with open(p) as f:
        return json.load(f)


def write_status(tag, data):
    # 原子写：tmp + rename，防 status 轮询读到半截 JSON。
    d = tag_dir(tag)
    os.makedirs(d, exist_ok=True)
    tmp = status_path(tag) + ".tmp"
    with open(tmp, "w") as f:
        json.dump(data, f)
    os.replace(tmp, status_path(tag))


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


def orphan_warning():
    # 实测坑（2026-09-25）：上一轮 sweep 挂死留下的孤儿 winterjs 进程会
    # 占端口/状态，污染本轮与后续 node 基线——start 前预警。
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
            "建议 pkill -f winterjs 后再 start",
            flush=True,
        )


def run_one(binary, path, cwd, env, timeout, prefix=("--run",), dump_dir=None, side=None):
    # 返回 (rc, 首行 stderr)——失败行的 stderr 尾巴直接进 results.log，
    # 省一轮手工复跑（exec 失败必须显式落 error 态，绝不能静默当
    # rc=0 假绿——§4.145 的 perl exec 假绿同源）。DIFF 时 dump_dir 非空
    # 则整份 stderr 落 debug/ 供排障。
    # prefix：wjs 用 ("--run",)，node 用 ()——node 22+ 的 `--run` 是
    # "跑 package.json scripts"（2026-09-25 实测坑：无 package.json 目录
    # 报 `Can't find package.json for directory` 假红全表）。
    try:
        p = subprocess.run(
            [binary, *prefix, path], cwd=cwd, env=env,
            capture_output=True, timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return 142, ""
    except OSError as e:
        raise RuntimeError(f"exec 失败 {binary}: {e}")
    err = (p.stderr or b"").decode("utf-8", "replace").strip().splitlines()
    tail = err[0][:160].replace("\t", " ") if err else ""
    if dump_dir and side and p.returncode not in (0, 142) and (p.stderr or p.stdout):
        os.makedirs(dump_dir, exist_ok=True)
        base = os.path.splitext(os.path.basename(path))[0]
        with open(
            os.path.join(dump_dir, f"{base}.{side}.log"), "w"
        ) as df:
            df.write(f"rc={p.returncode}\n--- stderr ---\n")
            df.write((p.stderr or b"").decode("utf-8", "replace"))
            df.write("\n--- stdout ---\n")
            df.write((p.stdout or b"").decode("utf-8", "replace"))
    return p.returncode, tail


def sweep_loop(st):
    d = st["dir"]
    env = dict(
        os.environ,
        NODE_SKIP_FLAG_CHECK="1",
        TEST_THREAD_ID=st["thread_id"],
        TEST_SERIAL_ID=st["thread_id"],
        NODE_COMMON_PORT=st["port_base"],
    )
    # 与 sweep4 家族 409 件基线可比：.js 与 .mjs 都进（§4.145：口径先对齐
    # 再比较；只滤 .js 会静默丢 6 件 mjs——2026-09-25 实测）。
    files = sorted(
        f for f in os.listdir(d)
        if f.startswith(st["prefix"])
        and (f.endswith(".js") or f.endswith(".mjs"))
    )
    scope = load_scope(st.get("scope"))
    if scope is not None:
        files = [f for f in files if f in scope]
    if st.get("only"):
        only = set(st["only"])
        files = [f for f in files if f in only]
    cache = load_cache() if st.get("node_cache", True) else None
    nver = node_version(st["node"])
    st["pid"] = os.getpid()  # 双 fork 后只有孙进程知道真实 pid，回填给 status/stop
    st["state"] = "running"
    st["total"] = len(files)
    write_status(st["tag"], st)
    results = open(os.path.join(tag_dir(st["tag"]), "results.log"), "w")
    dump_dir = os.path.join(tag_dir(st["tag"]), "debug")
    t0 = time.time()
    for i, f in enumerate(files):
        flags = tuple(suite_flags(os.path.join(d, f)))
        wrc, werr = run_one(st["wjs"], f, d, env, st["timeout_s"], flags + ("--run",), dump_dir, "wjs")
        fp = os.path.join(d, f)
        try:
            sb = os.stat(fp)
            ckey = f"{nver}|{f}|{int(sb.st_mtime)}|{sb.st_size}"
        except OSError:
            ckey = None
        if cache is not None and ckey in cache:
            nrc, nerr = cache[ckey]
        else:
            nrc, nerr = run_one(st["node"], f, d, env, st["timeout_s"], flags, dump_dir, "node")
            # TIMEOUT 不缓存（可能是机器负载，下次再测）。
            if cache is not None and ckey and nrc != 142:
                cache[ckey] = [nrc, nerr]
                if (i + 1) % 25 == 0:
                    save_cache(cache)
        line = None
        if 142 in (wrc, nrc):
            st["timeout"] += 1
            line = f"TIMEOUT wjs={wrc} node={nrc} {f}"
        elif wrc == nrc:
            if wrc == 0:
                st["same0"] += 1
            else:
                st["same1"] += 1
                line = f"SAME1({wrc}) {f}"
        else:
            st["diff"] += 1
            line = f"DIFF wjs={wrc} node={nrc} {f}"
        if line:
            if nerr:
                line += f" | node_err: {nerr}"
            if werr:
                line += f" | wjs_err: {werr}"
            results.write(line + "\n")
            results.flush()
        st["done"] = i + 1
        st["updated"] = time.time()
        st["elapsed_s"] = round(time.time() - t0, 1)
        write_status(st["tag"], st)
    results.close()
    if cache is not None:
        save_cache(cache)
    st["state"] = "done"
    st["finished"] = time.time()
    write_status(st["tag"], st)
    print(
        f"SWEEP DONE tag={st['tag']} total={st['total']} "
        f"SAME0={st['same0']} SAME1={st['same1']} "
        f"DIFF={st['diff']} TIMEOUT={st['timeout']}",
        flush=True,
    )


def cmd_start(ap_args):
    if not os.path.isdir(ap_args.dir):
        die(
            f"套件目录不存在: {ap_args.dir}——重建 node 套件 sparse 检出"
            "（/tmp 随时会被清，§4.144）"
        )
    wjs = resolve_wjs(ap_args.wjs)
    orphan_warning()
    # 裸名走 PATH 查找；带分隔符才当路径解析（abspath 相对 CWD，误吞裸名）。
    if os.path.sep in ap_args.node:
        node = os.path.abspath(ap_args.node)
    else:
        node = shutil.which(ap_args.node) or ""
    if not (node and os.path.isfile(node) and os.access(node, os.X_OK)):
        die(f"--node 不可执行: {ap_args.node}")
    d = tag_dir(ap_args.tag)
    os.makedirs(d, exist_ok=True)
    if os.path.exists(status_path(ap_args.tag)):
        old = load_status(ap_args.tag)
        if old.get("state") == "running" and pid_alive(old.get("pid", -1)):
            die(f"tag={ap_args.tag} 已有 sweep 在跑 pid={old['pid']}——先 stop 或换 tag")
    st = {
        "tag": ap_args.tag,
        "pid": None,
        "state": "starting",
        "prefix": ap_args.prefix,
        "dir": os.path.abspath(ap_args.dir),
        "timeout_s": ap_args.timeout,
        "thread_id": ap_args.thread_id,
        "port_base": ap_args.port_base,
        "wjs": wjs,
        "node": node,
        "scope": ap_args.scope,
        "node_cache": not ap_args.no_node_cache,
        "only": rerun_red_list(ap_args.rerun_red) if ap_args.rerun_red else None,
        "total": 0, "done": 0,
        "same0": 0, "same1": 0, "diff": 0, "timeout": 0,
        "started": time.time(), "updated": time.time(),
        "finished": None, "elapsed_s": 0,
    }
    # 双 fork 脱离本会话：fork → setsid → 再 fork，孙进程跑 sweep（pid 由
    # 孙进程在 sweep_loop 回填），中间进程即刻退出由 waitpid 收尸防僵尸；
    # stdout/stderr 全量落 worker.log。
    write_status(st["tag"], st)
    pid = os.fork()
    if pid == 0:
        try:
            os.setsid()
        except OSError:
            pass
        try:
            if os.fork() != 0:
                os._exit(0)
        except OSError:
            os._exit(1)
        devnull = os.open(os.devnull, os.O_RDWR)
        os.dup2(devnull, 0)
        logfd = os.open(
            os.path.join(d, "worker.log"), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644
        )
        os.dup2(logfd, 1)
        os.dup2(logfd, 2)
        code = 0
        try:
            sweep_loop(st)
        except BaseException as e:
            st["state"] = "error"
            st["error"] = str(e)
            st["updated"] = time.time()
            write_status(st["tag"], st)
            print(f"SWEEP ERROR: {e}", flush=True)
            code = 1
        finally:
            os._exit(code)
    os.waitpid(pid, 0)
    print(f"started tag={ap_args.tag} dir={st['dir']} prefix={st['prefix']}", flush=True)
    print(f"  工件: {d}/  轮询: scripts/sweep-bg.py status --tag {ap_args.tag}", flush=True)


def rerun_red_list(tag):
    p = os.path.join(tag_dir(tag), "results.log")
    if not os.path.isfile(p):
        die(f"--rerun-red: 未找到 {p}")
    out = []
    with open(p) as f:
        for line in f:
            for tok in line.split():
                if tok.startswith("test-") and (tok.endswith(".js") or tok.endswith(".mjs")):
                    out.append(tok)
                    break
    return out


def pid_alive(pid):
    if pid is None or pid < 1:
        return False
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def fmt_status(st):
    alive = pid_alive(st.get("pid"))
    state = st["state"]
    if state == "running" and not alive and st.get("pid"):
        state = f"dead(pid={st['pid']} 不在——查 worker.log)"
    done, total = st.get("done", 0), st.get("total", 0)
    pct = f" ({done * 100 // total}%)" if total else ""
    el = st.get("elapsed_s", 0)
    return (
        f"tag={st['tag']} state={state} {done}/{total}{pct} "
        f"SAME0={st.get('same0', 0)} SAME1={st.get('same1', 0)} "
        f"DIFF={st.get('diff', 0)} TIMEOUT={st.get('timeout', 0)} "
        f"elapsed={el}s updated={time.strftime('%H:%M:%S', time.localtime(st.get('updated', 0)))}"
    )


def cmd_status(ap_args):
    st = load_status(ap_args.tag)
    if ap_args.json:
        print(json.dumps(st))
    else:
        print(fmt_status(st))


def cmd_tail(ap_args):
    p = os.path.join(tag_dir(ap_args.tag), "results.log")
    if not os.path.isfile(p):
        die(f"未找到 results: {p}")
    with open(p) as f:
        lines = f.readlines()
    for line in lines[-ap_args.n:]:
        print(line, end="")


def cmd_wait(ap_args):
    while True:
        st = load_status(ap_args.tag)
        print(fmt_status(st), flush=True)
        if st["state"] not in ("running", "starting"):
            return
        if st.get("pid") and not pid_alive(st["pid"]):
            print("worker 已死且未落终态——查 worker.log", file=sys.stderr)
            return
        time.sleep(ap_args.interval)


def cmd_stop(ap_args):
    st = load_status(ap_args.tag)
    pid = st.get("pid")
    if st["state"] != "running" or not pid_alive(pid):
        print(f"tag={ap_args.tag} 未在跑（state={st['state']}）", flush=True)
        return
    # 孙进程 setsid 过，pid 即 pgid——killpg 连当轮 suite 子进程一起收，
    # 避免 worker 死后 suite 孤儿继续跑（SIGTERM 只杀 python 时会这样）。
    try:
        os.killpg(os.getpgid(pid), signal.SIGTERM)
        print(f"SIGTERM → pgid={os.getpgid(pid)}（worker + 当轮 suite 子进程）", flush=True)
    except (ProcessLookupError, PermissionError):
        os.kill(pid, signal.SIGTERM)
        print(f"SIGTERM → pid={pid}（killpg 不可用，回退单杀）", flush=True)


def main():
    ap = argparse.ArgumentParser(prog=TOOL, description="§4.202-③ sweep 常驻后台")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("start")
    p.add_argument("--prefix", default="test-http-")
    p.add_argument("--dir", default=DEFAULT_DIR)
    p.add_argument("--timeout", type=int, default=25)
    p.add_argument("--scope", default=DEFAULT_SCOPE if os.path.isfile(DEFAULT_SCOPE) else "none")
    p.add_argument("--rerun-red", default=None, metavar="TAG")
    p.add_argument("--no-node-cache", action="store_true")
    p.add_argument("--thread-id", default="3599")
    p.add_argument("--port-base", default="29999")
    p.add_argument("--tag", default="main")
    p.add_argument("--wjs", default=None)
    p.add_argument("--node", default="node")
    p.set_defaults(fn=cmd_start)

    p = sub.add_parser("status")
    p.add_argument("--tag", default="main")
    p.add_argument("--json", action="store_true")
    p.set_defaults(fn=cmd_status)

    p = sub.add_parser("tail")
    p.add_argument("--tag", default="main")
    p.add_argument("-n", type=int, default=20)
    p.set_defaults(fn=cmd_tail)

    p = sub.add_parser("wait")
    p.add_argument("--tag", default="main")
    p.add_argument("--interval", type=int, default=30)
    p.set_defaults(fn=cmd_wait)

    p = sub.add_parser("stop")
    p.add_argument("--tag", default="main")
    p.set_defaults(fn=cmd_stop)

    args = ap.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
