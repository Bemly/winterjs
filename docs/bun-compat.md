# Bun node: 兼容清单与移植评估

> 来源：`oven-sh/bun` HEAD `6a92015`（2026-09-12 浅克隆至 `/tmp/wjs-bun`，
> sparse 只取 `docs/runtime/` + `src/js/`；仓库内不留 Bun 源码）。
> 兼容表原文 targeting **Node v26**（`docs/runtime/nodejs-compat.mdx`）。
> “40%”指出处：第三方全量跑分（Node 官方套件 4457 项，Bun 1.3.14 约 40.6%，
> 2026-05），与 Bun 官方“分模块 90~100%”并存——前者是全量含未覆盖模块，
> 后者只算已覆盖模块，尺子不同都记下来。

## 0. 实现解剖（先看结构，再看清单）

- Bun 的 `node:` 面几乎全是**引擎无关的 JS/TS**：`src/js/node/` 66 个文件
  （`events.ts` 头注“Reference: nodejs/node lib/events.js”，即 Node 自家
  `lib/*.js` 的改写），`src/js/internal/` 45 项（`validators`/`errors`/
  `primordials.js`/`streams/`/`http/`/`net/`/`fs/`/`worker/`…）。
- 连最纯的 `events.ts` 也吃 6 处 Bun 内部件：`internal/validators`、
  `internal/abort_listener`、`internal/shared`、`node:util/types`、
  懒加载 `internal/util/inspect`、`internal/fixed_queue`、
  `node:async_hooks`——**没有一行是可逐字搬的**，件件要重映射。
- 真正碰引擎的是 native 底座：历史是 Zig + JSC C++ API，
  HEAD 正在迁往 Rust（`Cargo.toml` 入库，`src/*_jsc` 即绑 JavaScriptCore 的
  Rust crate，如 `src/http_jsc`、`src/sql_jsc`）。
- `buffer`（`src/js/builtins/JSBuffer*.ts`）、`console`
  （`src/js/builtins/ConsoleObject.ts`）等走 builtins 通道，无独立
  `src/js/node/` 文件（如 `string_decoder` 也无同名 JS 文件，疑原生）。
- License：JS 文件多带 Joyent/MIT 头（Node `./lib` 同源），vendoring
  须保留版权头（MIT，与本仓 MPL-2.0 兼容；`dependencies.md` 记）。

## 1. 模块总表（Bun 状态 | 形态 | winterjs | 移植）

形态列：`JS` = `src/js/node/` 有同名外壳（仍需重映射内部件）；
`JS+内` = 另吃 `src/js/internal/` 子件；`原生` = 主体在 native 底座。
winterjs 列：✅ = 已有（`src/builtins/node/` 8 件）。

