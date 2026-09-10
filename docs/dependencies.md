# winterjs 依赖清单

> 基线日期：2026-09-10（第三轮：ruzstd 全接管 / smmalloc 主分配器 / turso 定版 / TLS 豁免）。
> 入选标准：有轮子不自造；库龄超一年；近一年有维护（冻结型小库：下载量大且功能稳定，标“冻结”后接受）。
> 本文档的 ✅/⚠️/❌ 以实测证据为准（crates.io 依赖扫描、rustc cfg、mozjs release 附件、上游 README/Cargo.toml），
> 证据变化时更新本文档。
> 本文档是“已批准采购单”。
> **2026-09-10 决策（用户拍板）：全量引入**——表上全部依赖一次性进 `Cargo.toml`（含 §2 特性门控、
> 平台 cfg 门控），代码按 Phase 逐个接线使用；§15 原来的“按 Phase 逐批引入”作废。

## 0. 版本方案：CalVer（`YY.MM.PATCH`）

- 本包版本用 CalVer，如 `26.9.0`，首版即 `26.9.0`（2026 年 9 月）。
- Cargo 只接受 semver 格式的字符串，`26.9.0` 恰好合法，可直接写 `version`。
  副作用：`^26.9.0` 按 semver 语义等于 `>=26.9.0, <27.0.0`（年内自动升），
  按年切大版本正好对应，可以接受。
- 第三方依赖保持 semver 原样；`Cargo.lock` 入库。
- 钉版：`mozjs` 用 `=` 精确钉（铁律）；其余 caret + lock，
  逐个 `cargo update -p <包>` 升级，禁全量 `cargo update`。

## 1. 目标矩阵（10 列）

| 短列名 | 完整 triple |
|---|---|
| `macA64` | `aarch64-apple-darwin` |
| `macX64` | `x86_64-apple-darwin` |
| `linA64` | `aarch64-unknown-linux-gnu` |
| `linX64` | `x86_64-unknown-linux-gnu` |
| `winA64` | `aarch64-pc-windows-msvc` |
| `winX64` | `x86_64-pc-windows-msvc` |
| `andA64` | `aarch64-linux-android` |
| `andX64` | `x86_64-linux-android` |
| `ohA64` | `aarch64-unknown-linux-ohos` |
| `ohX64` | `x86_64-unknown-linux-ohos` |

纯度标准（传递闭包口径，先写表里，最后定夺）：
✅=库及全部传递依赖都是纯 Rust；
⚠️=库本身纯 Rust、但传递依赖含其他语言（括号注明）；
❌=完全不是纯 Rust，或该平台明确不支持。
TLS 链已豁免（§12）：经 `rustls` 的 ring（C+asm）标 ✅＋“TLS 豁免”注，不再是 ⚠️ 理由。
WASI（`wasip1/wasip2`）永不进矩阵：各 crate 的 wasi cfg 分支是惰性的，不进 CI 即无影响。
过程宏（derive 类）只跑在 host，天然全 ✅。
nightly 扫描：本表无 nightly-only 依赖（全部 stable 可构建；SIMD 走稳定 `std::arch`；
各 crate 的 nightly opt-in 后端一律不开）。

核心证据（2026-09-09/10 实测）：`rustc --print cfg --target aarch64-unknown-linux-ohos`
给出 `target_os="linux"`、`target_family="unix"`、小端——OHOS 自动命中全生态的
Linux/unix/小端分支。`getrandom` 官方支持表行 `*-linux-*` 覆盖 OHOS；
`tokio`/`mio` 官方保证 Linux/macOS/Windows/FreeBSD/iOS/Android（API 21+），OHOS 走同一 epoll 路径；
`mozjs-sys 153.0.0-1` 的 release 附件自带全套预构建包。

## 2. 特性门控铁律（配错 feature 把 ✅ 变成 ⚠️/❌，由引入人负责在 CI 矩阵里证明）

> 2026-09-10 按实测修订（两轮）：第一轮批量引入时逐个核过上游 manifest 的 `[features]`，
> 修正 reqwest 0.13 特性改名、vergen 拆包两处失效描述，新增 self_update / sentry /
> object_store / instant-acme / metrics-exporter-prometheus 五处"便捷特性硬绑违禁后端"的门控；
> 第二轮对全部原文门控做了 manifest 级复核（`links` 键 / `[build-dependencies]` /
> build.rs 内容三层口径，config 另做 37-crate 闭包穷尽审计），**判决：7 条准确、2 条过时已修
> （reqwest/vergen）、1 条乌龙（config 的 yaml）**。实测口径：`cargo tree -i aws-lc-rs /
> native-tls / openssl` 必须为空（已抽查 macA64/winX64/andA64/ohA64，其余目标由 CI 转正证明）。

- `reqwest`（0.13）：`default-features=false` + `rustls-no-provider` + 需要的协议特性
  （`http2/charset/json/stream/gzip/brotli/deflate`）。0.13 删了旧的 `rustls-tls` 特性；
  `rustls` 特性硬绑 aws-lc-rs，禁；`native-tls*` 禁（→`openssl-sys`，Linux 要装
  OpenSSL，Windows 走 VCPKG 地狱）；`zstd` 禁（C 后端）。TLS provider 由顶层
  `rustls`（ring）提供，TLS 首次使用前须 install_default（Phase 3 接线时落实）。
- `rustls` / `tokio-rustls`：`default-features=false` + `ring`——0.23 / 0.26 的默认
  provider 是 aws-lc-rs，必须显式换掉。
- `config`：**yaml 判乌龙并启用**（2026-09-10 用户拍板）。原文"禁 yaml（→`serde_yaml`→
  `unsafe-libyaml` 的 C）"两头不成立：0.15 的 `yaml = ["dep:yaml-rust2"]`（纯 Rust，与 §3
  直引同库，config 钉 ^0.11 故与直引 0.12 双版本共存——已接受）；且 `unsafe-libyaml` 本身是
  libyaml 的 Rust 转写，非 C。37-crate 启用闭包穷尽审计（links/build-deps/build.rs 三层）
  无任何 C。TOML/JSON/INI/YAML。
  0.15 没有 `env` 特性（`Environment` 源内建）；嵌套 env 键要显式 `.prefix_separator("_")`，
  否则跟随 `separator`（AGENTS.md §4.4）。
