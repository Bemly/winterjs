# Phase 9 依赖（纯 Rust）

> 结论先行：9a–9e 所需轮子**全部已在闭包内**（`docs/dependencies.md` 采购单），
> h2 服务端三家（§h2 表）经 `axum` 已在闭包，行级增补即可；
> quic 仅 `quinn` 可用（§quic 表，已拍板未接线）。
> 行级增补（补直引行，版本跟 lock）与真待定一样，动前照样记录+问用户。
> 版本号一律引用 `dependencies.md`，此处不复写（防数字腐烂）。
> 9d 剩余三项（zlib / https-tls / http2）2026-09-12 已拍板（§9d-剩余表）。

## 映射

| 切片 | 需要 | 轮子（已在树内） | 纯度口径 |
|---|---|---|---|
| 9a 纯 JS 先行 | 无（JS + 手写小件） | — | 不涉及 |
| 9b 流与缓冲 | Uint8Array（已有） | —（`stream` 纯 JS 整批映射） | 不涉及 |
| 9c fs | 文件 IO/错误码 | `fs-err` + std + `tokio` fs（§3/§8） | ✅ 全纯 Rust |
| 9d net/dns | TCP/UDP/lookup | `tokio` net + `hickory-resolver`（§3/§6） | ✅ 全纯 Rust |
| 9d http/https | 客户端/服务端/路由 | `reqwest` + `axum` + `tower`（§2 门控沿用） | ✅（TLS 豁免沿用 §12） |
| 9d http2 | h2 服务端 | `hyper`（`server`+`http2`，§h2 表；2026-09-12 已行级直引接线） | ✅ 全纯 Rust |
| 9d tls | 服务端 TLS | `tokio-rustls` + `rustls-pemfile`（§2/§10） | ✅（TLS 豁免沿用 §12） |
| 9d dgram/zlib | UDP/压缩 | `tokio` net + `flate2`/`brotli`/`ruzstd`（§6） | ✅（默认后端口径沿用） |
| 9e crypto | 非对称/杂凑差集 | RustCrypto 全家 + `rcgen`（§7，门控沿用） | ✅ 全纯 Rust |
| 9e child IPC | 进程/管道/组杀 | `tokio` process + `nix`（§3/§8） | ✅ 全纯 Rust |
| 9e perf/inspector | 观测/调试 | `tracing` + `metrics`（§3/§10） | ✅ 全纯 Rust |
| 9f vm/worker | compartment/线程 | std thread + `crossbeam`（§3，在树内；设计另议） | ✅ 全纯 Rust |
| 9f quic | QUIC | 仅 `quinn` 可用（§quic 表；**已拍板未接线**，v1 不验收） | 待定项 |
| 行级增补（闭包已有） | 直接 `use` 传递依赖时 | `idna`（`url` 带入；`punycode` 用）、`hyper`/`h2`（`axum` 带入；h2 服务端，§h2 表）、`flate2`/`brotli`/`ruzstd`/`tokio-rustls`/`rustls-pemfile`（§9d-剩余表，已拍板） | 版本跟 lock，补行即变更，走 §0.5 |

## 9e 落地注记（2026-09-12，零新 crate）

> 9e 全程未改 `Cargo.toml`/`Cargo.lock`（`git diff` 为空）：对称（`cbc`/`ctr`/
> `aes-gcm`/`chacha20poly1305`）、KDF（`hkdf`/`pbkdf2`/`scrypt`/`argon2`）、
> 非对称（`rsa 0.9` + `sha2_010` 改名直引 + `md-5`）、解析（`x509-cert`）、
> 随机（`getrandom`/`rand`）全在 §7 闭包内；perf_hooks/inspector 纯 JS + 既有
> `tracing`/`metrics`；child 角落复用既有 `tokio` process。
> 版本墙（digest 0.10 双轨 + `hmac 0.13`/`sha3 0.12` traits 互斥）走手写档解决，
> 未引 `sha1_010`（§0.5 问用户前置未触发），详 AGENTS §4.43。

## 9d-剩余三项拍板表（zlib / https-tls / http2，2026-09-12 已拍板）