| 模块 | Bun | 形态 | winterjs | 移植注记 |
|---|---|---|---|---|
| assert | 🟢 | JS | ✅ | 已有；深 equal 语义对齐另算 |
| buffer | 🟢（单体 4GiB 上限） | 原生（builtins） | 有全局（Uint8Array 子类） | `node:buffer` 模块面另补，多为 JS |
| console | 🟢 | 原生（builtins） | ✅ | 已有 |
| dgram 99% | 🟢 | JS+内 | — | 需 UDP 底座（`tokio net` 在树内） |
| diagnostics_channel | 🟡（缺 bounded/多数组内置通道） | JS | — | 纯 JS，好啃，排前 |
| dns | 🟢（缺 resolveTlsa） | JS+内 | — | `hickory-resolver` 在树内 |
| events 95% | 🟢 | JS（6 内部件） | — | **最先啃**：零 syscall，纯映射 |
| fs 98% | 🟢 | JS+内（binding 另有） | ✅（同步核心） | binding 底座 winterjs 已有（fs-err/std），补 JS 语义 + 异步面 |
| http | 🟢 | JS+内（7 文件） | — | `reqwest`/`axum` 底座在树内，面大，排中 |
| https | 🟡（无 SNI 回调等） | JS+内 | — | 随 http，`tokio-rustls` 在树内 |
| os | 🟢 | JS | ✅ | 已有 |
| path | 🟢 | JS（posix/win32 分文件） | ✅（纯 JS） | 已有；glob 语义差另算 |
| punycode | 🟢 100% | JS | — | 冻结小模块，随手捡 |
| querystring | 🟢 100% | JS | — | 冻结小模块，随手捡 |
| readline | 🟢 | JS | — | `rustyline` 在树内（REPL 侧已用） |
| stream | 🟢 | JS+内（20+ 文件） | — | 面大但纯 JS；`internal/streams` 整套值得整批映射 |
| string_decoder | 🟢 100% | 原生（无 JS 外壳） | 有 TextDecoder | 薄 wrapper，多为转调 |
| timers | 🟢（含 promises/scheduler） | JS+内 | ✅（核心 timers） | 已有；promises 面另补 |
| tty | 🟢 | JS | — | 薄，多为类型判定 |
| url | 🟢 | JS | ✅（URL 系） | 已有；`url pattern` 见 `urlpattern` 轮子 |
| zlib 98% | 🟢 | JS+内 | — | `flate2`/`brotli`/`ruzstd` 全在树内 |
| async_hooks | 🟡（仅 ALS/AsyncResource 实） | JS+内 | — | events 的依赖，先行件 |
| child_process | 🟡（IPC 若干缺口） | JS+内 | ✅（同步+spawn） | 已有；IPC/stdio 细节另算 |
| cluster | 🟡（http 多绑限 Linux） | JS+内 | — | 排后（多进程语义重） |
| crypto | 🟡（BoringSSL 缺口：ed448/secp256k1/CCM 等） | JS+内 | ✅（subtle 全家） | winterjs 用 RustCrypto，缺口与 Bun **不重合**，逐项对 |
| domain | 🟡 | JS | — | 遗留语义，排后 |
| http2 94% | 🟢 | JS+内 | — | `reqwest http2` 在树内；面大排中 |
| module | 🟡（缺 load/registerHooks 等） | JS+内 | 有 require | 自家 loader 另有体系，只借鉴 `require.cache` 语义 |
| net | 🟢 | JS+内 | — | `tokio net` 在树内；http 的前置 |
| perf_hooks | 🟡 | JS+内 | — | 排后（观测向） |
| process | 🟡 | JS+内 | ✅（全局） | 已有；细节对齐另算 |
| tls | 🟡（无 psk/OCSP/resume） | JS+内 | — | 随 net/http |
| util | 🟢 | JS | — | **先行件**：半数模块的前置（含 `util/types`） |
| v8 | 🟡（堆统计是 JSC 口径） | 原生 | — | 大部分无意义（引擎相关）；`serialize` 口径与 mozjs 不同，**跳过** |
| vm | 🟢（全，连 ESM classes） | 原生（JSC Context） | — | 绑引擎最深，排后（要 compartment/realm 设计） |
| wasi | 🟡 | 原生 | — | `wasmtime` 已否决（§14），**不做** |
| worker_threads | 🟡 | JS+内 | — | 线程模型要另设计（§6），排后 |
| inspector | 🟡 | JS+内 | — | 排后（调试向） |
| repl | 🟡 | JS | 有 REPL | 已有；补 Node 口径另算 |
| sqlite | 🟢 | 原生 | 有 bun:sqlite | turso 路线已定，不跟 Bun（系统 libsqlite 口径不同） |
| test | 🟡 | JS（test_runner） | ✅（node:test 起步） | 已有；reporter/diff 另算 |
| trace_events | 🟢 | JS | — | 小，`tracing` 在树内，随手捡 |
| quic | 🟢 99%（Node 实验性） | 原生 | — | 排后（实验性） |
| sea | 🔴（指去 `bun build --compile`） | — | — | 不做 |
| sys | 🟢（即 util） | — | — | 同 util |

子路径（`fs/promises`、`timers/promises`、`dns.promises`、`stream/*`、
`inspector/promises`、`path.posix/win32`）Bun 侧全有 JS 文件，
winterjs 侧仅 `fs/promises` 有，其余随主表。

## 2. “JSC 兼容 mozjs 更舒服” verdict：半对

- **对的部分**：要紧的兼容面（上表 `JS`/`JS+内` 行）**根本不碰引擎**——
  文件里既无 V8 API 也无 JSC API，只有 ECMAScript + Bun 内部件。
  所以移植不跟任何引擎打架，也没有 V8-ism 要绕；同为非 V8 也意味着
  不会踩“按 V8 行为写的捷径”（如 `node:test` 的 V8 inspector 预览）。
  Node 官方测试的期望断言可整段复用，这是最实在的资产。
- **错的部分**：舒服**不是来自“JSC≈SpiderMonkey”**——它俩的 embedding
  API 几乎没有交集（JSC C++ API vs SM JSAPI：rooting、compartment/realm、
  字符串、GC 模型全不同；且 Bun native 侧正在 Zig→Rust 搬家，绑哪套
  都不稳）。引擎相关工作（每个 native 经 `jsapi_glue` 落地、§6
  `unsafe` 收敛）在 mozjs 上**一件都省不掉**，换 V8 也一样。
- 一句话：好移植性来自“兼容面是引擎无关的 JS”，不是来自引擎相似。

## 3. 移植排序（Phase 9′ 用）

- N1 纯 JS 先行（零 syscall）：`events` → `util`（含 types）→
  `querystring`/`punycode`/`string_decoder` → `diagnostics_channel`/
  `trace_events`。每件工作 = 重映射内部件到 winterjs natives + Node
  套件断言入库（三件套照旧）。
- N2 有轮子的 native-backed：`fs` 语义补齐（底座已有）→ `stream`
  整批（`internal/streams` 对着 map）→ `zlib`/`dns`/`net`→`http/https`
 （底座全在树内，见 `dependencies.md` §6/§8/§10）。
- N3 排后：`vm`/`worker_threads`（要引擎/线程设计）、`crypto` 差集对齐、
  `child_process` IPC 角落、`cluster`、`inspector`、`perf_hooks`。
- 永不：`v8`（引擎口径）、`wasi`（§14 已否决）、`sea`（无对等需求）。
- napi（`.node` 原生插件，如 rolldown binding）不在本表——Bun 跑
  `node:` 自家面也不需要 napi；napi 是第三方原生包的事，另案评估。
