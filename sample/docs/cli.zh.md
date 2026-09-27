---
layout: docs
title: CLI 参考
lang: zh
stub: cli
permalink: /zh/cli/
---
# winterjs CLI 参考

> 真相以二进制为准：`winterjs --help`（中文用 `-l zh --help`）。
> 本页讲 flag 背后的**规则**。English version: [CLI reference](../en/cli/).

## 规则

1. **一次恰好一个动作**。`--run a.js --eval 1` → exit=1。
2. **无裸子命令、无裸位置参数**。`winterjs a.js` 是错的，用
   `winterjs --run a.js`。唯一的尾部位置参数是 `--run` 的*脚本参数*
   （`process.argv.slice(2)`），建议用 `--` 分隔：
   `winterjs --run app.js -- --port 8080`。
3. **修饰 flag 只在对应动作下生效**。配错直接 exit=1：
   `--X only works with --Y (see --help)`。显式传默认值也算给了（无 `--serve`
   却传 `--port 3000` 照样报错）；未知 `--flag` 直接拒绝、不吞掉：尾部位置参数
   只归 `--run`。

## 动作

| Flag | 效果 |
|---|---|
| `-r/--run <文件\|脚本名>` | 跑 JS 文件（已知脚本后缀）或 `package.json` 脚本（裸名）；打印完成值。`.bin` 里的 JS bin 经自身递归执行（零 node）。`--watch` 变更重跑文件（脚本串不可监视） |
| `-e/--eval <代码>` | 求值内联 JS，打印完成值 |
| `-c/--config [--schema]` | 打印解析后的配置，或其 JSON Schema |
| `--completions <SHELL>` | 打印 shell 补全脚本（bash/elvish/fish/powershell/zsh） |
| `-m/--man` | 打印 roff 手册页到标准输出 |
| `-a/--add <包...>` | 给本地 `node_modules` 加包 |
| `-i/--install <包...>` | 全局安装包（共享数据目录） |
| `-R/--remove <包...>` | 从本地 `node_modules` 删包（连带 `.bin` 链接 + lockfile） |
| `-U/--uninstall <包...>` | 卸载全局安装的包 |
| `-p/--publish [--dry-run]` | 发布当前包（`--dry-run` 只校验） |
| `--login` | 登录 registry（令牌进 `~/.npmrc`） |
| `-u/--upgrade [--dry-run]` | 自升级（需 `WINTERJS_UPDATE_GITHUB=owner/repo`） |
| `-I/--init [名]` | 建包脚手架；已有 `package.json` 依赖则安装 |
| `--repl` | 交互式 REPL |
| `-t/--test [路径...]` | 跑测试文件；无路径则从 cwd 自动发现。`--watch` 变更重跑 |
| `--lint [参数...]` | 转发给 `oxlint`（`.bin` 或 `PATH`），参数原样 |
| `-f/--fmt [参数...]` | 转发给 `oxfmt`，参数原样 |
| `-s/--serve [目录]` | 以 H1/H2/H3 提供目录/JS handler（`export default { fetch }`）；TLS 走 `--cert/--key` 或 ACME。`--watch` 变更重启服务 |

## 修饰归属

| 修饰 | 只配 |
|---|---|
| `--dry-run` | `--add/--install/--remove/--uninstall/--publish/--init/--upgrade/--serve`（ACME 方案打印） |
| `--registry` | `--add/--install/--publish/--login/--init` |
| `--tag` | `--publish` |
| `--token`、`--oauth` | `--login` |
| `--name`、`-y/--yes`、`--force` | `--init` |
| `--filter`、`--test-name-pattern` | `--test` |
| `--watch` | `--test`（重跑）/`--run`（重跑文件）/`--serve`（重启子进程） |
| `--dir/--host/--port/--handler/--limit-rps/--cert/--key/--acme-*` | `--serve` |
| `--schema` | `--config` |
| `--allow-read/--allow-write/--allow-env/--allow-run/--allow-ffi/--allow-all` | `--run/--eval/--test/--repl` |
| `-v/--verbose`、`-l/--lang` | 全局 |

沙箱模型：不给任何 `--allow-*` → 全开放（历史行为）；给了任一 → 开沙箱，
未授权类别拒绝并报 `PermissionError`。

## 退出码

`0` 正常 · `1` 运行/用法错（未捕获 JS、flag 配错） ·
`2` clap 解析错（如 `-l` 非法值） · `9` 非法 node 兼容旗值或自递归守卫 ·
脚本 `process.exit(n)` 透传 `n`。

## Node 兼容旗

node 运行时旗（`--expose-internals`、`--experimental-*` 等）解析前剥除，
记在 `process.execArgv`，经 `internal/options` 读回。剥过旗时接受
`node --flag file args` 形（自动补 `--run`）；无兼容旗的裸 `winterjs file.js`
照旧报错。非法旗*值* exit=9（绝不剥掉重跑同一文件）。
