# Phase 10 计划 — node: 落地（再到 Bun 高度）

> 立项 2026-09-15（用户拍板）。目标：`node:` 兼容从 deno 高度（plan2 收官，
> 半 16 全达/超、全 23 剩 5 欠账）再到 **Bun 高度**——Bun 表 🟢 项逐项对齐，
> Bun 🟡 项确认 parity，差集书面偏离（见 §4）。
> Bun 基线：`docs/bun-compat.md` 立项快照（`oven-sh/bun` HEAD `6a92015`，
> targeting Node v26；第三方全量跑分 Bun 1.3.14 约 40.6%，见 bun-compat 头注）。
> 立项后不追 Bun HEAD（它走得快），以快照为准；终局重测时再对新版复核。
>
> 方法（2026-09-25 用户重申）：**Bun 定高度**（范围 = Bun 自带 node 测试清单
> `docs/bun-scope.txt`，§0.2），**node 定逻辑**（`lib/` 原文 + `test/parallel`
> 断言原文）。deno 参照随 plan2（deno 高度）对齐即完结，本期不再使用；
> 早期"bun 当词典（`internal/` 文件名 1:1）"仅作命名参考。
>
> 纪律：AGENTS 三件套（模块单测 + 黑盒正常/报错/边界 + 冒烟 5/5，
> `UNSAFE-BOUNDARY` 新增配 panic 用例）；踩坑记 `docs/pitfalls.md`；
> vendoring 三家 JS 文件保留 MIT 头；新 crate 一律先走 §0.5（找轮子 →
> 记 `docs/dependencies.md` → **停下问用户** → 点头才引入）。
>
> 验收线（终局）：§1 矩阵 Bun 🟢 项全 ✅、Bun 🟡 项 parity 确认（✅/🟡+偏离注），
> 10f 对拍报告入库，`cargo test` 全绿 0 警告，冒烟 5/5。

## §0 执行方案修订（2026-09-25，**新会话只读本节 + §5 表即可开工**）

### 0.1 诊断：为什么 09-22 起进度变慢（证据）

1. **口径失焦——对着 node 100% 追，而非 Bun 高度**。§5"Bun 实现了的才是欠账"
   被执行成"Bun 🟢 域里 node 套件每一件红都是欠账"。http 域 09-22→09-25 四天
   ~150 提交（占同期 90%+，`framing_*.js` 三件从 ~2.6k 涨到 6.0k 行），
   SAME0 307→381（409 件）；按 Bun 自带清单（0.3）Bun 在 http 口径约 388 件，
   **http 已到 Bun 高度**，后面每件都是 node 独有的深水边角（挂死型/时序型）。
2. **优先级倒挂**。同期 Bun 🟢 的 **http2：Bun 自带 256/277 件，我们 SAME0
   8/276（3%）**——全仓最大真实缺口，四天零投入。node:test 五个 Slice（09-21）
   也属 Bun 🟡 域（Bun 清单 test-runner 0 件），按用户口径本不该排在前面。
3. **无止损线**。验收只写"全部转绿或逐簇书面偏离"，没有数量阈值和单件时间盒，
   agent 默认选"修"——`dump-req-when-res-ends` 这类挂死型 flaky 反复开轮。
4. **入口过期 + 文档负担**。旧"新会话入口"停在 09-20（仍把已收官的 G5/G8/
   URLPattern 当待办，把 G11 标 ~110）；§5 堆了 250 行逐轮日志（已迁
   `docs/plan3-journal.md`）；AGENTS.md 286KB / 206 条 §4（每会话自动载入，
   约 10 万 token 量级上下文税，且 §4 顺序错乱：4.72→4.136→§5/§6→4.73）。
   → 2026-09-25 已修：AGENTS 瘦身至 15KB、踩坑全集迁 `docs/pitfalls.md`、
   旧入口作废、文档索引 `docs/README.md`（存档件加横幅）。
5. **测量噪声**。sweep 不读套件头 `// Flags:`（node 侧 `--expose-internals`
   类 8 件 node 自红、我们"绿"=假绿，§4.126③）；`/tmp/wjs-node-test` 稀疏检出
   fixtures 不全（§4.158）且在系统盘 `/tmp`（会被清，§4.144）。

### 0.2 新口径（立即生效，覆盖 §5 旧口径）

- **欠账 = Bun 自带 node 测试清单 ∩ 我们红**。清单：`oven-sh/bun`
  `test/js/node/test/parallel/`（快照 `dc30df0` 2026-09-24，3656 件），
  入库为 `docs/bun-scope.txt`（文件名表）；与本仓 node 检出同名交集 3574。
  语义仍以 node 原文为准（Bun 的副本可能改过断言），清单只定**范围**。
- **用户拍板（2026-09-25）三条范围规则**：
  1. **已做且超过 Bun 的保留**（代码、黑盒、文档都不删，如 node:test 五个 Slice、
     diagnostics_channel 全语义、crypto 超 Bun 差集）——回归照常守。
  2. **清单外且未做的不做，直接清出**（清出名单见 0.7）；今后遇到清单外红件只在
     bun-parity 记一行"Bun 清单外"，不分析不修。
  3. 清单内的红件才进 0.4 队列，按 0.2 止损线啃。
- **域止损线**：域内"清单件"通过数 ≥ Bun 清单件数 × 95% 即域收官，余件批量
  一行定性（出局/偏离/另案编号），不再逐件开轮。
- **单件时间盒**：同一套件累计 2 轮（或约半天）无转绿即停手记档，换下一件；
  `flake-classify.py` 判 FLAKY / 挂死型的，记档后不追（§4.202-②）。
- **"另轮/infra"必须有编号进 0.4 队列**，否则等同出局——禁止无编号的"另案"。
- 顺序永远按"清单件差额"降序排域，不按"手上正热的域"。

### 0.3 全域基线（2026-09-26 base15，Bun 清单 3574 件，node 26.8.2 对照）

> 总计 **绿 2136/3574（60%）**（base13 2093 → base14 2106 → base15 2136）。口径：
> `scripts/sweep-report.py base15`（工件 `~/wjs-data/sweep/base15/`，2 并发 + 看门狗，40 分钟）；
> "绿"= 双侧 rc=0，"双红"= 两侧同红不计欠账。base13 起 process `exit` 事件生效、mustCall 计数
> 核对真实执行（旧 base12 2168 含 ~140 假绿）。sweep 标签：`HANG`=挂死自报 / `MUSTCALL`=计数不符。
> 下表只列红（DIFF+TIMEOUT）≥10 的域，按红数降序即排期顺序；其余域见报告脚本输出。

