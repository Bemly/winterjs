# Phase 9 计划 — node: 落地（先到 deno 高度）

> **存档（2026-09-25）**：deno 高度已对齐完结，deno 不再作为参照。 当前进度见 `docs/plan3.md` §0，索引 `docs/README.md`。


> 立项 2026-09-12（用户拍板）。目标：`node:` 兼容先到 **deno 高度**
> （全 23 + 半 16，见 §3 对标矩阵），终局全实现（除 §4 不做项）。
> v1 不含 napi（`.node` 原生插件另案）。
> 状态（2026-09-15）：deno 高度自评已达——半 16 全达/超（含 `vm`/
> `worker_threads` 反超 deno 🟡），全 23 剩 5 处欠账（`readline`/
> `node:sqlite`/`node:test` 深度/`tty`/`url-legacy`，见 §3 尾）。
> §4 两项（napi/quic）已反转落地，见尾注。
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

> 状态（2026-09-15）：9a–9f 切片全部收官（含 M5 vitest 牵引加餐）；下为原切片计划，存档。

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

现状列（2026-09-15 回填）：✅ = 落地可用；🟡 = 部分/薄桥（欠账见注）；— = 无。
切片列为原切片归属，收官项不再开工。

| 模块 | deno | 现状 | 切片 |
|---|---|---|---|
| assert | ✅ | ✅ | — |
| buffer | ✅ | ✅（模块面） | 9b 收官 |
| child_process | ✅ | ✅（fork 线程底座记档） | 9e+M5 收官 |
| console | ✅ | ✅ | — |
| crypto | ✅ | ✅ | 9e/9h/9i 收官 |
| diagnostics_channel | ✅ | ✅ | 9a 收官 |
| events | ✅ | ✅ | 9a 收官 |
| fs | ✅ | ✅ | 9c 收官 |
| module | ✅ | ✅（require 全家） | 9j 收官 |
| os | ✅ | ✅ | — |
| path | ✅ | ✅ | — |
| punycode | ✅ | ✅ | 9a 收官 |
| querystring | ✅ | ✅ | 9a 收官 |
| readline | ✅ | 🟡（最小桥；question/行编辑欠账） | 9j（vite 解挡） |
| sqlite | ✅ | ✅ bun:sqlite／— node:sqlite（turso 路线，欠账） | — |
| stream | ✅ | ✅ | 9b 收官 |
| string_decoder | ✅ | ✅ | 9a 收官 |
| test | ✅ | 🟡（起步；reporter/diff 欠账） | — |
| timers | ✅ | ✅ | 9b+M5 收官 |
| tty | ✅ | 🟡（薄面；termios 欠账） | 9a |
| url | ✅ | ✅（WHATWG+file 系；legacy 欠账） | 9j |
| async_hooks | 🟡 | 🟡（Bun 同款 stub 口径） | 9a（stub 口径） |
| dgram | 🟡 | ✅ | 9d 收官 |
| dns | 🟡 | 🟡（base 面；深件欠账） | 9d |
| http | 🟡 | ✅（整收口径） | 9d 收官 |
| http2 | 🟡 | ✅（h2c+H3；无 push） | 9d/9i 收官 |
| https | 🟡 | ✅（同 http 记档） | 9d 收官 |
| inspector | 🟡 | 🟡（薄层） | 9e（薄） |
| net | 🟡 | ✅ | 9d 收官（前置） |
| perf_hooks | 🟡 | 🟡（简化采样记档） | 9e |
| process | 🟡 | ✅ | 9e（细节收官） |
| tls | 🟡 | ✅（自建 roots；缺 SecureContext） | 9d 收官 |
| util | 🟡 | ✅ | 9a 收官（先行） |
| v8 | 🟡 | 🟡（最小桥；口径跳过） | 不做（§9f） |
| vm | 🟡 | ✅ | 9f 收官 |
| worker_threads | 🟡 | ✅ | 9f 收官 |
| zlib | 🟡 | ✅（zstd 恒 Fastest） | 9d 收官 |

> 到线结论（2026-09-15 自评）：半 16 全达/超（`vm`/`worker_threads`/
> `http`/`https`/`net`/`tls`/`util`/`zlib`/`dgram` 已超 deno 🟡，
> 其余口径对齐）；全 23 剩 5 处欠账——`readline`（最小桥）、
> `node:sqlite`（仅 `bun:sqlite`）、`node:test`（起步深度）、`tty`（薄面）、
> `url`（legacy 面）；`v8` 口径双方同跳过，不计差距。偏差明细见
> `docs/bun-compat.md` §1 注记与各模块头注。

## §4 v1 明确不做

`wasi`、`v8` 口径、`sea`、napi、quic（轮子已定：`quinn` 必选；
引入与接线顺延到 9f，v1 不验收）。
`cluster`/`domain`/`repl` 口径补齐顺延到 v1 之后（deno 侧 ❌ 的三项不列入验收）。

> 反转注记（2026-09-13/15）：napi（M0–M6 全收官，见 `docs/plan-napi.md`）、
> quic（9g 接线，`src/builtins/node/quic.rs` + `tests/quic.rs`）均已落地，
> 本节"不做"仅保留为 v1 切片时的历史口径，不再是现状。
