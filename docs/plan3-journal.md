# plan3 执行日志（2026-09-19 → 09-25，自 plan3.md §5 迁出）

> 2026-09-25 修订时原样迁出（逐字，未改写），plan3.md 只留现状与下一步。
> 本文件只追加不改写；新会话**不必读**，需要追溯某轮细节时再查。

### 欠账轮落账与新会话入口（2026-09-19 session 收官，新会话据此开工）

**本 session 完成（全部已提交 master）：**

| 簇 | 状态 |
|---|---|
| G1 dgram recvbuf | ✅ 上 session（DIFF 53→32） |
| G2 net server 选项面 | ✅ 上 session |
| G9-1 zlib Rust 状态机 | ✅ 上 session |
| G9-2 zlib JS 流类接线 | ✅ 上轮（6 目标套件全绿 + 连带 5 件，zlib 域 73/81） |
| G3 http 校验长尾/chunk 限深 | ✅ 上轮（子 agent 六提交移植，点名 45 件 SAME0 + 连带 15 件） |
| **G9-3 zlib 尾件**（brotli/zstd 字典/pledged/Web CS·DS） | ✅ 本轮（6 目标套件全绿 + 82 件对拍零回归；一次性压缩走引擎收口 + raw 字典构造期设 + 严格字典校验 + pledged errno=72 + constants 28 项 + Web 全局；AGENTS §4.145-147） |
| **G6 net 尾件** | ✅ 本轮 13/16 转绿（large-string/async-iter/write-after-end-nt/abort/ipv6/HE×3 + G2 顺手 2 件；全量 159 件对拍零回归，black-box 223 全绿）；残件 6 件 infra 级记档（throttle 流控/cluster 协议×2/worker fd 移交×3）；AGENTS §4.148 |
| **G4 fs validators 尾件** | ✅ 本轮 20 套件转绿（stat族/constants/bigint/throwIfNoEntry/DEP0180/fd标准流/statfs frsize+bigint/utimes秒口径/lchown·lchmod·lutimes·_toUnixTimestamp新面/null-byte全API/rename·truncate·fchown·mkdir校验面/latin1/writeFile encoding+abort/WriteStream真open）；黑盒 fs 13/13 + 冒烟 5/5；AGENTS §4.149（e94db50 + 3a4704e） |
| **G5 child 尾件** | ✅ 本轮 25 套件转绿（ChildProcess.spawn 方法面/spawn 事件+多监听fan-out+dispose/stdio 数组+spawnargs/空字节横向校验/`-p` 自举/env 归一/ipc 门/paused 读/removeAllListeners/二次 disconnect 门/uid-gid EPERM/send 校验/stdin 继承/ERR_IPC_ONE_PIPE+INVALID_HANDLE_TYPE；黑盒 child 18/18 + 冒烟 5/5；AGENTS §4.150-151） |
| **G8 fs watch 尾件** | ✅ 本轮 30 套件转绿（ignore 全形态+递归相对路径/StatWatcher 单例EE+异步 stop+零 Stats 首轮/FSWatcher ref-unref+异步 close/encoding 转码/promises.watch 迭代+全校验+_getActiveHandles/flush 选项/exit 首码赢/前沿防抖+Create 二判据分发侧+stat 真 unref+首轮 return 收口）；残件：fs.glob ×2（Bun 快照无此行，记档另案）+ flush 三套件（待 node:test runner 深度）；黑盒 fs 18/18 + 冒烟 5/5；AGENTS §4.152-155 |
| 集成修复 ×2 | ✅ net relisten `__closing` 挂死（46 分钟）+ net SEGV（with_str_args GC 悬垂）+ stream 9b 回归（上轮） |
| 验收 | `cargo test` 全量 21 target 0 失败 0 警告 + 冒烟 5/5 |
| AGENTS.md | ✅ §4.140-144 四坑 + §4.145-147 三坑 + §4.148 net 五坑 |
| 本节欠账表 | ✅ G3/G2/G9-2/G9-3/G6 划线转绿 |