| 域 | 清单件 | 绿 | DIFF | TIMEOUT | 双红 | 绿率 |
|---|---|---|---|---|---|---|
| http2 | 256 | 88 | 163 | 5 | 0 | 34% |
| tls | 185 | 40 | 144 | 1 | 0 | 21% |
| crypto | 120 | 37 | 83 | 0 | 0 | 30% |
| repl | 82 | 17 | 64 | 1 | 0 | 20% |
| stream | 215 | 156 | 57 | 2 | 0 | 72% |
| process | 82 | 21 | 59 | 0 | 2 | 26% |
| worker | 110 | 55 | 55 | 0 | 0 | 50% |
| cluster | 80 | 27 | 52 | 1 | 0 | 33% |
| vm | 95 | 46 | 48 | 1 | 0 | 48% |
| fs | 333 | 285 | 45 | 1 | 2 | 86% |
| whatwg | 53 | 12 | 41 | 0 | 0 | 22% |
| http | 389 | 349 | 36 | 4 | 0 | 89% |
| webcrypto | 39 | 1 | 37 | 1 | 0 | 2% |
| net | 138 | 99 | 38 | 0 | 1 | 72% |
| trace | 34 | 3 | 31 | 0 | 0 | 8% |
| https | 59 | 31 | 26 | 2 | 0 | 52% |
| child | 100 | 72 | 26 | 1 | 1 | 72% |
| module | 27 | 4 | 23 | 0 | 0 | 14% |
| diagnostics | 35 | 15 | 20 | 0 | 0 | 42% |
| util | 26 | 8 | 18 | 0 | 0 | 30% |
| readline | 20 | 3 | 17 | 0 | 0 | 15% |
| sqlite | 18 | 0 | 15 | 1 | 2 | 0% |
| internal | 19 | 4 | 15 | 0 | 0 | 21% |
| async | 27 | 12 | 15 | 0 | 0 | 44% |
| fastutf8stream | 14 | 0 | 14 | 0 | 0 | 0% |
| console | 16 | 2 | 14 | 0 | 0 | 12% |
| compile | 14 | 0 | 14 | 0 | 0 | 0% |
| timers | 56 | 43 | 13 | 0 | 0 | 76% |
| require | 21 | 8 | 13 | 0 | 0 | 38% |
| dgram | 75 | 61 | 12 | 1 | 1 | 82% |
| v8 | 13 | 2 | 11 | 0 | 0 | 15% |
| zlib | 61 | 53 | 8 | 0 | 0 | 86% |
| promise | 11 | 3 | 8 | 0 | 0 | 27% |
| dns | 25 | 17 | 8 | 0 | 0 | 68% |
| assert | 10 | 2 | 8 | 0 | 0 | 20% |
| webstreams | 10 | 3 | 7 | 0 | 0 | 30% |
| runner | 25 | 20 | 5 | 0 | 0 | 80% |
| quic | 233 | 228 | 5 | 0 | 0 | 97% |
| event | 28 | 23 | 5 | 0 | 0 | 82% |
| buffer | 63 | 59 | 4 | 0 | 0 | 93% |
| stdin | 11 | 9 | 2 | 0 | 0 | 81% |
| url | 14 | 13 | 1 | 0 | 0 | 92% |
| stream2 | 26 | 25 | 1 | 0 | 0 | 96% |
| path | 16 | 15 | 1 | 0 | 0 | 93% |
| eslint | 24 | 24 | 0 | 0 | 0 | 100% |

### 0.4 执行队列（按序；每项完工改本表状态）