- `cookie_store`：PSL 特性名是 `public_suffix`（纯 Rust 数据表，已在 default 里），
  不是 `publicsuffix`。
- `tower-http`：只开 `fs/cors/compression-gzip,br/trace`；禁 `compression-zstd`。
- `async-compression`：`default-features=false` + `gzip/br/deflate`。
  若将来开 `bzip2`：compression-codecs 用的 0.6.1 默认已是纯 Rust `libbz2-rs-sys`
  （C 是 `bzip2-sys` 特性 opt-in），届时禁它只剩"npm 不用"的范围策略，非纯度。
- `zip`：`default-features=false` + `deflate`（自带 `deflate-zopfli`，zopfli 0.8 纯 Rust，
  已实测在 lock 内）。`bzip2` 禁令**对 zip 成立**：zip 2.4.2 的可选 bzip2 是 ^0.5 线，
  **默认 C 后端**（`bzip2-sys`），纯 Rust 要显式开 `libbz2-rs-sys`（0.6 起才默认纯，
  届时可复议）。
- `flate2`：默认特性（`miniz_oxide` 纯 Rust），禁 `zlib`/`zlib-ng`。
- `hickory-resolver`：默认特性（`dnssec-ring` 不开）。
- `turso`：`default-features=false` + `pure-rust-crypto`（禁 `mimalloc`，远程 `sync` 先不开——
  0.6 的 `sync` 特性还硬绑 hyper-tls/native-tls，双保险）。0.7 全线 icu 死锁，2026-09-10
  用户拍板钉 `=0.6.1` 复入构建图（无 icu，实测同图通过、图中无 mimalloc）。
- `russh`：选 ring 后端，禁 `aws-lc`（cmake 重）。
- `rcgen`：默认即 ring 后端，禁 `aws-lc-rs`/`fips` 特性。
- `vergen` → `vergen-gitcl`：10 系起 git 支持拆到独立 crate；build-dependency 引
  `vergen-gitcl`（调 git CLI 取 commit），禁 `git`（→`git2` 的 C）。
- `self_update`：`default-features=false` + `reqwest/archive-zip/compression-zip-deflate`。
  禁它的 `rustls` 特性（映射 reqwest 0.13 的 aws-lc）与 `native-tls`；
  TLS 走全图统一的 `rustls-no-provider` + 顶层 ring。
- `sentry`：禁 `transport`（硬绑 native-tls）；开
  `backtrace/contexts/panic/reqwest/rustls-no-provider`，Transport 到 Phase 8 自实现。
- `object_store`：只进默认 `fs`；`http`/`aws`/`azure`/`gcp` 全部硬绑 aws-lc，禁。
- `instant-acme`：`default-features=false` + `ring`；`default` 和 `hyper-rustls`
  特性都拖 aws-lc，禁。
- `metrics-exporter-prometheus`：`default-features=false` + `http-listener`；
  `push-gateway` 硬绑 `hyper-rustls/aws-lc-rs`，禁。
- 审计附记（lock 内的"意外住客"，均实测不违规）：`openssl-probe` 0.2.1（经
  `rustls-native-certs` ← platform-verifier 进来）0 依赖/无 links，纯 Rust 的证书路径探测，
  不链 OpenSSL；`jni` 0.22 / `ndk-context`（hickory `system-config` 的 android 分支）
  纯 Rust（jni 无 links，build.rs 只设 cfg 标志），桌面构建惰性；`libz-sys`（links=z，C）
  唯一来源是 `mozjs_sys` 引擎自身（§12 特许），不是 flate2 引入。
- `gluesql` 系已移除（§9 改 turso），`sled` 存储不选。