**2026-09-21 test Slice A 收官**：API 核心面 11 套件转绿（suite/ctx/tags/
plan/waitFor/subtest/getTestContext/register；对拍 83 件 SAME0 6→17、
DIFF 64→53，零回归；黑盒 `phase10f_test_*` 四件；AGENTS §4.178）。
残 53：run API ~21/mock 全家并入下轮、spawn CLI ~18 与 reporter ~13 维持另案。

**2026-09-21 test Slice B1 收官**：MockTracker 核心落地
（`node:internal/test/mock` 新建 + 钩子归属重构为真机 Test.run 口径；
mocking.js 55/56，唯一红为私有字段 V8 文案引擎偏离；82 件零回归；
黑盒 `phase10f_test_mock_*` 两件；AGENTS §4.180）。
残：mock-timers 2 件（B2：fake 计时器基建）+ run API ~21 另轮。

**2026-09-21 test Slice B2 收官**：mock.timers 落地（对拍 SAME0 17→19、
DIFF 53→51，零回归；黑盒 `phase10f_test_mock_timers_*` 两件；
AGENTS §4.181）。残：run API ~21 另轮（`run()` 事件流）。

**2026-09-21 test Slice C 收官**：run(none) 事件流落地（同进程加载 +
事件六件 + 发现 + only/tag/plan 门 + 钩子时序全对；对拍 SAME0 19→24、
DIFF 51→46，零回归；testmod 按域拆 core/run；黑盒
`phase10f_test_run_none_and_plan_gates`；AGENTS §4.182）。
残：run process 隔离 ~15（子进程/线程传输）+ spawn CLI ~18 + reporter ~13。

**2026-09-21 test Slice E 收官**：run 语义深化（plan 子计数/stopTest 超时/
TestPlan wait/legacy done/tag 过滤子集/entryFile/调用点文件/种子洗牌/run
coverage 校验；对拍 plan/tags-events/entry-file/randomize 四转绿，
SAME0 30→34、DIFF 39→35，零回归；黑盒 `phase10f_test_run_semantics_*` +
`phase10f_test_run_tag_filter_and_randomize`；AGENTS §4.184）。
残：run 并发/上报深度 ~6 + spawn CLI ~18 + reporter ~13 + mocking 单行。

**2026-09-21 test Slice D 收官**：run(process) 经 worker 传输落地
（expect-error ×2/todo-skip/filetest-location 四转绿 + coverage ×2 附带；
对拍 SAME0 24→30、DIFF 46→39，零回归；黑盒
`phase10f_test_run_process_and_expect_failure`；AGENTS §4.183）。
残：run 并发/超时/randomize/tag 过滤 ~8 + spawn CLI ~18 + reporter ~13。

**2026-09-22 G11 http TIMEOUT 首轮收官**：18 提交（请求超时全家/101 摘池/
Trailer 校验/管线/FIN 递延/流出/abort 级联/1xx/头形态/maxHeadersCount/
keep-alive 修正/回池门/池键/ready 解禁/setTimeout 门控订正 + 黑盒
`tests/node/http/timeout.rs` 4 用例 + 构建 0 警告 + 冒烟 5/5 + http 域 17/17；
约 40 件转 SAME，http-only TIMEOUT 82→63、DIFF 105→100；
sweep2 混二进制（05:47–06:32 跨两次构建）仅当趋势，终局需干净重扫；
未闭环 3 件见上表 http 行；AGENTS §4.185）。

**2026-09-22 G11 半开双杀收官**：5 提交（写端 Close 即发 + holding/halfhold/
native 注册/JS 递延/黑盒；单测 `net_halfhold_balance` + 黑盒
`phase11_net_halfopen_releases_loop` + 冒烟 5/5 + http/net/stream/dgram
域 + bin 185 全绿；`server-keep-alive-timeout`/`server-close-idle-wait-
response` 转 SAME0；AGENTS §4.186）。

**2026-09-22 G11 upgrade 轮收官**：4 提交（升级块判定门 + 三形态 + 体路由/
直调/spill + socket 暂存/destroy 异步 + 黑盒 `phase11_http_upgrade_faces`；
node 域 271 全绿（t4）+ 冒烟 5/5；upgrade 6 件全转 SAME0；AGENTS §4.187）。