| # | 项 | 内容 | 预算 | 状态 |
|---|---|---|---|---|
| P0-1 | 数据迁外置盘 | ✅ 2026-09-25：node 检出复制到 `wjs-data/node-test/`，sweep 工件缺省 `wjs-data/sweep/`；`cargo test` 不改 TMPDIR（4.207） | — | ✅ |
| P0-2 | sweep 读 `// Flags:` | ✅ 2026-09-25：两侧透传（本仓 CLI 按 D1 规则剥除记录）+ `--scope` 清单过滤 + node 缓存（0.8） | — | ✅ |
| P0-3 | 全域基线 | ✅ 2026-09-25 base12（2 并发 + 看门狗，42 分钟，资源全程正常）；首跑 base10 触发 4.209 事故作废；旗表补齐（`--experimental-quic`/`--tls-min-v1.x` 等 24 个）后 272 件重跑，quic 233 件全转绿 | — | ✅ |
| P1 | http2 | 🟡 2026-09-25 首轮：绿 64 → **88/256**（mustCall 退出核对生效后的真实口径；同口径起点 58）。已修：v1 证书、trailer 有体挂死、请求体流式化、GET 缺省 endStream、会话随连接建立、compat/优先级告警、参数校验六批。余 168 大头是 hyper 底座不可达面——PUSH_PROMISE、ALTSVC/ORIGIN 帧、对端 SETTINGS 内省、原始帧协议错映射（GOAWAY/窗口溢出/未请求 ACK），以及 tls 面（createSecureContext/getPeerCertificate 等，随 P2 tls 一并）。**按 0.2 时间盒暂停，转 P2 tls**；底座不可达簇下轮统一评估：换 `h2` 裸库直驱 vs 书面偏离 | 已用约 1 天 | 🟡 |
| P2 | 共性簇 → tls → crypto → repl → process → stream | 🟡 2026-09-26 进行中。**共性簇先行**（mustCall 红按"未触发回调的事件名"聚类）：beforeExit 派发 + 致命错后发 exit + process.emit 上抛（+9）、emitWarning 按 node warning.js 移植 + CJS 栈帧绝对路径（+8）、fs 流 node 逐字移植 + fs 回调改宏任务 + setImmediate 不钳 1ms + unref 定时器按存活门控（+26）、repl .save/.load（+5）、dgram bind 前 unref。**tls**：SecureContext/createSecureContext/configSecureContext 逐字移植（选项校验 + OpenSSL 可观察报错）、Server 构造器/setSecureContext/connect 校验、tls.Server 无 new、rootCertificates 只读、setDefaultCACertificates 报错面（+23，未计入 base15）。**tls 下一簇**：✅ 2026-09-26 P2-tls-b 落地——TLSSocket 包裹引擎（rustls 手动模式由 JS 字节驱动，
`state::tls_engines` + `__wjs2_tls_wrap_*` 六 natives）+ CaptureVerifier（校验失败不中止握手）+
onConnectEnd/'secure'/convertALPNProtocols/UDS/destroySoon 逐字对齐；wrap 簇 24 件 **19 转绿**
（黑盒 7/7、nextest strict 717/717、冒烟 5/5；io_code 补 ECONNRESET/ENOTCONN 4.213、
listen path 误判 4.214、rustls close_notify 严格性适配 4.215）。**tls 域 sweep
（tlsb2）：SAME0 40→80（+40）/185**；https 域 18/59（同构建对照 17≈18，base15 的
31 系口径差异非回归）。余 4 件定性：streamwrap（阻塞于 stream.Duplex push(null)
不发 'end'——stream 域另案）、default-options（收尾 net=5 待建模）、destroy-soon
（'readable' 流量面）、async-wrap（ca 链）。**crypto 首簇** ✅ 2026-09-27：
`getcipherinfo`/`classes`/`verify-failure` 转绿（getCipherInfo nid/options/ocb 逐字移植
+ ccm nid 勘误 899/902 + Sign/Verify 无 new 形进默认导出；黑盒 2 新增、crypto 域 29/29、
bin 216、冒烟 5/5；`sign-verify` 余行 57 native `.library` 写 + STREAM-PIPE 4 件记档
P2-stream）。**repl 首两轮** ✅ 2026-09-27：17→31/82（+14）。
R1 input 簇 +8（Interface options 归一 + write 入流排空/关后码 + 历史倒序去重 +
legacy 位置形 + stdio 缺省 + writer.options；`{}` 无码 input.on 真机同形不动）。
R2 方法面 +6（defineCommand 函数形 + help 版式 + editor 收尾 + complete 空回；
tab/tab-complete-crash/no-warn/nosideeffects/definecommand/save-load-editor-mode）。
R3 子集补全 +8（成员链逐步求值/getter 拒入 + fs 路径 + bare 上下文键 +
大小写不敏感 + editor 公共前缀；save-load/computed-props/buffer/files/
new-expression/nosideeffects/custom-completer/on-editor-mode）。
余 ~43：getters 仅 proxy 两块（isProxy 引擎缺口）/setupHistory/useGlobal/
internal/repl（11）/ASSERT 散件，另轮。**crypto R1** ✅ 2026-09-29 MISSING-EXCEPTION
9 簇 4 转绿 44/120（坑 4.236）。**crypto R2** ✅ 2026-09-30 argon2 越界簇转绿
45/120（坑 4.239；crypto 域 62/62、冒烟 5/5）。**crypto R3** ✅ 2026-09-30
`enc-validation` + `pqc-encrypted-pkcs8`（PBES2 导出/AKP JWK/DER 嗅探）转绿
47/120（坑 4.240/4.241；`gcm-*-short-tag` 复验已绿；keys/ec 超限切片 909+116 /
942+74 字节恒等；crypto 域 62/62、冒烟 5/5、`check-lines` ok）。**crypto R4**
✅ 2026-09-30 keygen-async 簇 22 转绿 69/120（坑 4.242；crypto4 sweep
SAME0 25/76；crypto 域 62/62、冒烟 5/5；余 bit-length/dh-classic/keygen.js-4096/
raw-slh/pqc-objects/GCM 短 tag 解密/legacy-createCipher 记档）。**process R1**
✅ 2026-10-03 hrtime/nextTick/chdir 校验 + release 面 4 转绿 25/82（proc2 有效基线；
proc1 误跑旧二进制作废 4.244；hrtime 余 V8 私有语法行记档；黑盒
phase11_process_validation_faces 11 断言；process 域 13/13、冒烟 5/5）。
**process R2** ✅ 2026-10-03 abort/内存/cpu/umask 7 转绿 32/82（mach thread_info
真值 4.245；thread-worker 余 crypto 配额记档；黑盒 phase11_process_resource_faces
14 断言；process 域 14/14、冒烟 5/5）。**process R3** ✅ 2026-10-03 POSIX 身份
4 转绿 36/82（EPERM==1 撞码 4.246；process_cred.rs 拆分；黑盒
phase11_process_credential_faces；process 域 15/15、冒烟 5/5）。
**process R4** ✅ 2026-10-03 kill/原型/title 5 转绿 41/82（_kill 可 mock；
黑盒 phase11_process_kill_prototype_title_faces；process 域 16/16、冒烟 5/5；
R5 续啃 ppid/reallyExit/execpath 系）。
**process R5** ✅ 2026-10-03 ppid/reallyExit/软链自举 3 转绿 44/82（黑盒
phase11_process_spawn_faces；process 域 17/17、冒烟 5/5；余 35 件分簇见 journal）。
**process R6** ✅ 2026-10-03 execve 7 转绿 51/82（inspect 对表 4.247；
redirect-warnings 记档 fork-infra；黑盒 execve 报错面 + inspect 转义；
全量 856/856、冒烟 5/5）。
**process R7** ✅ 2026-10-03 capture 4 转绿 55/82（入口改道 + take-调-放回
4.248；黑盒 capture 三件；全量 857/857、冒烟 5/5；余 24 件分簇见 journal） | 各按止损线 | 🟡 |
| P3 | http 冻结收口 | ✅ 2026-09-30 R1：`matchKnownFields`（单例表删 content-encoding/x-forwarded-host，4.237）+ `outgoing-finished`（ServerResponse 补 OM 品牌五项等 close，4.238）转绿，`1.0-keep-alive` 复验绿；http 相关 52/52 strict；余 8 件按原定性记档不再开轮 | ≤1 天 | ✅ |
| D1 | ✅ 2026-09-25 已做 | node 运行时旗改**精确名单**（`src/cli_node_flags.rs`，取自 `node --help`；前缀族方案致 4.209 事故后废弃；winterjs2 自有同名旗与改执行模式的旗不收，必须带值的旗只认 `--k=v`、值非法 exit 9）+ `internal/options` getOptionValue 读真实旗值 + DEP0005 认 `--pending-deprecation` + shell 串自举保旗交 CLI。实测：位置参数本就已通（旧"~18 件"口径过期）；`buffer-constructor-node-modules` 转绿；余红与 spawn 无关，已拆成 D4/P2 项（错误输出形状、`process.stdin/stdout` 非 Stream、`node:stream/iter` 未实现、vm-sigint stdio null） | — | ✅ |
| D2 | ✅ 2026-09-25 已做 | AGENTS.md 瘦身：§4 206 条按编号重排迁 `docs/pitfalls.md`（带索引，编号不变），AGENTS 只留 §0–§3/§6 + 铁律摘要 + §5 入口（286KB→15KB） | — | ✅ |
| D3 | ✅ 2026-09-25 已做 | §0.9 纳入 `src/**/*.js`：8 件超限按方法边界拆 17 片（`concat!(include_str!…)` 字节恒等，逐件 `cmp` HEAD 原件；一文件一提交，域测试 + 冒烟绿）；守门 `scripts/check-lines.sh` | — | ✅ |
| O2 | ✅ 2026-09-25 事故 | 基线 sweep 触发系统 panic：D1 前缀放行致 `test-promise-unhandled-flag` 自 spawn 无限递归（pitfalls 4.209）。已修三道防线（精确旗表 + 自 spawn 深度闸 32 + sweep 进程组/进程数封顶）；base10 基线作废重跑 | — | ✅ |
| O1 | 观察 | `dgram::phase10a_dgram_multicast_connect` 本机挂死（2026-09-25 基线 stash 对照同挂，非本轮引入；疑本机组播路由/网卡环境），全量暂以 `--skip` 跑；再现于他机即升级为必查 | — | 👀 |
| D4 | ✅ 2026-09-25 已做 | 非 TTY 未捕获错误改 node 形（`file:line` + 源行 + `^` + `Name: msg` + `    at …` 栈；SM 帧转 `at fn (loc)`、`__wjs2_` 管线帧滤掉、TS 帧经 sourcemap 回映射；`throw 42` 打印值本身；ESM 入口同形）。`Display` 一行格式不动（worker 透传/退出码解析依赖）。转绿：`os-userinfo-handles-getter-errors`/`vm-api-handles-getter-errors`；`util-callbackify` 余 stderr 行数差（node 多 `processTicksAndRejections` 帧 + `Node.js vX` 尾行） | — | ✅ |
| W1 | WinterJS2 本体（§7 方向；09-28/29） | 🟡 进行中：repl 求值面统一（TLA + acorn vendored 声明提升；CLI 补全接真上下文 + `__wjs2_` 内部面位置纠正）+ console 全局统一 + `.doc`/浮窗文档（mdn 语料 + Bun/Deno 命名空间页 + termimad/reedline）+ `WinterJS2.image`（15 格式）+ `WinterJS2.media`（symphonia/rodio/rav1e/mp4-rs；f32→u8 去 unsafe 见 4.235）+ wstd/wsys/wcover B1-B6 门面 + 站单源化（luoli 唯一真相 + 部署门）。明细只写 journal，本表只留状态 | — | 🟡 |