## 3. Phase 0 — 底座：引擎/错误/日志/异步/序列化/内存/并发原语/二进制

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| JS 引擎 | `mozjs` | 0.26.0 | 2012 | 2026-09-06 | ⚠️（C++ 引擎，唯一特许） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| CLI | `clap` | 4.6.6 | 2015-03-01 | 2026-08-06 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 错误冒泡 | `anyhow` | 1.0.104 | 2019-10-05 | 2026-07-18 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 错误类型 | `thiserror` | 2.0.20 | 2019-10-09 | 2026-08-08 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 样板消除 | `derive_more` | 2.1.1 | 2016-03-28 | 2025-12-22 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 枚举映射 | `strum` | 0.28.0 | 2017-02-12 | 2026-02-22 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Builder 生成 | `derive_builder` | 0.20.2 | 2016-08-07 | 2024-10-08 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 位标志 | `bitflags` | 2.13.1 | 2015-01-15 | 2026-07-15 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 日志 | `tracing` | 0.1.44 | 2017-11-27 | 2025-12-18 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 日志输出 | `tracing-subscriber` | 0.3.23 | 2019-06-27 | 2026-03-13 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 日志落盘 | `tracing-appender` | 0.2.5 | 2020-05-05 | 2026-04-17 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 日志桥接 | `tracing-log` | 0.2.0 | 2019-06-27 | 2023-10-25 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 错误 span | `tracing-error` | 0.2.1 | 2020-02-05 | 2024-11-29 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 漂亮报错 | `miette` | 7.6.0 | 2021-08-03 | 2025-04-27 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| panic 美化 | `human-panic` | 2.0.8 | 2018-04-15 | 2026-04-02 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 默认分配器 | system（std） | — | — | — | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 主分配器 | `smmalloc` | 7.6.13 | 2026-01-02 | 2026-08-08 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ |
| 分配器回退 | `talc` | 5.1.1 | 2023-07-21 | 2026-09-09 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 异步 | `tokio` | 1.53.1 | 2016-07-01 | 2026-07-20 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 异步组合子 | `futures` | 0.3.34 | 2016-07-31 | 2026-08-11 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 异步工具 | `tokio-util` | 0.7.19 | 2018-02-01 | 2026-07-21 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Stream 宏 | `async-stream` | 0.3.6 | 2019-06-07 | 2024-10-01 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 自实现订阅 | `pin-project-lite` | 0.2.17 | 2019-10-22 | 2026-02-27 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 线程消息 | `crossbeam-channel` | 0.5.17 | 2017-11-26 | 2026-09-05 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 作用域并发 | `crossbeam-utils` | 0.8.23 | 2017-08-28 | 2026-09-05 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 数据并行 | `rayon` | 1.12.0 | 2015-12-10 | 2026-04-14 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 快速锁 | `parking_lot` | 0.12.5 | 2016-05-13 | 2025-10-03 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 并发表 | `dashmap` | 6.2.1 | 2019-08-25 | 2026-05-17 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| LRU 缓存 | `lru` | 0.18.4 | 2016-12-31 | 2026-09-03 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 保序表 | `indexmap` | 2.14.2 | 2018-01-30 | 2026-09-05 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Fx 哈希 | `rustc-hash` | 2.1.3 | 2018-05-24 | 2026-07-03 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 小向量 | `smallvec` | 1.16.0 | 2015-04-06 | 长期维护 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 小字符串 | `smol_str` | 0.3.6 | 2018-08-16 | 2026-03-04 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| UTF-8 路径 | `camino` | 1.2.5 | 2021-02-23 | 2026-07-28 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 字节缓冲 | `bytes` | 1.12.1 | 2015-01-30 | 2026-07-08 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| HTTP 类型 | `http` | 1.5.0 | 2014-11-20 | 2026-07-29 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Body trait | `http-body` | 1.1.0 | 2019-04-04 | 2026-07-13 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Body 工具 | `http-body-util` | 0.1.5 | 2022-10-25 | 2026-08-12 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 序列化 | `serde` | 1.0.229 | 2014-12-05 | 2026-07-18 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| JSON | `serde_json` | 1.0.151 | 2015-08-07 | 2026-07-20 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| JSON 快路径（可选） | `simd-json` | 0.18.1 | 2019-04-15 | 2026-08-23 | ✅（intrinsics） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 统一配置 | `config` | 0.15.25 | 2015-04-16 | 2026-06-26 | ✅（yaml 已审计启用） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| YAML 按需 | `yaml-rust2` | 0.12.0 | 2024-02-08 | 2026-08-18 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 配置 schema | `schemars` | 1.2.2 | 2019-08-08 | 2026-07-27 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 内容哈希 | `blake3` | 1.8.7 | 2019-09-17 | 2026-08-20 | ✅（SIMD） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 非加密哈希 | `xxhash-rust` | 0.8.18 | 2020-10-15 | 2026-07-21 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 十六进制 | `const-hex` | 1.19.1 | 2023-05-01 | 2026-05-23 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 零拷贝解析 | `zerocopy` | 0.8.57 | 2018-08-15 | 2026-09-08 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 二进制格式 | `binrw` | 0.15.2 | 2020-09-12 | 2026-07-23 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 缓存序列化 | `postcard` | 1.1.3 | 2019-04-03 | 2025-07-24 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 字节数组 | `serde_bytes` | 0.11.19 | 2017-04-08 | 2025-09-15 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 错误路径 | `serde_path_to_error` | 0.1.20 | 2019-01-07 | 2025-09-15 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| trait 克隆 | `dyn-clone` | 1.0.20 | 2019-12-23 | 2025-07-27 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 自引用结构 | `ouroboros` | 0.18.5 | 2020-09-20 | 2025-01-11 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 资源表 | `slotmap` | 1.1.1 | 2018-07-02 | 2025-12-06 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 热配置 | `arc-swap` | 1.9.2 | 2018-04-16 | 2026-06-28 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 迭代器 | `itertools` | 0.15.0 | 2014-11-21 | 2026-06-16 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 大文件映射 | `memmap2` | 0.9.11 | 2020-01-18 | 2026-06-22 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |

备注：分配器默认 system（零依赖零风险）；`smmalloc`（zooko，406 行，`cargo add smmalloc --rename smalloc`）
是桌面端主选项，移动端无支持代码 → 回退 `talc`（§16 有四条风险待定夺）。
注意：crates.io 上的 `smalloc` 0.1.2 是同名异物，禁引，只能用 `smmalloc`。
`dashmap` 用 6 系（7 在 rc）；`smallvec` 用 1 系（2 在 alpha）；`zerocopy` 用 0.8 系（0.9 在 alpha）。
`simd-json` x86_64 用 AVX2/SSE4.2、aarch64 用 NEON，其余标量回退。
`memmap2` 的 OHOS 格待验证。`config` 替代已停更的 `dotenvy`。
`yaml-rust2` 直引 0.12 作独立 YAML 按需解析；config 的 yaml 特性另带 ^0.11（同库
双版本共存，2026-09-10 用户拍板接受；两者皆纯 Rust，整树审计过）。
`binrw` 写 bundle trailer 等二进制格式；`postcard` 写缓存 blob；`zerocopy` 做零拷贝解析。

## 4. CLI / 终端 / 自升级 / 脚手架

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 补全脚本 | `clap_complete` | 4.6.9 | 2021-12-31 | 2026-08-06 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| man 手册 | `clap_mangen` | 0.3.3 | 2022-02-08 | 2026-08-12 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 版本信息 | `vergen` | 10.0.3 | 2015-02-12 | 2026-08-24 | ✅（§2 门控） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 终端样式 | `console` | 0.16.4 | 2017-05-09 | 2026-07-01 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 进度条 | `indicatif` | 0.18.6 | 2017-04-26 | 2026-07-01 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 交互问答 | `dialoguer` | 0.12.0 | 2017-05-11 | 2025-08-23 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| REPL 行编辑 | `rustyline` | 18.0.1 | 2015-09-05 | 2026-06-24 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| shell 切词 | `shlex` | 2.0.1 | 2015-06-22 | 2026-05-17 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 波浪线展开 | `shellexpand` | 3.1.2 | 2016-03-13 | 2026-02-23 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 终端宽度 | `unicode-width` | 0.2.2 | 2015-04-14 | 2025-10-06 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 时长参数 | `humantime` | 2.4.0 | 2016-05-20 | 2026-07-02 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 容量参数 | `bytesize` | 2.7.0 | 2015-04-19 | 2026-08-02 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 初始化模板 | `askama` | 0.16.1 | 2017-02-15 | 2026-09-04 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 自升级 | `self_update` | 1.3.0 | 2017-07-25 | 2026-09-02 | ✅（TLS 豁免） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |

备注：`rustyline`/`self_update` 的移动端格是“功能不需要”（REPL/自升级是桌面功能，
模块可 cfg 门控），不是编不过。`vergen` 开 `gitcl`（调 git CLI），禁 `git`（→`git2` 的 C）。
`shlex` 做 `bunx` 式参数透传的 shell 切词；`humantime`/`bytesize` 解析 `--timeout 30s`/`--max-old-space 512MB`；
`askama`（编译期模板）做 `winterjs init` 脚手架。

