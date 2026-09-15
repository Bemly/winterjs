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
