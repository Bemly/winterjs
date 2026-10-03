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

## 2026-09-25 P1 http2 首轮（本会话）

- 基线 base12（全域 Bun 清单 3574 件，2 并发 + 看门狗 42 分钟）：绿 2168（60%）。首跑 base10 因 D1
  前缀旗放行致 `test-promise-unhandled-flag` 自 spawn 无限递归、整机 panic（pitfalls 4.209），作废。
- 全局修复：`process 'exit'` 从未触发（this 绑错，mustCall 退出核对从未执行——此前假绿）、
  全局 console 走 util.format、require 透传原异常 + CJS 行号物理对齐、未捕获错误 node 形渲染、
  `new URL` 错误 node 形（ERR_INVALID_URL）、X.509 v1 证书（tls/https/http2 共 37 件同因）。
- 提速：`WINTERJS_HANG_EXIT`（挂死件报出未触发回调的创建点）、nextest（全量 ~100s）、
  sweep `--jobs/--scope/--rerun-red`/node 缓存/资源看门狗/进程组封顶、`scripts/split-js.py`。
- http2：64（旧口径）→ 88/256（真口径），余件见 plan3 §0.4 P1 行。

## 2026-09-26 P2 共性簇 + tls 首批

- base13（2093）较 base12（2168）净 −75：exit 事件修好后 mustCall 核对生效，143 件旧假绿翻红
  （聚类：`HANG` 挂死自报 108 / `MUSTCALL` 计数不符 132），非退化。sweep 标签由此拆分。
- 共性簇按"未触发回调的监听事件名"聚类找根因：beforeExit（9）、expectWarning（9）、fs 流 close/error（~20）。
  - process：beforeExit 派发、致命错先渲染再发 exit（exitCode 可改）、公开 emit 抛错上抛（4.211）。
  - emitWarning 逐字移植（缺省打印是表内监听，off/once 生效）；CJS 以绝对路径编译（node 栈帧口径）。
  - fs 流逐字移植 `internal/fs/streams.js`；暴露底座两处时序偏差：fs 回调微任务 → setImmediate、
    setImmediate 钳 1ms → 不钳；unref 定时器循环不 alive 不触发（4.212）。
  - 顺手：dgram bind 前 unref 落原生、repl .save/.load 逐字、ClientRequest.setTimeout 同步校验。
- base14 2106、base15 2136。tls：SecureContext 层逐字移植 + 错误码补齐，tls/https 单域 +23（base16 计）。
- 工具：`~/wjs-data/node-lib/` 导出 node 26.8.2 全部内建源；sweep 期间开发用 `CARGO_TARGET_DIR=~/wjs-data/target-alt`。
- `test-socket-write-after-fin-error` 回红：旧绿靠 immediate 1ms 时序；忠实修法需建模 shutdown 完成 → autoDestroy
  （试过 onend 下一 tick end()，连带 write-after-close 反红，已回退）。`exec-maxbuf` 两版本同样偶红（既有 flaky）。
- 待办：tls-b 内存 BIO 引擎（`tls.connect({socket})` 簇）；`test-http-keep-alive-max-requests` /
  `test-stream2-httpclient-response-end` 负载下偶红（immediate 不钳后时序敏感，单跑稳定绿）。

## 2026-09-26 P2-tls-b：TLSSocket 包裹引擎（rustls 由 JS 字节驱动）

- 仓库上 GitHub：私有库 `Bemly/winterjs`（master 跟踪 origin；此前无 remote 单副本）。
- `tls.connect({socket})` / `new TLSSocket(duplex)` 簇（base15 时 21 件红）全链落地：
  - 引擎 `src/builtins/node/tls_wrap.rs`：rustls 手动模式（read_tls → process_new_packets →
    write_tls/reader 排空），引擎表 `PlainState.tls_engines`（state/tls_wrap.rs，与 net 共用 id
    计数）；natives `__wjs_tls_wrap_{open,feed,write,eof,shutdown,kill}`——wrapped 密文 JS 喂入，
    回程 JSON 一次带密文/明文/握手旗/校验捕获/信息（明文逐轮排空防 rustls received_plaintext 撑满）。
  - CaptureVerifier：node 口径"校验失败不中止握手"——错误捕获后随 'secure' 回传 JS 处置；
    rejectUnauthorized:false（直拨面同样改捕获式）→ authorized=false 连接存活，真机口径。
  - tls.js：TLSSocket 构造器对齐现行 node（无 options 交换形；非 Duplex socket 参 TypeError；
    allowHalfOpen 有 socket 参即取 socket 自身）；_start/_finishInit/onConnectSecure/onConnectEnd
    逐字（'secure' 恒发，握手期 'end' → ECONNRESET 带 path/host/port，secureConnect 仅包装层发）；
    convertALPNProtocols 全家原文；UDS path 形（client 内建 net.Socket + 包裹引擎，server
    tls_listen UDS 分支：UnixListener+TlsAcceptor，连接走 ConnectionUds）。
  - net 面最小钩子：write/end/destroy 收敛 `__nativeWrite/__nativeEnd/__nativeKill`（net 本形
    直通原 native，包裹面覆写）；destroySoon 原文补齐。直拨面握手 EOF → End 事件（onConnectEnd 面）。
  - io_code 补 ECONNRESET(54/104)、ENOTCONN(57/107)（收尾 RST 曾落 UNKNOWN，4.213）。
  - https.Server：ALPNProtocols 缺省 ['http/1.1'] 原文口径；createServer 经 Server()。
- 结果：wrap 簇 24 件 16 转绿（EADDRNOTAVAIL NaN / HANG / mustCall 不触发三簇清零）；
  黑盒 tls/https 7/7；全量 nextest strict 717/717；冒烟 5/5。base16 待跑全域 sweep 计数。
- 关键坑：tls.Server.listen 字符串参曾是 host，误改判 UDS path（含 "/" 才是 path，node
  isPipeName 口径）——一度全量 tls/https listen(0,"127.0.0.1") 崩、address().port undefined（4.214）。
- 余 8 件（挂死收尾簇为主，下轮按 §0.2 时间盒）：`test-tls-socket-close`/`-destroy`/
  `-default-options`/`-streamwrap-buffersize`（收尾 net 不归零）、`-on-empty-socket`
  （late teardown error 误上抛）、`-client-destroy-soon`（'readable' 流量面）、
  `test-async-wrap-tlssocket-asyncreset`（ca 链另案）、`test-tls-wrap-econnreset-*` 已绿。

## 2026-09-26 P2-tls-b 第二轮：accept 重排 + EOF 适配（wrap 簇 19/24）

- `tls_listen`（TCP/UDS）：connection 在 TCP accept 即发（node net.Server 口径），
  TlsInfo 随握手完成到 conn 侧——'secure'/'secureConnection' 点位后移；握手失败发
  tlsClientError + conn Close。secureConnect 双发清零（直拨 __ev 只发 'secure'，
  secureConnect 归 onConnectSecure）。
- `TlsCleanEof` 读端适配器：rustls 对 FIN 无 close_notify 严格报 UnexpectedEof——
  node/OpenSSL 同场景是干净 EOF。socket-close/on-empty 的 "connect UNKNOWN" 噪声
  （无监听即崩）由此根除（4.215）。
- Server 内部 'connection' 监听（手动升级形 tlsServer.emit('connection', rawSocket)）；
  TLSSocket._destroySSL / bufferSize wrap 镜像 / Duplex 包裹即时起手。
- 结果：wrap 簇 19/24。余 4 件定性：`test-tls-streamwrap-buffersize`（阻塞于
  **stream.Duplex push(null) 不发 'end'**——duplexPair EOF 面断，stream 域另案）、
  `test-tls-socket-default-options`（收尾 net=5 待建模）、`test-tls-client-destroy-soon`
  （'readable' 流量面，net 域共性）、`test-async-wrap-tlssocket-asyncreset`（ca 链）。
- sweep 定量：tls 域（test-tls-，185 件）SAME0 40→**80**；https 域（59 件）18 绿——同构建对照（17≈18）证实 base15 的 31 系口径差异非回归；base16 全域待 P2 收尾统一跑。
## 2026-09-27 Vue/Vite 生态实测（26.9.27 release 二进制，vite 8.3.1 + vue 3.5.43，~/wjs-data/probe/vue-app）

- **✅ Vue 3.5 SSR 开箱即用**：`vue/server-renderer` renderToString 输出正确（ssr.mjs）。
- **✅ vite 8 模块图完整加载**：纯 ESM 深链 import 无一失败。
- **✅ N-API 原生 addon 生态**：rolldown 1.2 binding（.node）与 lightningcss 都能加载，
  `rolldown.build` 原生打包调用跑通——napi M0-M6 的实战首考通过。
