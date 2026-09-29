#!/usr/bin/env python3
"""Bun/Deno .d.ts TSDoc -> REPL .doc corpus pages (stdlib only).

Usage (repo root):
  scripts/gen-ns-docs.py --bun content/ns-dts/bun.d.ts \
      --bun content/ns-dts/bun.serve.d.ts --bun content/ns-dts/bun.shell.d.ts \
      --deno content/ns-dts/deno.ns.d.ts --deno content/ns-dts/deno_net.d.ts \
      --deno content/ns-dts/deno.unstable.d.ts --out content

Reads the two vendored .d.ts sources, extracts the TSDoc block + first
overload signature for each aliased symbol, and writes
  content/bun/<name>/index.md / content/deno/<name>/index.md
shaped exactly like MDN pages (frontmatter, prose paras, ## Syntax fence,
### Parameters) so src/repl_doc.rs summary_inner works unchanged.

Only symbols winterjs actually aliases (see SYMBOLS) get pages; missing
symbols warn on stderr and are skipped (no guessing, no authoring).
Idempotent: re-running overwrites the same bytes (asserted in-repo by
re-running and checking git diff is empty).
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

DENO_SYMBOLS = [
    "version", "args", "pid", "mainModule", "execPath", "build", "arch",
    "hostname", "osRelease", "env", "errors", "cwd", "chdir", "exit",
    "networkInterfaces", "systemMemoryInfo", "consoleSize", "stdin", "stdout",
    "stderr", "readFile", "writeFile", "readTextFile", "writeTextFile",
    "open", "stat", "lstat", "mkdir", "remove", "rename", "copyFile",
    "symlink", "readLink", "realPath", "readDir", "makeTempDir",
    "makeTempFile", "truncate", "chmod", "chown", "utime", "watchFs",
    "test", "serve", "connect", "listen", "listenDatagram",
    "resolveDns", "upgradeWebSocket", "addSignalListener",
    "removeSignalListener", "Command", "permissions",
]

BUN_SYMBOLS = [
    "version", "revision", "argv", "main", "env", "stdout", "stdin",
    "stderr", "file", "write", "spawn", "spawnSync", "$", "sleep",
    "sleepSync", "nanoseconds", "randomUUIDv7", "sha", "hash", "serve",
    "listen", "connect", "udpSocket", "fileURLToPath", "pathToFileURL",
    "resolveSync", "which", "gc", "shrink",
]

DOCBLOCK_RE = re.compile(r"/\*\*(.*?)\*/[ \t]*\n(?:[ \t]*\n)*[ \t]*([^\n]+)", re.S)


def clean_doc(raw: str) -> tuple[list[str], list[tuple[str, str]]]:
    """TSDoc -> (prose paras, [(param, desc)]). Drops @example fences
    (they would hijack summary_inner's first-fence rule), @tags/@category
    lines; {@linkcode X} -> X."""
    paras: list[str] = []
    params: list[tuple[str, str]] = []
    cur: list[str] = []

    def flush() -> None:
        if cur:
            paras.append(" ".join(cur))
            cur.clear()

    skip_fence = False
    for line in raw.splitlines():
        s = line.strip().lstrip("*").strip()
        if s.startswith("```"):
            skip_fence = not skip_fence
            continue
        if skip_fence:
            continue
        if not s:
            flush()
            continue
        if s.startswith("@example"):
            continue
        if s.startswith("@"):
            m = re.match(r"@param\s+(\S+)\s*-?\s*(.*)", s)
            if m:
                params.append((m.group(1), m.group(2)))
            continue
        s = re.sub(r"\{@linkcode\s+([^}]+)\}", r"\1", s)
        s = re.sub(r"\{@link\s+([^}|]+)(?:\|([^}]+))?\}",
                   lambda m: m.group(2) or m.group(1).strip(), s)
        s = s.replace("`", "")
        if re.fullmatch(r"\*\*.+?\*\*", s):
            continue
        cur.append(s)
    flush()
    return paras, params


def signature_text(decl: str) -> str:
    """First overload decl, collapsed to ≤3 lines."""
    lines = [l.rstrip() for l in decl.strip().splitlines()]
    out: list[str] = []
    depth = 0
    for ln in lines:
        out.append(ln.strip())
        depth += ln.count("(") - ln.count(")")
        if (ln.rstrip().endswith(";") or ln.rstrip().endswith("{")) and depth <= 0:
            break
        if len(out) >= 6:
            break
    sig = " ".join(out).rstrip(" {").rstrip()
    if not sig.endswith(";"):
        sig += ";"
    return sig


def extract(src: str, symbols: list[str]) -> dict[str, tuple[str, str]]:
    """symbol -> (tsdoc, decl). First match wins (first overload).
    decl = up to 6 source lines after the docblock (covers wraps)."""
    found: dict[str, tuple[str, str]] = {}
    lines = src.splitlines()
    for m in DOCBLOCK_RE.finditer(src):
        doc = m.group(1)
        start = src.count("\n", 0, m.end())
        ctx = lines[start:start + 6]
        first = next((l.strip() for l in ctx if l.strip() and not l.strip().startswith("//")), "")
        decl = "\n".join(ctx)
        for sym in symbols:
            if sym in found:
                continue
            if re.search(
                rf"(^|\b)(export\s+)?(function|const|class|interface|type|let|var|namespace)\s+{re.escape(sym)}(?![\w$])"
                rf"|\b{re.escape(sym)}\s*[:=(\[]",
                first,
            ):
                found[sym] = (doc, decl)
                break
    return found


def page(ns: str, sym: str, doc: str, decl: str) -> str:
    paras, params = clean_doc(doc)
    if not paras:
        paras = [f"The {ns}.{sym} API."]
    sig = signature_text(decl)
    topic = f"{ns}.{sym}"
    lines = [
        "---",
        f'title: "{topic}"',
        f"slug: {ns}/{sym}",
        "---",
        "",
        *paras,
        "",
        "## Syntax",
        "",
        "```ts",
        sig,
        "```",
        "",
    ]
    if params:
        lines += ["### Parameters", ""]
        for name, desc in params:
            lines += [f"- `{name}`", f"  - : {desc}" if desc else "  - : "]
        lines += [""]
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--bun", required=True, action="append")
    ap.add_argument("--deno", required=True, action="append")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()

    bun_src = "\n".join(Path(p).read_text() for p in a.bun)
    deno_src = "\n".join(Path(p).read_text() for p in a.deno)
    out = Path(a.out)

    missing = 0
    for ns, src, syms in (("Bun", bun_src, BUN_SYMBOLS),
                          ("Deno", deno_src, DENO_SYMBOLS)):
        found = extract(src, syms)
        for sym in syms:
            if sym not in found:
                print(f"warn: no docblock for {ns}.{sym}, skipped",
                      file=sys.stderr)
                missing += 1
                continue
            doc, decl = found[sym]
            d = out / ns.lower() / sym.lower()
            d.mkdir(parents=True, exist_ok=True)
            (d / "index.md").write_text(page(ns, sym, doc, decl))
        print(f"{ns}: {len(found)}/{len(syms)} pages", file=sys.stderr)
    return 0 if missing == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
