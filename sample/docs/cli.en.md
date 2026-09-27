---
layout: docs
title: CLI reference
lang: en
stub: cli
permalink: /en/cli/
---
# winterjs CLI Reference

> Source of truth is the binary itself: `winterjs --help`
> (or `-l zh --help` for Chinese). This page explains the **rules** behind the
> flags. 中文版见 [CLI 参考](../zh/cli/).

## Rules

1. **Exactly one action per invocation.** `--run a.js --eval 1` → exit 1.
2. **No bare subcommands, no positional actions.** `winterjs a.js` fails;
   use `winterjs --run a.js`. The only trailing positionals are *script
   arguments* for `--run` (`process.argv.slice(2)`), best separated by `--`:
   `winterjs --run app.js -- --port 8080`.
3. **Modifiers belong to their action.** A modifier given without its action
   exits 1 with `--X only works with --Y (see --help)`. Explicitly passing a
   default still counts as given (`--port 3000` without `--serve` errors).
   Unknown `--flags` are rejected, never swallowed: trailing positionals
   belong only to `--run`.

## Actions

| Flag | Effect |
|---|---|
| `-r/--run <FILE\|script>` | Run a JS file (known script extension) or a `package.json` script (bare name); prints the completion value. JS bins in `node_modules/.bin` re-execute through winterjs itself (no node needed). `--watch` re-runs files on change (scripts are not watchable) |
| `-e/--eval <CODE>` | Evaluate inline JS, print completion value |
| `-c/--config [--schema]` | Print resolved settings, or its JSON Schema |
| `-C/--completions <SHELL>` | Print shell completion script (bash/elvish/fish/powershell/zsh) |
| `-m/--man` | Print roff manual to stdout |
| `-a/--add <PKG...>` | Add packages to local `node_modules` |
| `-i/--install <PKG...>` | Install packages globally (shared data dir) |
| `-R/--remove <PKG...>` | Remove packages from local `node_modules` (prunes `.bin` links + lockfile) |
| `-U/--uninstall <PKG...>` | Uninstall globally installed packages |
| `-p/--publish [--dry-run]` | Publish current package (`--dry-run` validates only) |
| `--login` | Log in to a registry (token → `~/.npmrc`) |
| `-u/--upgrade [--dry-run]` | Self-upgrade (needs `WINTERJS_UPDATE_GITHUB=owner/repo`) |
| `-I/--init [NAME]` | Scaffold a package; installs deps when `package.json` exists |
| `--repl` | Interactive REPL |
| `-t/--test [PATH...]` | Run test files; no paths → auto-discover from cwd. `--watch` re-runs on change |
| `-L/--lint [ARGS...]` | Forward to `oxlint` (`.bin` or `PATH`), args verbatim |
| `-f/--fmt [ARGS...]` | Forward to `oxfmt`, args verbatim |
| `-s/--serve [DIR]` | Serve a directory and/or a JS handler (`export default { fetch }`) over H1/H2/H3; TLS via `--cert/--key` or ACME. `--watch` restarts the server on change |
| `-b/--db <FILE> [--exec <SQL>]` | Inspect a turso/SQLite database file (default lists tables). Storage files use `--storage-path` |

## Modifier scope

| Modifier(s) | Only with |
|---|---|
| `-d/--dry-run` | `--add/--install/--remove/--uninstall/--publish/--init/--upgrade/--serve` (ACME plan print) / `--db` (plan print) |
| `--registry` | `--add/--install/--publish/--login/--init` |
| `-T/--tag` | `--publish` |
| `--token`, `-o/--oauth` | `--login` |
| `-n/--name`, `-y/--yes`, `--force` | `--init` |
| `-F/--filter`, `--test-name-pattern` | `--test` |
| `-w/--watch` | `--test` (re-run) / `--run` (re-run file) / `--serve` (restart child) |
| `-D/--dir`/`-H/--host`/`-P/--port`/`--handler`/`--limit-rps`/`--cert`/`-k/--key`/`--acme-domain`/`-E/--acme-email`/`--acme-cache`/`--acme-production` | `--serve` |
| `-S/--schema` | `--config` |
| `--exec` | `--db` |
| `--storage-path` | `--run/--eval/--test/--repl/--serve` |
| `--allow-read`/`-W/--allow-write`/`--allow-env`/`--allow-run`/`--allow-ffi`/`-A/--allow-all` | `--run/--eval/--test/--repl/--db` |
| `-v/--verbose`, `-l/--lang` | global |

Sandbox model: no `--allow-*` → wide open (historic behavior); any `--allow-*`
→ sandbox on, unlisted classes denied with `PermissionError`.

## Exit codes

`0` ok · `1` runtime/usage error (uncaught JS, wrong flag combo) ·
`2` clap parse error (e.g. bad `-l` value) · `9` illegal node-compat flag value
or self-spawn recursion guard · script `process.exit(n)` propagates `n`.

## Node-compat flags

Node runtime flags (`--expose-internals`, `--experimental-*`, …) are stripped
before parsing, recorded on `process.execArgv`, and readable via
`internal/options`. With a stripped flag, `node --flag file args` shapes are
accepted (`--run` auto-inserted); bare `winterjs file.js` without compat flags
is still an error. Illegal flag *values* exit 9 (never re-run the same file).