### 0.5 运行环境（系统盘仅剩 ~5GB，大数据一律外置盘）

- 数据根：`/Volumes//wjs-data/`（与仓库同盘，148GB 余量）。路径含 U+F8FF，
  脚本一律 glob 解析（§4.145），或经家目录软链 `~/wjs-data`（只占一个链接）。
  - node 套件检出：`wjs-data/node-test/`（替代 `/tmp/wjs-node-test`，完整 fixtures）
  - sweep 工件：`wjs-data/sweep/<tag>/`（替代 `~/.wjs-sweep`）
  - Bun 清单：`wjs-data/bun-tree/`（blob-less 浅克隆）+ `bun-parallel.txt`
  - 探针脚本：`wjs-data/probe/`（替代 `/tmp/wjs-*`、`~/probe`）
- sweep / 手工探针：`export TMPDIR=/Volumes/*/wjs-data/tmp`（glob 展开后赋值），
  node 套件 `.tmp.*` 落外置盘。**`cargo test` 不改 `TMPDIR`**——卷名 U+F8FF 使
  `URL.pathname` 百分号编码，黑盒假红（pitfalls 4.207）；assert_fs 临时文件小且即删。
- 单件探针：`~/wjs-data/probe/run1.sh <suite> [secs]`（alarm + exec-or-die，
  输出落 `probe/<suite>.out`）。
- node 套件检出已复制到 `wjs-data/node-test/`（`sweep-bg.py` 缺省指向它；
  `/tmp/wjs-node-test` 可删）。卷名含 U+F8FF：个别以 `URL.pathname` 取文件路径的套件
  两侧可能同红（清单内约 10 件），P0-3 基线与 sweep9 对比时留意 SAME1 增量。
- `target/` 已在外置盘（仓库内），不动；禁建 worktree（§4.142）。
- **node 26.8.2 lib 源码**：`wjs-data/node-lib/`（`node -e` 经 `process.binding('natives')`
  整体导出 416 个内建模块源，11MB；node-test 检出只带顶层 lib，缺 `internal/fs/*` 等）。
  逐字移植先在此读原文（§4.115）。
- **sweep 期间别动 `target/debug`**（sweep 正在用它）：开发改用
  `CARGO_TARGET_DIR=~/wjs-data/target-alt cargo build`（首编 3.5 分钟，之后增量），
  单件探针 `WJS=~/wjs-data/target-alt/debug/winterjs2 run1.sh …`。
- 开工前 `df -h /`：系统盘余量 < 3GB 即先清 `/tmp/wjs-*`、`~/.wjs-sweep`、
  `~/Library/Caches/{JetBrains,Firefox,Homebrew}` 再跑任何构建。

### 0.6 验收节奏（省机时）

- 每簇：只跑对应域 sweep（清单过滤）+ 该域黑盒 + 冒烟 5/5。
- 每个 P 项收尾：全量 `cargo nextest run --profile strict` 一次（约 2 分钟）。
- 全域 sweep 只在 P0-3 与每个 P 项收尾各一次；禁重复全量子集（§4.126）。
- **资源看门狗（2026-09-25 事故后，默认开启）**：`sweep-bg` daemon 每 5s 采样系统盘/数据盘余量、
  内存压力（`kern.memorystatus_vm_pressure_level` + `memory_pressure` 空闲%）、winterjs2 进程数、
  load，每 15s 落 `monitor.log`；越线（系统盘 <3GB / 数据盘 <5GB / 内存 critical 或空闲 <10% /
  winterjs2 进程 >80）即杀全部在跑进程组、状态 `aborted` 并写明原因；内存 warn 暂停发新件。
  `status` 行尾带实时 `mon …` 读数。
- 本节状态表是唯一进度真相；逐轮细节写进 `docs/plan3-journal.md`（追加）与
  `docs/bun-parity.md`，**不再写进 plan3**。

### 0.8 开发提速（2026-09-25 盘点）

**已落地（本轮）**
1. **CJS 报错真位置 + 原异常透传**：node 套件几乎全是 CJS，修前任何未捕获错误都报
   prelude `424:53`、NodeError 文案为空——定位只能靠 mapper/插桩二分。现：`require`
   不再把用户异常转成字符串重抛（原对象透传，`catch` 到的即 node 同款异常，兼语义修复）；
   CJS 包装头编在第 0 行，栈/报错行号 = 物理行（mapper 去掉 +1 折算）；空 message 从
   `message` 属性回填（`jsapi_glue::fill_message`）。
2. **sweep-bg 提速**：`--scope`（缺省 `docs/bun-scope.txt`，只跑清单件）/ node 结果缓存
   （node 侧结果与本仓无关，二跑起 node 侧零开销；实测 url 域 11.1s→9.1s——大头是 25s 的 TIMEOUT 件，故红件复验一律 `--rerun-red`）/ `--rerun-red TAG`（修完只重跑红件）/
   套件头 `// Flags:` 两侧透传（消 node 侧假红 + 我方假绿）/ 工件与 node 检出默认落外置盘。
3. **守门与探针脚本**：`scripts/check-lines.sh`（§0.9）、`~/wjs-data/probe/run1.sh`（单件）。

4. ✅ sweep `--jobs 1..3`（槽位各自 `TEST_THREAD_ID` + 端口段 +1000，结果收尾按名排序；
   实测 path 域 4.8s→1.9s，结果与串行逐行一致）。上限 3 守 §4.126。
5. ✅ D4 错误输出 node 形（见 0.4）。
6. ✅ `cargo nextest`（brew 0.9.146，工具不进 Cargo 依赖；配置 `.config/nextest.toml`：
   4 并发、每测独立进程、30s×4 挂死即杀、失败自动重试 1 次并标 FLAKY）。实测全量
   702 测试 **105s 全绿**（`cargo test` 同口径 15min+，且 dgram 组播挂死会拖住整个
   target——nextest 下单件隔离，本机也过了）。提交前最后一跑用 `--profile strict`（不重试）。
8. ✅（2026-09-26）sweep 标签分 `HANG`（挂死自报）/ `MUSTCALL`（正常退出但回调次数不符），
   红因聚类先按"未触发回调监听的事件名"分组找共性根因（beforeExit/expectWarning/fs 流簇即此法找到）。
7. 每簇工作流固定为：`sweep-bg --prefix <域> --rerun-red <上轮tag>` → mapper/run1.sh
   定位 → 修 → 同命令复验；全域 sweep 只在 P 项收尾跑一次（0.6）。

### 0.7 已清出（Bun 清单外且未做，2026-09-25 按规则 2 删除待办）

以下待办从本计划与 §5 欠账表中**撤销**，不再排期；对应 bun-parity 条目视为
"Bun 清单外"（已绿的不受影响，照常守回归）：

