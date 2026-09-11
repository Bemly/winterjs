# Phase 9 依赖（纯 Rust）

> 结论先行：9a–9e 所需轮子**全部已在闭包内**（`docs/dependencies.md` 采购单），
> **无新第三方代码进构建图**。但有两类要说清（下表末两节）：
> ① **行级增补**：闭包里已有、但 `Cargo.toml` 没直引的（如 `idna`），
> 用到时必须补直引行（版本跟 lock 走）——算变更，动前照样记录+问用户；
> ② **真待定**：h2 服务端选型、quic，动前按 AGENTS §0.5 走
> （找轮子 → 记入本文档 → 停下问用户）。
> 版本号一律引用 `dependencies.md`，此处不复写（防数字腐烂）。

## 映射

| 切片 | 需要 | 轮子（已在树内） | 纯度口径 |
|---|---|---|---|
| 9a 纯 JS 先行 | 无（JS + 手写小件） | — | 不涉及 |
| 9b 流与缓冲 | Uint8Array（已有） | —（`stream` 纯 JS 整批映射） | 不涉及 |
| 9c fs | 文件 IO/错误码 | `fs-err` + std + `tokio` fs（§3/§8） | ✅ 全纯 Rust |
| 9d net/dns | TCP/UDP/lookup | `tokio` net + `hickory-resolver`（§3/§6） | ✅ 全纯 Rust |
| 9d http/https | 客户端/服务端/路由 | `reqwest` + `axum` + `tower`（§2 门控沿用） | ✅（TLS 豁免沿用 §12） |
| 9d http2 | h2 | `reqwest` http2（客户端已有）；**服务端实现选型待定**（开工定） | 待定项，不先引 |
| 9d tls | 服务端 TLS | `tokio-rustls` + `rustls-pemfile`（§2/§10） | ✅（TLS 豁免沿用 §12） |
| 9d dgram/zlib | UDP/压缩 | `tokio` net + `flate2`/`brotli`/`ruzstd`（§6） | ✅（默认后端口径沿用） |
| 9e crypto | 非对称/杂凑差集 | RustCrypto 全家 + `rcgen`（§7，门控沿用） | ✅ 全纯 Rust |
| 9e child IPC | 进程/管道/组杀 | `tokio` process + `nix`（§3/§8） | ✅ 全纯 Rust |
| 9e perf/inspector | 观测/调试 | `tracing` + `metrics`（§3/§10） | ✅ 全纯 Rust |
| 9f vm/worker | compartment/线程 | std thread + `crossbeam`（§3，在树内；设计另议） | ✅ 全纯 Rust |
| 9f quic | QUIC | **待定**（候选 `quinn`，未验：库龄/维护/纯度四问没跑，**不引入**） | 待定项 |
| 行级增补（闭包已有） | 直接 `use` 传递依赖时 | `idna`（`url` 带入；`punycode` 用）、`hyper`（`axum` 带入；h2 服务端候选之一） | 版本跟 lock，补行即变更，走 §0.5 |

## 测试资产（非依赖，仓库外按需取）

- Node：`test/parallel/test-<mod>-*.js`（`/tmp/wjs-node` sparse，单文件取）。
- Deno：`tests/unit_node/`（`/tmp/wjs-deno` sparse，按需）。
- Bun：Node 套件直跑（无自有断言资产，不取）。

## 审计口径

- 本表无 C/C++ 新增；TLS 链 `ring` 豁免沿用 §12；`mozjs` 钉死铁律不变。
- 若开工时发现缺轮子（如 h2 服务端、quic）：跑新增四问
 （库龄超一年/近一年维护/传递闭包纯 Rust/不要 nightly），记入本表 §映射，
 然后停下问用户——与 §15 同纪律。