**2026-09-22 G11 头面 batch5 收官**：校验门三件（数字头名 HTTP_TOKEN/
奇数组 ARG_VALUE/重发头 HEADERS_SENT）+ 拼写覆写 + 220 unknown +
数组双行 + 对形 writeHead + Host 恒拼/IPv6 框 + 拒写旗（新码
BODY_NOT_ALLOWED，检查禁入 `_write`）+ 黑盒
`tests/node/http/surface.rs::phase11_http_header_face_batch5`；
28 件头面对拍 SAME0（`header-overflow` 的 `socket.push` 系既定另轮）+
http/net/https 域 + 冒烟 5/5 + 构建 0 警告；AGENTS §4.188）。

**2026-09-22 G11 TIMEOUT 深水第一铲**：hostname 优先 + auth 补 Basic +
CONNECT（authority-form/Host 取 path/隧道 detach 双端 end:1）+
server timeout 进门 + socket HWM 65536 + 基类 setTimeout + req.protocol +
黑盒 `tests/node/http/surface.rs::phase11_http_timeout_deep_host_auth_connect`；
11 件转 SAME0；http 27/27 + net 18/18 + 冒烟；AGENTS §4.189。
未闭环：`outgoing-properties`（wl 记账专项）+ handler 抛吞 hang（另单元）。

**2026-09-22 G11 TIMEOUT 深水第二铲**：server 选项类（IM/SR 请求期构造）+
建连选项透传（HWM 进 Socket 构造器）+ socket 双 65536/res 跟随 +
黑盒 `tests/node/http/surface.rs::phase11_http_server_options_surface`；
3 件转 SAME0；http 28/28 + net 18/18 + 冒烟；AGENTS §4.190。
附带 splitting 一件（ERR_INVALID_CHAR `["key"]` 后缀）：response-splitting
转 SAME0 + 黑盒 `phase11_http_invalid_char_key`；AGENTS §4.191。
附带 response 双件（write-after-end 拦截 + 状态码门注册）：res-write-after-end/
response-statuscode 转 SAME0 + 黑盒 `phase11_http_response_gates`；
AGENTS §4.192；未竟 response-cork（另单元）。

**2026-09-23 G11 收尾轮收官**：7 提交（cork 面 / uncaught 双向 / 小面四件 /
multi-CL；§4.193）。cork 三件套（response-cork/drain-cork/outgoing-end-cork）+
uncaught-from-request-callback + test-http-1.0 + null-prototype-options +
max-headers-count + response-multi-content-length 转 SAME0；request-timeout-
keepalive 实为绿（15s sweep alarm 误判"真机自挂"，25s 实证双边绿——§4.193 坑五）。
终局 serial 重扫（sweep4，25s alarm，干净二进制，TEST_THREAD_ID 3599）：
409 件 SAME0=294/SAME1=0/DIFF=102/TIMEOUT=13（8 件转绿逐项复核在册）。
基建轮（2026-09-23，AGENTS §4.194）终局重扫（sweep6，同口径）：
409 件 SAME0=307/SAME1=0/DIFF=91/TIMEOUT=11（+13：push 面 2 + HPE 面 5 +
记账/队列 2 + eager 连带 4；零新增红项）。残件：reuse-drained（process.report
缺失，另域）+ execPath spawn ~18（待拍板）+ parser 内省 ~4（记档偏离）。
黑盒 `phase11_http_cork_faces` + `phase11_http_uncaught_throws` + http 域 25/25 +
node 域 278/278 + 冒烟 5/5。**残件全部定性**：drain-writable-length +
outgoing-properties（outputData 记账 + writableLength 合成 getter）与
header-overflow/read-in-error（socket.push）同根——需 **net.Socket 写侧
流式化**（socket 层写队列/HWM/drain）+ **eager-parse outgoing 队列**
（管线请求立即建 res、无 socket 排队），基建轮另案；execPath spawn ~18 件
（套件 spawn process.execPath 裸脚本 vs CLI 全 flag 铁律 §0.8，需拍板）；
parser 内省 ~4（_http_common parser.initialize/onIncoming 面，记档偏离）；
余散件（async_hooks 资源面/domain 集成/Atomics.wait/process.report/
optimize-empty-requests 等）逐套件记 bun-parity。