## 5. loader + 转译 + 编译期注册 + 内嵌资源

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 模块寻址 | `oxc_resolver` | 11.24.3 | 2023-09-04 | 2026-08-24 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 转译＋压缩 | `oxc` | 0.149.0 | 2023-07-06 | 2026-09-07 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| tsconfig 解析 | `jsonc-parser` | 0.33.1 | 2020-04-22 | 2026-07-26 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 堆栈映射 | `sourcemap` | 9.3.2 | 2016-06-05 | 2026-01-20 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 内建注册表 | `linkme` | 0.3.37 | 2019-01-27 | 2026-07-18 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 宏拼接 | `paste` | 1.0.15 | 2018-11-01 | 2024-05-07 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 完美哈希 | `phf` | 0.14.0 | 2014-11-22 | 2026-06-21 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 模块图 | `petgraph` | 0.8.3 | 2015-01-11 | 2025-09-30 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 标识符表 | `string-interner` | 0.20.0 | 2017-02-06 | 2026-04-30 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 保格式编辑 | `toml_edit` | 0.25.13 | 2017-12-17 | 2026-07-14 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| target 解析 | `target-lexicon` | 0.13.5 | 2018-05-24 | 2026-02-15 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 内嵌 builtin | `include_dir` | 0.7.4 | 2017-06-07 | 2024-06-17 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

备注：`oxc_resolver` 的 `rustix` 门控是 `cfg(any(macos, linux))`，
OHOS 因 `target_os="linux"` 命中同一分支；`simd-json` 加速门控只看小端，10 列全小端。
`linkme` 做 builtin 分布式注册（各 builtin 文件自注册，杀掉中央手写清单）；
`phf` 做 op 名/状态码等静态表；`petgraph` 做模块图的环检测＋拓扑序；
`target-lexicon` 给 `--target` 交叉构建参数用；`include_dir` 把 JS builtin 打进二进制。
转译错误经 `miette` 渲染；快照用 `insta`（§11）；`oxc` 系自带 linter/formatter，
`winterjs lint/fmt` 按需开特性，不另引轮子。

## 6. Web API：fetch / 编码 / WebSocket / 重试 / Cookie / TLS 文件

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| fetch client | `reqwest` | 0.13.5 | 2016-10-16 | 2026-09-08 | ✅（TLS 豁免，§2 门控） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| TLS | `rustls`（ring 后端） | 0.23.44 | 2016-08-27 | 0.23 系 | ✅（TLS 豁免） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 系统根证书 | `rustls-platform-verifier` | 0.7.0 | 2024-01-03 | 2026-04-12 | ✅（TLS 豁免） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| 自带根证书 | `rustls-native-certs` | 0.8.4 | 2019-11-04 | 2026-06-01 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| PEM 加载 | `rustls-pemfile` | 2.2.0 | 2020-12-28 | 2024-09-30 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 可选 DNS | `hickory-resolver` | 0.26.2 | 2023-09-26 | 2026-09-03 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 重试退避 | `backon` | 1.6.0 | 2022-04-12 | 2025-10-18 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| URL | `url` | 2.5.8 | 2014-11-14 | 2026-01-05 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| data: URL | `data-url` | 0.3.2 | 2018-02-02 | 2025-08-21 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| base64 | `base64` | 0.23.1 | 2015-12-04 | 2026-08-04 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 百分号编码 | `percent-encoding` | 2.3.2 | 2017-06-13 | 2025-08-21 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 表单编码 | `form_urlencoded` | 1.2.2 | 2020-06-19 | 2025-08-21 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 表单解码 | `serde_urlencoded` | 0.7.1 | 2016-09-11 | 2022-01-17 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 文本编解码 | `encoding_rs` | 0.8.41 | 2016-07-09 | 2026-09-09 | ✅（内含 SIMD） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Cookie jar | `cookie_store` | 0.22.1 | 2019-01-10 | 2026-02-16 | ✅（+PSL 特性） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| WebSocket | `tokio-tungstenite` | 0.30.0 | 2017-03-17 | 2026-07-11 | ✅＋tokio | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 流式压缩 | `async-compression` | 0.4.46 | 2019-05-14 | 2026-09-09 | ✅（§2 门控） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 压缩 | `flate2` | 1.1.10 | 2014-11-11 | 2026-08-28 | ✅（默认后端） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 压缩 | `brotli` | 9.0.0 | 2015-11-30 | 2026-09-02 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| zstd 编解码 | `ruzstd` | 0.9.0 | 2019-11-04 | 2026-07-26 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| CIDR 匹配 | `ipnet` | 2.12.2 | 2017-08-14 | 2026-09-06 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 正则 | `regex` | 1.13.1 | 2014-12-13 | 2026-07-15 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 内存查找 | `memchr` | 2.8.3 | 2015-06-11 | 2026-07-08 | ✅（SIMD） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

备注：`ruzstd` 经 GitHub README 核实是**完整** zstd 实现（decode 全量＋encode 五档：
`Fastest`≈level1、`Default`≈3、`Better`≈7、`Best`≈11，另有 checksum；字典暂不开）。
解码走 `StreamingDecoder`，编码走 `ruzstd::encoding::{compress,compress_to_vec}`，
serve 静态预压缩用 `Fastest`/`Default`；流式编码按 `FrameEncoder` 在实施时确认。
侦查教训：crates.io 一句话描述（"A decoder…"）是 stale 的，以上游 README 为准。
`cookie_store` 开 `public_suffix` 特性即带 PSL（纯 Rust 数据表，已在 default 里），
无需另引 `psl`。（2026-09-10 勘误：特性名是 `public_suffix`，原文写的 `publicsuffix` 不存在。）
`encoding_rs` x86/x64 多版本 SIMD 分发、aarch64 NEON，其余标量。
移动端根证书策略实施时定（三选一：platform-verifier / 系统 store / 内嵌 webpki-roots）。
`serde_urlencoded` 4 年未动但它是 url 团队的冻结小桥，接受。