| 域 | 清出项 | 原归类 |
|---|---|---|
| test（node:test） | 残 ~35 件全部（spawn CLI ~18 / reporter ~13 / run 并发·上报 ~6 / mocking 私有字段单行 / `test-runner-force-exit-flush`）——Bun 清单 test-runner 0 件 | Slice F+ 另轮 |
| http | `test-http-dump-req-when-res-ends`（挂死型 flaky，dump 机制需重写流端口 flowing 排空） | 独立轮 |
| child_process | `server-close` / `recv-handle` / `send-returns-boolean`（live 句柄跨会话） | 出局待定 |
| net | `listen-handle-in-cluster-2` + `server-transfer-worker` / `socket-transfer-worker` / `socket-transfer-worker-http`（跨线程 fd 移交） | G6 残件 |
| fs | `readfile-one-roundtrip` 末段（async_hooks FSREQCALLBACK 资源面） | 另轮 |
| diagnostics_channel | child-process / gc-maintains-subscriptions / gc-race-condition / http / http-server-start / memory-leak / module-import(-error) / module-require(-error) / net / tracing-channel-promise-unhandled / web-locks | 🟡/⏭️ 记档 |
| buffer / os / dns | buffer alloc-alignment · isutf8-isascii-fast · swap-fast；os checked-function · fast；dns lookup-promises · memory-error · perf_hooks | ⏭️ 记档 |
| timers / url / util / vm | timers async-store-leak · fast-calls；url parse-deprecation；util format · inspect（`%o`/布局引擎边界）；vm dynamic-import-callback-missing-flag · module-linkmodulerequests · module-modulerequests · property-definer-partial-update · proxy-sandbox-property-query | 🟡 记档 |

仍保留（在清单内，照 0.4 排队）：`net-throttle`、`net-listen-handle-in-cluster-1`、
`child-process-send-keep-open`、`fs-glob` ×2、`fs-promises-file-handle-read-worker`/
`pull`/`pullsync`/`writer`、fs flush 三件、`worker-terminate-*`、http P3 余 11 件、
D1 涉及的 `timers-nan/negative-duration-warning` 等。未逐件核对的簇（dgram 余件、
worker 环境面等）由 P0-3 基线按清单自动分流，不再人工判。

## §1 缺口清单（Bun 快照 vs winterjs2 现状，2026-09-15）

| 模块 | Bun | 现状 | 缺口 | 切片 |
|---|---|---|---|---|
| http | 🟢 | ✅（10b 流式；sweep9 SAME0 381/409，已达 Bun 线） | keep-alive、流式 req/res 体、IncomingMessage/ServerResponse 流全家 | 10b |
| https | 🟡（无 SNI 等） | ✅（同 http 记档） | 随 http 流式化走；SNI 回调等与 Bun 同缺，不追 | 10b |
| http2 | 🟢 | 🟡（h2c+H3 可用；compat 层薄壳，node 套件 SAME0 8/276——§0.4 P1） | trailer/push/Upgrade 三件评估：push 系 Web 已死特性、trailer 等 h2 流式切片、Upgrade 浏览器不用 | 10b |
| readline | 🟢 | ✅（10c-2：行/history/question/按键解码/Emacs 子集/迭代器全绿） | `question` 真实现、行编辑/history/异步迭代器、Emacs 快捷键子集 | 10c |
| tty | 🟢 | ✅（10c-1：net.Socket 基座 + ioctl winsize + 真 raw；构造器非 TTY 即抛与真机同） | net.Socket 基座、ioctl winsize、setRawMode 真标志（termios 按平台记档） | 10c |
| repl（模块面） | 🟡 | ✅ CLI + ✅ 模块（10c-3：REPLServer/start/Recoverable 全绿） | `node:repl` 注册：REPLServer/start/Recoverable（复用 10c 的 Interface） | 10c |
| url | 🟢 | ✅（WHATWG+file 系） | legacy `parse/format/resolve` + `Url` 类 + domainTo*（follow-redirects 已走原生分支，低风险） | 10a |
| timers | 🟢（promises/scheduler） | ✅（setImmediate 近似） | setImmediate check 语义定案（`scheduler.yield/wait` 已有；无 macrotask 分层，近似验收或偏离） | 10a |
| util | 🟢 | ✅（三件未移植） | `parseArgs`、`MIMEType/MIMEParams`、`getSystemErrorName/Message/Map`（uv errno 表随 fs 错误映射） | 10a |
| zlib | 🟢 | ✅（zstd 恒 Fastest） | `crc32` 落地（ISO-HDLC 自实现，零新依赖）；非 Fastest 档等上游 ruzstd（偏离，复议） | 10a |
| dns | 🟢（缺 resolveTlsa） | 🟡（std 底座） | CNAME/MX/TXT/SRV 深件经 hickory-resolver（已在树内；✅ 2026-09-15 拍板：全套+系统配置） | 10d |
| sqlite | 🟢 | ✅ bun:sqlite／— node:sqlite | `node:sqlite` 注册 + DatabaseSync/StatementSync 口径对齐（turso 底座，不跟系统 libsqlite） | 10d |
| dgram | 🟢 | ✅（10a-6 收官：connect/组播投递/ref 真计数；剩 recvbuf 系另切片） | 组播全家（addMembership/dropMembership/setBroadcast/组播 TTL/loopback）、connect/disconnect、ref 真计数 | 10a |
| cluster | 🟡（http 多绑限 Linux） | ✅（线程底座） | Bun 🟡 对等面已齐：primary/worker、fork/disconnect、scheduling 策略 | 10e |
| domain | 🟡 | 🟡（薄面） | 遗留薄面已齐：create/run/bind/intercept + error 路由（同步；异步不路由记档） | 10e |
| crypto | 🟡（缺 ed448/secp256k1/CCM 等） | ✅（差集已闭） | 差集已闭：✅ 2026-09-15 用户全批——`ccm` 0.6 + `ghash` 0.6 直引 + `ed448-goldilocks` 特批钉 `=0.14.0-pre.15` + GCM 任意 iv（J0 手工）+ bf-cbc 删项（真机无 bf 系）+ Ed448/X509；ocb 非 Node 面出局。采购单见 `docs/dependencies3.md` §1 | 10e |
| test | 🟡 | 🟡（起步） | parity 确认（10f 对拍定深浅，不预设切片） | 10f |
| v8 | 🟡（堆统计 JSC 口径） | 🟡（最小桥） | 堆统计/serialize 双方数字皆引擎口径、不可比——书面偏离，不做（见 §4） | — |
| vm | 🟢（全+ESM classes） | ✅（Module/SourceText/Synthetic 全链） | parity 确认（10f 对拍；`compileFunction`/`measureMemory` 行为差即修） | 10f |
| events/fs/stream等 | 🟢 | ✅ | parity 确认（10f 对拍，不预设改动） | 10f |
| sys | 🟢（即 util） | ✅（10a-1：同 util 单例，import/require 双形态） | `node:sys` 别名注册（一行，9x 顺手级） | 10a |
| wasi | 🟡 | — | 不做（plan2 §4 维持否决） | — |
| sea | 🔴 | — | 不做（无对等需求） | — |

## 切片

### 10a 注册与小面（零新依赖，全 JS/既有轮子）

> ✅ 2026-09-15 收官：sys 别名/url legacy/setImmediate+Timeout 真类（附带修
> fire_due 实参展开，见 AGENTS §4.85）/util 三件/crc32/dgram 全家全绿，
> `cargo test` 全绿 0 警告，冒烟 5/5。