**2026-09-23 对拍提速 mapper + createConnection 转绿**：§4.202-① 断言 mapper
落地（`tests/node/helpers.rs` run_suite_mapped：实际值截 200 字 + 套件侧
调用点折算物理行 ±2 节选三行定位；`WJS_MAP_SUITE=` + `phase_mapper_locate_suite
-- --ignored` 用；raw-headers 物理 110 / mutable-headers 物理 187 一击定位；
实测钉住：无壳位置=assert SOURCE 行号安套件名、栈帧行号=CJS 前奏 +1、
rejection 拦不到/exit-hook fatal 不触发；AGENTS §4.202）。同轮
test-http-createConnection 转绿（修前 TIMEOUT：请求级 createConnection 的
oncreate 吞 err——async cb 错永悬、sync throw 靠穿透构造器侥幸；修法真机
_http_client.js 591-607 行逐字 err 臂 nextTick emitErrorEvent + try/catch
收口 + settled 防双投；黑盒三形 `phase11_http_create_connection_error_routing`；
AGENTS §4.203）。http/net/https 三域 52 绿 + node 域 285 绿 + 冒烟 5/5。

**2026-09-23 sweep8 红件簇清扫（15 件转绿）**：四簇连修——① 重复头真机
表驱动口径（joinable+未知头恒 ', '、19 头单值表首个赢、查询面过滤自动头、
GET+用户 TE 帧化；multiheaders×5/mutable-headers/raw-headers 转绿）；
② agent 池（res.destroy 后复用 + req close 蕴含 destroyed；abort-keep-alive/
override-global-agent 转绿）；③ parser 面（TE 整词 token+teInvalid 400、
冒号空格拒收、parser 全局 freelist、writeInformation 门序三形、
optimizeEmptyRequests+IM._dumpAndCloseReadable；smuggling/te-repeated/
parser-free/write-information/optimize-empty/chunk-extensions-limit 转绿）；
④ res 侧 timeout 桥（responseOnTimeout 打 res + IM.setTimeout 自武装，
监听数契约守卫；client-response-timeout 转绿）。出局 4（internals/flags×3 +
process.report）+ 偏离 2（domain 异步/Atomics.wait）+ 预存挂 1（client-
timeout-on-connect）。残：DIFF 5（set-timeout-server/request-timeout-upgrade/
url.parse-https.request/headers-timeout-keepalive/server-capture-rejections）+
TIMEOUT 5（no-read-no-dump/capture-rejection/non-utf8-header/reject-chunked/
should-keep-alive，流控与二进制头深水）。提交 f4b8a52/029a244/834bc15/431fc0e；
AGENTS §4.204；node 域 285 绿 + 冒烟 5/5 ×4 轮。

**2026-09-25 sweep 残部二批（5 件转绿）**：server captureRejections 兜底
（nodejs.rejection 逐字）+ TLSSocket _secureEstablished + ServerResponse.
setTimeout + IM/server 超时桥带 socket 实参 + HPE 门序前置（TE+CL 先于
requireHost）+ 头串 latin1 上网 + OutgoingMessage hasInstance 品牌判定
（原型桥改道 super 全链实锤后弃用）。capture-rejections/url.parse-https.
request/reject-chunked/non-utf8-header/set-timeout-server(前四块) 转绿；
提交 2b1a1ab/566117d；AGENTS §4.204 追补；node 域全绿 + 冒烟 5/5 ×3 轮。
残 4：outgoing-message-capture-rejection / should-keep-alive / no-read-no-dump
（流控与判定矩阵深水）+ set-timeout-server 末段 exit-hold（paused client
EOF 急切检测，G6 infra 族）。