- **✅ vite createServer 全链**：config 解析（vite:config/env 全过）、插件管线
  （vite:oxc/builtin:vite-resolve 等 26 插件）、pluginContainer.buildStart、
  **dev server LISTENING**（http://localhost:5199/）。
- **❌ 首个 HTTP 请求挂起**：raw socket TCP 能连，请求后无响应——请求处理管线
  （transformRequest/中间件）某处挂，transformRequest 探针另出现
  「unhandled rejection 拒因为 Promise → 输出 [object Promise] 且提前退出」怪象，
  同文件三连一致、与相邻文件行为不同，未定位（下轮首要）。
- **❌ vite CLI 直跑静默退出**：bin 的 `import('../dist/node/cli.js')` 后无输出 rc=0；
  cac/argv 形状（process.argv=[bin, script]）待查。
- 未测：vite build、HMR、Vue 客户端 hydration。
- 工程面：`--run x.js -- --help` 的 `--` 收尾口径（4.61）再次生效。

## 2026-09-27 P2-crypto 首簇：三件同源簇转绿（getCipherInfo/Sign/Verify）

- base15 crypto 红 83 件按 `wjs_err` 聚类：MISSING-EXCEPTION 9 / VALUE 6 / keygen-UNHANDLED
  散簇 / STREAM-PIPE 4 / INTERNAL-MODULE 3（`--expose-internals` 件，node 侧过、我方
  `Cannot find module 'internal/crypto/*'`）/ NO-BINDING 2 等。首刀选三件同源簇：
  `getcipherinfo`（nid 往返 deepStrictEqual 红）+ `classes`（`crypto[clazz]` undefined）
  + `sign-verify`（`Sign is not a function`）+ 附带 `verify-failure`（class 无 new 直调抛）。
- 根因二：① `getCipherInfo` 只认名字符串（nid 入参回 undefined）且无视 options；
  ② `Sign`/`Verify` 用 class 直出（无 new 即抛）且漏进默认导出表（`__api` 无此二门，
  `typeof crypto.Sign === "undefined"`，`createX instanceof X` 全灭）。
- 修法（`3c205a2`）：`getCipherInfo` 逐字移植 `internal/crypto/cipher.js`（空串/nid
  越界→undefined；非串非数/非法 options→`ERR_INVALID_ARG_TYPE`；key/iv 错配→undefined；
  ccm iv 7–13、ocb iv 1–15 可变窗；ocb 三档仅元数据，create 保持 Unknown）；
  ccm 192/256 nid 勘误 897/898→899/902（真机）；`Sign/Verify` 改 legacy 函数形
  （Cipheriv 同款无 new 包装）+ 进 `__api`。黑盒新增 `p2_crypto_cipherinfo_nid_options`
  + `p2_crypto_sign_verify_nonew`（正常/报错/边界）。
- 结果：三件转绿 0（`getcipherinfo`/`classes`/`verify-failure`，run1.sh 复验）；
  crypto 黑盒 29/29、bin 单测 216、冒烟 5/5。`sign-verify` 行 57 止步：
  真机探针证实 `this[kHandle].sign` 内 native 写 `.library`（C++ 层，JS 无此概念，
  `grep library internal/crypto/*` 零命中）——结构性不可复刻；且该套件后续要
  `Sign` 真流式（`s.end()`），与 STREAM-PIPE 4 件同源，记档 P2-stream，不追。
- 顺手：`strip_banner_flags` 未用导入致 2 警告，去之回基线 1（linker 环境音）。
- 下一站：crypto MISSING-EXCEPTION 9 件（逐件小校验）或按队列转 repl（64+1）。

## 2026-09-27 P2-repl 首两轮：input 簇 +8，方法面 +6（17→31/82）

- R1（`af13ccc`）：base15 repl 红按 `wjs_err` 聚类，`input.on is not a function` ~10 件
  二形态——legacy 位置形 `start('', stream, eval)`（string 首参被吞成 `{prompt}`，
  input 空）与 `new Interface(options)` 直构（位置形构造器误吃 options 对象）。
  真机核对：`createInterface({})` 无码 `input.on` 与我方逐字同形——抛错不动，
  修上游误喂。readline Interface 构造器 options 归一（node internal 180 行口径）；
  write 入流排空 + 关后 `ERR_USE_AFTER_CLOSE`（新增码）+ 写即 resume；
  终端历史多行倒序存（reverseString 口径，单文件单行格式使然）+
  removeHistoryDuplicates 清全表；repl legacy 位置形 + 双缺 stdio（lib/repl.js
  299 行）+ `write` 直通 + writer 携 options。黑盒 2 新增；readline 4/4、repl 2/2。
- R2（`7623609`）：复验 repl1（65 红→8 绿：另含 multiline/nested-repls/evalcallback
  被 write 带绿）。方法面：defineCommand 函数形 + help 版式（排序/最长+3/裸名/
  Ctrl 尾行；editor 终端独有致列宽 6）+ editor 缓冲/C-d 求值/空行收尾（.save 末
  换行 node 同款）+ complete 空回（completer 实现另案）。再 +6；
  repl2 复验 57→6 绿。黑盒 1 新增；repl 4/4、readline 4/4、bin 215、冒烟 5/5。
- 坑（记 pitfalls）：同流自回显即真机亦无限递归（黑盒改 duplex 映射避开）；
  `{}` 抛错与真机逐字同形时禁"修正"（先对真机）；多行历史倒序存反直觉但原文有注。
- 余 ~51：真 completer（save-load/computed-props/buffer/files 等约 8）/
  setupHistory（3）/useGlobal（reset-event）/internal/repl（11）/ASSERT 散件 18/
  MUSTCALL 4/sigint 2（spawn 记档面）/杂项，另轮。`--interactive` 旗缺口
  （array-prototype-tempering HOST，wjs=-9/TIMEOUT）记 D1-CLI 另案。

## 2026-09-27 P2-repl R3：子集补全 +8（31→39/82）

- 转绿：save-load（成员补全）/computed-props（串数下标+大小写不敏感）/
  buffer（键枚举去下标/非标识符）/files（fs 路径：既存目录列子项裸名）/
  new-expression（new 剥除）/nosideeffects（调用形恒拒）/custom-completer
  （同步返回形包回调）/on-editor-mode（公共前缀收敛）。
- 关键真机口径（逐项实测）：补全过滤大小写不敏感；`allowBlockingCompletions`
  为 fs 面开关（无之回空）；目录如内列子项且 completeOn 置空；分组/三元纯
  表达式可求值（调用形才拒）；bare `Uin` 经 getOwnPropertyNames（SM 全局键
  非枚举，keys 不可用）。
- 坑：path 分支劫持含引号成员行（分支重排：成员→路径→等号段→拒答→bare；
  walk-fail 即拒不穿透）；模板字面量拒答误杀纯串（纯模板放行/tag·插值拒）；
  同流自回显教训重申（4.221）。
- getters 套件 5/7：余 proxy 两块需 `isProxy`（恒 false 引擎缺口，native 活，
  另案问用户）；plain/getter 拒绝面全过。黑盒新增 `p2_repl_subset_complete`。

## 2026-09-27 REPL 渲染修复（阶梯/prompt 竞争）+ 文档 pane 签名化 + R4 收尾

- 用户实测报 UI 三症：① 函数体回显多行阶梯右移；② miette 错误框碎片错位；
  ③ 报错贴下一轮 prompt 后；④ 补全文档 pane 只有 "timer function" 一类短语，
  无传参/输出提示。
- 根因（pty 抓字节实锤）：读行线程 `read_line` 阻塞时终端处 crossterm raw
  mode（OPOST/ONLCR 关），主循环直写终端的多行输出裸 LF 不回车即阶梯；
  readline 线程发行后立即回环渲染下一轮 prompt，与求值输出竞争（详见 4.225）。
- 修法两层（`7f…` 渲染修复提交）：① 哨兵协议——发行后 drain 旧哨兵 +
  `blocking_recv` 等"本轮输出完毕"，期间不进 read_line（主循环端
  `pending_flush` 旗，行处理轮 pump/rejection 收尾后发）；② REPL TTY 会话
  `REPL_TTY_OUTPUT` 旗 + `crlf()` 工具，console emit 与 repl_out/
  print_completion/error render_string 统一 CRLF 化（覆盖哨兵后异步窗口，
  管道/黑盒路径字节恒等不受影响）；附带 SIGINT 置忽略（哨兵窗口防内核默认
  终止）。`error.rs` 拆 `render_string`（render 文本恒等）。