- 做：`node:sys` 别名注册；url legacy 面（parse/format/resolve/Url/domainTo*，
  真机逐项对码与文案）；setImmediate 口径定案（`scheduler.yield/wait` 已有，
  只定 check 语义验收标准，不造分层）；
  `util` 三件（parseArgs/MIMEType/getSystemError*）；zlib `crc32`
  （ISO-HDLC 自实现——`flate2::Crc` 不收 seed；同步纯函数）；dgram 组播全家 + connect/disconnect +
  ref 真计数（tokio UdpSocket 底座能力先实测）。
- 验收：`test-url-*.js` legacy 子集、`test-timers-*.js` scheduler 项、
  `test-util-*.js` 三件项、`test-zlib-*.js` crc32 项、`test-dgram-*.js`
  组播项（回环组播 hermetic：`239.0.0.x` 本机环回）点名绿；黑盒三件套照旧。

### 10b HTTP 流式化（深水，http 栈重构）

> ✅ 2026-09-15 收官：帧层重写（IM/Res/Req 进 stream 全家）+ keep-alive
> （Agent 池/`reusedSocket`）+ chunked 双向 + 1MB 大体 + https 随行；
> http2 三件 triage 全偏离。`cargo test` 全绿 0 警告，冒烟 5/5。
> 踩坑见 AGENTS §4.86–4.89。

- 做：http 整收改流式——keep-alive 连接复用、req/res 体流式（IncomingMessage/
  ServerResponse 进 `node:stream` 全家，可 pipe/for-await）、分块编码；
  https 随行（同帧层）；http2 trailer/push/Upgrade 三件先评估后定做/偏离。
- 前置：§4.18/§4.35/§4.46 的结算/排空时序是流式体的生死线，改前重读；
  新事件/数据交叉点走 §4.35"先 emit 再喂体"口径。
- 验收：`test-http-*.js` 流式子集（含 keep-alive 复用计数、chunked 对拍、
  中途 destroy 语义）；既有回环黑盒全绿无回归；大体（≥1MB）压测无 §4.40 类崩
  （GC 压力探针同跑）。

### 10c 终端与交互（tty → readline → node:repl，顺流）

> ✅ 2026-09-15 收官：tty（Socket 基座/ioctl winsize/真 raw/构造抛，pty 实测）
> + readline 全面 + node:repl 全家 + vm sync-in 回落修（AGENTS §4.90）。
> `cargo test` 全绿 0 警告，冒烟 5/5。

- 做：tty 换 net.Socket 基座 + ioctl winsize（libc 经树内轮子？先查，无则
  §0.5；仅 unix，win 记档）+ setRawMode 真标志；readline 全面
  （`question` 真实现、行编辑/history 文件/异步迭代器/Emacs 子集，
  纯 JS over 输入输出流，CLI 的 rustyline 不动）；`node:repl` 注册
  （REPLServer/start/Recoverable/writer，骑新 Interface）。
- 验收：`test-tty-*.js`、`test-readline-*.js` 子集点名绿；`node:repl`
  黑盒（start/prompt/eval 入口/Recoverable）；TTY 相关 hermetic（伪终端无则
  用 pipe + `isTTY` 桩分支断言，不碰真终端）。

### 10d 数据与目录（拍板门×2）

> ✅ 2026-09-15 收官：dns 深件（hickory 全套：Cname/Mx/Txt/Srv/Ns/Ptr +
> resolveAny + getServers/setServers/setDefaultResultOrder，系统配置直读，
> `lookup` 维持 std）+ `node:sqlite`（DatabaseSync/StatementSync：turso 底座，
> CRUD/命名参数/迭代器/列元数据/BigInt 口径全绿）。`cargo test` 全绿 0 警告，冒烟 5/5。

- 做：dns 深件经 hickory-resolver（CNAME/MX/TXT/SRV + promises 面；
  ✅ 2026-09-15 用户拍板：接线做全套——Cname/Mx/Txt/Srv/Ns/Ptr + resolveAny +
  getServers/setServers/setDefaultResultOrder，读系统 DNS 配置（/etc/resolv.conf），
  `lookup` 维持 std `ToSocketAddrs`（真机 getaddrinfo 口径）；crate 已在树内）；
  注册 + DatabaseSync/StatementSync 口径对齐（turso 底座；Node 22+ 实验面
  为准，StatementSync 迭代器/命名参数逐项对）。
- 验收：`test-dns-*.js` 深件子集（hermetic：本地 stub DNS？无则只断 localhost
  + 错误形状，沿 §4.34 符）；`node:sqlite` 黑盒（建表/读写/预处理/事务，
  与 `bun:sqlite` 行为一致性断言）。

### 10e 新域与差集（设计先行）

> ✅ 2026-09-15 收官：crypto 差集（CCM 三档 + GCM 任意 iv + Ed448，
> 真机逐字节交叉）+ cluster 🟡对等面（fork/message/exit/disconnect，
> 线程底座）+ domain 薄面（同步路由，异步不路由记档）。
> `cargo test` 全绿 0 警告，冒烟 5/5。bf-cbc 删项（真机 26 无 bf 系）。
> 踩坑见 AGENTS §4.92–4.93。

- 做：cluster Bun 🟡 对等面（primary/worker、fork/env、disconnect/suicide、
  scheduling 策略 RR；骑 fork/worker 线程底座——真多进程语义不做，书面记档）；
  domain 遗留薄面；crypto 差集开工（✅ 2026-09-15 全批落锁：ccm/ghash/
  ed448-pre.15，bf-cbc 真机实测后定，见 `docs/dependencies3.md` §1）。
- 验收：`test-cluster-*.js` 基础项（fork+message+exit 码）；
  `test-domain-*.js` 薄面项；crypto 评估报告进 `docs/dependencies.md` 候选节。

### 10f 对拍验收（Bun 高度的证明）

> ✅ 2026-09-19 收官：点名全覆盖——10f 清单 22 模块 + §1 划入的 `test`
> 行（83 件 `test-runner-*` 补点名，DIFF 66 分簇定性：API 面 ~35 下轮可修 /
> 自 spawn CLI ~18 另案 / reporter 深度 ~13 偏离；矩阵维持 🟡，见
> `docs/bun-parity.md ## test`）。其余 23 域红项逐簇定性——本轮修复进
> 三件套回归，长尾分簇"下轮可修 / 另案（理由）/ 双红对齐"全部书面记档
> （红即修或记 §4，无第三种状态）。§1 矩阵据此维持 ✅/🟡+偏离注。
> 各域转化：buffer 63✅+9⏭️、timers 45✅+14⏭️、vm 47✅+26🟡+27⏭️、
> stream 190✅+50⏭️、fs 同绿 39→123、net 30→86（七轮校验族 11/11）、
> crypto 24→38+六轮 PSS/PBES2/raw 门、http 95→125+流式头体分离、
> worker 35→55+七轮环境面、zlib Zip 面 15/17、child 同步族+exec/abort 面、
> url/timers/dc/dns/path/assert/os 收官红 0。连带根修：§4.137 入口失败+
> 开着句柄永不收割（eval/模块双路）。`cargo test` 全绿 0 警告（仅依赖
> proc-macro-error2 的 future-incompat 提示，非本仓代码），冒烟 5/5。