**2026-09-25 §4.202-②③ 对拍提速工具落地**：① mapper 已落地（§4.202-①），
本轮补齐 ② `scripts/flake-classify.py`（新红先分类：整文件×3 + 单块 repro×N，
GREEN/FLAKY/RED-DETERMINISTIC/NODE-FLAKY 四分流，flaky 走定级法勿深挖）+
③ `scripts/sweep-bg.py`（全量 serial sweep 双 fork 后台直跑、status/wait/
tail/stop 轮询、工件落 ~/.wjs-sweep/、失败行 stderr 首行 + debug/ 全量落盘）。
两坑实证（AGENTS §4.205）：统一 runner 给 node 带 `--run` 假红全表（node 22+
--run=跑 package.json scripts）；前缀过滤只认 .js 丢 6 件 mjs（基线 409 口径）。
dogfood 首件：**dump-req-when-res-ends 判 FLAKY（wjs 0,142,142 挂死型，
node 2/2 绿）**——挂死型 flaky，归 §4.148 流控 infra 族随 no-read-no-dump
同轮处理。sweep7 全量 409 件基线后台直跑中（终态见 ~/.wjs-sweep/sweep7/）。

**2026-09-25 sweep 残部三批（2 件转绿 + ②③工具轮）**：②③落地后逐件啃
http 尾巴——① **outgoing-message-capture-rejection** 转绿（`fcf416b`）：
ServerResponse.destroy(err) 把 err 丢在 super.destroy() 外、_destroy 永裸杀
（socket 'error' 不发）→ _destroy 从 __resErrored 找回；连带修 client 侧
体未齐断连 error 递送（destroy(__e) 被 IM._destroy 吞错口径递不出去，改同步
守门递送，真机 p4 差分序 aborted → error ECONNRESET → close 对齐）；②
**should-keep-alive** 转绿（`a683ca5`）：__release 回池门只看 Connection 头，
1.0 缺省响应 socket 被错误入池 → 复用死连接挂死 → 门改 req.shouldKeepAlive
（版本×Connection 折算）+ 池态 socket 收 EOF 即销毁摘池（node socketOnEnd
口径）。黑盒两件（capture_rejection_routing / should_keep_alive_matrix）+
家族对拍零回归。**dogfood ② 分类**：dump-req-when-res-ends 判 FLAKY
（0,142,142 挂死型）；child exec_shell_self 负载形 flaky（6/6 单跑绿）。
**sweep7 终局基线**（409 件，sweep-bg 首跑）：SAME0=375 / SAME1=6 / DIFF=19 /
TIMEOUT=9（sweep6 SAME0=307 → +68）。残件定性：http 尾巴剩 no-read-no-dump
（流控 infra）+ 时序敏感两件（request-timeout-upgrade / headers-timeout-
keepalive）+ set-timeout-server exit-hold（G6 infra 族）；sweep7 新现红
（outgoing-finished / matchKnownFields / 1.0-keep-alive [object Object] 文案 /
catch-uncaughtexception / client-parse-error / writable-true-after-close /
chunk-extensions-limit flake）另批分类。node 域 286 黑盒全绿 + 冒烟 5/5。

**2026-09-25 sweep 残部四批收官（http 尾巴 5/7 转绿）**：接三批续啃——
④ **should-keep-alive**（`a683ca5`）：__release 回池门只看 Connection 头，
1.0 缺省响应 socket 被错误入池 → 复用死连接挂死；门改 req.shouldKeepAlive
+ 池态 socket 收 EOF 即销毁摘池。⑤ **no-read-no-dump**（`7c916b6`）：服务端
体背压流控 infra 四件联动（泵 backpressured 旗 / __feed 停读+pause /
Socket pause/resume 事件 / req._read 消费即解暂停）+ res 完成清 framing 根修
（体在途字节被当新请求头 → HPE 断连）。⑥ **时序敏感两件**（`2ce37ef`）：
Host 校验搬位到升级检测后（node parserOnIncoming 头部对 upgrade return 0）+
劫持撤计时 + headersTimeout 计时模型统一（连接建立/新消息首字节开、请求完成
撤、空闲归 keepAliveTimeout）。黑盒新增三件，node 域 288 全绿，冒烟 5/5；
AGENTS §4.206。**残件**：dump-req-when-res-ends（挂死型 flaky——今日根因
定位到流端口 flowing 排空语义，dump 机制需先修 readable_flow push/flow，
独立轮；§4.206 坑二）+ set-timeout-server 末段 exit-hold（G6 infra 族）+
sweep7 新现红件分类（outgoing-finished/matchKnownFields/1.0-keep-alive 文案
等）。**sweep8 终局**（409 件）：SAME0=379 / SAME1=6 / DIFF=16 / TIMEOUT=8——五件
修复零红；但暴露两件**本轮回归**（outgoing-flush-drain TIMEOUT +
upgrade-large-body-unread DIFF，sweep7 均绿）。**回归根修**（`6a2d64e`）：
二分三段实锤 chunked 泵背压 early-return 为元凶（终结段扣 fr.buf 等再喂
而包不会再有 → 泵停摆）；终解=背压改**状态驱动事件**（缓冲 ≥HWM 发
'pause'、落回发 'resume'，泵不停读不中断，缓冲有界=体长）+ 泵恢复无早退形；
两回归转绿 + 五件守卫绿 + node 域 288 全绿（fifo 按 §4.175 剔除）。AGENTS
§4.206 坑四/坑五（分离 HEAD 提交：bisect 后直接 commit 落 detached，父=旧
提交缺后续修复——cherry-pick 回 master 解）。