- 文档 pane（用户口径"要传参/输出提示"）：`candidates()` 全表签名化
  （`setTimeout(cb, ms?, ...args) → Timeout` 形，40 项）。
- R4 收尾：`p2_repl_options_surface` 挂死＝同流 input/output + terminal:true
  自回显（4.221 真机同挂，测试写法对齐 bug）——input/output 分离复验过；
  `completer_faces` 断言随签名表更新。
- 验证：pty 字节复验四场景（回显/错误框/console 多行/timer 异步）全行首对齐；
  repl 域 nextest 30/30；冒烟 5/5（target-alt）；全量 strict 见 plan3 §0.4。

## 2026-09-28 P2-repl R5：CLI 补全接真上下文（Tab 与实际环境打通）

- 用户实测指出：REPL 里 `global` 明明是对象，Tab 补全却只出静态表里的
  `globalThis`——CLI 补全与真实环境脱节（R3 只把真上下文补全做进了
  `node:repl` 模块，CLI reedline 仍是 Rust 静态表，成员形更是全拒）。
- 方案：CLI 补全跨线程接 `node:repl` 的补全核心——`__defaultComplete`
  增 `__ctxEval` 的 globalThis 分支（间接 eval，与 CLI 经典脚本求值面同源），
  新导出 `cliComplete(line)`（同步返回 `[list, completeOn]`，同挂默认导出，
  4.218 教训重申）；`JsCompleter` 改双源——Tab 请求经通道投递主循环
  （`JSContext` !Send，真上下文枚举只在 JS 线程），JS 线程桥
  `__wjs_cli_complete`（会话启动时注入）求值回 JSON，id 配对防迟到旧包，
  150ms 超时降级静态表；`completeOn` → reedline span 换算
  （非行尾段拒映射）；bare 面动态候选与静态表合并去重（静态描述补充）。
- 坑两枚：① `cliComplete` 只挂具名导出致 `require` 面 undefined（4.218
  重演，挂默认导出解决）；② 桥内与主循环脚本各 `JSON.stringify` 一次
  ——双重编码使 Rust 侧拿到字符串而非数组，`v.get(0)` 落 fallback 空集
  （pty 插桩定位：REQ 到、回包到、items=[]）。桥改返对象，主循环统一编码。
- 验证：黑盒 `p2_repl_cli_complete_bridge`（bare 含 global/成员链/大小写
  不敏感/调用形拒答）；repl 域 32/32；pty 端到端 `gl`+Tab 出 `global`、
  `console.lo`+Tab 出 `console.log`；冒烟 5/5；strict 见 plan3 §0.4。
- 补遗（同轮）：`console.`/`global.` 点后**空前缀** Tab 仍 NO RECORDS——Rust 侧
  `prefix.is_empty()` 提前返回挡在动态请求前（R3 JS 侧 filter="" 全键枚举本就
  支持）；修为成员形空前缀直发动态，黑盒补 `dot-empty`/`global-dot` 两断言，
  pty 复验 `console.`→log/assert、`global.`→全键。

## 2026-09-28 R5 方向纠正（用户拍板）：winterjs repl 底座显性化，node:repl 反向复用

- 用户指出根本架构问题：**CLI REPL 是 winterjs 本体，node:repl 兼容面应反过来
  骑 winterjs 自身 repl 底座**——而不是把 CLI 专用能力塞进 node:repl。并要求
  审计全仓同类"方向反了"的域。
- 审计结论（plan3 §7 详表）：node:* ~50 域中 **13 域方向正确**（timers/buffer/
  stream_web/console 无流面/process/path/crypto/sqlite/worker/vm/url/test/http），
  ~25 域语义独立无对应物，**实锤反了 3 处**：① repl（CLI 与模块完全平行，
  求值面都不同源——CLI 每行独立 evaluate_script vs 模块 vm context）；
  ② readline 补全语义（R3 核心在模块、CLI 曾是静态表——桥接方向对但放置错）；
  ③ console 格式化（全局 console Rust join_args vs node:console util.format，
  部分反，待议）。
- 本轮落点（位置纠正）：node:repl 公开导出面恢复 node 真机同形（删
  cliComplete/签名表），补全核心经 `globalThis.__wjs_repl_default_complete`
  注册内部面（`__wjs_` 惯例，不进导出）；CLI 桥 `__wjs_cli_complete` +
  `__SIG` 签名表（~120 条，SM native toString 无形参名故手写；用户函数
  toString 真形参优先）+ 描述摘要（描述符沿链安全读不触发 getter）全部
  搬 **prelude/repl_complete.rs**（winterjs repl 底座第一块显性域）。
  Rust 协议扩为 `(全文, 描述)` 对，IdeMenu 右侧 pane 成员方法也有签名。
- 坑：`__wjsReplSig` 普通对象字面量查裸键沿**原型链**命中 Object.prototype
  同名方法（propertyIsEnumerable 查表拿到函数自身，desc 序列化 null）——
  查表对象一律 `Object.create(null)`（4.23 同型教训的查表版）。
- 验证：黑盒九断言（公开面干净/注册/bare/成员/签名/类型摘要/拒答/点后空/
  global.）；repl 域 32/32；pty `console.` 首屏带 `log(...args)`、
  `assert(cond, ...data)`；strict 见 §0.4。
- **方向铁律（入库 AGENTS §6）**：node:* 兼容面是 winterjs 自身能力的
  下游包装，禁把 CLI/产品专用能力放进 node:* 公开导出面；共享逻辑经
  `__wjs_` 内部注册面复用。repl 深度统一（CLI 求值改骑 vm context、
  CLI=REPLServer 默认实例形态）另案 plan3 §7 待拍板。

## 2026-09-28 R6：repl 求值面统一（top-level await 落地；§7 反关系纠正①）

- 用户拍板"把反的全部纠正了"。事实摸底：CLI 的 let/const 跨行**已天然成立**
  （SM 经典脚本 global lexical env 跨 script 持久，真机同形）；求值面实质差异
  = **top-level await**（node lib/repl.js defaultEval 经 internal/repl/await.js
  的 acorn 重写 + awaitPromise 收割；CLI 直接不支持，带 hint 文案）。
- 落法（无 acorn 的线性近似）：原码编译失败且行含 `await` → 试 async-IIFE
  包装重试；包装文本经底座桥 `__wjs_repl_tla_wrap`（prelude/repl_complete）——
  **末条顶层语句改写 `return { value: (expr) }`**（node await.js 同款语义：
  防 async 返回对 Promise 值二次解包；声明/return 结尾不改写）+ `.then` 双臂
  装标记对（rejected 不进 jobqueue 的 unhandled 收割）。挂起 promise 存
  `state.repl_tla`（§4.40 Heap+trace；跨轮/跨 GC），主循环逐轮 pump 后
  `settle_tla` 查结算（4.116 realm 回落 + wrap_cx）——Resolved 打印完成值、
  rejected 当场 realm 内转 engine pending 取信息，realm 外 D4 渲染。EOF 时
  drain（30s 上限）。非 TTY 分支同步兵背压（行按序，挂起期间管道阻塞）。
- 坑三枚（调试实录）：① 跨 `.await` 的栈式 RootedGuard 存 Promise 不可靠且
  违反 §4.40——改 state Heap 槽；② pump 后未回主 realm 直接裸 JSAPI
  （JS_GetProperty→Atomize 空指针，macOS 崩溃报告定位）——settle 全包
  AutoRealm；③ 初版包装无 return 改写，P1 resolve undefined 值被吞——
  补末表达式改写后才通。`set_pending_exception` 的 rejected 处理也须
  realm 内（同 ①②）。
- 缺口（记 §7）：声明提升（`let a = await x` 跨行存活）为 node acorn AST
  重写语义，待拍板引 acorn（vendored JS）后逐字移植；嵌套 Promise 双解防护
  已随 `{ value }` 包装落地。
- 验证：黑盒新增 repl_tla_await_resolves/rejects/in_async_fn_untouched
  （repl 域 35/35）；管道四场景 + pty TTY 两场景全对（41/tick/boom 渲染/
  后续行继续）；strict 772/773（child exec 单跑 1s 过=并发偶发）；
  冒烟 5/5；行数守门 ok（state/mod.rs 压行 1000）。

## 2026-09-28 R6b：声明提升（acorn vendored 落地；§0.5 拍板）

- 用户拍板引包：acorn 8.18.0 / acorn-walk（node 26.8.2 deps 内建同款，MIT，
  `wjs-data/node-lib` 取得）——prelude/vendor/ 切片 7 片（split-js.py 字节恒等）+
  包装 IIFE 强制 UMD CJS 分支挂 `globalThis.acorn`/`acornWalk`。