## 7. 加密全家（WebCrypto + node:crypto，一次引全，免得逐个踩坑；全员纯 Rust）

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| SHA-1（git/WS/旧） | `sha1` | 0.11.0 | 2014-11-21 | 2026-07-10 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| SHA-2 | `sha2` | 0.11.0 | 2016-05-06 | 2026-03-25 | ✅（硬件加速） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| SHA-3 | `sha3` | 0.12.0 | 2016-10-06 | 2026-05-15 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| BLAKE2 | `blake2` | 0.11.0 | 2016-10-06 | 2026-08-26 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| MD5（兼容旧） | `md-5` | 0.11.0 | 2017-04-06 | 2026-03-27 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| HMAC | `hmac` | 0.13.0 | 2016-10-06 | 2026-03-29 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| HKDF | `hkdf` | 0.13.0 | 2015-01-03 | 2026-03-30 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| PBKDF2 | `pbkdf2` | 0.13.0 | 2017-02-28 | 2026-04-21 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| scrypt | `scrypt` | 0.12.0 | 2016-10-06 | 2026-04-22 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| argon2 | `argon2` | 0.6.0 | 2017-02-28 | 2026-08-27 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| AES 分组 | `aes` | 0.9.3 | 2016-10-06 | 2026-08-28 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| CBC 模式 | `cbc` | 0.2.1 | 2021-04-10 | 2026-05-20 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| CTR 模式 | `ctr` | 0.10.1 | 2018-07-30 | 2026-05-20 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| DES（兼容旧） | `des` | 0.9.0 | 2016-04-24 | 2026-04-10 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Blowfish（旧） | `blowfish` | 0.10.0 | 2016-10-06 | 2026-04-10 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| AES-GCM | `aes-gcm` | 0.11.1 | 2019-08-16 | 2026-08-21 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| ChaCha20-Poly | `chacha20poly1305` | 0.11.0 | 2016-10-06 | 2026-08-05 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Ed25519 | `ed25519-dalek` | 3.0.0 | 2016-12-09 | 2026-07-06 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| X25519 | `x25519-dalek` | 3.0.0 | 2017-09-14 | 2026-07-06 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| P-256 | `p256` | 0.14.0 | 2018-10-03 | 2026-07-03 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| P-384 | `p384` | 0.14.0 | 2018-10-03 | 2026-07-06 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| P-521 | `p521` | 0.14.0 | 2018-10-03 | 2026-07-08 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| secp256k1 | `k256` | 0.14.0 | 2019-12-05 | 2026-07-08 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 大数模幂 | `crypto-bigint` | 0.7.5 | 2021-04-30 | 2026-06-22 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| DSA（旧） | `dsa` | 0.7.0 | 2018-07-13 | 2026-06-30 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| RSA | `rsa`（0.9 系） | 0.9.10 | 2018-07-24 | 长期维护 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| DER 编解码 | `der` | 0.8.2 | 2020-12-17 | 2026-09-05 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| PEM 解析 | `pem-rfc7468` | 1.0.0 | 2021-02-16 | 2025-11-08 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| PKCS#8 | `pkcs8` | 0.11.0 | 2020-06-12 | 2026-04-27 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 公钥结构 | `spki` | 0.8.0 | 2020-12-03 | 2026-04-04 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 证书解析 | `x509-cert` | 0.3.0 | 2022-03-12 | 2026-07-09 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 证书签发 | `rcgen` | 0.14.10 | 2019-01-03 | 2026-08-28 | ⚠️（ring，默认即 ring） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 密钥保密 | `secrecy` | 0.10.3 | 2018-10-04 | 2024-10-09 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 随机数 | `getrandom` | 0.4.3 | 2019-01-19 | 2026-06-17 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 随机数 | `rand` | 0.10.2 | 2015-02-03 | 2026-08-25 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| UUID | `uuid` | 1.26.0 | 2014-11-11 | 2026-08-26 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

备注：全部 RustCrypto / Dalek 系（audited），主版本号各异是常态。
`rsa` 用 0.9 系（0.10 在 rc）；`getrandom` 的 `*-linux-*` 支持行覆盖 OHOS。
`crypto-bigint` 给 `DiffieHellman` 的 MODP 模幂；`rcgen` 默认即 ring 后端（禁 `aws-lc` 系特性）；
serve `--cert` 只收 PEM（`rustls-pemfile`+`x509-cert`），PFX 暂不支持。
其余 legacy 对称算法（rc2/idea/cast5/seed 等）按需再补。
c-4 增补（2026-09-10 用户拍板）：`sha2_010`（sha2 0.10 改名直引，纯 Rust；
`rsa` 0.9 的签名/填充接口绑 `digest` 0.10，直引 sha2 0.11 的类型传不进去，
0.10 已由 `oauth2` 在传递闭包内，双版本先例同 `yaml-rust2` 0.11+0.12）；
`p256`/`p384`/`p521` 显式加 `ecdh,pkcs8` 特性（之前靠 `russh→ssh-key`
传递特性 unify 硬撑，显式声明防上游变卦）。`rsa::rand_core`/`signature`/
`pkcs8` 重导出直用，不另引 `rand_core` 0.6/`signature`（§0.5 零新增）；`ecdsa`
经 `digest` 0.11 与直引 sha2 0.11 互通，用 `sign_prehash`/`verify_prehash`
避开版本面。

