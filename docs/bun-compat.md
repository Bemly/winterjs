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
node 列全 ✅（它是参照系，无例外）；deno 列数据源 `docs.deno.com`
（全 23 + 半 16 + 无 6，Deno 2.7/2.8 口径）。
winterjs 列（2026-09-15 现状回填，证据见后表注记；注册表以
`src/builtins/node/mod.rs` `BUILTINS` 为准）：✅ = 落地可用（黑盒覆盖）；
🟡 = 部分/薄桥（缺口见注记）；— = 无/不做。

| 模块 | node | Bun | deno | 形态 | winterjs | 移植注记 |
|---|---|---|---|---|---|---|
| assert | ✅ | 🟢 | ✅ | JS | ✅ | `assert` + `assert/strict` 双注册；深 equal 语义对齐另算 |
| buffer | ✅ | 🟢（单体 4GiB 上限） | ✅ | 原生（builtins） | ✅ | 全局子类 + 模块面 + 整数/浮点读写 32 方法（DataView 直通，9h） |
| console | ✅ | 🟢 | ✅ | 原生（builtins） | ✅ | 具名全表 + `Console` 类；`table` 无列对齐（记档） |
| dgram 99% | ✅ | 🟢 | 🟡 | JS+内 | ✅ | 回环/connect/组播投递 + close 语义（§4.36 checklist 三查）；未做 recvbuf 系（另切片） |
| diagnostics_channel | ✅ | 🟡（缺 bounded/多数组内置通道） | ✅ | JS | ✅ | 全语义移植（含 TracingChannel/BoundedChannel），Bun 缺口本仓无 |
| dns | ✅ | 🟢（缺 resolveTlsa） | 🟡 | JS+内 | ✅ | `lookup/resolve4/6`（std）+ CNAME/MX/TXT/SRV/NS/PTR/Any/servers/order（hickory 全套，10d） |
| events 95% | ✅ | 🟢 | ✅ | JS（6 内部件） | ✅ | 全语义移植（`once/on` 迭代器、AsyncResource 挂载） |
| fs 98% | ✅ | 🟢 | ✅ | JS+内（binding 另有） | ✅ | 同步全家 + `fs/promises` + watch；promises 底层同步实现（记档） |
| http | ✅ | 🟢 | 🟡 | JS+内（7 文件） | ✅ | Server/Client 回环；整收口径（无 keep-alive、体整收、非流全家，记档） |
| https | ✅ | 🟡（无 SNI 回调等） | 🟡 | JS+内 | ✅ | 与 http 同帧层 + tokio-rustls；`ca` 零证书 fail fast |
| os | ✅ | 🟢 | ✅ | JS | ✅ | 全同步 natives |
| path | ✅ | 🟢 | ✅ | JS（posix/win32 分文件） | ✅ | 纯 JS 双空间 + 子路径双注册；glob 语义差另算 |
| punycode | ✅ | 🟢 100% | ✅ | JS | ✅ | 冻结小模块全移植 |
| querystring | ✅ | 🟢 100% | ✅ | JS | ✅ | 全移植 |
| readline | ✅ | 🟢 | ✅ | JS | ✅ | 行/history/question/按键解码/Emacs 子集/迭代器；无 completer（记档） |
| stream | ✅ | 🟢 | ✅ | JS+内（20+ 文件） | ✅ | lib 逐字内嵌 + Duplex/Transform/pipeline/compose 全链 |
| string_decoder | ✅ | 🟢 100% | ✅ | 原生（无 JS 外壳） | ✅ | 纯 JS 重写（含遗产 lastNeed/lastChar 面） |
| timers | ✅ | 🟢（含 promises/scheduler） | ✅ | JS+内 | ✅ | 回调 + `timers/promises` 双面；`setImmediate` 近似、`scheduler` 未导出（记档） |
| tty | ✅ | 🟢 | ✅ | JS | ✅ | net.Socket 基座 + ioctl winsize + 真 raw + 方法全家；构造器非 TTY 即抛（真机同） |
| url | ✅ | 🟢 | ✅ | JS | ✅ | WHATWG 全局 + file 系（真机逐项对码）；legacy parse/format 不导出（记档） |
| zlib 98% | ✅ | 🟢 | 🟡 | JS+内 | ✅ | gzip/deflate/br + zstd（恒 Fastest，记档）；crc32/zip 实验不做 |
| async_hooks | ✅ | 🟡（仅 ALS/AsyncResource 实） | 🟡 | JS+内 | 🟡 | Bun 同款口径：ALS/AsyncResource 实，createHook stub，跨 await 传播不支持 |
| child_process | ✅ | 🟡（IPC 若干缺口） | ✅ | JS+内 | ✅ | spawn/exec 真进程 + fork（线程底座，stdio 恒 null 等记档） |
| cluster | ✅ | 🟡（http 多绑限 Linux） | ❌ | JS+内 | — | 排后（多进程语义重） |
| crypto | ✅ | 🟡（BoringSSL 缺口：ed448/secp256k1/CCM 等） | ✅ | JS+内 | ✅ | Hash/Hmac/对称/非对称 + ml-kem/ml-dsa + X509 verify；GCM iv 限 12B、ccm/ocb 不做（记档） |
| domain | ✅ | 🟡 | ❌ | JS | — | 遗留语义，排后 |
| http2 94% | ✅ | 🟢 | 🟡 | JS+内 | ✅ | h2c prior-knowledge + H3 分支（串行记档）；无 push（Web 已死）/trailer（等流式切片）/Upgrade（浏览器不用） |
| module | ✅ | 🟡（缺 load/registerHooks 等） | ✅ | JS+内 | ✅ | require（CJS/type 口径/require(esm)）+ createRequire + registerHooks/import.meta.resolve + 静态具名发现 |
| net | ✅ | 🟢 | 🟡 | JS+内 | ✅ | 回环 + allowHalfOpen/destroy 语义全对；port 0 hermetic |
| perf_hooks | ✅ | 🟡 | 🟡 | JS+内 | 🟡 | performance/计时全家可用；monitorEventLoopDelay 简化、eventLoopUtilization 恒值（记档） |
| process | ✅ | 🟡 | 🟡 | JS+内 | ✅ | 全局 + 模块面（argv/env/stdio/exit 哨兵） |
| tls | ✅ | 🟡（无 psk/OCSP/resume） | 🟡 | JS+内 | ✅ | 自建 roots + rejectUnauthorized 口径；无 SecureContext/getPeerCertificate（记档） |
| util | ✅ | 🟢 | 🟡 | JS | ✅ | format/inspect/promisify/parseEnv（真机全例对拍）；parseArgs/MIMEType/getSystemError* 按需未移植 |
| v8 | ✅ | 🟡（堆统计是 JSC 口径） | 🟡 | 原生 | 🟡 | startupSnapshot 最小桥（vite 解挡）；堆统计/serialize 跳过（双方同为口径缺口） |
| vm | ✅ | 🟢（全，连 ESM classes） | 🟡 | 原生（JSC Context） | ✅ | SourceText/Synthetic/linker 全链 + thenable 认领；CCW 记档（已超 deno 🟡） |
| wasi | ✅ | 🟡 | ❌ | 原生 | — | `wasmtime` 已否决（§14），**不做** |
| worker_threads | ✅ | 🟡 | 🟡 | JS+内 | ✅ | MessageChannel/Worker + 循环引用信封（已超 deno 🟡） |
| inspector | ✅ | 🟡 | 🟡 | JS+内 | 🟡 | 薄层（Runtime.evaluate 真求值，domains 仅 ack） |
| repl | ✅ | 🟡 | ❌ | JS | ✅ CLI + ✅ 模块 | CLI `--repl` 全功能；`node:repl`（REPLServer/start/Recoverable/内建命令，无 caret 行） |
| sqlite | ✅ | 🟢 | ✅ | 原生 | ✅ bun/✅ node | `bun:sqlite`（turso 路线）+ `node:sqlite`（DatabaseSync/StatementSync，10d；高级 Session/backup 未做） |
| test | ✅ | 🟡 | ✅ | JS（test_runner） | 🟡 | `node:test` 起步 + `winterjs test` runner；reporter/diff 深度欠账 |
| trace_events | ✅ | 🟢 | ❌ | JS | ✅ | 全语义移植（类别集 JS 侧） |
| quic | ✅（实验性） | 🟢 99%（Node 实验性） | —（deno 无此模块） | 原生 | ✅ | 实验性全链（secure 回环 + H3 headers；串行记档） |
| sea | ✅ | 🔴（指去 `bun build --compile`） | ❌ | — | — | 不做 |
| sys | ✅（即 util） | 🟢（即 util） | ✅（即 util） | — | 同 util | `node:sys` 别名未单注册（走 `node:util`） |

