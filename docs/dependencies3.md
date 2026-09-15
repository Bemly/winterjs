# Phase 10 采购单（Bun 高度，`docs/plan3.md`）

> 建单 2026-09-15。`dependencies.md`（Phase 0–8）/ `dependencies2.md`
> （Phase 9）之后第三单：只记 Phase 10（Bun 高度）新增与候选。
> 口径沿用前两单（§2 纯 Rust 红线：native-tls/openssl、aws-lc、bzip2、
> zstd(C) 一律禁；caret 除非注明钉版）。

## 决策记录

- **2026-09-15 用户全批（10e crypto 差集）**：`ccm` 0.6（AES-CCM 三档）、
  `ghash` 0.6（直引，GCM 任意 iv 的 J0 构造；闭包内已有精确 0.6.0，随
  aes-gcm 0.11 带入，直引零新增传递依赖）、`ed448-goldilocks`
 （Ed448 签名；**特批**：稳定版停在 0.9.0，钉 `=0.14.0-pre.15` 预发布，
  上游发稳定版后第一时间回 caret，见下表注）。
  bf-cbc：零新依赖（`blowfish 0.10` + `cbc 0.2` 全在树内），真机 26
  `getCiphers()` 有 bf 系则做、无则删项（10e 开工时实测）。
  `ocb`：非 Node 面，直接出局，不评估。
- **2026-09-15 用户已批（10d dns 深件）**：hickory-resolver 接线做全套
  （Cname/Mx/Txt/Srv/Ns/Ptr + resolveAny + getServers/setServers/
  setDefaultResultOrder），读系统 DNS 配置，`lookup` 维持 std
  （crate 0.26 已在树内，无需新增）。
- 待查（10c tty winsize 的 ioctl 轮子：先查树内 `nix` 能否开特性顶，
  无则另走 §0.5；`node:sqlite`/`node:repl`/cluster 全零新轮子，不占用本单）。

## §1 加密差集（10e，Bun 🟢 对齐）

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| AES-CCM 三档（aes-128/192/256-ccm） | `ccm` | 0.6.1 | 2020-06-03 | 2026-08-21 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| GCM 任意 iv（NIST SP 800-38D J0 构造；`aes-gcm` crate 只收 12B nonce） | `ghash` | 0.6.0 | 2016-10-06 | 2026-02-28 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Ed448 签名（generateKeyPair/sign/verify） | `ed448-goldilocks` | 0.14.0-pre.15（钉版特批，见注） | 2020-05-09 | 2026-06-24 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

注：

- `ccm` 0.6.1 的 deps（aead 0.6 / cipher 0.5 / ctr 0.10）与树内
  （cipher 0.5.2 + ctr 0.10.1 + aes 0.9.3）零版本墙；下载量 1300w+，
  2026-08 在维护。AES-CCM 经任意 cipher 0.5 BlockCipher 泛型，aes 三档直通。
- `ghash` 闭包内已有精确 0.6.0（aes-gcm 0.11 带入，见 Cargo.lock），直引
  零新增传递依赖；J0 构造约 50 行（`aes` block + `ghash`），侧信道记档
  （与既有 PKCS#7 非恒定时间注记同口径）。
- `ed448-goldilocks` 特批说明：稳定版停在 0.9.0（2022 线，rand_core 0.6，
  与本仓 rand 0.10 栈不兼容），可用线为 `0.14.0-pre` 滚动预发布；
  用户特批钉 `=0.14.0-pre.15`，上游发稳定版后回 caret（10e 开工时复核）。
  Bun 同缺 ed448，不挡 Bun 高度；本单属超配。
- 矩阵依据：三家全 RustCrypto 纯 Rust、无平台相关代码，与树内 `aes`/
  `sha2` 同级；mobile 列随其余 RustCrypto 行按 ✅ 计（待 CI 转正）。

## §2 10a–10f 轮子审计（2026-09-15，除 §1 外零新 crate）

> 勘误（2026-09-15，10c-1）：本节"零新 crate"已过期——`libc` 直引获批新增
> （见 §3），`nix` 开 `term` 特性（同 crate，仍零新 crate）。
- 10a：`node:sys` 别名/url-legacy/setImmediate/`util` 三件/dgram 组播——
  零新依赖（tokio 已在树内）。
  `getSystemErrorName/Message/Map`：手写 UV errno 定表（~100 条固定数据，
  Node `lib/` + `uv_errno_t` 对抄；libuv 系轮子全是 C binding，§2 禁，无轮子可找）。
- 10b：http 流式化无新轮子（hyper/h2 全在闭包，h2 0.4.19 经 hyper 带入）；
  server push 若需直引 `h2`，10b 开工时先查 Bun http2 自身是否含 push
  （Bun 94%，大概率不在其面内 → 不做），届时另走 §0.5。
- 10c：winsize 取 `nix` 0.31 的 `ioctl` 特性（空依赖列表：纯 macros + libc，
  libc 已是 nix 必需依赖，零新 crate；macros 全平台可编译，unix 下使用，
  win 记档）→ 仅 Cargo.toml features 加 `"ioctl"`（同 crate 开特性，
  ✅ 2026-09-15 用户拍板，已开）。
- 10d：hickory-resolver 0.26 的 default 特性已含 `system-config` + `tokio`
  （注册表 Cargo.toml 实测），10d 零改动直接开工。
- 10e：cluster/domain 零轮子；crypto 见 §1。
- 10f：无。
- `mime` 0.3.17 虽在闭包（reqwest/hyper 带入），`util.MIMEType` 是 WHATWG
  纯算法解析，不需要它，不直引。

## §3 tty 底座（10c-1，2026-09-15 用户拍板两项）

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| TIOCGWINSZ 类型与常量（ioctl winsize；纯 FFI，无代码） | `libc` | 0.2.189（走 0.2 线，与锁同版） | 2015-01-11 | 2026-08-29 | ✅（FFI 声明） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| termios 真 raw 模式（tcgetattr/cfmakeraw/tcsetattr） | `nix`（开 `term` 特性，同 crate） | 0.31.3（树内现版） | — | — | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

注：`nix` 0.31 无 winsize 现成件（`tiocgwinsz` 须自写 `ioctl_read_bad!`，
故须直引 `libc` 取类型与常量）；`term` 特性空依赖（纯 libc 包装）。
`libc 1.x` 因本地 index 无稳定版元数据暂不可解析，走 0.2 线（锁内已有，
零新增传递依赖）。unix-only 使用，win 回落记档（见 `node/tty.rs` 头注）。