**sweep9 终局（本轮收官基线）**：409 件 SAME0=381 / SAME1=6 / DIFF=16 /
TIMEOUT=6——**七件修复全零红**（sweep6 基线 307 → +74）。残件：dump-req-when-res-
ends（挂死型——dump 机制需先修流端口 flowing 排空语义，独立轮）+
set-timeout-server 末段 exit-hold（G6 infra 族）+ sweep8 散红分类
（matchKnownFields/outgoing-finished/1.0-keep-alive 文案/catch-uncaughtexception/
client-parse-error/writable-true-after-close + node 侧独红的 loader 形 8 件）。

## 旧版"新会话入口"（2026-09-20，已作废，见 plan3 §0）

**新会话入口（按优先级，2026-09-20 G8 轮后更新）：**

1. **G5 child ~30 件 → G8 fs watch ~23 件 ✅ 双收官**（G5 25 套件 + G8 30 套件；
   AGENTS §4.150-155；残件：fork/IPC handle 传递出局 + fs.glob×2/flush 三套件
   待 node:test + enoent-after-deletion 间歇超时另查）。
2. **fs 残簇**（2026-09-21 七轮收官：cp/write/read/handle 129 件 70/59 →
    **126 SAME/3 DIFF**，e2f0d28/6412095/b2a473e/4703a16/4f9cc70/七轮读流，
    残件与黑盒见 bun-parity fs 七轮注记）：残 3 全另案——read-worker ×1
    （worker fd 移交，归 G6 残件同族）、eagain/flush ×2（node:test mock，
    runner 深度）；expose-internals ×3 跳过类；pull/writer ×3（需
    stream/iter+zlib/iter 新模块，另轮）；stream 余 err（增量流重写轮）+
    write-patch-open（fork 父端 exit，child 域）；黑盒 fs 26/26 + 全量
    cargo test 21 target 0 失败。
3. **G6 残件 6 件**（infra 级，需独立轮）：throttle（native 读门控+写 EAGAIN
   流控）、cluster×2（internalMessage 协议）、worker×3（跨线程 fd 移交）。
4. **大簇另案**：G10 http2 compat ~105 件、G11 http TIMEOUT ~110 件、
   dgram 余 ~32 件、http OutgoingMessage 缓冲模型 5 件（G3 遗留专项）、
   async_hooks 资源面（FSREQCALLBACK 生命周期，fs roundtrip 末段牵引）。
5. **URLPattern 归属**：对 Bun 1.3 实测后拍板（见上"待核对"）。

（编号对表：G4=fs validators 尾件行、G5=child_process 行、G6=net 尾件行、
G8=fs watch 簇行、G10=http2 compat 行、G11=http TIMEOUT 行；
G1/G2/G3/G9 已收官。）

**新会话开场提示**：先读 AGENTS.md §4.140-149（三轮十一坑，尤其 §4.140
"手工过/cargo 挂≠环境问题"、§4.142 "禁连续建 worktree"、§4.145 跑分
"exec or die"+glob 路径）+ 本节欠账表；跑分 `TEST_THREAD_ID` 用 35xx+
（§4.122 互踩防线）；net 域黑盒先单跑验证（`cargo test --test node net`，
全绿后再并发——§4.140/§4.141 两坑都在 cargo harness 时序下才现形）。