子路径（`fs/promises`、`timers/promises`、`dns.promises`、`stream/*`、
`inspector/promises`、`path.posix/win32`）Bun 侧全有 JS 文件，
winterjs 侧已全注册（`fs/promises`、`dns/promises`、`stream/promises`、
`stream/consumers`、`stream/web`、`timers/promises`、`assert/strict`、
`path/posix`、`path/win32`、`inspector/promises`、`util/types`，
见 `node/mod.rs` `BUILTINS`）；其余随主表。

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

## 3. 移植排序（Phase 9′ 用，已收官，存档）

> 状态（2026-09-15）：N1/N2 全收官，N3 收官大半（`vm`/`worker_threads`/
> `crypto` 差集/`child_process`/`inspector`/`perf_hooks` 均落地；`cluster`
> 仍排后）。下为切片时的原排序，保留为过程记录。

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
  `node:` 自家面也不需要 napi；napi 是第三方原生包的事，另案评估
  （已于 M0–M6 另案收官，见 `docs/plan-napi.md`）。

## 4. 三源对照（node × Bun × deno，2026-09-12）

> 拉取：`/tmp/wjs-node`（nodejs/node，sparse 取 `lib/`）、
> `/tmp/wjs-deno`（denoland/deno，sparse 取 `ext/node/`），与 §0 同法，
> 仓库内不留第三方源码。

