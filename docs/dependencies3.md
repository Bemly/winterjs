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
- **2026-09-15 bf-cbc 删项（10e 收官）**：真机 26.8.2 `getCiphers()` 实测无
  `bf` 系（OpenSSL 3 默认 provider 已移出 Blowfish），按立项条件"有则做、无则删"
  直接删项，零代码、零依赖。
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

## §5 x448（10f crypto，2026-09-18 调研并拍板 ✅）

> 背景：crypto 对拍二轮记档"key-objects.js：x448 无轮子"——**勘误：有**，且与
> 树内钉版完美咬合（当时调研漏检了 elliptic-curves 仓库的 x448 子 crate）。
> **2026-09-18 用户拍板：引**（钉 `=0.14.0-pre.12` + `static_secrets` 特性门，
> 与 x25519-dalek 同款按值 secret）。落地当日全链转绿：三 native + OKP DER
> 三档 + JWK/raw 面 + 低阶点拒收（`tests/node/crypto.rs::phase10f_crypto_x448_parity`）。

## §5.1 决策记录（追加）

- **2026-09-18 用户拍板（10f crypto x448）**：引 `x448` crate（X448 DH，
  RFC 7748；RustCrypto/elliptic-curves 正家，零新增传递依赖）。

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| X448 DH（RFC 7748：clamp 内置 + low-order/全零输出检查） | `x448` | 0.14.0-pre.12 | 2019（RustCrypto/elliptic-curves） | 2026-06-24（与 ed448-goldilocks 同列车发版） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

注：

- **零新增传递依赖**：deps = `ed448-goldilocks ^0.14.0-pre.15`（树内已特批钉
  `=0.14.0-pre.15`，精确命中）+ `zeroize ^1`（锁内 1.9.0 已有）+
  `serdect ^0.4`（optional，serde 面，默认关）。
- API 即 node DH 所需：`x448::x448(scalar[56], point[56]) -> Option<[u8;56]>`
  （clamp 在内，全零输出 None → node 口径映射）；`Secret/Public/
  diffie_hellman` 全家（generateKeyPair/deriveKey 两条路都省了）。
- 钉版模式与 ed448-goldilocks 同款：`0.14.0-pre` 滚动预发布线，建议钉
  `=0.14.0-pre.12`，上游发稳定版后随 ed448-goldilocks 一并回 caret。
- 备选（不引 crate）：树内 ed448-goldilocks 的 `MontgomeryPoint` 自架——但
  `EdwardsScalar` 只有 mod-L 约减构造器（clamped 标量永不 canonical），素数
  子群键与 OpenSSL 等价、任意 u 语义不严格；要精确 RFC 7748 须用其
  `FieldElement` 手写 ladder ~40 行（H 档，零依赖）。
- node 面：`test-crypto-key-objects.js` x448 DH 段（生成/导入/derive/
  asymmetricKeyType 'x448'）。

## §3 tty 底座（10c-1，2026-09-15 用户拍板两项）

| 用途 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | macA64 | macX64 | linA64 | linX64 | winA64 | winX64 | andA64 | andX64 | ohA64 | ohX64 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| TIOCGWINSZ 类型与常量（ioctl winsize；纯 FFI，无代码） | `libc` | 0.2.189（走 0.2 线，与锁同版） | 2015-01-11 | 2026-08-29 | ✅（FFI 声明） | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| termios 真 raw 模式（tcgetattr/cfmakeraw/tcsetattr） | `nix`（开 `term` 特性，同 crate） | 0.31.3（树内现版） | — | — | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

注：`nix` 0.31 无 winsize 现成件（`tiocgwinsz` 须自写 `ioctl_read_bad!`，
故须直引 `libc` 取类型与常量）；`term` 特性空依赖（纯 libc 包装）。
`libc 1.x` 因本地 index 无稳定版元数据暂不可解析，走 0.2 线（锁内已有，
零新增传递依赖）。unix-only 使用，win 回落记档（见 `node/tty.rs` 头注）。