## 8. fs / os / 包管理工具链

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 路径规范化 | `dunce` | 1.0.5 | 2017-11-22 | 2024-08-04 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 相对路径 | `pathdiff` | 0.2.3 | 2017-09-20 | 2024-11-25 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 沙箱 join | `normpath` | 1.5.1 | 2020-11-09 | 2026-05-05 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 能力 fs（后期） | `cap-std` | 4.0.3 | 2020-06-25 | 2026-08-20 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| 可执行查找 | `which` | 8.0.6 | 2015-10-06 | 2026-08-26 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 目录遍历 | `walkdir` | 2.5.0 | 2015-09-27 | 2024-03-01 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| glob 展开 | `glob` | 0.3.4 | 2014-11-11 | 2026-07-21 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| glob/ignore | `ignore` | 0.4.33 | 2016-10-30 | 2026-08-04 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| watch | `notify`（钉 8 系） | 8.2.0 | 2014-12-20 | 2025-08-03 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| watch 防抖 | `notify-debouncer-mini` | 0.7.0 | 2022-08-14 | 2025-08-03 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 系统目录 | `dirs` | 7.0.0 | 2015-11-24 | 2026-09-05 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| 系统信息 | `sysinfo` | 0.39.6 | 2015-07-25 | 2026-07-09 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| 网卡地址 | `if-addrs` | 0.15.0 | 2020-08-10 | 2026-02-08 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 网卡 MAC | `mac_address` | 1.1.8 | 2018-04-03 | 2025-02-10 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 用户信息 | `uzers` | 0.12.2 | 2023-08-21 | 2025-12-16 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| 系统语言 | `sys-locale` | 0.3.2 | 2021-05-13 | 2024-11-01 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| FS 错误增强 | `fs-err` | 3.3.1 | 2020-02-02 | 2026-07-03 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 并行删除 | `remove_dir_all` | 1.0.0 | 2017-03-29 | 2024-11-22 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 时间 | `jiff` | 0.2.35 | 2024-02-17 | 2026-07-25 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| semver | `semver` | 1.0.28 | 2014-11-11 | 2026-04-04 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| npm 范围语法 | `deno_semver` | 0.10.1 | 2023-04-03 | 2026-06-09 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| license 解析 | `spdx` | 0.13.5 | 2019-06-19 | 2026-08-06 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| npmrc 解析 | `rust-ini` | 0.21.3 | 2015-01-26 | 2025-08-30 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| git 依赖 | `gix` | 0.87.1 | 2023-02-10 | 2026-08-24 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| git+ssh（后期） | `russh` | 0.63.3 | 2022-03-13 | 2026-09-09 | ⚠️（ring/aws-lc 二选一，选 ring） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 解包 tgz | `tar` | 0.4.46 | 2014-11-11 | 2026-05-18 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 解包 zip | `zip` | 2.4.2 | 2014-11-21 | 长期维护 | ✅（见备注） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 跨卷拷贝 | `reflink-copy` | 0.1.30 | 2023-07-10 | 2026-06-18 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Win 链接 | `junction` | 2.0.0 | 2019-05-14 | 2026-05-01 | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ |
| 完整性校验 | `ssri` | 9.2.0 | 2019-05-19 | 2023-07-18 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 文件锁 | `fs4` | 1.1.0 | 2021-12-31 | 2026-04-28 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| 时间戳保持 | `filetime` | 0.2.29 | 2015-05-04 | 2026-05-12 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| dlopen（bun:ffi） | `libloading` | 0.9.0 | 2015-11-08 | 2025-11-05 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| unix 调用 | `nix` | 0.31.3 | 2014-11-11 | 2026-05-11 | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ✅ | ✅ | ✅ | ✅ |
| Win 服务 | `windows-service` | 0.8.1 | 2018-06-04 | 2026-05-08 | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ |
| systemd 就绪 | `systemd` | 0.10.1 | 2015-02-27 | 2025-07-19 | ✅ | ❌ | ❌ | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| 开浏览器 | `webbrowser` | 1.2.4 | 2015-12-08 | 2026-08-05 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| OAuth 登录（后期） | `oauth2` | 5.0.0 | 2014-12-16 | 2025-01-21 | ✅（TLS 豁免） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 可选上报（后期） | `sentry` | 0.49.2 | 2016-05-20 | 2026-08-26 | ✅（TLS 豁免，默认关闭零成本） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| 令牌保管（后期） | `keyring` | 4.2.0 | 2016-02-10 | 2026-08-29 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ⚠️ | ⚠️ |
| 远程缓存（后期） | `object_store` | 0.14.1 | 2022-05-13 | 2026-07-15 | ✅（TLS 豁免） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |

备注：`zip` 用 2 系稳定（9 在 pre），且 `default-features=false` 只开 `deflate`，
避开 `bzip2` 的 C 后端。`ssri` 的 SRI 格式冻结，接受。
`dirs`/`sysinfo`/`uzers`/`fs4`/`libloading`/`webbrowser` 的移动端格待真机 CI 转正。
`notify` 钉 `8.2.0`（9 在 rc）；`jiff` 在 OHOS 的时区库路径待验证（UTC 不受影响，
`tzdb` 内嵌特性实施时确认，可解）。
`nix` 是 unix-only（Windows ❌，用 tokio/标准库顶）；`windows-service`/`systemd`
分别是 Windows/Linux 专用行。`gix` 约 50 个子 crate，全纯 Rust，PM 的 git 依赖就它了。
`normpath` 做 `--allow-read` 沙箱前的路径归一；`cap-std` 是权限模型的后期轮子。
`oauth2`（publish 登录）/`sentry`（崩溃上报）/`object_store`（远程缓存）/`russh`（私有仓 SSH）
都是后期，届时再引入。
npmrc 附记（2026-09-10 实测）：`rust-ini` 把 `:` 也当键值分隔符
（`parse_str_until(&[Some('='), Some(':')])`），会从冒号处切断 npmrc 的
`//<host>/:_authToken` 与 `@<scope>:registry` 键，不适合解析 npmrc；
`src/pm/npmrc.rs` 改手写行解析（首个 `=` 切分，约 20 行），`rust-ini`
依赖保留（批准单内其他 INI 场景备用），此处记偏离。

## 9. 内建 DB：turso（定版，其他不用）

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 内建 DB | `turso` | 0.7.2 | 2025-07-01 | 2026-07-30 | ✅（§2 门控） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |

决策记录（2026-09-10，二改）：**钉 `=0.6.1`**（用户拍板）。0.7 全线非可选依赖
`icu_locale ^2.2.0`，与 `mozjs_sys` 钉死的 `icu_capi =2.1.2`（icu_locale ~2.1.1）死锁，
无法同图构建；0.6.1 无 icu，实测通过。0.6 的 `sync` 特性硬绑 hyper-tls（继续不开）。
偏离 caret 政策属刻意钉版（icu 地雷系列），待上游解冲突后回 caret。
落选（无技术否决，只是不选）：`redb`（KV 无 SQL）、`gluesql+redb-storage`（SQL 可但生态小于 turso）、
`fjall`（LSM 备选）、`rusqlite` bundled（含 C）。
`turso` 行名库龄不足一年（2025-07）、移动端（尤其 OHOS）待 CI 转正，见 §16。
出局：`limbo`/`limbo_core`（0.0 版+停更）、`sled`（冻结）。