### 4.1 落点总表

| 概念 | node | Bun | deno | winterjs 对位 |
|---|---|---|---|---|
| 模块正文 | `lib/<mod>.js`（CJS） | `src/js/node/<mod>.ts`（TS） | `ext/node/polyfills/<mod>.ts` + `_<mod>.mjs` 实作 | `src/builtins/node/<mod>.rs` 内嵌 ESM（现有模式） |
| 内部件 | `lib/internal/*` | `src/js/internal/*`（同名居多） | 散在 polyfills（`_utils.ts` 等） | 手写小件（validators/ERR 码/队列） |
| native 调用 | `internalBinding`（C++） | `$*` 全局 + `Bun.*` | `op_*`（`ext/node/ops/*.rs`） | natives 经 `jsapi_glue`（现有模式，§6） |
| 防篡改 | `primordials`（宿主注入） | `internal/primordials.js` | `__bootstrap` 快照（IIFE 包裹） | 手写最小表或文档注明（单测防全局污染另算） |
| 装载 | CJS `internal/` | `require("internal/")` | `ext:` scheme | `linkme` 内嵌源（现有模式） |
| 测试资产 | `test/parallel/test-<mod>-*.js` | Node 套件直跑 | `tests/unit_node/` + `tests/node_compat/` | 断言原文黑盒入库（按需单文件取，不整目录拉） |

### 4.2 实例：events 三栏（行数 1256 / 1016 / 1343+17）

- node `lib/events.js`：`primordials` 解构 + `internal/util`、
  `internal/util/inspect`（懒）、`internal/errors`、`internal/validators`、
  `internal/events/abort_listener`，懒 `internal/event_target`、
  `internal/fixed_queue`、`internal/events/symbols`。
- Bun `src/js/node/events.ts`：上表**几乎逐项同名**
 （`internal/validators`、`internal/abort_listener`、`internal/shared`、
  `node:util/types`，懒 `internal/util/inspect`、`internal/fixed_queue`），
  另吃 `node:async_hooks`——“翻译词典”现成：Bun 文件名即 Node 文件名。
- deno `ext/node/polyfills/events.ts`（17 行壳）→ `_<mod>.mjs` 实作，
  IIFE + `__bootstrap`（`core, primordials`）注入。
- winterjs 映射：6 处内部 import 逐一落地——validators/ERR 复用手写件、
  abort 用现有 AbortSignal、shared/fixed_queue 手写约 30 行、
  `util/types` 等 `util`（N1）先行、async_hooks 取 stub（Bun 同款“只 ALS 实”）。

### 4.3 取用优先级

1. **语义原文看 node**（`lib/` 最干净，无宿主私货）。
2. **“离 Node  runtime 之外怎么活”看 Bun**（它已解过一次 internal/ 重映射，
   文件名 1:1，直接当词典）。
3. **分层照抄看 deno**（`op_*` Rust 底 + TS 壳 = winterjs 的
   `jsapi_glue` + prelude 的镜像；`ext/node/ops/` 按模块列 Rust 文件，
   找底座对位最快）。
4. License：三家文件头 MIT（Joyent/Deno/Bun）vendoring 时原样保留。