- `__wjs_repl_tla_wrap` 升级为 node internal/repl/await.js 的
  processTopLevelAwait **逐字移植**（primordials 直映原生方法；Recoverable 删——
  CLI validator 保证行平衡）：末表达式 return 化 + 顶层 let/const/var/class/
  function 声明提升（跨 async 边界存全局词法）。声明完成值 undefined（node 同形）。
- 坑三枚：① split-js.py 把第一片写回原路径——项目内 acorn.js=片 1（设计使然），
  完整原件在 wjs-data/node-lib；② UMD 包装两参 `({exports:{}}, {...}.exports)`
  是**两个不同对象**——factory 写后者、module.exports 是前者空对象 → 同一对象
  双参才挂得上；③ UMD global 分支挂好的对象会被我们的尾挂行**覆盖成空**
  （单参调用时）——链路错位虚虚实实，必须 CJS 分支确定性。
- 坑四：acorn 进 PRELUDE 后 worker/child 小窗口时序测试竞态翻红（worker
  terminate 5/5 失败；全会话启动慢 ~10ms 错开 50ms 窗口）——**acorn 惰性化**：
  移出 PRELUDE，仅 REPL 会话启动注入（vendor 常量 + repl_complete::REPL_TLA_JS
  由 runtime/repl 注入）；摘除后 worker 5/5 恢复、启动开销归零。
- 验证：`let a = await 41` 跨行 42、多声明+function 提升、resolve/reject/timer
  全对（repl 域 37/37）；acorn 22 键 version 8.18.0；strict 见 §0.4。

## 2026-09-28 §7-②：console 格式化统一收尾（全局 console 全量对齐 node）

- 范围（纯 JS，零 Rust 改动、零新依赖、零 unsafe）：`REQUIRE_PRELUDE` 的 console
  包装器扩展——assert（首参字符串前缀/否则 unshift，经 wrap 后的 warn 二次格式化，
  constructor.js 475-484 行原文）+ trace（format 取 message + captureStackTrace 取帧，
  自拼首行，空消息裸 `Trace`，V8 同形；帧行 SM 口径偏离记档）+ 8 缺失方法
  （table 沿模块面 format 落盘偏离；dirxml/groupCollapsed 别名；context/Console
  惰性复用 node:console 同一类；profile/profileEnd/timeStamp/createTask stubs 与
  模块面同形）+ `__wjsReplSig` 补 7 项。
- 实测：assert 4 场景（格式化/多参/裸参/真值静默）逐字对真机；trace 首行对齐；
  8 方法 `typeof` 全 function；`console.Console === node:console Console` 同一类；
  模块面 assert/table 经包装器同行为；冻结内建套件双侧 exit 0（包装器先于冻结安装）。
- 连带红一枚：`p2_repl_cli_complete_bridge/dot-empty`（trace 文档 `stderr`→通用回落，
  4.226）——修测试断言为回落形 + 补签名表，strict 回 776/776。
- 量化：console 域 sweep（tag console3，前缀须 `test-console`）SAME0 2→3/16，
  余 13 DIFF（写错流/颜色/TTY/栈/proxy/toString）深水记档不追（0.2 时间盒）；
  黑盒新增 `phase11_console_global_unified`（正常+报错+边界）；冒烟 5/5；行数守门 ok。
- 遗留记档：trace 帧含 2 行 `__wjs_` 管线帧（D4 过滤仅覆盖未捕获路径，另案）；
  countReset 无标签警告沿模块面偏离；table 无列对齐沿既有偏离。

## 2026-09-28 §7-②跟进：补全文档遮蔽（4.227）

- 用户报 REPL `console.` 补全没有文档——实测手写表有文案但被 `__sigDesc` 的
  toString 优先分支永久遮蔽（JS 包装自带 `...args`）。修法：精确 dotted 路径
  表查询提到 toString 之前，Ctor/裸名回落不动（`o.assign(a,b)` 探针仍显示自身
  形参）。`console.` 全员恢复通道/语义摘要；`dot-empty` 断言回到通道信息形。
- 验证：repl/console/builtins 120 件全绿；行数守门 ok。

## 2026-09-28 irb 方向：JS 面补全文档全覆盖（node 面除外）

- 用户拍板：对标 irb 不对标 node repl；JS 面全包（WinterCG + SpiderMonkey 内建 +
  winterjs 自有），node:* 兼容面不投文档（兼容层非本体，"不是复刻 node"）。
- 语料证据（实测）：mdn/content 全仓 ~496MB，其中 web/javascript 9.1MB（1356 件）、
  web/api/console 168KB（30 件，`~/wjs-data/mdn-content` 稀疏检出可查）；
  nodejs/node LICENSE 覆盖 doc（MIT，同既有 vendoring 纪律）；
  WHATWG Console Standard 为 CC-BY 4.0 且源码内引用部分转 BSD-3——console 释义
  按此改写零摩擦；MDN 正文 CC-BY-SA（NPL-1.1 不在其兼容名单，逐字搬运须独立文件
  保留原协议头），故一律手写改写、只取事实（事实不受著作权保护）。
- 落法：`__wjsReplSig` 从签名表升级为"签名 + 一句话"（console 17 项 WHATWG 改写，
  偏离处如实写记档如 table/countReset/clear；WinterCG 36 bare + 33 成员，存在性逐项
  `--eval` 实证，FormData/navigator/SubtleCrypto 缺席即跳过）；匹配序收敛为
  "表（精确路径/completed 名）优先，具体 Ctor（非 Object）次之，toString 兜底"——
  原生短形参（`get(n)`）让位文档，普通用户对象永远真相。
- 验证：新黑盒 `phase11_repl_sig_js_docs`（正常+遮蔽边界）；strict 777/777；
  冒烟 5/5；行数守门 ok。深水不追：`crypto.subtle` 方法面、table 列对齐。

## 2026-09-28 `.doc` 整篇文档（irb show_doc 方向，用户拍板三连）

- 语料 JS 全量进仓：`mdn-content/`（1783 `.md`，7.6MB；`web/javascript` 全量 +
  `web/api/console` 全量 + WinterCG 接口页；图床不要，原件不动，路径即出处；
  `ATTRIBUTION.md` 载 CC-BY-SA 署名）。只做 `.doc` 命令（CLI 专属，node:repl 不动；
  TTY 走 termimad 样式，管道走纯文本）。termimad 0.35.5 MIT，闭包无 links，
  §0.5 登记进 `docs/dependencies.md` §4。
- 用户裁定（回头看最关键的一句）：pane 禁搬运文档句，文档只读语料——
  `__wjsReplSig` 回签名本位（R3 原样 + 4.227 排序），我贴进去的 94 条 MDN 句、
  23 个【】注记、80 个 WinterCG/存根表项全退（`git diff` 净删 100+ 行）。
  偏离记档住老地方（bun-parity/模块头/journal），不嵌文档串，语料更新零负债。
- 实现：`src/repl_doc.rs`（显式 slug 72 + console/SM 派生规则，存在性校验；
  sanitize 修三族宏显示文字 + jsxref；`Dot::Doc` 进点命令分发，TTY/管道共用）。
- 验证：单测 slug 全枚举 72/72 + 黑盒 `.doc` 三路 + TTY pty 实测样式化整篇；
  strict 781/781；冒烟行数全过。坑 4.228（文案撇号）。

## 2026-09-28 浮窗文档实时读（用户纠正会错意后重做）

- 纠正：pane 一句话此前是粘贴副本（94 条），用户裁定"不要搬运，每次直接读"——
  表回签名本位（R3 原样），浮窗摘要改为 Tab 时实时读语料（与 `.doc` 同源）。
- 实现：`__wjs_doc_summary(topic)` native（`builtins/mod.rs` 注册）+
  桥 `withSig` 内拼装（签名 — 摘要）；实例面（变量名）经 `Ctor.key` 再试，
  普通 Object 跳过（用户方法不受染）；桥直调与 Tab 同一代码路径。
- 坑两枚：① APFS 大小写不敏感致 slug 校验假绿，改 git ls-tree 精确校验
  （72/72）；② MDN 源码段落折行，摘要须按段取（首行截断教训）。
- 验证：strict 782/782；行数守门 ok。

## 2026-09-28 浮窗两行化 + 整段文档（用户：签名文档别挤一行，文档不止一句）

- 格式：`签名\n文档`（reedline 描述盒原生多行，50×10 容下）；文档 = 首页首段 +
  首个代码块（调用形状），机械提取（宏解析显示文字、跳引用块），无编撰。