- 做：Node `test/parallel` 子集逐模块点名（events/fs/stream/crypto/http/net/
  timers/util/dns/zlib/vm/worker/buffer/path/url/querystring/punycode/
  string_decoder/diagnostics_channel/trace_events/os/assert/child_process），
  出 parity 报告：绿/红/偏离（红即修或记 §4，无第三种状态）。
- 交付：`docs/bun-parity.md`（模块 × 用例 × 结果 × 偏离理由），§1 矩阵据此
  转正（✅/🟡/偏离注）；红项修完进三件套回归。

## §4 不做与书面偏离（v1）

- `wasi`（plan2 §4 维持否决）、`sea`（无对等需求）。
- `v8` 堆统计/serialize：双方数字皆引擎口径、跨引擎不可比——跳过是正确语义，
  不是欠账（`startupSnapshot` 桥保留，vite 解挡）。
- zstd 非 Fastest 档：等上游 ruzstd 补齐实现后复议（AGENTS §4.37）。
- `setImmediate` check 阶段语义：本仓无 macrotask 分层，`setTimeout(0)` 近似
  为既定口径（10a 只定案验收标准，不造分层）。
- 既有记档维持（不因换高度而重做）：fork 线程底座（stdio 恒 null 等）、
  GCM iv 限 12B（除非 10e 批了重做）、promises 底层同步实现、CCW 跨域判定、
  H3 串行、`node:test` reporter 深度（Bun 同 🟡）。

## §5 Bun 🟢 域欠账清单（2026-09-19 盘点；**判定口径已由 §0.2 取代**：
## 以 Bun 自带 node 测试清单为准，下表仅作历史簇索引）

> **2026-09-25：本节已被 §0.2/§0.7 取代——表中凡列入 §0.7 清出名单的残件作废。**
>
> 口径修正（本轮起生效）：欠账判定以 **bun-compat.md 快照的 Bun 列**为准，
> 不再以 node 套件红数为准——child_process（Bun 🟡：IPC 若干缺口）与
> worker_threads（Bun 🟡）整体降级为"parity 确认"档，其 DIFF 中 Bun 同缺的
> 簇**出局不算欠账**；仅下表所列 Bun 🟢 域的簇是真实欠账。

### 已收官（Bun 🟢 且红 0 / 仅引擎边界偏离，无欠账）

buffer、events、stream、url、path、querystring、punycode、string_decoder、
os、assert（message 文本偏离）、timers、util（`%o` 布局引擎边界）、vm
（26 偏离记档）、trace_events、dns、readline、tty、sqlite、repl、crypto
（超 Bun 🟡）、sys、dgram（recvbuf 簇已收官，余 connect/membership 等小簇见下）。

### 欠账（Bun 🟢 且我们有记档红簇，逐簇以后再说）

| 域 | 欠账簇 | 规模 | 性质 |
|---|---|---|---|
| http | TIMEOUT 簇（expect-continue/upgrade/trailer/管线背压/max-connections） | ~110 → sweep9 真红 12 件（2026-09-25） | **冻结**：已达 Bun 线（§0.3），余件按 §0.4 P3 一次性定性，不再开轮；逐轮记录见 `docs/plan3-journal.md` |
| http | ~~校验长尾 / chunk 限深~~ ✅ 2026-09-19 转绿（G3 六提交：chunk 扩展 413/trailer 431/校验门 15 件/Agent createSocket/IPC socketPath/write-after-end 语义/FIN 半开收口，点名 45 件 SAME0；余 OutgoingMessage outputData 缓冲模型 5 件**出局另轮专项**、假 socket socket.push 2 件需 net 流式化、TIMEOUT 110 归下行） | ~35→5 件 | 已收官，残件另案 |
| http2 | compat 层 `Http2ServerRequest/Response` 全流面 | ~105 件 | 最大单体簇，与 10b 同型工程 |
| http2 | server 流面 / settings/priority/ALPN 校验 | ~15 件 | 随 compat 轮 |
| fs | ~~validators 尾件 / unhandled-rej 尾件~~ ✅ 2026-09-19 转绿（G4 两提交：constants 16键+null原型/stat-bigint 全option链+Ns四键/throwIfNoEntry 只豁免 ENOENT/__Stats DEP0180 可调用形/fd_table 预注册标准流/statfs frsize+bigint/utimes·lutimes·futimes 秒口径+utimensat/lchown·lchmod·_toUnixTimestamp·lutimes 新面/rename oldPath·newPath/truncate len 校验/null-byte 全API含URL/__fdCb 值优先+孤儿promise根除/latin1字节直映/writeFile encoding+abort交付面/WriteStream真open；AGENTS §4.149）；残件：roundtrip 末段 async_hooks FSREQCALLBACK 资源面另案 | ~40→1 件（async_hooks 另案） | 已收官，async_hooks 资源面另轮 |
| fs | ~~watch hang（promises-watch/recursive/encoding）/ watch-ignore-glob / flush 选项 / pipe 读形~~ ✅ 2026-09-20 转绿（G8 八提交：ignore 全形态+递归相对路径/StatWatcher 单例+异步 stop+零 Stats/FSWatcher ref-unref+异步 close/encoding 转码/promises.watch 迭代+校验/_getActiveHandles/flush 选项/exit 首码赢/前沿防抖+Create 二判据+stat unref+首轮收口，watch 域 40/46；AGENTS §4.152-155）；残件：fs.glob ×2（Bun 快照无此行，记档另案）+ flush 三套件（待 node:test runner 深度）+ enoent-after-deletion 间歇超时另查 | ~23→3 件 | 已收官，残件另案 |
| net | ~~server close/listen 时序~~ ✅ 2026-09-19 转绿（G2 server 选项面 blockList/maxConnections/drop/pauseOnConnect/close 窗口 + G2 relisten `__closing` 残留旗修复）；余 cargo-harness 下 net_remote/unix_socket 的 SEGV 已修（with_str_args GC 悬垂） | ~10 件 | 已收官 |
| net | ~~TIMEOUT 9 / Happy Eyeballs 3 / worker 投递 3 / large-string 1~~ ✅ 2026-09-19 G6：13/16 转绿（large-string 分包解码/async-iter end(cb)挂finish/write-after-end-nt EPIPE面/abort-controller 信号面+侧表/ipv6 lookup family透传/HE-default 串行回落/HE校验×2；max-connections×2+bytes-stats 随 G2 已绿），残件 6 件 infra 级记档——throttle（native 读门控+写EAGAIN流控）/cluster×2（internalMessage协议，非net面）/worker×3（跨线程fd移交，可骑holdToken机制，独立轮） | ~16→6 件 | 残件另轮（bun-parity net 八轮，AGENTS §4.148） |
| zlib | ~~增量语义~~ ✅ 2026-09-19 转绿（G9-1 Rust 状态机 + G9-2 JS 流类接线：write 即时压出/flush 档位即时出边界/finishFlush 容忍/rejectGarbageAfterEnd/一次性面切引擎错误口径对真机；6 目标套件+连带 5 件全绿，zlib 域 73/81） | ~6 件 | 已收官 |
| zlib | ~~brotli 字典 / zstd pledged-src-size / Web DecompressionStream~~ ✅ 2026-09-19 转绿（G9-3：raw 字典构造期主动 set_dictionary；一次性压缩走引擎收口（dict/pledged/错误口径单点，字节对比零回归）；__zDictBytes 严格校验；pledgedSrcSize 全校验面 + 引擎终检 errno=72；constants.ZSTD_error_* 28 项；Web CS/DS 全局+stream/web——4 格式 roundtrip/尾垃圾 readable TypeError/proto 独立；6 目标套件全绿 + 82 件对拍零回归） | — | 已收官 |
| dgram | ~~recvbuf 系~~ ✅ 2026-09-19 转绿（G1：recvbuf/sendbuf 四方法+隐式绑定+EMSGSIZE 回调路由+数组 send+族匹配解析+ALREADY_BOUND/EBADF 形状；DIFF 53→32，余 connect 族/membership/bindSync/ipv6only 等独立小簇 ~32 件顺延） | 小簇 | ✅ 2026-09-21 独立小簇轮：send-bad-arguments（buffer 先行/端口先于地址/越界）+ child-index（fork env 宽容）+ cluster-reuse（SO_REUSEADDR 双落）+ getSendQueue*/getActiveResourcesInfo 双 API；16→11 跳过（internals）+ 5 G6 轮（_getServer/handle/3 TIMEOUT）+ 2 API 落盒（queue-info respawn transport 出局/unref 挂死归 cluster 退出 race，见 §4.177） |
| child_process | ~~async AbortSignal 尾件 / async 句柄面 / exec 多字节截断~~ ✅ 2026-09-21 转绿（G5b 三提交：参数归一逐字/kill+stdin+flush/stdio 转交+close 重放/fork env+internalMessage+net reusePort；作用域 76 件 DIFF 16→4，黑盒 19→23） | ~30→4 件 | 残 4 出局（handle 传递，见下行） |
| child_process | 残件：send-keep-open/server-close/recv-handle/send-returns-boolean（live 句柄跨会话 + 4 元 stdio + backlog 记账） | 4 件 | 出局（Bun 🟡 IPC 缺口同缺，拍板维持；线程底座 token 共享另轮） |
| worker_threads | terminate 深水 / 环境面尾件 | 部分 | Bun 🟡 → parity 确认档；Atomics.wait（引擎面）与 stdio 流面（Bun 同缺倾向）另核 |