## §4 matchesGlob 候选（10f `test-path-glob.js`，2026-09-15 调研，✅ 已拍板 H）

> **2026-09-15 用户拍板：H 手写**（零新依赖）。F/A/G 均有套件外暗语义缺口，
> 详下表；`decode` 落账见 `src/builtins/node/path.rs` 头注 + 142 条差分探针
> （`/tmp/wjs-globprobe` 离线实证 + `/tmp/wjs-probe/mm-*.js` 真机对拍）。

> 需求：`path.{posix,win32}.matchesGlob(path, pattern)` = Node
> `internal/fs/glob` 的 `matchGlobPattern`（minimatch `Minimatch.match`，
> 固定选项 `nocase=host(mac/win)` + `windowsPathsNoEscape` + `nonegate` +
> `nocomment` + `platform` + `nocaseMagicOnly`）。真机实测钉住三条暗语义：
> ① `nocaseMagicOnly` 是**逐段**门控（`foo\b*` vs `FOO\BAR` → false，
> 无 magic 段恒大小写敏感）；② dot 规则（`*` 不配前导 `.`）；
> ③ `nonegate` 下前导 `!` 为字面量（`!foo` vs `bar` → false）。
> 实证：`/tmp/wjs-globprobe` 离线 scratch（`glob =0.3.4` + `glob-match =0.2.1` +
> `fast-glob =1.1.1`）跑 17 条套件用例三轮子**全过**；分歧全在套件外探针。

| 方案 | crate | 最新版本 | 建库时间 | 最新维护 | 纯 Rust | 10 列 | 套件外缺口（须 shim/记档） |
|---|---|---|---|---|---|---|---|---|
| A 树内直用 | `glob`（已直引，零新增） | 0.3.4 | 2014 | 维护中 | ✅ 零依赖 | ✅ | `{}` 不支持（文档明示）；大小写整 pattern 开关，表达不了逐段门控 |
| F 行级增补 | `fast-glob`（闭包内 1.1.1，经 oxc 带入；直引跟锁） | 1.1.1 | 2024-05-27 | 2026-08-31（oxc 系） | ✅（仅 arrayvec） | ✅ | 前导 `!` 取反（`\\!` 转义重写可救）；dot 规则无（前检查可救）；大小写无选项（逐段小写驱动可救，但 `{a/b,c}` 含 slash brace 需 brace 感知切分） |
| G 新引 | `glob-match` 0.2.1 | 0.2.1 | 2023-01-16 | 2023-02-07 后无维护（13.5M 下载，冻结型候选） | ✅ 零依赖 | ✅ | 与 F 同三缺口（F 是其修 bug 分支，G 无胜场） |
| H 手写 | —（零新依赖，`Cargo.lock` 不动） | — | — | — | ✅ | ✅ | extglob `+(…)` 不做（套件无，真机罕见，记档）；Unicode 大小写折叠走 `to_lowercase` 近似（regex `i` 的 Turkic-I 类边角记档） |

注：

- `globset`（闭包内 0.4.20）出局：gitignore 语义（无 slash pattern 配 basename），
  `*` vs `foo/bar/baz` 会误配 true，与套件 `false` 断言直接冲突。
- `wax`（2021 建/2026-01 维护，MIT）出局：自有方言非 minimatch 口径；
  默认带 `walk` 特性（walkdir），关特性也救不了语义。
- `rs-minimatch-core`/`glob-matcher`（2026 新 crate）出局：库龄不足一年。
- 备选 F 的行级增补若拍板：`fast-glob = "1"`（跟锁 1.1.1，零新增传递依赖）。
- 备选 H 若拍板：Rust 实现约 150 行（段切分 + `**` 跨段 + 段内 `*?[]` + `{}` 展开 +
  逐段 nocase + dot 规则 + 前导 `!` 字面），单测用本轮探针真值表钉住。
