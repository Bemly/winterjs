<p align="left">
  <a href="https://github.com/Bemly/winterjs"><img src="assets/logo.jxl" width="110" height="110" alt="升级浏览器Update Browser，JXL支持Support Chrome155+、Firefox158+、Safari17+" /></a>&nbsp;&nbsp;
  <a href="https://github.com/Bemly/winterjs"><img src="assets/winterjs.svg" width="415" alt="WinterJS" /></a>
</p>

# winterjs ❄️

[English](./README.md) · [文档站](https://winterjs.bemly.moe/) · [样例](./sample/) · [开发日志](./docs/plan3-journal.md)

*winterjs 是跑在 **Mozilla SpiderMonkey** 上的**类 Bun JS 运行时**——一个二进制跑 JS 文件、
`package.json` 脚本、测试、lint 与静态/动态 HTTP 服务，`node:` 兼容对标 **Bun 高度**。*

```bash
./target/debug/winterjs --run sample/http/server-client.js
./target/debug/winterjs --eval 'await (await fetch("data:text/plain,hi")).text()'  # → hi
```

> 说明：与 [wasmerio/winterjs](https://github.com/wasmerio/winterjs)（WinterCG server，已归档）
> 只是同名。本项目是 Bun/Node 赛道的通用运行时——基于 `servo/mozjs` 推倒重写，无 server 框架。

## 快速开始

```bash
export SDKROOT="$(xcrun --show-sdk-path)"          # macOS，每个新 shell 都要
export LIBCLANG_PATH="/opt/homebrew/opt/llvm/lib" # bindgen 用
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
cargo build
./target/debug/winterjs --eval '40 + 2'            # → 42
```

5 分钟上手：[English](https://winterjs.bemly.moe/#/en/quickstart) / [中文](https://winterjs.bemly.moe/#/zh/quickstart) ·
[`sample/`](./sample/) 下 50 个可运行样例（每个 API 域一件，全离线可跑）。

## 用法

一次调用恰好一个动作；修饰 flag 只在对应动作下生效
（`--port`→`--serve`、`--filter`→`--test`、`--watch`→`--test/--run/--serve`、
`--schema`→`--config`）：

| Flag | 效果 |
|---|---|
| `-r/--run <文件\|脚本名>` | 跑 JS 文件或 `package.json` 脚本（JS bin 经自身递归执行，零 node；`--watch` 变更重跑） |
| `-e/--eval <代码>` | 求值内联 JS，打印完成值 |
| `-t/--test [路径]` | 跑测试文件（自动发现，`--filter/--watch`） |
| `-s/--serve [目录]` | H1/H2/H3 提供静态 + JS `fetch` handler + WebSocket（`--watch` 变更重启） |
| `-b/--db <文件> [--exec <SQL>]` | 查看 turso/SQLite 数据库文件（默认列出全部表）；存储库经 `--storage-path` 指定 |
| `--repl` | 交互式 REPL |
| `-a/--add`、`-i/--install`、`-R/--remove`、`-U/--uninstall`、`-p/--publish`、`--login`、`-u/--upgrade`、`-I/--init` | 包生命周期（npm registry） |
| `-L/--lint`、`-f/--fmt` | 转发给 oxlint/oxfmt |
| `-c/--config`、`-C/--completions`、`-m/--man`、`-v`、`-l/--lang` | 配置/帮助/国际化/日志 |
| `-hide_banner` / `--hide_banner` | 隐藏启动 banner（走 stderr，非 TTY 自动跳过） |

完整参考：[CLI (EN)](https://winterjs.bemly.moe/#/en/cli) / [CLI (中文)](https://winterjs.bemly.moe/#/zh/cli)。

## 原理

winterjs 经 `servo/mozjs` 直连 **Mozilla SpiderMonkey**（`mozjs =0.26.0`，Gecko 153，精确钉版），
其余一切——事件循环、loader、Web/Node 内建、`node:` 垫片——全是**纯 Rust**。
`unsafe` 只存在于 mozjs 边界（rooting、`AutoRealm`、FFI）；JS 跑在独占线程，
Rust 侧只经消息队列与之通信，绝不跨线程共享 `&mut JSContext`。

## `node:` API 兼容（Bun 高度）

目标：Bun 自带 node 测试清单全过；语义跟 Node（`lib/` 源码 + `test/parallel` 断言）。
分模块状态、样例与**已知偏离**见 [API 参考](https://winterjs.bemly.moe/#/zh/api)：

| 领域 | 状态 | 说明 |
|---|---|---|
| `fs/net/http/https/http2/tls/dgram/dns` | ✅ 稳定 | 流式体、keep-alive、H2C、UDP 回环 |
| `crypto/zlib/buffer/stream/events/timers` | ✅ 稳定 | AEAD 套件、brotli、WHATWG 流 |
| `child_process/cluster/worker_threads/vm/module/test` | ✅ 稳定 | 线程底座的 cluster/worker |
| `sqlite`（`node:` + `bun:sqlite`）、`quic`、`readline/repl/tty` | ✅ / 🔶 | `quic` 回环握手超时（另案追查） |
| `storage` / `localStorage`（WinterCG 自有） | ✅ 稳定 | turso 单文件 KV（`--storage-path`，默认 `./winterjs-storage.db`）；经 `-b/--db` 查看 |
| `v8/inspector/trace_events/domain` | 🔶 桥接 | 有意裁剪（堆数字引擎口径不可比） |
| `wasi`、`sea` | ❌ | 设计上不做 |

Web 全局（`fetch`、`URL`、`TextEncoder`、Web Streams、WebCrypto、`WebSocket`、
`structuredClone` 等）随行——见 API 参考。

## 已知限制

* 跨引擎数字不可比（`v8` 堆统计；`allocUnsafe` 恒零填）。
* `structuredClone` 只保纯数据（Date/Map/Set 回来是普通对象）。
* `node:quic` 回环握手超时；`node:https` 单连接目前派发两次 `request`（请幂等守卫）。
* `wasi` / `sea` 不会实现。

## 开发

```bash
cargo build                        # debug 全量约 25 秒（mozjs 走预构建静态库）
./target/debug/winterjs --eval '40 + 2'   # 冒烟（AGENTS.md §3 有 5 条标准命令）
cargo nextest run --profile strict # 全量约 2 分钟
bash scripts/check-lines.sh        # 全部 .rs / 内嵌 JS ≤ 1000 行
```

工作规约：[AGENTS.md](./AGENTS.md) · 进度：[`docs/plan3.md`](./docs/plan3.md)
· 踩坑全集：[`docs/pitfalls.md`](./docs/pitfalls.md)。

## 许可

NPL-1.1（见 [LICENSE](./LICENSE)）。vendored 第三方 JS 保留其 MIT 头。
