# Phase 9 计划 — node: 落地（先到 deno 高度）

> 立项 2026-09-12（用户拍板）。目标：`node:` 兼容先到 **deno 高度**
> （全 23 + 半 16，见 §3 对标矩阵），终局全实现（除 §4 不做项）。
> v1 不含 napi（`.node` 原生插件另案）。
>
> 方法（三源对照，详 `docs/bun-compat.md §4`）：**node 定语义**
> （`lib/` 原文 + `test/parallel` 断言原文入库），**bun 当词典**
> （`internal/` 文件名 1:1），**deno 对分层**
> （`op_*` Rust 底 ↔ natives 经 `jsapi_glue`，TS 壳 ↔ prelude）。
>
> 纪律：AGENTS 三件套（模块单测 + 黑盒正常/报错/边界 + 冒烟 5/5，
> `UNSAFE-BOUNDARY` 新增配 panic 用例）；踩坑记 §4；
> vendoring 三家（node/Bun/deno）JS 文件保留 MIT 头；
> 测试断言取 Node 套件原文（`test/parallel/test-<mod>-*.js` 按需单文件取）。

## 切片

### 9a 纯 JS 先行（零 syscall）

- 做：`events` → `util`（含 `util/types`）→ `querystring`/`punycode`/
  `string_decoder` → `diagnostics_channel`/`trace_events`/`tty` →
  `async_hooks`（ALS/AsyncResource 实，其余 stub，Bun 同款）。
- 前置：手写小件（validators/ERR 码表/fixed_queue/shared，约三份 30 行级）。
- 验收：`test/parallel/test-events-*.js`、`test-util-*.js` 子集点名全绿。

### 9b 流与缓冲

- 做：`buffer` 模块面（全局子类已有）→ `stream` 整批
  （`internal/streams` 20+ 文件对着 map）→ `timers/promises`、
  `stream/*` 子路径。
- 验收：`test-stream-*.js`、`test-buffer-*.js` 子集。

### 9c fs 补齐

- 做：`fs` JS 语义补齐（binding 底座已有：fs-err/std）+ 异步面扩展。
- 验收：`test-fs-*.js` 子集（`fs.watch` 已有，不回归）。

### 9d 网络栈

- 做：`net` → `dns` → `http`/`https` → `http2`（`hyper` 必选，2026-09-12 拍板；
  `h2` 直驱/`httparse` 手写仅作条件 fallback）→ `tls` → `dgram` → `zlib`。
- 底座全在树内（见 `dependencies2.md`）；`hyper` 行级增补（已在闭包）。
- 验收：`test-net-*.js`、`test-http-*.js`、`test-dns-*.js` 子集；
  本机回环 hermetic（fetch/ws 测试同构，不碰外网）。

### 9e 系统与进程

- 做：`crypto` 差集对齐（RustCrypto，缺口与 Bun 不重合逐项对）→
  `child_process` IPC 角落 → `perf_hooks` → `inspector`（调试向，薄）。
  `cluster` 排最后（多进程语义重）。
- 验收：`test-crypto-*.js` 差集项、`test-child-process-*.js` 角落项。

### 9f 引擎深水（排后）

- 做：`vm`（compartment/realm 先设计）→ `worker_threads`（线程模型先设计，
  §6）→ `quic`（实验性，轮子待定）。
- 不做（v1）：`wasi`（§14 已否决）、`v8` 口径（引擎相关，跳过）、`sea`。

## §3 对标矩阵（deno 高度 = 全 23 + 半 16）

现状列：✅ = 已有。切片列见上。

| 模块 | deno | 现状 | 切片 |
|---|---|---|---|
| assert | ✅ | ✅ | — |
| buffer | ✅ | 有全局 | 9b |
| child_process | ✅ | ✅ | 9e（角落） |
| console | ✅ | ✅ | — |
| crypto | ✅ | ✅ | 9e（差集） |
| diagnostics_channel | ✅ | — | 9a |
| events | ✅ | — | 9a |
| fs | ✅ | ✅ | 9c（补齐） |
| module | ✅ | 有 require | 9e（语义借鉴） |
| os | ✅ | ✅ | — |
| path | ✅ | ✅ | — |
| punycode | ✅ | — | 9a |
| querystring | ✅ | — | 9a |
| readline | ✅ | — | 9a |
| sqlite | ✅ | 有 bun:sqlite | —（turso 路线） |
| stream | ✅ | — | 9b |
| string_decoder | ✅ | 有 TextDecoder | 9a（wrapper） |
| test | ✅ | ✅ | — |
| timers | ✅ | ✅ | 9b（promises 面） |
| tty | ✅ | — | 9a |
| url | ✅ | ✅ | — |
| async_hooks | 🟡 | — | 9a（stub 口径） |
| dgram | 🟡 | — | 9d |
| dns | 🟡 | — | 9d |
| http | 🟡 | — | 9d |
| http2 | 🟡 | — | 9d |
| https | 🟡 | — | 9d |
| inspector | 🟡 | — | 9e（薄） |
| net | 🟡 | — | 9d（前置） |
| perf_hooks | 🟡 | — | 9e |
| process | 🟡 | ✅ | 9e（细节） |
| tls | 🟡 | — | 9d |
| util | 🟡 | — | 9a（先行） |
| v8 | 🟡 | — | 不做（§9f） |
| vm | 🟡 | — | 9f |
| worker_threads | 🟡 | — | 9f |
| zlib | 🟡 | — | 9d |

## §4 v1 明确不做

`wasi`、`v8` 口径、`sea`、napi、quic（轮子待定顺延）。
`cluster`/`domain`/`repl` 口径补齐顺延到 v1 之后（deno 侧 ❌ 的三项不列入验收）。
