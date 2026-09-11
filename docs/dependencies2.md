# Phase 9 依赖（纯 Rust）

> 结论先行：9a–9e 所需轮子**全部已在闭包内**（`docs/dependencies.md` 采购单），
> h2 服务端三家（§h2 表）经 `axum` 已在闭包，行级增补即可；
> quic 仅 `quinn` 可用（§quic 表，待定夺，未引入）。
> 行级增补（补直引行，版本跟 lock）与真待定一样，动前照样记录+问用户。
> 版本号一律引用 `dependencies.md`，此处不复写（防数字腐烂）。

## 映射

| 切片 | 需要 | 轮子（已在树内） | 纯度口径 |
|---|---|---|---|
| 9a 纯 JS 先行 | 无（JS + 手写小件） | — | 不涉及 |
| 9b 流与缓冲 | Uint8Array（已有） | —（`stream` 纯 JS 整批映射） | 不涉及 |
| 9c fs | 文件 IO/错误码 | `fs-err` + std + `tokio` fs（§3/§8） | ✅ 全纯 Rust |
| 9d net/dns | TCP/UDP/lookup | `tokio` net + `hickory-resolver`（§3/§6） | ✅ 全纯 Rust |
| 9d http/https | 客户端/服务端/路由 | `reqwest` + `axum` + `tower`（§2 门控沿用） | ✅（TLS 豁免沿用 §12） |
| 9d http2 | h2 服务端 | `hyper`（`server`+`http2`）或 `h2` 直驱（§h2 表；双双已在闭包，行级增补） | ✅ 全纯 Rust |
| 9d tls | 服务端 TLS | `tokio-rustls` + `rustls-pemfile`（§2/§10） | ✅（TLS 豁免沿用 §12） |
| 9d dgram/zlib | UDP/压缩 | `tokio` net + `flate2`/`brotli`/`ruzstd`（§6） | ✅（默认后端口径沿用） |
| 9e crypto | 非对称/杂凑差集 | RustCrypto 全家 + `rcgen`（§7，门控沿用） | ✅ 全纯 Rust |
| 9e child IPC | 进程/管道/组杀 | `tokio` process + `nix`（§3/§8） | ✅ 全纯 Rust |
| 9e perf/inspector | 观测/调试 | `tracing` + `metrics`（§3/§10） | ✅ 全纯 Rust |
| 9f vm/worker | compartment/线程 | std thread + `crossbeam`（§3，在树内；设计另议） | ✅ 全纯 Rust |
| 9f quic | QUIC | 仅 `quinn` 可用（§quic 表；**待定夺，未引入**） | 待定项 |
| 行级增补（闭包已有） | 直接 `use` 传递依赖时 | `idna`（`url` 带入；`punycode` 用）、`hyper`/`h2`（`axum` 带入；h2 服务端，§h2 表） | 版本跟 lock，补行即变更，走 §0.5 |

## h2 服务端候选（9d，2026-09-12 实测入库，待定夺）

> 三家全纯 Rust且**全已在闭包**（`hyper 1.11.1` + `h2 0.4.19` 经 `axum` 入图，
> `Cargo.lock` 实测在列），h2 无任何新 crate 需求；`hyper` 按需特性
> `server`+`http2`（默认全关，门控写法见下）。

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| h2 服务端引擎 | `hyper` | 1.11.1 | 2014-11-22 | 2026-08-28 | ✅（特性门控见备注） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| h2 直驱（无高层封装） | `h2` | 0.4.19 | 2017-03-09 | 2026-08-24 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| HTTP/1 解析器（手写基线件） | `httparse` | 1.10.1 | 2015-02-20 | 2025-03-03 | ✅（零依赖，冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

备注：`hyper` 用法 `default-features=false` + `server,http2`
（`http2` 拉 `h2`；禁 `client`/`ffi`/`capi`；`tracing` 按需）——§2 同款门控写法，
定夺时照抄；`MSRV 1.63` 远低于本仓。`h2` 直驱要手管连接/流控，
 只在 `hyper` 的 server 语义与 `node:http2` 兼容细节冲突时 fallback。
 `httparse`（零依赖/`no_std`，SIMD 仅 cfg 检测）是手写 h2/h1 解析的保底件，
 非首选。`axum` 组合已在树内但它是 Web 框架层，做 `node:http2` 外形仍需
 二选一引擎，故 devoted 表只列引擎。

## quic 候选（9f，2026-09-12 实测入库，待定夺）

> 仅 `quinn` 过门：默认特性即 ring（`rustls-ring`，aws-lc 全是 opt-in），
> 依赖全纯 Rust/树内（bytes/rustc-hash/pin-project-lite/thiserror/tracing/
> tokio/socket2 + `ring` 豁免）；`platform-verifier` 对应 `rustls-platform-verifier`
> （已在树内）。用法 `default-features=false` +
> `runtime-tokio,rustls-ring`（禁 `rustls-aws-lc-rs*`），§2 同款门控。

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| QUIC 传输 | `quinn` | 0.11.11 | 2018-10-02 | 2026-06-22 | ✅（§2 门控，默认 ring） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| QUIC 传输（备选） | `s2n-quic` | 1.88.0 | 2022-02-16 | 2026-08-21 | ❌（双路皆禁，见备注） | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| P2P QUIC（层错配） | `iroh` | 1.2.0 | 2022-03-10 | 2026-09-09 | ✅（默认 ring） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| QUIC（C，禁） | `quiche` | 0.29.3 | 2019-01-24 | 2026-07-14 | ❌（BoringSSL C++/cmake） | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| QUIC（无 crate，禁） | `neqo` | —（`neqo`/`neqo-common` 查无此包） | — | — | ❌（NSS C） | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| QUIC（C 绑定，禁） | `msquic` | 2.5.1-beta | 2021-07-18 | 2025-07-11 | ❌（平台 C 库绑定） | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |

备注：`s2n-quic` 默认 TLS 走 `s2n-tls`（C），`provider-tls-rustls` 切过去后
其 `s2n-quic-rustls` 仍硬绑 `rustls aws-lc-rs`（manifest 实测），双路皆撞 §2，
出局；另 `rust-version 1.92` + provider 重型体系亦减分。`iroh` 本体纯 Rust
但它是 P2P 栈（公钥拨号/中继/打洞），`node:quic` 要的是裸传输——真用只会
取其内层 `noq`（quinn fork），不如直引 `quinn`，故不引。
`msquic` 另撞两条：beta + 2025-07 后无维护（库龄规则双杀）。
`neqo` 在 crates.io 无可用包（404 实测）且 NSS 即 C，双杀。

## 测试资产（非依赖，仓库外按需取）

- Node：`test/parallel/test-<mod>-*.js`（`/tmp/wjs-node` sparse，单文件取）。
- Deno：`tests/unit_node/`（`/tmp/wjs-deno` sparse，按需）。
- Bun：Node 套件直跑（无自有断言资产，不取）。

## 审计口径

- 本表无 C/C++ 新增；TLS 链 `ring` 豁免沿用 §12；`mozjs` 钉死铁律不变。
- 若开工时发现缺轮子（如 h2 服务端、quic）：跑新增四问
 （库龄超一年/近一年维护/传递闭包纯 Rust/不要 nightly），记入本表 §映射，
 然后停下问用户——与 §15 同纪律。