- 验证：pty Tab 菜单签名行/文档行同屏；strict 782/782；行数守门 ok。

## 2026-09-28 文档 pane 浅底整块（irb 式区分，用户要色块）

- 落法：`doc_description_style()`（黑字亮灰底）经
  `IdeMenu::with_description_text_style` 正门刷漆，零布局风险；
  pty 实证描述行包在 `\x1b[107;30m … \x1b[0m` 里（nu-ansi-term 的
  LightGray 即亮白底 107）。
- 行内多色（签名参/返、散文/code 各异）判不可做：reedline 0.52 描述盒
  `split_string` 按字节算宽 + grapheme 裸切分，内嵌 ANSI 会被拦腰切断
  （源码实证 `menu/ide_menu.rs:1014`）；要做须 fork 菜单渲染或等上游，另案。
  行间已按"签名/散文/代码"分行落在同一浅底块里区分。
- 验证：单测钉 Style 值；pty 验块；strict 783/783；行数守门 ok。

## 2026-09-28 浮窗整块文档（用户：签名文档别挤一行，文档不止一句）

- 两处改：① 摘要加料（前两段 + 首个代码块 + Parameters 节机械提取，
  条目边界截断 600 字；`console.timeEnd` 现含 See-Timers 段 + 调用形状 +
  `label:` 参数）；② 描述只放文档（描述盒按空白重排，签名文档同放恒挤成
  一段——`__wjsReplSig` 只做缺页回落；`_斜体_` 去标记，标识符保留）。
- 验证：pty 浅底块内纯文档（`outputs a message` 在 `107;30m` 块里）；
  strict 784/784；行数守门 ok。

## 2026-09-28 签名文档分两块（用户：别挤一行）

- 落法：桥改三元组 `[全文, 左格签名后缀, 描述]`（`__dispDesc` 去名留参，
  非函数/无参即 null；`__descOf` 描述符抽取与 `__sigDesc` 共用）→ Rust
  `display_override = 全文 + 后缀`（左格签名块，只改显示；选中写入仍走
  `value` 原文——点命令 `.he` 线早有同构先例）→ 描述只放文档（右盒文档块）。
- 可行性依据（源码实证）：左列三处全 ANSI 感知（`parse_ansi` 保留转义、
  `strip_ansi` 算宽、`truncate_with_ansi` 截断），右盒 `split_string` 按空
  白重排——分块是唯一正门，行内分色仍另案。
- 验证：pty 左格 `console.info  (...data) — stdout`、右盒纯文档浅底块；
  会话 Tab/回车后求值正常、无签名串泄漏；strict 784/784；行数守门 ok。

## 2026-09-28 右盒纯文档（用户：签名不进候选框；右盒签名上文档下）

- 按截图字面落：候选框干净名（`display_override` 回 `None`，三元组退回对子），
  右盒只放文档（分隔线方案经 `split_string` 仿真证伪：24 字规则窄屏并入文档
  流、宽屏 trailingige sig，恒无干净断行——描述盒按空白重排是铁律，203 字自创
  chrome 已删；签名表仅缺页回落，零搬运原则不动）。
- 验实：仿真 `console.timeEnd` 在 30/40/46/48 列下的重排确认无断行手段；
  strict 784/784；行数守门 ok。
- 环境注记：本轮 pty Tab 探针系统性 `NO RECORDS`（首 Tab 冷 `require` 撞 150ms
  超时）——bisect 证 triple 版同症，系机器负载（Blender/ffmpeg 并跑，load 11+），
  非本轮回归；交互真机验机待负载回落补。

## 2026-09-28 空行 Tab 列全局（用户：别 NO RECORDS）

- 落法：桥展开（`s.trim()===''` → `getOwnPropertyNames(globalThis)` 过标识符、
  隐 `__wjs_` 内部面、排序；completeOn 置空）+ Rust 空前缀一律走动态 +
  `complete_span` 空串回零宽 span（光标处插入）。R3 核心 `bm === null`
  空集不动（node:repl 模块面保守口径，CLI 本体面展开，§7 方向）。
  词法绑定（let/const）不可枚举，同 R3 记档。
- 验证：新黑盒 `repl_empty_line_lists_globals`（量/有序/无内部面/空 completeOn）；
  strict 785/785（中途两连负载抖动：`read_stream_fifo_end` 超时 +
  `set_immediate_not_clamped` 计时，双双单跑 <1s 过，零交集，见 §4 先分类）。
- 待补：交互 Tab 真机验机（首 Tab 冷 `require` 撞 150ms 超时；机器负载
  load 10+ 未消除，bisect 已证非回归）。

## 2026-09-28 文档第 4 路 slug（用户：encodeURI 没文档？）

- 根因：slug 只有三路（WinterCG 显式/`console.X`/`Head.method`），bare 全局函数
  （`encodeURI`/`eval`/`Proxy`/`parseInt`…）直通 `None`。语料里 51 页全有
  （`global_objects/{lower}/index.md`，git 精确校验；`WebAssembly` 住别处不管）。
- 落法：第 4 路 bare 分支（字符集限字母数字/`_`/`$`，无穿越可能；无需 allowlist，
  `lookup` 存在性是唯一真相）。node 私货（`setImmediate`）有形无页，自然 None。
- 验证：单测 slug 形状 + 存在性 + 穿越拒收；`.doc encodeURI` 黑盒；浮窗经
  `__wjs_doc_summary` 自动带出；strict 785/785；行数守门 ok。

## 2026-09-28 文档全接上（用户：mdn 有的全接上）

- 现状：已进仓但够不着的三块——sm_head 白名单外的头（Atomics/Temporal/
  ArrayBuffer…）、Web 静态方法（`URL.parse` 的 `_static` 惯例）、语句/操作符
  （`for`/`typeof`）。`WebAssembly` 等住 `web/` 别处的，不管（没进仓）。
- 落法：`slug` 主规则不动（精确可测），`lookup` 加存在性试探（首中即返，
  段字符集限定防穿越）：`A.b` → `global_objects/a/b`、`web/api/a/b`、
  `web/api/a/b_static`；bare → `statements/t`、`operators/t`、`web/api/t`。
  白名单从此只走快路径，不再是覆盖边界。
- 验证：单测试探四路 + 双 miss；`.doc Atomics.add` 黑盒；浮窗自动带出；
  strict 785/785；行数守门 ok。

## 2026-09-28 空行 Tab 真修（用户：做好了怎么又掉了）

- 根因（实测）：桥本身通（148 项），但交互链恒超时——空行 148 项 × 逐页
  `summary` 实测 **458ms**（warm），远超 150ms 补全窗。黑盒走管道不经超时窗，
  故此前全绿，属测试盲区。认错：只验了桥，没量热路径。
- 三管：`SUMMARY_CACHE`（语料静态纯函数，会话内复用；4.41 无需复位，内容确定性）
  + 启动预热 `require('node:repl')`（REPL 专属，worker/child 不走）+ 超时
  150→800ms（成功路径 200µs 步进即时返回，cap 只管失败显示；覆盖首轮 + 负载余量）。
- 测试补盲：`completer_empty_line_live_response`（有应答假 JS 线程：候选 + 零宽 span）。
- 验证：130 件目标 + strict 786/786（load 21 下一次过）；行数守门 ok。
- 残留诚实注记：load 20+ 时 pty 首 Tab 仍可能撞窗（冷摘要 450ms+ 畸变），
  二 Tab（缓存热）即稳；终极解法是描述懒加载（reedline 无此 API，另案）。

## 2026-09-29 命名空间文档轮（Bun/Deno `.doc` 进语料）

- 语料：`scripts/gen-ns-docs.py`（stdlib）由上游 `.d.ts` TSDoc 抽取 MDN 形状页
  （散文+`## Syntax`+`### Parameters`，`summary_inner` 零改）：bun 29 页
  （`bun.d.ts`+`serve.d.ts`+`shell.d.ts`）+ Deno 53 页（ns+net+unstable；
  `listenDatagram` 注 unstable）+ WinterJS 手写 5 页；MIT 各记 ATTRIBUTION。
- 真机校准三处：`Bun.cwd`/`Deno.statFs` 真机无（删别名，node 面照常用），
  `Deno.readLink` 大写 L（小写旧别名改名）；坑 4.230（prelude 快照死引用）。
- 接线：`repl_doc.rs` 三语料路由（MDN 之后、fallback 之前）+ 签名表 86 项；
  单测 `ns_lookup_hits_generated_corpus` + 黑盒 `repl_doc_ns_pages`。