### 待核对（1 项，已核对）

- **URLPattern** ✅ 2026-09-20 实测归属：Bun 1.4.2 实现（`new URLPattern` +
  `exec().pathname.groups` 可用），本仓 `typeof URLPattern === "undefined"`——
  按 §5 口径（Bun 实现了的才是欠账）列为真实欠账，另轮专项（WHATWG
  URLPattern 匹配语义 + groups 回填）。
  ✅ 2026-09-21 收官：`urlpattern` 0.6 接线（Rust 桥 + prelude 真类 +
  `node:url` 双导出），真套件三件双侧 rc=0，黑盒 `phase11_urlpattern_*`，
  组序记档偏离（AGENTS §4.176）。

> 验收口径：上表全部转绿/或逐簇书面偏离前，Phase 10 对"Bun 🟢 域"不算
> 逐字节到位；10f 的收官（§1 矩阵/报告/终局门）不受影响——欠账已全部
> 定位、定性、定量。

### 执行日志（已迁出）

> 2026-09-19 → 09-25 的逐轮落账（G1–G11、test Slice A–E、sweep4–9、工具轮）
> 原样迁至 `docs/plan3-journal.md`；新会话不必读。最新基线：http sweep9
> SAME0=381 / SAME1=6 / DIFF=16 / TIMEOUT=6（409 件）。下一步见 §0.4。

## §7 方向纠正：winterjs2 本体与 node:* 兼容面的关系（2026-09-28 用户拍板）

> 铁律见 AGENTS §6：CLI/产品能力是本体，`node:*` 兼容面是下游包装；禁把
> CLI/产品专用能力放进 `node:*` 公开导出面；共享逻辑经 `__wjs2_` 内部注册面。

### 审计结论（node:* ~50 域）

- **方向正确 13 域**（骑自身底座）：timers、buffer、stream_web、console（无流面）、
  process、path、crypto、sqlite（骑 bun:sqlite turso 底座）、worker_threads、vm、
  url、test（骑 --test runner）、http（骑 node:net；serve 的 axum 栈是产品功能，平行合理）。
- **语义独立无对应物 ~25 域**：fs/net/dgram/tls/dns/zlib/assert/querystring/punycode/
  string_decoder/child_process/cluster/domain/os/util/inspector/diagnostics_channel/
  async_hooks/trace_events/perf_hooks/events(EventTarget 是另一 API)/nodemodule/
  require_cjs/quic/v8 等——无反关系问题。
- **实锤反了 3 处**：
  1. **repl**（最重）：CLI REPL（src/repl.rs + runtime/repl.rs）与 node:repl
     完全平行，求值面都不同源（CLI 每行独立 evaluate_script、无持久词法/
     top-level await；模块走 vm context）。node 真机形态 = CLI 即 repl.start()
     默认实例。
  2. **readline 补全语义**（R5 已位置纠正：核心在模块注册内部面，桥/签名表住
     prelude/repl_complete.rs）。
  3. **console 格式化**（部分反，待议）：全局 console Rust join_args vs
     node:console util.format；真机全局 console 即 Console 实例。

### 另案：repl 深度统一（待拍板；非本 Phase）

- CLI 求值改骑 vm context（持久词法 → top-level await 可用）；
- CLI 会话形态向"REPLServer 默认实例"靠拢（node 原文形态）；
- node:repl 的 REPLServer 求值/历史经底座桥复用；
- 全局 console 格式化统一（骑 util.format）。

### §7 进度（2026-09-28 R6）

- **纠正① repl 求值面**：✅ TLA 落地（试错包装 + state Heap 挂起槽 +
  EOF drain + 非 TTY 哨兵背压；黑盒/管道/pty 全对）。求值面与 node
  defaultEval 同构。**声明提升 ✅（R6b）**：acorn 8.18.0 vendored
  （node 26.8.2 内建同款，用户拍板），processTopLevelAwait 逐字移植
  （末表达式 return 化 + let/const/var/class/function 提升）。
- **纠正③ readline 补全语义**：✅ R5 已位置纠正（核心模块注册内部面，
  桥/签名表住 prelude/repl_complete）。
- **纠正② console 格式化**：✅ 2026-09-28 单轮收尾——09-25 已骑 util.format
  的 5 方法不动；assert/trace 按 constructor.js 原文包装备案（assert 首参 past/
  warn 二次格式化、trace 自拼首行+栈；空消息裸 `Trace`）+ 补 8 缺失方法
  （table 沿模块面 format 落盘偏离、dirxml=log/groupCollapsed=group 别名、
  context/Console 惰性复用模块面、profile 系 no-op）+ 补全签名表 7 项。
  console 域 sweep 16 件 SAME0 2→3，余 13 深水（流写错/颜色/TTY/栈/proxy）记档；
  strict 776/776；坑 4.226（形态变则 toString 消费者同步改）。