> 三项轮子**全部已在闭包**（版本号/建库/维护/纯度/矩阵照抄 `dependencies.md` §6/§10
> 与本文件 §h2 表，`Cargo.lock` 实测在列），零新 crate；
> `flate2`/`brotli`/`ruzstd`/`tokio-rustls`/`rustls-pemfile` 已是 `Cargo.toml`
> 直引（§2/§6/§10 门控沿用），`node:` 侧直接 `use` 无需增补；
> `hyper`/`h2` 经 `axum` 在闭包；`node:http2` 已于 2026-09-12 行级增补接线
> （`hyper = { version = "1", default-features = false, features = ["server", "http2"] }`，
> 版本跟 lock 1.11.1；`h2` 未直引，条件 fallback 保留）。

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 压缩（zlib：gzip/deflate） | `flate2` | 1.1.10 | 2014-11-11 | 2026-08-28 | ✅（默认后端） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 压缩（zlib：brotli） | `brotli` | 9.0.0 | 2015-11-30 | 2026-09-02 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| zstd 编解码（serve 已用，zlib 备选） | `ruzstd` | 0.9.0 | 2019-11-04 | 2026-07-26 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 服务端 TLS（https） | `tokio-rustls` | 0.26.5 | 2017-02-22 | 2026-09-04 | ✅（TLS 豁免） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| PEM 加载（https 证书） | `rustls-pemfile` | 2.2.0 | 2020-12-28 | 2024-09-30 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| h2 服务端引擎（http2 必选，见 §h2 表） | `hyper` | 1.11.1 | 2014-11-22 | 2026-08-28 | ✅（特性门控见备注） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| h2 直驱（http2 条件 fallback，见 §h2 表） | `h2` | 0.4.19 | 2017-03-09 | 2026-08-24 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

备注：zlib 对应 `node:zlib`（gzip/gunzip/deflate/inflate + brotli；`flate2` 默认
`miniz_oxide` 纯 Rust 后端，禁 `zlib`/`zlib-ng`，§2 门控沿用）；
https-tls 对应 `node:https` + `node:tls` 服务端（`tokio-rustls` 用法
`default-features=false` + `ring`，禁 aws-lc，§2 门控沿用；PEM 只收
`rustls-pemfile` + `x509-cert` 口径，PFX 不支持）；
http2 定夺沿用 §h2 表（`hyper` 必选，用法 `default-features=false` +
`server,http2`，禁 `client`/`ffi`/`capi`；`h2` 仅条件 fallback）。
`httparse`（§h2 表保底件）https/http2 用不上，不列入本表。

## h2 服务端候选（9d，2026-09-12 实测入库，已拍板：`hyper` 必选）

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
定夺时照抄；`MSRV 1.63` 远低于本仓。
定夺（2026-09-12 用户拍板）：**`hyper` 必选**；`h2` 仅当 `hyper` 的 server
语义与 `node:http2` 兼容细节冲突、需手管连接/流控时启用；`httparse` 仅当前
两者都不行时启用（手写基线）。三者中 `hyper`/`h2` 已在闭包，行级增补即可。
`httparse`（零依赖/`no_std`，SIMD 仅 cfg 检测）是手写 h2/h1 解析的保底件，
 非首选。`axum` 组合已在树内但它是 Web 框架层，做 `node:http2` 外形仍需
 二选一引擎，故 devoted 表只列引擎。

## quic 候选（9f，2026-09-12 实测入库，已拍板未接线：`quinn` 必选，v1 不验收）

> 仅 `quinn` 过门：默认特性即 ring（`rustls-ring`，aws-lc 全是 opt-in），
> 依赖全纯 Rust/树内（bytes/rustc-hash/pin-project-lite/thiserror/tracing/
> tokio/socket2 + `ring` 豁免）；`platform-verifier` 对应 `rustls-platform-verifier`
> （已在树内）。用法 `default-features=false` +
> `runtime-tokio,rustls-ring`（禁 `rustls-aws-lc-rs*`），§2 同款门控。

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| QUIC 传输 | `quinn` | 0.11.11 | 2018-10-02 | 2026-06-22 | ✅（§2 门控，默认 ring） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| P2P QUIC（层错配） | `iroh` | 1.2.0 | 2022-03-10 | 2026-09-09 | ✅（默认 ring） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

备注：`s2n-quic` 默认 TLS 走 `s2n-tls`（C），`provider-tls-rustls` 切过去后
其 `s2n-quic-rustls` 仍硬绑 `rustls aws-lc-rs`（manifest 实测），双路皆撞 §2，
出局；另 `rust-version 1.92` + provider 重型体系亦减分。`iroh` 本体纯 Rust
但它是 P2P 栈（公钥拨号/中继/打洞），`node:quic` 要的是裸传输——真用只会
取其内层 `noq`（quinn fork），不如直引 `quinn`，故不引。
`msquic` 另撞两条：beta + 2025-07 后无维护（库龄规则双杀）。
定夺（2026-09-12 用户拍板）：**`quinn` 必选**（用法
`default-features=false` + `runtime-tokio,rustls-ring,ring`，
禁 `rustls-aws-lc-rs*`；`optional` 经 `--features quinn` 按需，默认零成本，
`cap-std` 同款）；其余 quic 一律不要。
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