- 验证：builtins+repl 45/45 strict；vendor 与代码分两提交；冒烟 5/5。

## 2026-09-29 命名空间补全轮（Deno.version. 空集）

- 根因：R3 getter 拒入（刻意）撞上命名空间全 getter 值——`T2` 对照实证
  （数据 ✓/getter ✗/冻结数据 ✓），冻结无辜。
- 修法：值型成员全改数据属性 + `__wjs_ns_sync()`（NODE_PRELUDE 尾经 `__wjs_`
  内部面调，§7 顺向；Deno 刷新后冻结，Bun/WinterJS 保持可写）+ 文档
  `ns_lookup` 认 `global.` 前缀（`global` 是 Node 口径别名）。
- 附带回答：`Deno.version` 求值 `[object Object]` 是对的（`{deno,v8,typescript}`
  对象）；`Deno.args` 在 `--eval` 下空是 argv 本就短，`--run f -- a b` 即有值。
- 验证：builtins+repl 47/47 strict；坑 4.231。

## 2026-09-29 WinterJS.image 轮（15 格式编解码进本体）

- 范围：`WinterJS.image.{formats,info,decode,encode}`；位图 13 格式经 `image`
  （dds 双 false 进不来，avif 不在树内）；svg/svgz 经 `resvg`（解预乘）；
  jxl 经 `jxl-oxide` 首帧。质量参数全透传（jpeg quality/png 压缩+滤波/gif
  speed+repeat/pnm subtype+encoding；webp 无损无参、svg scale 记档）。
- 坑三连（全 abort 级，修后进用例）：ppm-RGBA（4.232）、farbfeld-16 位、
  exr-f32；另 TGA 无魔数须显式格式、jxl 截断零填不报错（上游流式语义）。
- 缺口诚实记：jxl 成功路径缺真 fixture（错误路径全覆盖，跟进项）。
- 验证：全量 strict 811/812（唯一红 `child_stdin_legacy` 单跑即过，负载 flake，
  与本轮零交集）；冒烟 5/5；行数守门 ok。

## 2026-09-29 站导航五分 + WinterJS 文档补齐

- 站（`sample/docs`，中英 luoli + md 镜像）：导航 `Modules` 单组拆五组
  （WinterJS 原生 / Web 标准 / Node 兼容 / Bun 兼容 / Deno 兼容）；表格同分
  （原生表置顶：storage 移出 Web 表，sqlite 拆 node:sqlite + bun:sqlite，
  bun:ffi 移 Bun 表；新增 WinterJS.image/WinterJS/Bun/Deno 四行）。
  行列锚点双向 65↔65 对过；`api-node-sqlite-bun-sqlite` 旧锚改名（站新建不久，无外链）。
- 样例：`sample/winterjs/image.js` + `namespaces.js`（双语头，全离线，`--run` 双绿）。
- REPL：`winterjs-content/image-{decode,encode,info,formats}` 四页（组机制免改码直通；
  加页须 touch 重编见 4.233）+ 签名表 4 项；`.doc WinterJS.image.decode` 可读。
- 附带修：svg scale 宽高取自未缩放 info（像素已缩放）→ 跟随 scale。

## 2026-09-29 站全白抢修（luolita 双雷 + 部署门）

- 修：`native`→`wjs`（保留字）+ cli-zh 两处多余括号（模板行 472a343 落的 +
  actions 行尾 ` }]`，与英文版逐字对）+ `pages.yml` 部署门。
- 校验法：`scripts/check-luoli.js`（原版 coffee 编全部 8 页 coffee: 段；
  另用 pug 去缩进渲染验过 api 表格 5 组 + 新锚点，属一次性探针未进仓）。
- 流程教训记 4.234：推站后等 Actions 成功再看站（本轮即此口径执行）。

## 2026-09-29 站文档单源化（api.md 并入 luoli）

- 结论：luoli 真站与 api.en/zh.md 双源并存且已裂（qrcode 只在 md+REPL 有，
  站上无；md 另有 assert/util/punycode 行站上无）。md 无站外引用
  （README 只链在线站；Jekyll frontmatter 已无消费方），遂删 md，
  luoli 为唯一真相；别名注记（storage/CompressionStream 的 WinterJS.*）
  已搬进 luoli。
- 教训：`git add` 遇已删路径整体失败——删文件走 `git rm` 后同命令再 add
  其余文件会全丢（本轮 cover 行漏提交，靠线上 raw 发现；SHA 钉死 URL
  可破 CDN 缓存验）。

## 2026-09-29 WinterJS.media 轮（symphonia+rodio+rav1e+mp4-rs）

- 拍板四件全进树：`symphonia` 0.6（直引解码）+ `rodio` 0.22（只开 playback，
  解码走直引 0.6——rodio 自带 0.5 子树已摘）+ `rav1e` 0.8（关 default：
  含 git2/nasm/cli，只开 threading）+ `shiguredo_mp4`（git tag
  2026.6.0-canary.0 钉死，零依赖）。
- 面：`decodeAudio/audioInfo`（f32 交错全量，首轨）+ `play/stop`（后台线程
  即返 id，rodio stderr 提示已关）+ `videoEncode`（RGBA→YUV420 BT.601，
  手写 IVF，不另引轮子）+ `mp4Info/mp4Samples/mp4Sample`（demux；mux 跟进项）。
  jxl 成功路径 fixture 同款缺口：mp4 用上游自带 beep-flac 进仓
  （tests/fixtures/media/，Apache-2.0）。
- 验证：模块单测 + 黑盒正常/报错/边界 + 冒烟；全量 strict 待收尾跑。

## 2026-09-29 media 去 unsafe 跟进（bytemuck 直引；W1 收尾 strict 846/846）

- 用户指认 `src/builtins/media.rs set_rval_f32` 业务层
  `from_raw_parts` unsafe 违规（§6 + §0.6）——§6 三问第①问即证伪：
  `bytemuck 1.25.2` 已在锁内（image/jxl-oxide/resvg 带入），直引零新增传递。
- 修法：`Cargo.toml` §12 加 `bytemuck = "1"` + 该行改
  `bytemuck::cast_slice(out)`；余下 `TypedArray::create` 块是 §6 引擎边界
  （不可去），注释正名。`docs/dependencies.md` §16-6 跟进；坑 4.235。
- 验证：模块单测 6/6 + `sample/media/basics.js` 全 true +
  报错路径干净 TypeError + 冒烟 5/5 + 全量
  `cargo nextest run --profile strict` **846 passed / 5 skipped**
  （repl 死代码 3 警告 + linker 环境音 + proc-macro-error2
  future-incompat，皆预存非本轮）。
- 文档顺带刷新：AGENTS 存量基线 615/1453（09-29 实测）、README pitfalls
  236 条到 4.235（4.232 重号两条如实注）、§0.4 加 W1 本体行。

## 2026-09-29 P2-crypto R1：MISSING-EXCEPTION 9 簇（4 转绿，44/120）

- crypto3 sweep（83 件 rerun 中 80 落本轮域）：SAME0=4（padding/gcm-implicit/
  gcm-explicit/secret-keygen）+ TIMEOUT 1（keygen-async-dsa：校验过后跑真 DSA，
  debug 慢超时）+ DIFF 75。crypto 域 40→**44/120**。
- 根因三：① `CbcEnc` 无 autopad 字段 + 两侧 `setAutoPadding` 空转（构造期
  options 亦丢）；② OSSL 错误缺 `reason`、解密坏填充错码（应 BAD_DECRYPT）；
  ③ generateKey 旧 `(options)` 签名 + keypair 入口无同步校验 + 编码形/rsa
  参数无校验。
- 修法：CbcEnc.autopad + `__wjs_cipher_set_autopad` 新 native（UNSAFE-BOUNDARY
  标签 + 注册；黑盒 double-final 覆盖 panic 路径）+ JS 两侧透传；`__cryptErr`
  补 OSSL reason；CbcDec/Ecb 解密坏填充改 BAD_DECRYPT；generateKey 重做
  `(type, options, cb)`（hmac 按位落盘、校验同步抛）；`__checkKeyPairHead/
  TypeKnown/Encs/RsaKeyOptions` + sign.js 同步预检；GCM `__gcmTagLen`
  （{4,8,12-16}）+ setAuthTag 即时校验 + enc 短 tag 前导切片（套件 379 行口径）。
- 真机实测 10+ 处（tag 有效集/setAuthTag 码形/key lengths/inspect 形/async
  同步抛/二参非法/modulus 无缺省…）；翻转旧断言两处（4.65）：sign.js 二参
  默认 `{}`、rsa modulus 2048 缺省——自家 ed448 黑盒同步改显式 `{}`。
