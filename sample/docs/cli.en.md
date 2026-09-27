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
   exits 1 with `--X only works with --Y (see --help)`.

## Actions

| Flag | Effect |
|---|---|
| `-r/--run <FILE\|script>` | Run a JS file (known script extension) or a `package.json` script (bare name); prints the completion value. JS bins in `node_modules/.bin` re-execute through winterjs itself (no node needed) |
| `-e/--eval <CODE>` | Evaluate inline JS, print completion value |
| `-c/--config [--schema]` | Print resolved settings, or its JSON Schema |
| `--completions <SHELL>` | Print shell completion script (bash/elvish/fish/powershell/zsh) |
| `-m/--man` | Print roff manual to stdout |
| `-a/--add <PKG...>` | Add packages to local `node_modules` |
| `-i/--install <PKG...>` | Install packages globally (shared data dir) |
| `-p/--publish [--dry-run]` | Publish current package (`--dry-run` validates only) |
| `--login` | Log in to a registry (token → `~/.npmrc`) |
| `-u/--upgrade [--dry-run]` | Self-upgrade (needs `WINTERJS_UPDATE_GITHUB=owner/repo`) |
| `-I/--init [NAME]` | Scaffold a package; installs deps when `package.json` exists |
| `--repl` | Interactive REPL |
| `-t/--test [PATH...]` | Run test files; no paths → auto-discover from cwd |
| `--lint [ARGS...]` | Forward to `oxlint` (`.bin` or `PATH`), args verbatim |
| `-f/--fmt [ARGS...]` | Forward to `oxfmt`, args verbatim |
| `-s/--serve [DIR]` | Serve a directory and/or a JS handler (`export default { fetch }`) over H1/H2/H3; TLS via `--cert/--key` or ACME |

## Modifier scope

| Modifier(s) | Only with |
|---|---|
| `--dry-run` | `--add/--install/--publish/--init/--upgrade/--serve` (ACME plan print) |
| `--registry` | `--add/--install/--publish/--login/--init` |
| `--tag` | `--publish` |
| `--token`, `--oauth` | `--login` |
| `--name`, `-y/--yes`, `--force` | `--init` |
| `--filter`, `--test-name-pattern`, `--watch` | `--test` |
| `--dir/--host/--port/--handler/--limit-rps/--cert/--key/--acme-*` | `--serve` |
| `--schema` | `--config` |
| `--allow-read/--allow-write/--allow-env/--allow-run/--allow-ffi/--allow-all` | `--run/--eval/--test/--repl` |
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