## 10. serve（HTTP 服务，全量铺开）

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Web 框架 | `axum` | 0.8.9 | 2021-07-22 | 2026-04-14 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 框架扩展 | `axum-extra` | 0.12.6 | 2021-12-02 | 2026-04-14 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 中间件 | `tower` | 0.5.3 | 2016-12-23 | 2026-01-12 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 中间件包 | `tower-http` | 0.7.1 | 2017-03-10 | 2026-08-31 | ✅（§2 门控） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 类型化头 | `headers` | 0.4.1 | 2016-08-09 | 2025-06-02 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 静态类型 | `mime_guess` | 2.0.5 | 2015-07-04 | 2024-06-29 | ✅（冻结表） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Cookie | `cookie` | 0.18.2 | 2014-11-22 | 2026-08-08 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| HTTP 日期 | `httpdate` | 1.0.3 | 2016-10-15 | 2023-08-13 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 服务端 TLS | `tokio-rustls` | 0.26.5 | 2017-02-22 | 2026-09-04 | ✅（TLS 豁免） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 限流 | `governor` | 0.10.4 | 2019-11-15 | 2025-12-16 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 可观测 | `metrics` | 0.24.6 | 2015-09-03 | 2026-05-13 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Prometheus | `metrics-exporter-prometheus` | 0.18.3 | 2020-06-17 | 2026-04-30 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 事件循环观测 | `console-subscriber` | 0.5.0 | 2021-12-16 | 2025-10-30 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 本机 IP | `local-ip-address` | 0.6.13 | 2021-06-15 | 2026-05-19 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| 端口占用 | `netstat2` | 0.11.2 | 2020-02-09 | 2025-08-14 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| 自动证书（后期） | `instant-acme` | 0.8.5 | 2022-05-12 | 2026-02-24 | ✅（TLS 豁免） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ | ⚠️ |
| 局域网二维码 | `qrcode` | 0.14.1 | 2014-11-28 | 2024-07-05 | ✅（冻结） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 手写 SIMD | `wide` | 1.7.0 | 2019-09-21 | 2026-08-27 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

备注：静态文件/CORS/压缩/追踪全用 `tower-http`（压缩后端复用 §6 的 flate2/brotli/ruzstd 解码）；
multipart 表单走 `axum::extract::Multipart`；路由/SSE/JSON/Query 全走 axum 内建。
`mime_guess`/`httpdate`/`qrcode` 是冻结型小库（MIME 表/HTTP 日期/QR 规范不变），接受。
`console-subscriber` 是开发期观测工具（需 `tokio_unstable` cfg）。
`local-ip-address` 的 OHOS 格待验证；`qrcode` 只负责矩阵生成，终端渲染手写约 20 行。

## 11. dev 依赖（只跑在 host，不占 target 矩阵）

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 说明 |
|---|---|---|---|---|---|
| 测试 diff | `similar`（prod 也用，测试 reporter 输出） | 3.2.0 | 2021-01-24 | 2026-08-17 | getsentry 出品 |
| 快照测试 | `insta` | 1.48.0 | 2019-01-13 | 2026-06-11 | 转译输出快照 |
| CLI 测试 | `assert_cmd` | 2.2.2 | 2018-05-28 | 2026-05-11 | 黑盒测 `run/eval` 退出码与输出 |
| FS 测试 | `assert_fs` | 1.1.4 | 2018-05-28 | 2026-05-26 | loader/PM 的临时目录断言 |
| 临时文件 | `tempfile` | 3.27.0 | 2015-04-14 | 2026-03-11 | 同上 |
| 串行测试 | `serial_test` | 4.0.1 | 2018-12-30 | 2026-07-25 | 端口冲突的测试串行化 |
| 参数化测试 | `rstest` | 0.27.0 | 2018-10-14 | 2026-09-06 | resolver 矩阵用例 |
| 断言美化 | `pretty_assertions` | 1.4.1 | 2017-03-26 | 2024-09-15 | 冻结，接受 |
| 基准 | `criterion` | 0.8.2 | 2017-12-02 | 2026-02-04 | transpile/resolve 热路径 |
| 缓存grind | `iai-callgrind` | 0.16.1 | 2023-03-08 | 2025-07-30 | 分配器/转译调优（要装 valgrind） |
| 堆分析 | `dhat` | 0.3.3 | 2020-12-08 | 2024-02-04 | 冻结，配分配器故事 |
| 属性测试 | `proptest` | 1.11.0 | 2017-06-18 | 2026-03-24 | resolver/module-graph 随机用例 |

备注（2026-09-10 实测）：`cargo build` 本仓 0 警告；唯一残留是传递闭包里
`iai-callgrind → proc-macro-error2@2.0.1` 的 future-incompat 报告（E0365，
`pub use proc_macro` 将在未来 Rust 变硬错误）。上游最新即 2.0.1，无修复版可升，
且仅影响 dev 依赖的编译报告、不影响构建成功——接受现状，待上游修后随 caret 自动消。

## 12. 含 C/C++ 依赖说明（特许＋豁免＋门控禁掉之后，只剩注释）

- `mozjs`（C++ 引擎，⚠️ 唯一特许）：预构建，无需本地编译，见 AGENTS.md §6。
- TLS 链已豁免：`ring`（C+asm，经 `rustls`/`rcgen`/`russh` 间接）是 Rust 生态无纯实现环节的最轻选项
  （经 cc、无 cmake，比 `aws-lc` 轻一个量级）。
- `mimalloc`（C）：只出现在 `turso` 默认特性里，§2 门控禁掉，不进构建。
- `zstd`/`rusqlite` bundled/`tikv-jemallocator` 的 C 已随换轮子出局（§3/§6/§9）。

## 13. 剩下必须手写的（轮子到头的地方，刻意保持小）

- mozjs 边界胶水（rooting、`AutoRealm`、FFI 调用，§6 允许的唯一 `unsafe` 区）。
- event-loop drain 编排（microtask/macrotask 调度本身，禁 `&mut` 别名）。
- ESM link/instantiate 编排（resolve 归 `oxc_resolver`，转译归 `oxc`，注册归 `linkme`，
  图算法归 `petgraph`，中间的拼装手写）。
- streams 引擎实现（挂在 mozjs 对象上，无轮子）。
- 测试 reporter 的 JS 侧 harness（输出 diff 复用 `similar`，表格复用 `unicode-width`）。
- REPL 高亮（`oxc` 词法结果直染，约 50 行；不引 `syntect`/`tree-sitter`）。
- npm registry 客户端胶水（传输归 `reqwest`，寻址归 `deno_semver`，校验归 `ssri`，
  缓存布局归 `dirs`+`blake3`+`fs4`，解包归 `tar`+`flate2`，并发归 `futures`+`rayon`）。