- 验证：新黑盒 `p2_crypto_cipher_setautopadding` 14 断言一次过；crypto 域
  30/30；冒烟 5/5；行数守门 ok。坑 4.236（async 同步校验）。
- 留尾（下轮）：keygen 剩余（dsa/ec-curve/dh/pss 参数 + 4096 慢件策略：
  **单件 4096 keygen debug 下 53s，sweep 天花板**，全绿需 release 探或分片）；
  GCM 短 tag 解密验签（native，對称 CTR+GHASH 手工）；argon2 越界；
  pqc 错口令；enc-validation legacy createCipher。

## 2026-09-30 P3-http 冻结收口 R1：matchKnownFields + outgoing-finished 转绿

- `matchKnownFields`：base15 DIFF（`content-encoding:test` vs `test, value`）—
  单例表多收 `content-encoding`/`x-forwarded-host`（返回 `\u0000…` 系可合并，
  单例仅 18 项无前缀）。删两项 + 注释列来源；`probe/dup.mjs` 双侧同串
  （`content-encoding:gzip, br` / `x-forwarded-host:a, b` / `authorization:1` 首个赢）；
  `run1.sh` 0（node 0），坑 4.237。
- `outgoing-finished`：base15 DIFF（`closed` false）—`ServerResponse` 缺 OM 品牌
  五项致 `willEmitClose=false`，`finished` 跑在 `close` 前。构造期补
  `_closed/_defaultKeepAlive/_removedConnection/_removedContLen/_sent100`
  （+ `writeContinue` 置位）；`probe/fin.mjs` 修后 `finish→close→FIN` 与真机同序；
  `run1.sh` 0，坑 4.238。
- `1.0-keep-alive`：`run1.sh` 0（base15 即绿，复验确认）。
- 验证：http 相关 nextest 52/52 strict + 冒烟 5/5 + 行数守门 ok。
- P3 余件按原定性记档（`reuse-drained`=process.report、`client-response-domain`=
  domain 异步、`keep-alive-timeout-race`=Atomics.wait、`set-timeout-server`/
  `catch-uncaughtexception`/`client-parse-error`/`writable-true-after-close`/
  `client-timeout-on-connect`=挂死型），不再开轮。

## 2026-09-30 P2-crypto R2：argon2 越界簇转绿（44→45/120）

- 根因：async 参数校验丢进 microtask（4.236 同族）+ `passes` 下限 0（真机 1）+
  缺参错码（NaN 偷渡）+ nonce/算法文案分码与真机不合（逐项见坑 4.239）。
- 修法：`crypto_kdf.js __argon2Args`（下限/缺参门/文案/算法分码）+ `argon2()`
  先同步全校验再验回调排队（顺序先参数后回调）。
- 验证：`run1.sh test-crypto-argon2.js` 0；crypto 域 nextest 62/62 strict；
  冒烟 5/5；行数守门 ok。
- 全量 strict 附记：P3 后首跑 846 passed + `phase9e_crypto_asym_errors` 1 超时；
  单跑该件 2.2s 绿，判负载 flake（非本轮回归），下轮收尾重跑确认。

## 2026-09-30 P2-crypto R3：enc-validation + pqc-encrypted-pkcs8 转绿（45→47/120）

- `enc-validation`：`__outBuf` 复用吞非法编码（hash 口径），cipher 真机是独立
  粘住门——新 `__cipherOut` + 实例 `__decoder`（首编码粘、`buffer` 不粘、再换
  `cannot be changed`、未知 `ERR_UNKNOWN_ENCODING`；hash 侧不动）。坑 4.240。
- `pqc-encrypted-pkcs8` 三连：① pkcs8+口令导出误走传统 PEM——新 `__pbes2Encrypt`
  （PBKDF2-SHA256/2048/8B 盐 + AES/DES，PRF 带 NULL 与解密对称；der/pem 同构，
  `ENCRYPTED PRIVATE KEY`）；② JWK `AKP` 未实现——`alg` 表 6 集 + 种子形 PKCS#8
  自拼 + 既有展开派生比对 pub（零新 native；三形文案逐字对真机）；③ DER 加密体
  导入不嗅探——显式 pkcs8 + 口令 + PBES2 OID 即先解密。坑 4.241。
- `gcm-*-short-tag`：crypto3 复验双 0（R1 已修，非本轮欠账）。
- 行数：keys 1025/ec 1016 超限 → `split-js.py` 切片（909+116 / 942+74，字节恒等，
  一文件一提交，域测试绿）。
- 验证：crypto 域 62/62 strict + 冒烟 5/5 + `check-lines` ok。

## 2026-09-30 chore：语料五目录并入 `content/`（用户点名根目录乱）

- 布局（用户拍板）：`content/{bun,deno,mdn,winterjs,ns-dts}/`（mdn 内 `files/en-us`
  不动；`vendor/` 消失；历史 pitfalls/journal 旧路径不动）。
- 两提交：①纯搬移（`git mv`，字节不动）；②接线（`repl_doc.rs` 4 处 `include_dir!`+
  注释、`gen-ns-docs.py` 用法 + `out/ns.lower()`、`ns-dts/README` 再生命令、
  bun/deno `ATTRIBUTION.md` 源路径、`repl_complete.rs` 注释）。
- 验证：脚本新 invocation 重跑 29/29 + 53/53 零 diff（幂等成立）；`touch repl_doc.rs`
  重编（4.233）；`.doc fetch/Bun.serve/Deno.readFile/WinterJS.image.decode` 四路全中；
  repl 45/45 + 全量 strict **847/847** + 冒烟 5/5 + `check-lines` ok。

## 2026-09-30 docs(site)：WinterJS sys 一行拆 12 行独立表示 + 独立 sample

- 用户指认 `api-winterjs-sys` 把 shell/hex/time/retry/graph/git/oauth/transpile/log/
  mime/cookie/httpdate 12 个模块挤在一行——中英 `api-*.luoli` 各拆 12 行
  （`api-winterjs-<name>` 独立锚点/说明）+ `sample/wsys/<name>.js` 12 个独立样例
  （双语头、全离线；`basics.js` 已 `git rm`）。
- 联动：`app.luoli navWjs` 1 锚点补 12 个（侧栏按钮走 `data-anchor`，行列双向对上）；
  中英 id 集合 diff 一致。
- 验证：12 样例 `--run` 行行 true；`check-luoli.js` 8 页全 ok（coffeescript 装
  `~/wjs-data/luoli-mod`，不进仓）；余下同类堆叠行（utils/cover/net/proc/conc/os/ev）
  未动，后续同 pattern 跟进。

## 2026-09-30 docs(site)：全站堆叠行拆独立行 + 独立 sample（用户点名"写到一堆"）

- 范围：web 8 行→24 行、wjs 7 行→35 行（utils/cover/net/proc/conc/os/ev）、core
  querystring/punycode→2 行；共 132 行/132 锚，中英 id 集合一致、导航双向零缺口。
  保留别名/变体行（storage/localStorage、fs/WinterJS.fs、`(+/promises)` 系同一事物，
  非堆叠）。
- 样例 78→128：每行独立文件（双语头、全离线；tcp/serve/tls 走回环，git 需检出目录，
  quic 仅接口面）；orphan  lump 文件全 `git rm`（blob-file/streams/compression/events、
  wstd/basics、wcover/b1-b6、codecs/basics 改名 string-decoder 并瘦身）。
- 实测钉住两处真偏离（写进 row notes）：`Request.clone` / `Response.json` 不存在；
  `WinterJS.ffi` 本构建恒 null（`require('bun:')` 不可用）——样例直引 bun:ffi。
- 样例数 50→128：home/quickstart 中英 + 中英 README 同步（"每个模块一件"）。
- 验证：新样例 `--run` 行行 true；`check-luoli.js` 8 页 ok；分三批提交推送
 （`9ce38e6` web / `bbc11bd` wjs / `872b857` core+计数）。

## 2026-09-30 feat(fetch)：Request.clone + Response.json 对齐真机

- 归属确认（用户问"是谁的"）：皆 WHATWG fetch 标准（四家全有），node 26.8.2
  双 `function`，本仓双 `undefined`——引擎真缺口，非文档误标。
- 真机抠出口径三则：① clone().signal 永 fresh（无信号即新未 abort，有信号即跟随
  abort，`addEventListener once` 转接）；② `json(undefined/函数/BigInt)` 即
  `TypeError: Value is not JSON serializable`（`JSON.stringify` 抛错/回 undefined
  一律吞为该错，用户 toJSON 抛错亦同）；③ `json(null)` 体 `"null"`，init 未给
  content-type 才补 `application/json`，坏 status 走构造器 RangeError。
- 实现（`prelude/http.rs` 纯 JS，零新 native）：`Response.json` 静态 + `Request.clone`
  （快照切片/serve 流 tee 源流；bodyUsed 即 TypeError）。
- 验证：黑盒 `phase11_response_json_faces` + `phase11_request_clone_faces` 双绿；
  serve 流 tee 探针绿；fetch+stream 域 48/48；样例回填 + 站偏离注去掉；冒烟 5/5。

## 2026-09-30 P2-crypto R4：keygen-async 簇 22 转绿（47→69/120）

- 真根因（坑 4.242）：异步 `generateKeyPair` 调两次 `__genPairSync`，公钥私钥错配
  （x-JWK/RSA 解密/DSA 超时同源）。同轮带走：jwk 免 type（6 件）/paramEncoding 翻正
  （der/pem 系误读，无覆盖，按 4.65 翻转）/raw 格式放行 + 兼容门提前（省 RSA/DSA
  慢生成空烧，raw 套件 9s→0.9s 快败）/JWK 曲线错码/传统缺口令改 INTERRUPTED/
  DSA 参数校验（`{}` 缺省翻转，4.65）/P-521 掩码（528 位随机 99.2% 越界）/
  PSS 约束回贴 + 非 PSS padding 禁  + salt 缺省摘要长/promisify 定制/ml
  details `{}` + 私钥 JWK pub 派生/MISSING_PASSPHRASE（DER）。
- 验证：crypto4 sweep（crypto3 红 76 件重跑）SAME0 25（R2/R3 3 件 + 本轮 22）；
  crypto 域 62/62 strict；冒烟 5/5；行数守门 ok。
- 留尾记档（时间盒止损）：bit-length（DSA-2049，dsa 0.7 无任意尺寸）/
  dh-classic（无 dh keygen）/keygen.js（4096 debug 慢天花板）/raw-slh
  （需新轮子，§0.5 待批）/pqc-key-objects×2 + sign-verify（priv-only 形态保持，
  material 模型改）/GCM 短 tag 解密/legacy createCipher。

## 2026-09-30 chore(rename)：全仓 winterjs/WinterJS→winterjs2/WinterJS2 + 仓库改名

- 动因：与 wasmerio/winterjs 重名（README 旧注记），改 winterjs2 避让。
- 范围（用户拍板三条）：logo 双 SVG 改名 + 内文 WinterJS→WinterJS2（svg 格式名不动）；
  历史文档（pitfalls/journal/plan 存档）不动；域名 winterjs.bemly.moe 不动。
- 三提交：`0830586` 机械改名（595 文件：包/二进制/JS 全局/`__wjs2_`/`WINTERJS2_`/
  tracing/语料/样例/站/活文档）+ `1f55941` 四处修复 + 本记账。
- 改名抓出真 bug 三处：env 前缀裸 `WINTERJS`（pattern 漏下划线）、prim 信封
  `slice(11)` 未跟前缀变长、c4x 向量消息串误改（fixture 禁改名）。见坑 4.243。
- 验证：全量 strict 849/849；冒烟 5/5；站 132/132 + 门绿；`gh repo rename`
  winterjs→winterjs2，remote 已切，RAW_BASE 200，Pages 成功，站 200 且新品牌。

## 2026-10-03 P2-process R1：hrtime/nextTick/chdir 校验 + release 面（21→25/82）

- 真机口径（node 原文：`lib/internal/process/per_thread.js` hrtime/
  `task_queues.js` nextTick/`does_own_process_state.js` chdir）：hrtime 非数组→
  `ERR_INVALID_ARG_TYPE`、非 2 元→`ERR_OUT_OF_RANGE`；nextTick 非函数→
  `ERR_INVALID_ARG_TYPE`；chdir 非串→`ERR_INVALID_ARG_TYPE` + 缺失路径→`ENOENT`
  （code/errno/syscall/path/dest 五件）；release `{name:'node',lts:'Jod',…}`
  （versions.node 22.12 口径）。
- hrtime 差值借位：单 BigInt 取余带负号致 `diff[1]<0`（真机秒/纳秒分开减+借位，
  nodejs/node#4751）——改分减后 `d[1]` 恒 ∈ [0,1e9）。
- hrtime 余 `%PrepareFunctionForOptimization` 行：V8 私有语法，SM 永 SyntaxError——
  引擎边界记档（hrtime-bigint.js 同理），时间盒止损。
- 工具链事故：改名漏了 sweep-bg.py/run1.sh 默认二进制路径，proc1（21/82）系旧
  二进制无效基线；修后 proc2（`target/debug/winterjs2`）25/82 方有效（坑 4.244）。
- 验证：run1.sh 4/5（hrtime 记档）；黑盒 `phase11_process_validation_faces`
  （正常+报错+边界 11 断言）；process 域 nextest 13/13 strict；冒烟 5/5；
  `check-lines` ok。

## 2026-10-03 P2-process R2：abort/内存/cpu/umask 面（25→32/82）

- 真机口径：abort 箭头函数（无 prototype/new 即 TypeError，实际 abort 由套件
  行为外保证）；available/constrainedMemory 经 sysinfo 真值（后者无约束回总量，
  套件要 number）；cpuUsage/threadCpuUsage 逐字移植 wrapProcessMethods
  （对象门→user 数门→user 范围门→system 同序）；umask 串形八进制门+数形
  uint32 门（`parseFileMode` 逐字）。
- 底座三事（坑 4.245）：macOS 无 `RUSAGE_THREAD`，线程 CPU 走 Mach `thread_info`
  真值（`mach_port_deallocate` 需手补 extern，§6 三问）；`THREAD_BASIC_INFO`
  i32→u32；errors 端口无 RangeError 子构造，范围错手拼码名文案三件。
- 余 thread-worker：卡 `crypto.randomBytes` 64KiB 配额（proc2 旧红同源），
  crypto 配额域另案，时间盒止损。
- 验证：run1.sh 7/8；黑盒 `phase11_process_resource_faces`（14 断言）；
  process 域 nextest 14/14 strict；proc3 sweep 25→32/82；冒烟 5/5；
  `check-lines` ok。

## 2026-10-03 P2-process R3：POSIX 身份设置簇（32→36/82）

- 真机口径（`does_own_process_state.js` wrapPosixCredentialSetters 逐字）：
  setuid/setgid/seteuid/setegid（validateId 数串双形 + 未知身份错 +
  EPERM 系错）/setgroups（数组门 + 逐元素门 + 未知组位错）/initgroups
  （双门 + 先解组后解用户，双未知报组错）。
- 真凶一枚（坑 4.246）：native 返回码 0/1/errno 三义共 int，EPERM 本体即 1——
  非 root 下全被误读成"未知身份"；改 0/1/-errno，setgroups/initgroups 早同口径。
- 拆分（§0.9 超限）：`process_.rs` 1104→897，身份系整块搬
  `process_cred.rs`（217 行，字节恒等，注册改址；另顺手清 `///`-on-extern
  与 mach deprecate 两告警，dsa.rs 同款 allow）。
- 验证：run1.sh 4/4；黑盒 `phase11_process_credential_faces`（无副作用路径 9 断言）；
  process 域 nextest 15/15 strict；proc4 sweep 32→36/82；冒烟 5/5；
  `check-lines` ok。

## 2026-10-03 P2-process R4：kill/原型/title 面（36→41/82）

- 真机口径：kill 逐字移植（`pid != (pid|0)` 松散门 + 信号数形直通/名形查 os 表/
  `this._kill` 可 mock + errno 成错；native 纯数值 kill）；原型链按真机五断言
  复刻（proto≠EE.prototype 但链上含之 + 自有 constructor 槽；fixup 拼在
  REQUIRE_PRELUDE 之后，prelude 主体求值时 require 尚无）；title 零 Rust 改动
  （--title 末个赢读 execArgv，缺省回 execPath 基名，set 透写）。
- 附带转绿：remove-all-signal-listeners（自发 SIGINT 面；自 spawn 可用，
  先前"CLI 裸参家族不可为"的判断被证伪——execpath/ppid/really-exit 等系
  缺 API（ppid/reallyExit）而非 CLI 挡道，R5 续啃）。
- 验证：run1.sh 4/4；黑盒 `phase11_process_kill_prototype_title_faces`（8 断言，
  kill 只验参+自检不真发信号）；process 域 nextest 16/16 strict；proc5 sweep
  36→41/82；冒烟 5/5；`check-lines` ok。