- URLPattern 路由匹配（`url` 之上的小状态机；serve 内部路由直接用 axum）。
- SSE（axum 内建）；`bin` 链接（unix symlink + Windows `junction`，约 20 行）；
  子进程树杀掉（`nix` killpg + Windows Job Objects，手写，约 80 行，无可信轮子）；
  Windows CLI 通配符展开（`glob` 之上约 15 行，不引停更的 `wild`）；
  命令回显转义（`shlex::quote` 若缺则手写约 15 行，不引已死的 `shell-escape`）；
  NO_COLOR/TTY 判断（`std::io::IsTerminal`，稳定，无需依赖）；
  Cache API 的 freshness 计算（`headers`+`httpdate` 之上约 80 行）。

## 14. 明确不引（含本轮新筛掉的）

- `dotenvy`（23 年后未更，用 `config` 替）、`swc_core`（选了 oxc）、
  `ring` 直引（跟 rustls 走）、`mime` 直引（`reqwest` 传递依赖里有就够了）、
  `simdutf` 类再绑一份 C++ 的做法（与 §6 冲突）。
- 分配器：`rpmalloc`（21 年后停更）、`wee_alloc`（为体积优化、server 性能错配）、
  `linked_list_allocator`（内核向）、`mimalloc` 直引（C，turso 内已门控禁掉）。
  注意：crates.io 上的 `smalloc` 0.1.2 是同名异物，禁引，只能用 `smmalloc` 并 rename。
- DB 落选不等于否决（只是不选）：`redb`/`gluesql+redb`/`fjall`/`rusqlite`（见 §9）。
  出局：`limbo`（0.0 版+停更）、`turso` 的 pre 线（用 0.7.2 稳定线）、`sled`（冻结）。
- 压缩：`zstd` 本体出局（`ruzstd` 接管解码，编码不支持）；`bzip2`/`xz` 系（npm 不用）。
- `wasmtime`（WASM 由 SpiderMonkey 引擎自己执行，不需要第二个运行时）。
- `openssl`/`native-tls`/`tokio-native-tls`/`hyper-tls`（全线 rustls，躲开 VCPKG/OpenSSL 地狱）。
- `async-trait`（原生 async trait 已稳定）、`tokio-tar`（`spawn_blocking`+`tar` 足够）、
  `fs_extra`（3 年半停更，walkdir+glob+remove_dir_all 顶掉）、
  `wild`（2 年半停更，见 §13 手写 15 行）、`shell-escape`（6 年已死，见 §13）、
  `jsonwebtoken`（用户态需求）、`opentelemetry`（过重，`metrics`+tracing 足够）、
  `daemonize`（容器时代跑前台）、`termimad`（clap+miette 足够）。
- 模板只留 `askama`（`tera`/`handlebars`/`minijinja` 不引）；
  builder 只留 `derive_builder`（`bon` 库龄不够、`typed-builder` 不引）；
  样板三件套（`enum_dispatch`/`auto_impl`/`delegate`）样板量未到阈值时不引；
  `byteorder`（→`binrw`）、`bincode`（→`postcard`，更瘦）、`compact_str`+`lasso`（`smol_str`+`string-interner` 已各选其一）、
  `directories`（用 `dirs`）、`multer`（随 axum 来）、`reedline`（→`rustyline`）、
  `pkcs12`（serve 只收 PEM）、`users`（→`uzers`）、`hex`（→`const-hex`）、
  `tree-sitter*`（→`oxc`）、`syntect`（→手写高亮）、`rental`/`owning_ref`（停更，用 `ouroboros`）、
  `qcell`（aliasing 靠架构纪律，不引 GhostCell）、`redb` 的其他包装（无）。
- 打包器（rolldown 级）与 HTML/CSS 工具链（`lol-html`/`lightningcss`/`html5ever`）：
  Phase 待定，用时再验再入表。

## 15. 验证计划

- CI 矩阵先上 6 个桌面/服务器 triple（有 mozjs 预构建，构建快）；
  android/ohos 列由交叉编译 CI 逐个转正，⚠️ 挂了就地修并回写本表。
- 新增依赖四问：库龄超一年？近一年有维护（冻结型小库看下载量+稳定度）？
  传递闭包全纯 Rust（有 C/C++ 则标 ⚠️+括号，TLS 已豁免）？要 nightly 吗（要就删）？
  任一不过就换方案或记 ⚠️+理由。
- `Cargo.toml` 引入节奏：**2026-09-10 起全量引入**（用户拍板，覆盖旧的“逐批引入”规则）；
  代码按 Phase 接线，“新增依赖四问”继续有效——表外新轮子仍走 §0 规则5（记录→停下问用户）。
- 特性门控（§2）是红线：配错 feature 把 ✅ 变成 ⚠️/❌ 的，由引入人负责在 CI 矩阵里证明。

## 16. 待定夺事项（证据已列，请拍板）

1. `smmalloc` 四风险：① crates.io 首发 2026-01（名下库龄不足一年，965 下载，
   但 GitHub 525 commits 在持续开发）；② 单次 alloc 上限为最大 size class
   （smallest slot 4B × 32 类 ≈ 8 GiB），超限返回空指针**无回退**
   （2026-09-10 实测：33 GiB alloc → null，`tests/alloc_probe.rs --ignored` 已钉案；
   JS 侧超大 ArrayBuffer 建议届时在 JSAPI 边界显式检查或走回退分配器）；③ 作者明示无安全加固；
   ④ 移动端无支持代码（已定 talc 回退）。License 可选 MIT（`MIT OR Apache-2.0 OR TGPPL-1.0`），与 MPL-2.0 兼容。
2. `turso` **已了结（2026-09-10 用户拍板）：钉 `=0.6.1`，0.7 的 icu 死锁绕开**；
   库龄不足一年、`sync` 硬绑 native-tls（不开）、OHOS 待 CI。
3. TLS 豁免已记录（§1/§12）：`reqwest`/`rustls`/`tokio-rustls`/`platform-verifier`/
   `oauth2`/`sentry`/`self_update`/`object_store`/`rcgen`/`russh` 的 ring 后端不再标 ⚠️ 理由，
   但 §2 的后端选择门控继续有效（禁 aws-lc/cmake、禁 native-tls）。
