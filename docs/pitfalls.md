# 踩坑全集（原 AGENTS.md §4，2026-09-25 迁出）

> 编号 `§4.N` 原样保留——代码注释/文档里的 "AGENTS §4.N" 即本文件 4.N。
> 新坑追加到末尾（编号续排），格式：症状 → 根因 → 修法 → 复现 → 推广铁律。
> AGENTS.md 只保留铁律摘要；**动某域前按下方索引 grep 该域条目**，不必通读。

## 索引

- 4.1 取异常堆栈必须重进 Realm，否则 SEGV（2026-09-09）
- 4.2 mozjs 153 删了旧 wrapper（2026-09-09）
- 4.3 shell 小坑：zsh 的 `=cmd` 展开（2026-09-09）
- 4.4 config 0.15 的 prefix 分隔符默认跟随 separator（2026-09-10）
- 4.5 集成测试不继承 bin 的 `#[global_allocator]`（2026-09-10）
- 4.6 tracing-subscriber 的 `init()` 已内建 log 桥接（2026-09-10）
- 4.7 `UseInternalJobQueues` 在 153 下 SEGV，改 RustJobQueue glue（2026-09-10）
- 4.8 引擎/运行时析构期 StoreBuffer 悬垂边 SEGV（2026-09-10）
- 4.9 native 内 `Rooted<ValueArray>` 注册会 SEGV（2026-09-10）
- 4.10 `WINTERJS_LOG` 被 config 误收导致启动失败（2026-09-10）
- 4.11 模块 hook 的 referrer 定位：走脚本文件名，不走私有值（2026-09-10）
- 4.12 file URL 必须规范化，否则同一模块判重失效（2026-09-10）
- 4.13 `TsconfigDiscovery::Auto` 只对 `resolve_file` 生效（2026-09-10）
- 4.14 `with_rooted`/`with_plain` 不可嵌套（2026-09-10）
- 4.15 入口 promise 捕获必须在事件循环前挂载（2026-09-10）
- 4.16 criterion bench 须 `harness = false`（2026-09-10）
- 4.17 `await` 在参数位置不报 await 错，TLA 重试须放宽触发（2026-09-10）
- 4.18 结算后退出的循环顶排空 race：progressed 轮不退（2026-09-10）
- 4.19 tower-http 默认追踪打错 target，会被默认 filter 静默（2026-09-10）
- 4.20 写文件命令的手工实测必须先 `cd` 进 probe 目录（2026-09-10）
- 4.21 同一文件的 edit 与 append 禁并行（2026-09-10，工具约束注记·非代码坑）
- 4.22 `rt`/`engine` 声明顺序即 drop 逆序，搬代码别搬反（2026-09-10）
- 4.23 `Object.create(prototype)` 实例无私有方法 brand 槽（2026-09-11）
- 4.24 同进程多次 `runtime::run`：引擎单例 + 每文件独立线程（2026-09-11）
- 4.25 clap builder 的 by-value 改造 + `value_name` 只要 `&'static str`（2026-09-11）
- 4.26 全 flag CLI 的两条铁律（2026-09-11）
- 4.27 增量解码两阶段 + BYOB 三坑（2026-09-11）
- 4.28 空工具调用可能回滚工作区（2026-09-11，工具约束注记·非代码坑）
- 4.29 serde 缺 rename 即静默丢字段（2026-09-11）
- 4.30 `mime_guess` 把 `.ts` 当 MPEG 视频流，`serve` 须自改 MIME（2026-09-12）
- 4.31 Node CJS → ESM 移植三坑（2026-09-12，Phase 9a）
- 4.32 逐字移植的解环/形态坑（2026-09-12，Phase 9b）
- 4.33 fs 补齐三坑（2026-09-12，Phase 9c）
- 4.34 net 事件循环收尾四坑（2026-09-12，Phase 9d-1）
- 4.35 node:http 回环两坑（2026-09-12，Phase 9d-3）
- 4.36 新事件域 checklist（2026-09-12，Phase 9d-4 dgram 二进宫沉淀）
- 4.37 zlib 两坑（2026-09-12，Phase 9d-5）
- 4.38 https/tls 三坑（2026-09-12，Phase 9d-6）
- 4.39 http2 请求事件双发：构造器与包层别双注册（2026-09-12，Phase 9d-7）
- 4.40 `Heap::set` 后禁移动，违者 nursery GC 必崩（2026-09-12，Phase 9d-7 总根因）
- 4.41 `#[serial]` 只保互斥不保顺序，读全局态的用例须先复位（2026-09-12）
- 4.42 黑盒标签断言禁子串，`&&` 合并打印禁弱断言（2026-09-12，Phase 9e）
- 4.43 手写密码学面的版本墙：digest 0.10 双轨 + HMAC 自架（2026-09-12，Phase 9e）
- 4.44 `format!` 拼 JS 一律绕行：花括号冲突改文件落盘（2026-09-12，Phase 9e）
- 4.45 9e 杂项小坑三则（2026-09-12）
- 4.46 `progressed` 后直接 park 会饿死 microtask-only 结算（2026-09-12，§4.18 推广）
- 4.47 `newListener` 在监听入表*之前*触发（2026-09-12，Phase 9f-2）
- 4.48 native 重名静默覆盖 + `static` 判重跨会话误报（2026-09-12，Phase 9f-2）
- 4.49 worker 早失败必须也发 rendezvous，否则主侧超时 + 事件丢失（2026-09-12，Phase 9f-3）
- 4.50 任务尾的资源释放：重构拿掉等待点后即变杀手（2026-09-13，Phase 9g-2）
- 4.51 校验必须待在 `__callNative` 闭包之外（2026-09-13，Phase 9g-2）
- 4.52 会话收尾先收半端任务，否则 teardown 噪声变 fatal（2026-09-13，Phase 9g-2）
- 4.53 quinn 默认 idle 30s：悬空会话 hang 测试，收尾必须显式关（2026-09-13，Phase 9g-2）
- 4.54 试解循环按长度分发，撞长即误判：看 OID 不看坐标（2026-09-13，Phase 9h-1）
- 4.55 k256 验签拒 high-S：验前 normalize_s（2026-09-13，Phase 9h-1）
- 4.56 Node PKCS#8 省公钥 y：`y=g^x mod p` 补算（2026-09-13，Phase 9h-1）
- 4.57 跨域 `instanceof Promise` 恒 false + 跨域求值恒异步（2026-09-13，Phase 9i-1）
- 4.58 迁移排空三件套：offer 留 target + forwarded 排空 + 分发回退（2026-09-13，Phase 9i-2）
- 4.59 CJS 互操作垫片吞掉纯 TLA 的 `.js` 依赖（2026-09-13，Phase 9j）
- 4.60 `util.parseEnv` 结果键按 ASCII 排序，非插入序（2026-09-13，Phase 9j）
- 4.61 自递归子进程的动作 flag 碰撞：脚本参数禁复用 winterjs 动作名（2026-09-13，Phase 9j）
- 4.62 stash 期间构建会污染 target，pop 后必须重编再探（2026-09-13，通用构建卫生·非本仓代码坑）
- 4.63 子进程复用自家 CLI 时透传参数必须 `--` 收尾（2026-09-13，9k）
- 4.64 成对分隔符夹在中间时 strip_suffix 必空（2026-09-13，9k）
- 4.65 宽松 API 重构到严格语义：先真机实测，再改伪语义断言（2026-09-14，M5）
- 4.66 napi-rs 3 的 Promise 转换暗面 + Either 兜底吞真因（2026-09-14，M5）
- 4.67 vite dev 真变更 139：fsevents 回调专属，polling 通（2026-09-14，M5）
- 4.68 TSFN 的 JS 回调漏标 GC 根：fsevents 真变更 139 根因（2026-09-14，M5）
- 4.69 目录首条目 tarball 炸解包：暂存预建（2026-09-14，M5 vitest 牵引）
- 4.70 入口失败 + 开着的句柄 = 事件循环永不收割（2026-09-14，M5 fork 牵引）
- 4.71 `Object.assign` 不自建自引用 + 真机 assert 形态先行（2026-09-14，M5）
- 4.72 fork 单槽监听 + CJS 具名上线后的黑盒同步（2026-09-14，M5）
- 4.73 线信封循环引用：先序 id + ref marker，两端同序才成立（2026-09-14，M5）
- 4.74 `process.stdout.write("", cb)` 无回调实现 = 等落定的协议永挂（2026-09-14，M5）
- 4.75 napi 建面 AB 数据指针必须终身稳定（2026-09-15，M5 终线）
- 4.76 napi_wrap 的 ref 出参两路同发：拒发即 addon 对象无根（2026-09-15，M5 终线）
- 4.77 pin-all（槽位不截断）反例：finalize 链整体死亡（2026-09-15，M5 终线）
- 4.78 GC sweep 内禁 addon finalizer：入队 + 安全点排空（2026-09-15，M5 终线）
- 4.79 截断语义下跨回调裸持 napi_value = 悬垂，call_impl 先验 func 形态（2026-09-15，M5 终线）
- 4.80 CJS 包装五连链的裸 JSVal 栈拷贝：GC 搬移即悬垂（2026-09-15，M5 终线②）
- 4.81 require 条件族：resolver 必须按调用方分流（2026-09-15，M5 终线②）
- 4.82 语义升级（require(esm)+detect-module）后旧边界断言全翻转（2026-09-15，M5 终线②）
- 4.83 无编码 fs 读必须返回 Buffer：String(buf) = utf8 内容（2026-09-15，M5 终线②）
- 4.84 napi_get_cb_info 余槽必须填 undefined（node Args() 契约）（2026-09-15，M6）
- 4.85 定时器实参从未展开：`fire_due` 直调 + 缓存的 helper 从未被读（2026-09-15，10a）
- 4.86 Writable 默认 `autoDestroy:true` 会杀保活连接（2026-09-15，10b）
- 4.87 增量泵的"等更多数据"必须带空 rest（2026-09-15，10b）
- 4.88 TLS 写半部 drop 不发 close_notify，双边读端永 block（2026-09-15，10b）
- 4.89 `end()` 与 connect 竞速：`_final` 时 socket 未就绪则请求永不发出（2026-09-15，10b）
- 4.90 vm 重跑即炸：sync-in 的重定义必须回落赋值（2026-09-15，10c-3）
- 4.91 turso 懒执行 vs Node prepare eager：校验走只备不步进的 cols（2026-09-15，10d）
- 4.92 RustCrypto 泛型顺序与 AEAD 两侧：ccm 的 M/N 反直觉 + 解密 GHASH over 密文（2026-09-15，10e）
- 4.93 后台 sleep/kill 判 hang 必看退出码（2026-09-15，10e，§4.62/§4.67 姊妹篇）
- 4.94 park 唤醒集与存活判定集必须分家：unrefed 到点要醒、不续命、不算 progress（2026-09-15，10f timers）
- 4.95 宿主调用户回调一律 Reflect.apply，禁走实例属性查找（2026-09-15，10f timers）
- 4.96 CJS→ESM 移植的 primordials 残留：未定义符号在错误路径换错误类型（2026-09-15，10f timers）
- 4.97 全局单例的方法族必须 this 基；`--run` 完成值回显随 CJS 主模块化消失（2026-09-15，10f timers）
- 4.98 自写编码器不能指望 `encodeURIComponent` 兜底：unreserved 恒放行（2026-09-16，10f url）
- 4.99 `assert.throws` 函数形期望：`instanceof` 必须以 Error 子类为门（2026-09-16，10f url）
- 4.100 肉眼同形异码点：探针先核对码点，NFKD/NFKC 跟 UTS46 走（2026-09-16，10f url）
- 4.101 `exit`/`close` 事件 node 是双参 `(code, signal)`；spawn 默认 stdio 是 pipe（2026-09-16，10f url）
- 4.102 `process.emitWarning` 是 nextTick 异步派发（2026-09-16，10f url）
- 4.103 Proxy 包 Uint8Array 必须透传 newTarget，否则子类化全灭（2026-09-16，10f buffer）
- 4.104 伪 ArrayBuffer 须品牌拒收，不能信 tag（2026-09-16，10f buffer）
- 4.105 全局 structuredClone 的 transfer 须 detach，否则 isAscii 视残留为真（2026-09-16，10f buffer）
- 4.106 池 AB 不可转移：共享要池化、转移要拒收，两件缺一即挂池套件（2026-09-16，10f buffer）
- 4.107 `util.inspect` depth -1 空容器显体 + 函数不走 primitives（2026-09-16，10f buffer）
- 4.108 `process.nextTick` 裸 throw 变 unhandled rejection：uncaught 路由须先探监听（2026-09-16，10f diagnostics_channel）
- 4.109 `AsyncLocalStorage.enterWith` 是文档化主入口，`enter` 只是别名（2026-09-16，10f diagnostics_channel）
- 4.110 `instanceof` 右侧自定义 `hasInstance` 内禁裸调 `getPrototypeOf`（2026-09-16，10f diagnostics_channel）
- 4.111 运行时从不 emit `unhandledRejection`：tracePromise 无 catch 即 fatal（2026-09-16，10f diagnostics_channel，偏离另案）
- 4.112 阻塞 native 停转事件循环：定制查询改投递+轮询（2026-09-16，10f dns）
- 4.113 `assert.throws(fn, Error)` 裸类须先 `instanceof`，门错即全灭（2026-09-16，10f dns）
- 4.114 双 `Received` 口径：ARG_TYPE 用 helper 形，ARG_VALUE 用 inspect 形（2026-09-16，10f dns）
- 4.115 移植先读全文件：文件头校验段漏读=返工三轮（2026-09-16，10f 过程教训·非代码坑）
- 4.116 跨域微任务先 AutoRealm 进执行 global 再 RunJSMicroTask（2026-09-17，10f vm）
- 4.117 cjs goal 探测禁包络形：包装会吞语句语义（2026-09-17，10f vm，§4.59 姊妹）
- 4.118 nextTick 双层调度：原生队列收割，microtask 合并队列语义必反（2026-09-17，10f stream）
- 4.119 `__fsCall` 闭包内抛的校验错误被 `__fsErr` 重包成 UNKNOWN（2026-09-17，10f fs）
- 4.120 `__cb1` 末参即 cb 的校验次序与 node 相反：f 系 fd/mode 先于 cb（2026-09-17，10f fs）
- 4.121 `fs_err` 包装吞 raw errno：io_code 落 UNKNOWN（2026-09-17，10f fs）
- 4.122 对拍并行跑分未设 TEST_THREAD_ID：全进程共享 `.tmp.0` 互踩（2026-09-17，10f fs）
- 4.123 命名函数表达式遮蔽外层绑定 → custom promisify 无限递归（2026-09-17，10f fs）
- 4.124 Buffer/TypedArray 自带 `Symbol.iterator`（吐数字）：视图必须排除在"可迭代 data"分支外（2026-09-17，10f fs）
- 4.125 net 可观测表面五连坑（2026-09-17，10f net 五轮）
- 4.126 重负载验证禁令：禁并行压力循环与重复全量子集（2026-09-17，机器约束）
- 4.127 zlib 轮子 framing 与收尾四坑（2026-09-17，10f zlib 首轮）
- 4.128 child 同步族六坑（2026-09-17，10f child 首轮）
- 4.129 crypto 首轮六坑（2026-09-17，10f crypto 首轮）
- 4.130 crypto 二轮八坑（2026-09-17，10f crypto 二轮）
- 4.131 http 对拍首轮八坑（2026-09-18，10f http 首轮）
- 4.132 worker 对拍五轮七坑（2026-09-18，10f worker 二轮起）
- 4.133 crypto 四轮 + http2 流式化六坑（2026-09-18，10f crypto/http2 收尾）
- 4.134 crypto 五轮 raw 门六坑（2026-09-18，10f crypto 五轮）
- 4.135 crypto 六轮 PSS/PBES2 八坑（2026-09-18，10f crypto 六轮）
- 4.136 Zip移植双坑：HideStackFrames漏挂 + 解压背stop缺失（2026-09-18，10f zlib Zip）
- 4.137 入口失败 + 开着句柄 = 永不收割（eval 路径版）：then 吞 rejection + unhandled 只在循环尾（2026-09-19，10f child 牵引）
- 4.138 net listen 面三坑：字符串端口当 UDS + 无重听守卫 + 柄不随出口清（2026-09-19，10f net）
- 4.139 fork 非 silent 的 stdio null 三面：流式初始化别覆盖真机 null 缺省（2026-09-19，10f child）
- 4.140 G2 `__closing` relisten 残留：手工过/ cargo 挂 ≠ 环境问题（2026-09-19，欠账轮）
- 4.141 with_str_args 参数跨分配悬垂：rooted 必须在第一个分配之前（2026-09-19，欠账轮）
- 4.142 bisect 禁止连续建 worktree：每个 worktree 的 target 都是全量重编（2026-09-19，欠账轮·灾难记录）
- 4.143 全量 cargo test 禁套 alarm/timeout（2026-09-19，欠账轮）
- 4.144 手工复现脚本放 /tmp 会被清（2026-09-19，欠账轮）
- 4.145 跑分包装 exec 失败静默假绿：`exec or die` + glob 路径（2026-09-19，G9-3 轮）
- 4.146 一次性 native 丢 opts 二进宫 + raw 字典必须构造期设（2026-09-19，G9-3 轮）
- 4.147 Web CS/DS 错误落 readable：pipeTo 的 cancel 语义（2026-09-19，G9-3 轮）
- 4.148 net 尾件五坑：分包解码/end 回调挂点/abort 发射时序/EPIPE 三条件/HE 回落钩（2026-09-19，G6 轮）
- 4.149 G4 fs validators 轮七坑（2026-09-19，欠账 G4 轮）
- 4.150 edit 工具可能静默吞行：oldString 跨行即 diff 复核（2026-09-20，G5 轮·工具约束）
- 4.151 外层模板字符串内禁写内层模板字面量（2026-09-20，G5 轮）
- 4.152 静默窗防抖在持续写下饿死：fs.watch 改前沿触发（2026-09-20，G8 轮）
- 4.153 notify 回调线程的 TLS state 是错表：分类移分发侧（2026-09-20，G8 轮）
- 4.154 `process.exit` 哨兵被用户 catch 即覆盖：首个码赢（2026-09-20，G8 轮）
- 4.155 首轮特例的无条件 return 会吞真变迁（2026-09-20，G8 轮）
- 4.156 内部别名 normalize 只认裸形：`node:` 前缀形落空（2026-09-20，G11 联调缺口）
- 4.157 错误包装函数必须幂等：`__fsErr` 剥前缀致嵌套重包（2026-09-20，G8 轮）
- 4.158 稀疏检出缺 fixtures 即污染对拍基线（2026-09-21，fs 残簇轮）
- 4.159 mustNotMutateObjectDeep 的 Proxy 断 WeakMap 身份键（2026-09-21，fs 残簇轮）
- 4.160 sync 底座流的双重早退：同步终结派发 + 无句柄续命（2026-09-21，fs 残簇轮）
- 4.161 require 的 make_fn 裸值窗口 + 文件名假相关二分法（2026-09-21，dgram 轮）
- 4.162 fork env 丢失即指数 fork 炸弹（2026-09-21，child 轮）
- 4.163 Rust Stdout 块缓冲吞常驻输出（2026-09-21，child 轮）
- 4.164 数字信号 0 无枚举值，落空即 SIGKILL（2026-09-21，child 轮）
- 4.165 UDS listen 异步绑与同步 cp 的竞态（2026-09-21，fs 轮）
- 4.166 宿主 mock 可见性：内部调用须经默认导出（2026-09-21，fs 轮）
- 4.167 fifo 双 open 死锁：数据必经已开 fd 读（2026-09-21，fs 轮）
- 4.168 旧 SAME1 掩盖 + 跑分两手法（2026-09-21，fs 轮·过程教训）
- 4.169 仅 error 监听的流够不着懒 open + 补丁分支置空（2026-09-21，fs 轮）
- 4.170 tower-http 的 `not_found_service` 恒改写 404 + 非 GET/HEAD 缺省 405（2026-09-21，plan4 T1）
- 4.171 serve 停机 Wake + 响应构造快照边界（2026-09-21，plan4 T1）
- 4.172 H3 半关闭 FIN + h3-axum 请求体整收（2026-09-21，plan4 T3）
- 4.173 T4 WS 五坑：握手归属/GUID 记忆/构建盲区/自动应答/101 表达（2026-09-21，plan4 T4）
- 4.174 全并行全量偶发 mozilla mutex 解锁失败（2026-09-21，观察中·未闭环）
- 4.175 fifo 黑盒全并行负载下挂死：写者缺席读端零 CPU 睡眠（2026-09-21，观察中）
- 4.176 URLPattern 专项四坑（2026-09-21，plan3 §5）
- 4.177 dgram 余簇五件：校验序/端口序/fork 宽容/伪语义翻转/挂死归属（2026-09-21）
- 4.178 node:test Slice A 四坑（2026-09-21，plan3 test API 轮）
- 4.179 全并行 2 挂再现（2026-09-21，§4.174 家族）
- 4.180 node:test 钩子归属 + MockTracker（2026-09-21，plan3 test B1 轮）
- 4.181 mock.timers 四件（2026-09-21，plan3 test B2 轮）
- 4.182 run(none) 五坑（2026-09-21，plan3 test C 轮）
- 4.183 run(process) 五坑（2026-09-21，plan3 test D 轮）
- 4.184 run 语义深化六坑（2026-09-21，plan3 test E 轮）
- 4.185 http TIMEOUT 轮八坑（2026-09-22，plan3 G11）
- 4.186 服务端 destroy 失声 + 半开续命：两处 hang 一次清（2026-09-22，plan3 G11）
- 4.187 http 升级流八坑（2026-09-22，plan3 G11 upgrade 轮）
- 4.188 http 头面 batch5 九坑（2026-09-22，plan3 G11 头面轮）
- 4.189 http TIMEOUT 深水第一铲：host/auth/CONNECT 隧道九坑（2026-09-22，plan3 G11）
- 4.190 http TIMEOUT 深水第二铲：server 选项面三坑（2026-09-22，plan3 G11）
- 4.191 splitting 一件：ERR_INVALID_CHAR 缺 `["key"]` 后缀（2026-09-22，G11）
- 4.192 response 双件：write-after-end 毒化终结块 + 状态码门未注册（2026-09-22，G11）
- 4.193 G11 收尾轮：cork 面双 CRLF + uncaught 吞错 + 小面四件（2026-09-23，plan3 G11）
- 4.194 基建轮：socket.push + 服务端解析错 + 写侧流式化（2026-09-23，plan3 基建）
- 4.195 socket 写错透传四坑：异步确认计数 + 递送顺序 + end 取已记错（2026-09-23，剩余轮 outgoing 面）
- 4.196 TLS 无 connecting 面 + 销毁响应禁回池（2026-09-23，剩余轮）
- 4.197 同一失败的二次投递：socket 迟到错 + flaky 定级（2026-09-23，剩余轮）
- 4.198 readable closed 随 close 发射翻位（2026-09-23，剩余轮）
- 4.199 incomingmessage-destroy 双件 + error/close 分排（2026-09-23，剩余轮）
- 4.200 abort 与 destroy 的错误分流 + 孤儿连接守卫（2026-09-24，剩余轮）
- 4.201 req.signal 早夭 + res-close 排序重构（2026-09-24，剩余轮）
- 4.202 对拍提速三件套（2026-09-24，效率专项·①已落地②③待做）
- 4.203 请求级 createConnection 错误被吞：oncreate 只认 socket 不认 err（2026-09-23，G11 TIMEOUT 轮）
- 4.204 http 欠账清扫轮七坑（2026-09-23，plan3 G11 sweep8 红件簇）
- 4.205 统一 runner 给 node 也带 `--run`：假红全表 + 过滤丢件（2026-09-25，②③工具轮）
- 4.206 服务端流控/计时三面：pause 事件、dump 机制、headers 计时归属（2026-09-25，http 尾巴轮）
- 4.207 `TMPDIR` 放 U+F8FF 卷即黑盒假红：`URL.pathname` 是百分号编码（2026-09-25，D1 轮）
- 4.208 require 把用户异常转串重抛：位置恒 prelude 424:53、NodeError 空文案、原对象丢失（2026-09-25）
- 4.209 node 旗前缀放行 = 自 spawn 无限递归，fork 链吃光内核致系统 panic（2026-09-25，D1 回归）
- 4.210 rustls/webpki 拒收 X.509 v1 证书：服务端绕 keys_match，客户端验签兜底（2026-09-25，P1）
- 4.211 修一个结算点会放出一串假绿：beforeExit 缺失 + process.emit 吞错 + 致命错不发 exit（2026-09-26，P2）
- 4.212 "同步底座 + 特判"的流实现一改就碎：fs 流改逐字移植，底座时序补两处（fs 回调微任务、setImmediate 钳 1ms）（2026-09-26，P2）
- 4.213 io_code 漏 ECONNRESET/ENOTCONN：拆链收尾错全落 UNKNOWN（2026-09-26，P2-tls-b）
- 4.214 listen 字符串参误判 UDS path：address().port 全 undefined（2026-09-26，P2-tls-b）
- 4.215 rustls 对 FIN-无-close_notify 严格报错，node/OpenSSL 视为干净 EOF（2026-09-26，P2-tls-b）
- 4.216 watch 过滤复用 test 表：serve 改 html/css 不触发重启（2026-09-27，CLI --watch 轮）
- 4.217 按调用编译 Regex::new 是启动慢放：CJS 发现 8 正则现场编译烧 1.7s（2026-09-27，F2 轮）
- 4.218 concat ESM 具名导出≠默认导出：新类只挂具名即用户面 undefined（2026-09-27，P2-crypto）
- 4.219 原型污染 setter 探针：native 内部写 JS 层 grep 不到即结构性偏离（2026-09-27，P2-crypto）
- 4.220 自家抛错与真机逐字同形时禁"修正"，修上游误喂（2026-09-27，P2-repl）
- 4.221 同流自回显即真机亦无限递归：repl 黑盒入出分离（2026-09-27，P2-repl）
- 4.222 补全分支劫持含引号成员行：成员→路径→等号段→拒答→bare（2026-09-27，P2-repl）
- 4.223 allowBlockingCompletions 是 fs 补全面开关，无之回空（2026-09-27，P2-repl）
- 4.224 TUI 行编辑替换三坑：管道分流/prompt 拼接/Display 单行（2026-09-28，REPL C 档）
- 4.225 读行线程持 raw mode 时他线程直写终端：多行 LF 阶梯 + prompt 竞争（2026-09-27，REPL 渲染修复）
- 4.265 process.exit 裸传当回调即 receiver 错位（2026-10-04，base16）
- 4.266 throwDeprecation 同步抛是伪语义：真机 nextTick 异步走 uncaught（2026-10-04，base16）
- 4.267 注释写的"真机实测"与套件矛盾时以套件为准（2026-10-04，base16）
- 4.268 http2.connect 无视 lookup + 缺 promisify.custom（2026-10-04，base16）
- 4.269 TLS 服务端同字节双派发：单 st 双 __feed（2026-10-04，base16·记档未修）
- 4.270 修好即多活：DIFF 快败翻 TIMEOUT 挂死（2026-10-04，base16）

## 条目

### 4.1 取异常堆栈必须重进 Realm，否则 SEGV（2026-09-09）

- 症状：`throw new Error("boom")` 进程 exit=139（SIGSEGV），而非干净报错。
- 根因：`evaluate_script` 内部用 `AutoRealm` 进 realm，返回时已退出；
  此时调 `error_info_from_exception_stack`（内含 JSAPI）因无 current realm 直接野指针。
- 修法（`src/main.rs` Err 分支）：先 `AutoRealm::new_from_handle(rt.cx(), global.handle())`，
  再把 `&mut realm`（Deref 到 `&mut JSContext`）传给上报函数。
- **推广为铁律**：任何在 `evaluate_script` 返回之后调 JSAPI 的地方，
  先确认是否在 realm 内；不在就进 `AutoRealm`。

### 4.2 mozjs 153 删了旧 wrapper（2026-09-09）

- `Context::from_runtime`、`cx.await_native` 等老 API 在 Gecko153 已删
  （上游 "Drop deprecated code and old wrappers"）。
- 现用：`Runtime::new(engine.handle())` + `rt.cx() -> &mut JSContext` /
  `rt.cx_no_gc() -> &JSContext`；`rooted!(&in(rt.cx()) …)`；`CompileOptionsWrapper::new`。
- 对照以上游 `servo/mozjs` main 分支 `mozjs/examples/{minimal,eval}.rs` 为准，
  不要抄 `winterjs-old` 的 `sm_utils.rs`（那是 0.14 时代写法）。

### 4.3 shell 小坑：zsh 的 `=cmd` 展开（2026-09-09）

- `echo ===` 这类以 `=` 开头的词会被 zsh 当命令路径展开而报错，脚本里要加引号。

### 4.4 config 0.15 的 prefix 分隔符默认跟随 separator（2026-09-10）

- 症状：`WINTERJS_LOG__COLOR=always` 环境变量覆盖配置永远不生效。
- 根因：`Environment::with_prefix("WINTERJS").separator("__")` 时，prefix 分隔符
  **默认跟随 separator**，即前缀变成 `WINTERJS__`，`WINTERJS_` 开头的变量全部被跳过。
- 修法：显式 `.prefix_separator("_").separator("__")`（src/settings.rs）。

### 4.5 集成测试不继承 bin 的 `#[global_allocator]`（2026-09-10）

- 症状：超大分配探针在 `tests/` 里永远"通过"，以为 smmalloc 没问题。
- 根因：`tests/*.rs` 是独立 crate，链接的是 System 分配器，src 里的
  `#[global_allocator]` 对它不可见。
- 修法：探针测试文件里自己声明 `#[global_allocator] static ALLOC: smmalloc::Smalloc`。

### 4.6 tracing-subscriber 的 `init()` 已内建 log 桥接（2026-09-10）

- 症状：进程启动即 panic `failed to set global default subscriber: SetLoggerError(())`。
- 根因：`SubscriberInitExt::init()`（tracing-log 特性启用时）内部就会装 log 桥；
  再显式调 `tracing_log::LogTracer::init()` 抢占 `log::set_logger` 即冲突。
- 修法：二选一，用 `init()` 就不要再调 LogTracer（src/logging.rs 取前者）。

### 4.7 `UseInternalJobQueues` 在 153 下 SEGV，改 RustJobQueue glue（2026-09-10）

- 症状：最小 Runtime 下调 `js::UseInternalJobQueues` 即 SEGV，realm 内外皆崩。
- 根因：原因未深究（记坑）。
- 修法：用 `mozjs_sys` 自带 RustJobQueue glue（servo 同款）：`CreateJobQueue` +
  `SetJobQueue`，traps 的 `runJobs` 用 MicroTask 朋友 API 排空
  （`PeekNextMicroTask` / `DequeueNextRegularMicroTask` / `RunJSMicroTask`），
  首段脚本前在 realm 内 `install`；不装则 `RunJobs` 无队列可用同样 SEGV
  （`src/jobqueue.rs`，`src/runtime.rs`）。

### 4.8 引擎/运行时析构期 StoreBuffer 悬垂边 SEGV（2026-09-10）

- 症状：`Runtime` / `JSEngine` 正常 drop 时在 `JS_DestroyContext` / destroyRuntime
  的小 GC 里 SEGV（含带 timer 路径）；`RootedTraceableBox` 的 TLS 析构晚于引擎
  同样会 SEGV/abort。
- 根因：`Runtime` 的 StoreBuffer 记有指向 `RootedState` Heap 槽位的边，
  先 drop 槽位再销毁引擎即悬垂。
- 修法：结果就绪后 `process::exit` 跳过 teardown（`src/main.rs` `dispatch` 返回
  退出码）；`Runtime`/`JSEngine` 经 `forget_engine` 刻意泄漏（`src/runtime.rs`）；
  `RootedState` 经 `StateGuard` 在引擎存活期内从 TLS 摘除并 `mem::forget`
  （`src/state.rs`，进程退出由 OS 回收）。

### 4.9 native 内 `Rooted<ValueArray>` 注册会 SEGV（2026-09-10）

- 症状：native 回调内用 `Rooted<ValueArray>` 传参即 SEGV。
- 根因：其根注册路径在 native 内调用时有问题（未深究，记坑）。
- 修法：单实参用 `HandleValueArray::from(raw_handle(已 rooted 值))` 直构；
  `thisObj` 传 null 会 SEGV，必须传有效对象（用 global）
  （`src/builtins/clone.rs`）。

### 4.10 `WINTERJS_LOG` 被 config 误收导致启动失败（2026-09-10）

- 症状：`WINTERJS_LOG=winterjs=debug …` 启动即
  `failed to load settings: invalid type: string …, expected struct LogSettings`。
- 根因：`WINTERJS_LOG` 按 `WINTERJS_` 前缀规则被收进 `log` 表（string 覆盖 struct）；
  它本是日志运行时的直读变量（`logging.rs` 直接读 EnvFilter），不经 config。
- 修法：`Settings::load` 期间暂存并移出 `WINTERJS_LOG`/`WINTERJS_LOG_FILE`，
  构建完原样恢复（启动期单线程）；回归测试 `winterjs_log_filter_does_not_break_config`。

### 4.11 模块 hook 的 referrer 定位：走脚本文件名，不走私有值（2026-09-10）

- 症状：`SetModulePrivate` + `SetScriptPrivate` 写入 URL 字符串后，
  load hook 的 hostDefined 仍为 undefined，相对导入 base 丢失。
- 根因：153 下 GC 字符串私有值送不到 hook（机制只适合 `PrivateValue` + ref hooks）。
- 修法：hook 内用 `JS_GetScriptFilename(referrer)` 取文件名
  （CompileOptions 写入的即模块 URL）作 base；`SetScriptPrivate` 整段删除。
- 附带：`ModuleLink` 要求先走完加载态，直接 link 报
  `module record has unexpected status: New`——动态 import 分支内嵌
  `load_dependencies` 再 link（`src/modules.rs` `ensure_subgraph`）。

### 4.12 file URL 必须规范化，否则同一模块判重失效（2026-09-10）

- 症状：循环 a↔b 跑出 `a b a`（模块被求值两次），而非 spec 序 `b a`。
- 根因：macOS `/var` 是到 `/private/var` 的 symlink，两边拼出的 URL 字符串不同，
  注册表按 URL 去重即失效。
- 修法：resolve 返回前一律 `canonicalize`（`src/loader/resolve.rs` `canonical_file_url`）；
  复现：`phase2_circular_import_no_deadlock`。

### 4.13 `TsconfigDiscovery::Auto` 只对 `resolve_file` 生效（2026-09-10）

- 症状：tsconfig `paths` 别名（如 `@lib/*`）报 `Cannot find module`。
- 根因：上游文档注明 Auto 发现只走 `resolve_file`，`resolve` 不读 tsconfig。
- 修法：有真实发起文件走 `resolve_file`，cwd 锚点才走 `resolve`
  （`src/loader/resolve.rs` `caller_file`/`resolve_with`）；
  复现：`phase2_tsconfig_paths_alias`。

### 4.14 `with_rooted`/`with_plain` 不可嵌套（2026-09-10）

- 症状：TS 报错路径 abort（`RefCell already borrowed`，non-unwinding panic，经 microtask 回调炸）。
- 根因：在 `with_plain` 闭包内调了同样走 `with_plain` 的函数（`entry_reason_string` 查 `module_debug`）。
- 修法：先算串再进 `with_plain`；推广为铁律：TLS 访问闭包内只做纯数据操作，
  不调同样走 TLS 的函数（`src/state.rs`）。

### 4.15 入口 promise 捕获必须在事件循环前挂载（2026-09-10）

- 症状：模块顶层抛错被报成无位置的 `unhandled rejection: ...`。
- 根因：`ModuleEvaluate` 的 rejection 在事件循环收尾被通用 unhandled 路径先收走，
  事后挂专用捕获已晚。
- 修法：`ModuleEvaluate` 成功后、进 `event_loop` 前即挂 `entry_*` 捕获，
  循环后只收割（`src/runtime.rs` `run_module`）。

### 4.16 criterion bench 须 `harness = false`（2026-09-10）

- 症状：`cargo bench` 只跑出 `running 0 tests`。
- 根因：bench target 默认 libtest harness，把 criterion main 当测试跑。
- 修法：`Cargo.toml` 加 `[[bench]] harness = false`。

### 4.17 `await` 在参数位置不报 await 错，TLA 重试须放宽触发（2026-09-10）

- 症状：`console.log(await ...)` 报 `missing ) after argument list`，模块重试不触发
  （旧逻辑只认 `await is only valid` 文案）。
- 根因：`await` 在参数位置按标识符解析，语句位置才报 await 错。
- 修法：经典 `SyntaxError` 一律用 oxc 试探解析，能解则跑模块，否则保留原始经典报错；
  eval 侧触发放宽到一切 `SyntaxError`，包装解不出回落原始报错（非 `exhausted`）。
  复现：`console.log(await Promise.resolve(5))`（`src/runtime.rs`）。

### 4.18 结算后退出的循环顶排空 race：progressed 轮不退（2026-09-10）

- 症状：流式 body 第三个 `read()` 永不决议（时序一变则进程 0 退出但丢输出）。
- 根因：`settle`（循环顶 `try_recv` 非阻塞排空）同步决议 promise，只排队
  microtask；随后退出检查全零即 `break`，掉队 microtask 等不到下一轮 `RunJobs`。
  旧缓冲实现罕发（单消息多在 `select` 臂内结算，次轮 `RunJobs` 兜住），流式多消息
  必发（chunk/Done 堆在顶排空）。
- 修法：本轮结算过（`progressed`）即使全 idle 也不退，回顶再 `RunJobs`；
  `select` 的 None 臂遇全 idle 直接 `continue`（只剩 microtask，不 park，否则永睡）。
  复现：`tests/fetch.rs::phase3_fetch_body_streams_chunks`（修前必挂；2026-09-12 拆分前在 tests/cli.rs）。
  推广为铁律：任何同步决议 JS promise 的结算点之后，必须保证至少一轮 `RunJobs`。
- 追补（2026-09-10，模块顶层 `process.exit` 必发 `[object Promise]` 案）：
  抛错的 job 会截断当轮 `RunJobs` 排空，入口捕获的反应 job 留到下一轮；
  若退出旗检查放在排空**前**，下一轮直接返回，反应永不触发（收割 None → 误打印
  promise + 仅靠旗退出）。修法：旗检查一律放 `RunJobs` **之后**。
  教训：`with_plain` 写本身无辜——二分时曾误判它，实为检查点顺序问题。

### 4.19 tower-http 默认追踪打错 target，会被默认 filter 静默（2026-09-10）

- 症状：`WINTERJS_LOG=winterjs=debug` 下 serve 有起停 INFO，但无逐请求日志。
- 根因：`TraceLayer::new_for_http()` 默认回调打 `tower_http::trace::*` target；
  默认 filter `winterjs=<level>`（`src/logging.rs`，依赖库保持安静）把它过滤。
  另：`CompressionLayer` 默认 predicate 跳过小 body（16B 无 content-encoding，
  5KB 才有），属轮子正常行为非 bug。
- 修法：`on_request/on_response/on_failure` 手写回调，用
  `tracing::debug!/warn!(target: "winterjs::serve", …)` 只记 method/uri/status/
  latency（不记 body/头）；压缩黑盒用大文件测（`src/serve.rs`）。
- 推广为铁律：凡引入打日志的轮子，先确认其事件 target 是否在默认 filter 内；
  不在就用回调/适配转进 `winterjs::*`，禁为此放宽默认 filter（依赖噪音）。

### 4.20 写文件命令的手工实测必须先 `cd` 进 probe 目录（2026-09-10）

- 症状：本机验证 `init` 时在仓库根直接跑，把 `package.json/index.js/hello.test.js`
  写进了 winterjs 仓库（差点污染提交）。
- 根因：`init`/`test` 这类以 cwd 为作用域的命令，实测 shell 的 cwd 即作用域；
  肌肉记忆 `./target/debug/winterjs …` 让人忘了先 `cd`。
- 修法：删掉误建文件并 `git status` 确认干净；此后凡实测写文件命令，
  一律 `mkdir -p /tmp/wjs-*-probe && cd` 进去再跑。
- 推广为铁律：黑盒测试不受影响（assert_cmd 设了 `current_dir`），只约束手工实测。

### 4.21 同一文件的 edit 与 append 禁并行（2026-09-10，工具约束注记·非代码坑）

- 症状：给 `tests/cli.rs` 同时发 edit（改 man 计数）与 bash heredoc append
  （加 4 个 init 测试），append 的内容全部丢失，测试数 127 不增。
- 根因：两工具调用并行执行，edit 基于旧内容写回，覆盖了 append 的写入。
- 修法：补回 append；此后同一文件的多次变更一律串行（不同文件可并行）。
- 推广为铁律：工具并行只用于无依赖的不同文件；同文件操作串行排队。

### 4.22 `rt`/`engine` 声明顺序即 drop 逆序，搬代码别搬反（2026-09-10）

- 症状：抽 `init_session` 后 `Promise.reject(...)` 报
  `There are outstanding JS engine handles` panic（exit=101），而非 exit=1 可读错。
- 根因：重构把 `let mut rt` 写到了 `let engine` 前面，`?` 早退（跳过
  `forget_engine`）时 engine 先 drop，rt 仍持有 handle，`JSEngine::drop`
  的 outstanding 断言必炸；成功路径因双双 forget 被掩盖，只有报错路径暴露。
- 修法：`engine` 先声明、`rt` 后声明（`run_inner` 注释已钉住顺序）。
- 推广为铁律：凡涉及 `§4.8 forget_engine` 的重构，成功/报错双路径都要跑
  （报错路径用 rejection/timer-error 用例覆盖）。

### 4.23 `Object.create(prototype)` 实例无私有方法 brand 槽（2026-09-11）

- 症状：`bun:sqlite` 的 Statement 经 `Object.create(Statement.prototype)` 造
  （构造器要抛 Illegal constructor，故不能 new），调 `get #st()` 私有访问器即报
  `can't access private field or method: object is not the right class`。
- 根因：私有方法/访问器在实例上装 brand，`Object.create` 造的对象没有构造器的
  brand 槽，brand 检查必炸（私有 `#` 字段同理；原型上挂 WeakMap 查不到这个问题）。
- 修法：prelude 内部类若实例走 `Object.create` 造，状态一律 WeakMap + 自由函数，
  禁用 `#` 私有成员（`src/builtins/bun/sqlite.rs` SOURCE）。
- 复现：`await import("bun:sqlite")` 后 `db.run("CREATE TABLE t (x)")`（修前必炸）。
- 推广为铁律：prelude 新类先定实例制造方式——能 `new` 才可用 `#` 私有成员；
  `Object.create` 造的（内部类/状态后置挂的）一律 WeakMap 自由函数
 （URL/Headers/fetch 系既有类全走此路，与此一致）。

### 4.24 同进程多次 `runtime::run`：引擎单例 + 每文件独立线程（2026-09-11）

- 症状：`winterjs test` 多文件第二个文件起全报 `failed to init JS engine`
  （e1 起就坏，黑盒只放一文件没抓到；test --watch 把它显性化）。修引擎单例后
  又暴露第二层：`js::NewContext` 里 EXC_BAD_ACCESS（同线程建第二个 Runtime）；
  再改成 run 结束正常 drop Runtime，带 timer/microtask 残留的路径 SEGV（§4.8
  所记 teardown 问题如实复现，139 例黑盒掉 30+）。
- 根因（三层）：① `JSEngine::init()` 每进程只能成功一次（二次
  `AlreadyInitialized`）；② mozjs 的 CONTEXT TLS 断言一线程一 context，
  `Runtime::create` 前就炸（SEGV 在 `js::NewContext` C++ 侧，非 rust assert）；
  ③ `Runtime::drop` 的收尾 GC 在事件循环未完全排空（timer/rejection 残留）时
  必炸——§4.8 的 process::exit 就是为躲它。
- 修法（`src/runtime.rs`）：① `engine_handle()` 进程级单例——JSEngine::init
  一次后本体 `mem::forget`（永不 shutdown），handle（Clone）给每次 run；
  ② `run_isolated(source, filename, args)`：每文件独立 OS 线程（16MB 栈——
  引擎 STACK_QUOTA 按主线程量级假设，线程默认栈不够）内起 current-thread
  tokio + LocalSet 跑 `run`，Runtime 照 §4.8 泄漏，线程退出即清 CONTEXT/state
  TLS（隔离边界 = 线程生灭）；③ `end_session` 维持 forget 泄漏语义（曾试图改
  正常 drop，实证炸 timer 路径后回退）。
- 复现：两个 `console.log` 的 `*.test.js` 跑 `winterjs test`（修前 exit=1
  第二个 `failed to init JS engine`）；修后全过 exit=0。
- 推广为铁律：凡"单 run 进程"假设的代码（runtime::run 内部多处）、要在同进程
  再跑一次 JS 的（test/watch/未来并行），一律走 `run_isolated` 新线程，
  禁在同一线程叠建 Runtime、禁改 `end_session` 为 drop。多 run 功能的黑盒
  必须含 ≥2 文件/≥2 轮的用例（e1 的教训：单文件用例漏掉整层回归）。

### 4.25 clap builder 的 by-value 改造 + `value_name` 只要 `&'static str`（2026-09-11）

- 症状：`cmd.about(x)` 报 `cannot move out of *cmd`（E0507）；`a.value_name(String)`
  报 `Str: From<String> 未实现`（E0277）；`get_value_names()` 回的是 `Option` 不是切片。
- 根因：`Command::about/mut_arg` 是 `mut self -> Self`（by-value，非 `&mut`），
  `&mut` 引用上调即 E0507；`value_name` 只收 `&'static str`。
- 修法：顶层用 `cmd = cmd.about(..)` 串联；子命令经 `get_subcommands_mut` +
  `mem::replace` 占位换回；译文 `value_name` 经 `Box::leak` 给 `'static`
  （中文 locale 下约 60 短串，进程生命期，注释写明；英文 locale 译文==原文直接跳过，
  零泄漏且英文输出逐字节不变）（`src/cli.rs` `localized_command`）。
- 附带：`rust-i18n` 的 `t!` 接受变量 key（运行时查表正常；扫不到的只是
  `cargo i18n` 提取工具——key 全在 yml 里，无需提取）。

### 4.26 全 flag CLI 的两条铁律（2026-09-11）

- 动作的必需值必须紧贴其 flag：`--run`（`num_args(1)`）后直接跟别的 flag
  即判缺值（`--run -l zh --help` 炸）。写法是值前置（`-l zh --run f.js --help`），
  测试里 9 处 `wjs(&["--run", <flags>, file])` 全因此调序。
- 机械改名脚本会误伤非 winterjs 调用：`&["init", "-q", ...]`（git helper）
  撞上 `["init", ` 模式。修法：脚本断言计数 + 事后按命令名复核 bare 残留；
  跨工具同名（git init）逐个手改（`src/cli.rs` 全 flag 重写，`tests/cli.rs`）。

### 4.27 增量解码两阶段 + BYOB 三坑（2026-09-11）

- `decode_to_string` 的 `InputEmpty + read=0` 是"截断已缓存、等下次 feed"，
  不是"继续"——当继续写 loop 即 busy-loop（现象是超时无输出，`cargo test`
  也会被卡死；实测 `[E2]` 回 `(InputEmpty, read=1)`，`[] last=true` 才吐 `�`）。
  修法：排空（`last=false`）+ 收尾空调（`last=true`）两阶段；收尾零推进则置空
  输入再调一次落定（`encoding.rs stream_decode_chunk`，模块单测秒级验终止）。
- pump 里无条件 ByteToQueue 会提前搬空 byteQ，BYOB 永见 `byteLen 0`。
  修法：只在 default 读等待（`wantValue` 标记，closed 等待不算）时搬。
- 无关闭、无释放的 reader + 无条件 pull = prefetch 空转，进程退不出。
  修法：字节流纯按需 pull（BYOB 排队或 default 读等待才拉）；
  default 老路径不动（§4.18 时序敏感）。
- 追补（防抖 key 刷出顺序）：防抖表用 HashMap 即非确定序——新文件
  Create+Modify 双事件谁先刷不一定（`ev: change` vs `ev: rename` flaky）。
  修法：插入序 Vec，同键只留首事件并刷新 deadline，刷出按到达序
  （首事件赢，与无防抖时的先到先得一致）（`fs.rs debounce_loop`）。

### 4.28 空工具调用可能回滚工作区（2026-09-11，工具约束注记·非代码坑）

- 症状：一次无参数的 edit 调用被 abort 后，工作区 4 个文件被回滚到旧快照
  （`mod.rs -190`/`fs.rs -47`/`tests -89`/`child.rs` 重现已删 PIPEDBG；
  未提交的 `publish.rs` 改动全丢；已提交的批2内容 HEAD 完好）。
- 根因：未深究（记坑；疑似 harness 对空调用/中断恢复了 stale 快照）。
- 修法：`git checkout HEAD -- <files>` 恢复已提交部分，未提交部分按历史重做；
  大 edit 后立即 `grep`/`tail` 确认落盘再跑测试。
- 推广为铁律：绝不发送无参数/空参数的工具调用；同文件 edit 串行且每次验落盘；
  大改动每完成一文件即 `git add`（不 commit 也先进 index，丢了能从 index 找回）。

### 4.29 serde 缺 rename 即静默丢字段（2026-09-11）

- 症状：`optionalDependencies` 装不上（求解树里根本没出现），且 `pkg@latest`
  之类 tag 安装也一直是坏的——两者都静默通过，无任何报错。
- 根因：serde 缺省按 Rust 字段名精确匹配；npm 线名是 kebab/驼峰
  （`dist-tags`/`optionalDependencies`），对不上即当未知字段忽略 +
  `#[serde(default)]` 补空，全程无声。
- 修法：`#[serde(rename = "...")]` 逐个显式改名（`registry.rs` Packument/
  VersionMeta）；回归测试反序列化真实线名（`npm_field_names_deserialize`）。
- 推广为铁律：凡对接外部 JSON（registry/npmrc/各类 API），单测必须用真实线名
  断言 roundtrip；`#[serde(default)]` + 外部源组合出现时先查 rename。

### 4.30 `mime_guess` 把 `.ts` 当 MPEG 视频流，`serve` 须自改 MIME（2026-09-12）

- 症状：Vue 工程经 `winterjs serve` 起静态，浏览器拒载 `/src/main.ts`：
  `使用了不允许的 MIME 类型（"video/vnd.dlna.mpeg-tts"）`。
- 根因：`.ts` 与 MPEG-TS 同扩展名，`mime_guess`（冻结表，`ts/mts` 双中招；
  `jsx` 在 2.0.4 表里还是非法的 `text/jscript`）判错；`tower-http 0.7` 的
  `ServeDir` 写死 `mime_guess::from_path`，无覆盖接口（`append_mime_override`
  不存在，翻轮子源码确认）。
- 修法：`ServeDir` 外包最内层 `from_fn` 中间件，`ts/mts/cts/tsx/jsx`
  （大小写不敏感）成功响应改 `text/javascript`（Vite 对等），404 不动
  （`src/serve.rs` `rewrite_ts_mime` + `ts_family_js_mime`）。
- 复现：`tests/serve.rs::phase6_serve_ts_mime_as_javascript`（修前 content-type 含 video；2026-09-12 拆分前在 tests/cli.rs）。
- 取舍：扩展名本身有歧义（TypeScript 源码 vs MPEG-TS 视频，共用 `.ts`），静态服务器
  从后缀无法知道作者意图——与 Vite 一样按 Web 开发上下文判 JS 源码。
  真要服 MPEG-TS 视频请用 `.m2ts`/`.m2t` 后缀（同表，无歧义），不要用 `.ts`。

### 4.31 Node CJS → ESM 移植三坑（2026-09-12，Phase 9a）

- 症状一：`import { codes: { X } } from 'm'` 报 `Expected ',' or '}'`——import 绑定
  不支持嵌套解构（CJS `const { codes: { X } } = require(...)` 的习惯带进 ESM）。
  修法：`import errors from 'm'` 后再解构（node/internal 四处）。
- 症状二：`return { [Symbol.dispose]() { ...this.end / currentContext.set(this, ...) } }`
  里的 `this` 指向**返回的对象字面量本身**，不是外层通道/ALS——静默错绑不报错
  （BoundedChannel.withScope 报 undefined、ALS.__wjsWithScope 静默污染，两次踩中）。
  修法：`const self = this` 闭包捕获。
- 症状三：`dc.subscribe` 不返回退订函数（Node 同款返回 undefined），黑盒想当然存
  返回值致退订恒 false。教训：黑盒设计前先核对 Node 套件原文断言，勿凭记忆。
- 复现：`tests/node/diagnostics_channel.rs::phase9a_diagnostics_channel_surface`（修前 `outside [object Object]`；拆分前在 tests/cli.rs）。

### 4.32 逐字移植的解环/形态坑（2026-09-12，Phase 9b）

- 症状一：`compose` 中路报 `Duplex is not a constructor`（p2 探针只测了 async
  generator 分支，Duplex 构造分支未覆盖）。根因：CJS→ESM 懒解环包装
  `__ensureDuplex()` 只包了 `.from()` 路径，`new Duplex({...})` 用了裸 `let`
  绑定，静默 undefined 运行时才炸。修法：解环包装完成后 grep 该符号在本模块
  全部裸引用逐处收口（本次 5 处漏 1）（`internal/streams/compose.rs:147`）。
- 症状二：`node:buffer` SlowBuffer 垫片写了 `new Buffer.alloc ? ...`——class
  静态方法无 `[[Construct]]`，`new Buffer.alloc` 直接 TypeError（想当然造的
  三元，非 Node 原文）。修法：逐字移植禁"顺手改写"，垫片按 Node 原文
  `return new Buffer(size)`（`node/buffer.rs`）。
- 症状三：unhandled rejection `aggregateTwoErrors is not a function`——4 个
  内部模块解构 errors 的符号而 errors 模块没导出，解构得 undefined 无声，
  运行时才炸。修法：内部模块新增解构 errors 符号前先 grep 导出面是否齐
  （`internal/errors.rs` 补 aggregateTwoErrors，errors.js:172 同款）。
- 教训延续（§4.31 症状三同源，9b 黑盒 4 处断言错全在"凭记忆"）：
  ① `pipeline`/`finished` 的 node:stream 命名导出是 callback 形态（末参必须
  函数，popCallback validateFunction），promise 形态只在 `node:stream/promises`；
  ② `Readable.from("ab")` 吐单块不逐码元（真 Node 实测同款）；
  ③ string chunk 经 push/write 转 Buffer（writable.js:475/readable.js:488），
  `chunk.constructor.name` 是 "Buffer" 非 "Uint8Array"；
  ④ close 事件时点 `isDestroyed` 已为 true。
  实现侧零 bug——全部先实测真 Node 再改断言，勿在黑盒里编码记忆里的语义。
- 复现：`tests/node/stream.rs::phase9b_stream_duplex_transform_pipeline`
  （compose Duplex 分支修前报 `Duplex is not a constructor`）。

### 4.33 fs 补齐三坑（2026-09-12，Phase 9c）

- 症状一：`openSync(p, "wx")` 对不存在文件报 ENOENT（应创建）。根因：JS 侧
  flags JSON 发 `createNew`，Rust `OpenFlags` 结构体字段 `create_new` 按
  serde 默认名匹配不上 → 静默 false → 无 O_CREAT（§4.29 二进宫：跨 JS/Rust
  JSON 边界一律 camelCase 线名 + `#[serde(rename)]`，新结构先写 roundtrip 测）。
- 症状二：`openSync` 返回值传 `readSync` 报 `fd must be a number`。根因：native
  经 `set_rval_str` 返回的 fd 是**字符串**，JS 侧忘了 `Number()` 包装。教训：
  本仓 native 数值返回统一走字符串，JS 层负责转数。
- 症状三：fd 写入报 `UNKNOWN`。根因：`io_code` errno 表缺 9 → EBADF。教训：
  新增 syscall 面（fd 系）前先对照 `io_code` 码表补 errno 映射。
- 探针侧：回调 API 黑盒里相互独立的异步链会交错执行，内容断言全脆——
  修法：严格嵌套链 + 内容断言只放链内确定点；另核实三个"断言错、实现对"：
  `"hello!"` 是 6 字节、`statSync` 对 symlink `isSymbolicLink()` 为 false（跟随
  语义，Node 同款）、readFile ENOENT 的 `err.syscall` 是 `"open"`。
- 复现：`tests/node/fs.rs::phase9c_fs_sync_extras`（wx 修前 ENOENT）。

### 4.34 net 事件循环收尾四坑（2026-09-12，Phase 9d-1）

- 症状一：Server `__ev` 里 `this.emit is not a function`。根因：`call_two`
  派发以 **global 为 this** 调 target 方法（jsapi_glue 调用约定）——类方法做
  事件钩子必须 `this.__ev = this.__ev.bind(this)` 预绑定成自有属性（§4.31
  症状二的引擎版：非对象字面量，而是 native 调用约定）。
- 症状二：回环 echo 后进程 hang 不退。根因：**Node 默认 `allowHalfOpen=false`
  ——socket 收到远端 FIN（'end'）后自动 end 本端**；漏掉该语义则连接半开，
  `net_open` 永不归零，事件循环 idle 判定失败。教训：IO 面的生命周期必须逐条
  对齐 Node 默认关闭语义，`*_open()` 计数 + idle 检查会把缺口暴露成 hang。
- 症状三：`destroy()` 后对端 'close' 不发/双发。根因二连：① Close 事件在
  task 内 **先 purge 后派发**，dispatch 读不到 target（顺序坑：清态必须在
  派发之后）——改为 task 只置 `close_sent` 单发旗，purge 统一在 dispatch 后；
  ② writer task 死后（destroy 断写端），reader EOF 时 JS auto-end 的 End
  命令无人消费——entry 加 `writer_alive` 旗，reader EOF 见 writer 已死则
  代行 `close_once + Close`。
- 症状四：hermetic 陷阱——bogus 域名（`nope.invalid`）在 macOS 会被系统
  解析器经 search domain 意外"解析成功"。教训：DNS/网络黑盒**只依赖
  localhost + 空主机名**，失败路径断言 Error 形状（code 为 string）不断
  具体码；server 端口一律 `port 0`（并行测试安全）。
- 附：serde `SocketAddr` 序列化为 `"ip:port"` 串（IPv6 `[ip]:port`）；
  io_code 的 EADDRINUSE 是双 errno（macOS 48 / Linux 98）——平台差异 errno
  映射一律双码同列 + 单测双断言。
- 复现：`tests/node/net.rs::phase9d_net_echo_loopback`（allowHalfOpen 修前 hang）。

### 4.35 node:http 回环两坑（2026-09-12，Phase 9d-3）

- 症状一：POST 体丢失、res 永不结束、请求看似收到两次。根因：解析器在
  `emit("request", req, res)` **之前**就把体喂完（data/end 先发）——用户
  request 监听器里再挂 `req.on("data")` 永远收不到，res.end 不执行。修法：
  体先备齐缓存，**先 emit("request") 再喂体**（Node parser 同口径：request
  事件先于 body）。推广：凡"事件 + 数据流"API，数据派发必须在用户监听器
  可注册之后。
- 症状二：客户端 'end' 双发。根因：`res.__feed`（整收口径发 data+end）与
  `__finish`（又补 end）重复——整收口径下 end 只能由一处派发，其余路径只
  收尾连接（sock.end + req 'close'）。
- 黑盒教训：请求**无监听 error 事件即抛错**是 Node 正确行为——错误路径黑盒
  要补 `req.on("error", () => {})` 空监听，而不是改实现吞错。
- 复现：`tests/node/http.rs::phase9d_http_loopback`（喂体时序修前 POST 挂死）。

### 4.36 新事件域 checklist（2026-09-12，Phase 9d-4 dgram 二进宫沉淀）

- dgram 落地时把 §4.34 的坑 1（`__ev` 忘预绑定 → `this.emit is not a function`）
  和坑 3（task 侧 `net_purge` 抢在 Close 派发前 → 'close' 事件丢失）**各重踩一遍**。
- 沉淀为 checklist——今后凡基于 `dispatch → target.__ev` 模式新增事件域（tls/
  worker 等），三查：
  ① JS 构造器内 `this.__ev = this.__ev.bind(this)`（dispatch 以 global 为 this）；
  ② task 侧只置 `close_once` 旗发事件，**purge 一律放 dispatch 派发 Close 之后**；
  ③ native 数值返回（id/fd）JS 侧记得 `Number()` 包装。
- 另：构造器参数校验的 TypeError 要带 `e.code`（Node 口径），黑盒断言 code 而非
  message 前缀。
- 复现：`tests/node/dgram.rs::phase9d_dgram_loopback`（修前 'close' 丢/`bad-type undefined`）。

### 4.37 zlib 两坑（2026-09-12，Phase 9d-5）

- 症状一：`gzipSync(s, { level: 99 })` 报 `Z_DATA_ERROR` 而非 `ERR_OUT_OF_RANGE`。
  根因：Sync 把 `__zLevel` 校验写进了 `__zCall(fn)` 的 try 内——校验抛的
  RangeError 被错误包装函数按无前缀默认成 `Z_DATA_ERROR` 重包。
  修法：参数校验一律提到包装调用之外先执行；包装函数再加保险——已有
  `ERR_*` 码的错误直通不重包（`src/builtins/node/zlib.rs` `__zErr`）。
- 症状二（轮子文档与源码不符）：`dependencies.md` §6 记 ruzstd"编码五档"，
  实测 `encoding/mod.rs` 只有 `Fastest` 可用（`Default`/`Better`/`Best` 标
  `UNIMPLEMENTED`）。修法：zstd 编码恒 `Fastest`，`level` 接受忽略并记档；
  教训：轮子能力断言以源码/实测为准，不抄 README 一句话（§4.32 教训延续）。
- 复现：`tests/node/zlib.rs::phase9d_zlib_errors_boundary`（`lv-hi` 行修前为
  `Z_DATA_ERROR`）。

### 4.38 https/tls 三坑（2026-09-12，Phase 9d-6）

- 症状一：`https.get(url, opts, cb)` 三参回环 hang（测试 300s 超时）。
  根因：实现只收两参，`cb` 位置拿到 options 对象 → response 监听器未注册 →
  `server.close()` 永不执行 → 事件循环不退。修法：帧层加
  `normalizeRequestArgs`（url/options 合并，options 优先，跨协议即
  `ERR_INVALID_PROTOCOL`），http/https 双包层同走。
  教训：请求侧"无监听即挂起"是正常语义（§4.35 同源）——先查形态覆盖，再查实现。
- 症状二：`ca: "garbage"` 静默通过（空 roots，握手必挂）。
  根因：`rustls_pemfile::certs` 对无 armor 文本产空迭代无错。
  修法：`ca` 给出但零证书即同步 TypeError（fail fast；服务端 PEM 同口径）。
- 症状三（测试件）：openssl 默认自签带 `CA:TRUE`，rustls 报
  `CaUsedAsEndEntity`。教训：hermetic TLS 测试证书须 end-entity
  （`basicConstraints=CA:FALSE` + serverAuth EKU），或直接用 rcgen
 （`generate_simple_self_signed`，自带正确扩展；serve 黑盒同款）。
- 复现：`tests/node/https.rs::phase9d_https_loopback`（三参修前 hang）。

### 4.39 http2 请求事件双发：构造器与包层别双注册（2026-09-12，Phase 9d-7）

- 症状：3 路 h2 请求，服务端 `srv-req` 打印 6 次；多路复用黑盒丢一整流。
- 根因：`Http2Server` 构造器内 `if (typeof options === "function") this.on(...)`
  与 `createServer/createSecureServer` 包层接线重复——函数首参同时命中两处。
- 修法：构造器不再碰 request 监听器，只由包层接线（`src/builtins/node/http2.rs`）。
- 复现：3 流探针（修前每流双 `srv-req`）；`tests/node/http2.rs::phase9d_http2_cleartext`。

### 4.40 `Heap::set` 后禁移动，违者 nursery GC 必崩（2026-09-12，Phase 9d-7 总根因）

- 症状：回调内制造 GC 压力（2 万小对象 / btoa 大串 / 100KB+ http2 体）后进程
  SIGSEGV/SIGBUS（exit=138/139），崩点不定（dispatch 内外皆可）；timer-only
  同样崩，与 net/http2/zlib 无关——此前"大字符串崩""http2 大包崩"全是该根因的表象。
- 根因：mozjs `Heap::set` 的 post-barrier 记录的是槽地址，set 后移动即悬垂
  （上游 `jsgc.rs` 明写 + 专设 `Heap::boxed` 防此）；本仓 `Vec<NetTarget/
  TimerEntry/ModuleEntry/CjsEntry/WatchCallback/ChildTarget/FetchCallback/
  StreamWaiter>` + `unhandled` 的 `push/realloc/retain/remove` 件件在搬运已 set
  的 Heap，下次 minor GC 读悬垂 store-buffer 边即炸。旧测试全绿只因从未在回调内
  制造 nursery 压力（btoa 大串走大对象空间，未必触发 minor GC，故时崩时不崩）。
- 修法：全部持 JS 值的 Vec 元素字段改 `Box<Heap<T>>`（Box 移动只搬指针，
  槽地址恒稳；`drop` 自带 clearing barrier，摘除安全）；构造点一律
  `Heap::boxed(v)`；读侧 `.get()` 经 Deref 零改；trace impl 零改（`Box` blanket）；
   `RootedState` 直属单值字段不动（已在 `RootedTraceableBox` 内稳定）。
   （`src/state.rs` + `timers.rs`/`modules.rs`/`runtime.rs` + napi 面
   `env/class/asyncwork/loader/promise/refcount.rs`，共 32 构造点，
   以 `rg -o "Heap::boxed" src/ | wc -l` 实测为准，不再手写死数。）
- 复现：`tests/builtins.rs::phase1_gc_pressure_keeps_rooted_targets`
  （修前 exit=139；探针 `setTimeout` 内 2 万对象分配）。
- 推广为铁律：新增跨 GC 存活的 JS 值存储，一律 `Box<Heap>` 定址；
  禁裸 `Heap` 进一切可搬运容器（`Vec` 元素/`retain`/`remove`/结构体按值移动，
  含 interval 重排这类"自家搬自家"）。

### 4.41 `#[serial]` 只保互斥不保顺序，读全局态的用例须先复位（2026-09-12）

- 症状：全量 `cargo test` 里 `permissions::tests::grant_semantics` 挂
  （`check_read("/etc/passwd")` 期望未安装默认全开放），单跑、单线程全过。
- 根因：该用例首断言依赖"全局槽从未被安装过"；`#[serial]` 只保证串行，
  不保证顺序——9e 新增十余个单测改变线程调度后，
  `sandbox_denies_undropped_classes` 先抢锁装了沙箱，`grant_semantics` 后跑即挂。
  属旧测试的时序假设 bug，非功能回归（9e 未碰 permissions）。
- 修法：tests 模内加 `reset()`（槽写回 `None`），`grant_semantics` 首行调用；
  其余用例先 `install` 再断言，本就顺序无关不动（`src/permissions.rs`）。
- 推广为铁律：凡读进程级全局（权限槽/env/分配器开关）的单测，
  先复位再断言；`#[serial]` 只防并发不防跑序。

### 4.42 黑盒标签断言禁子串，`&&` 合并打印禁弱断言（2026-09-12，Phase 9e）

- 症状一（空转）：`assert!(out.contains("md5 true"))` 恒过——输出里另有一行
  `hmac-md5 true`，子串命中，md5 哈希路径坏了也测不出。
  修法：纯哈希标签改名 `md5vec`，使任一标签都不构成另一标签的子串
  （`tests/node/crypto.rs::phase9e_crypto_hash_hmac`，`hmac`/`hmac-md5`/`hmac-s3`/
  `md5vec` 四标签互不包含）。
- 症状二（弱断言）：`console.log("empty", a && b && c)` 只打一个布尔——
  挂了不知挂在哪项，且复制粘贴时易漏项。
  修法：多布尔分参打印 `console.log("empty", a, b, c)`，断言逐项精确匹配
  （`5fb5692`，perf 黑盒 empty/timerify/mel 三处）。
- 推广为铁律：黑盒输出标签设计时即做子串检查；一行多断言一律分参，
  禁 `&&` 打包成单个布尔。

### 4.43 手写密码学面的版本墙：digest 0.10 双轨 + HMAC 自架（2026-09-12，Phase 9e）

- 背景：`Cargo.toml` 实测——`digest` 双版共存（0.10.7 供 `rsa 0.9`，0.11.3 供
  `sha1/sha2 0.11`）、`hmac 0.13`（绑 digest 0.10 系 traits）、`sha3 0.12`
  （digest 0.11 系），三者 traits 互不兼容。
- 症状：`rsa 0.9` 的 OAEP/v1.5 接口要 `digest 0.10` 的哈希类型，
  手头 `sha2 0.11` 传不进去；`hmac 0.13` 与 `sha3` 组合不出 HMAC-SHA3。
- 修法（零新依赖）：`sha2_010` 改名直引（c-4 旧例）+ OAEP-SHA1/v1.5-SHA1-MD5
  手写（MGF1 + `rsa::BigUint` 模幂，`src/builtins/node/crypto.rs`
  `rsa_encrypt_v15/rsa_decrypt_v15/crypto_mgf`）+ HMAC 通用构造自架；
  双向真机交叉验证钉住（本仓⇄真 Node 互解）。
  `sha1_010` 这类新 crate 按 §0.5 须先问用户——本次没问，直接手写。
- 推广为铁律：RustCrypto 系先查 `Cargo.lock` 里谁绑谁（digest 大版本），
  不兼容先走重导出/改名直引/手写三档，最后一档才 §0.5 问用户加依赖。

### 4.44 `format!` 拼 JS 一律绕行：花括号冲突改文件落盘（2026-09-12，Phase 9e）

- 症状：测试想把大段 PEM/JS 经 `format!` 拼进探针脚本，JS 的 `{`/`}` 全被当
  占位符——转义 `{{}}` 满屏且一漏就编译错/运行时串错。
- 修法：大块载荷（X509 内嵌证书）改文件落盘——`dir.child("c.pem").write_str(pem)`，
  JS 侧 `fs.readFileSync("c.pem")` 读回（`tests/node/crypto.rs::phase9e_crypto_x509`）；
  `format!` 只拼小标量（路径/数字）。
- 推广为铁律：`format!` 与 JS 模板字符串/对象字面量同现时，默认选文件落盘，
  不选 `{{}}` 转义。

### 4.45 9e 杂项小坑三则（2026-09-12）

- `cmd | tail` 掩盖退出码：管道退出码是 `tail` 的，前面的测试/构建挂了也看不见。
  修法：断言前先取 `${PIPESTATUS[0]}` 或改 `cmd >file 2>&1; rc=$?; tail file`。
- `gen` 是 edition 2024 保留字（生成器）：Rust 变量/字段/函数名避开 `gen`
  （如 DH/素数生成相关命名用 `genkey`/`generate` 全称），编译期即报错，改名即好。
- 真机口径先行：`createHash('nope')` 无码原文错（不自编 `code`）、`getMacs`
  真机不存在（不做）、Hmac 二次 digest 回空、`digest(badEnc)` 回 Buffer、
  `checkHost` 返匹配串、`hkdfSync` 回 ArrayBuffer、空 histogram 哨兵
  （`min=INT64_MAX`）——黑盒先对真机实测再写断言（§4.31 症状三/§4.32 教训延续）。

### 4.46 `progressed` 后直接 park 会饿死 microtask-only 结算（2026-09-12，§4.18 推广）

- 症状：worker 端口 `postMessage` 后进程 hang（无 timer 时必发；有 timer 时
  到 sleep 醒才送达——延迟送达是同一根因的轻症）。
- 根因：端口 `__ev` 只排 `queueMicrotask`（flush 在下轮 `RunJobs`），而循环在
  `progressed` 后直接进 `select!` park——再无 `RunJobs` 机会。旧代码只对
  "结算致 idle"（fetch 模型）有效：`idle && progressed` 经 None 分支 `continue`
  回顶；而端口/net 类结算不改变 open 计数，`idle=false` 即 park。
  同理潜伏：timer 回调内决议 promise + 他域 open 计数未清，同样 hang。
- 修法（`src/runtime.rs`）：`idle && !progressed` 照旧 break，其后
  `if progressed { continue; }`——回顶下一轮 pump 先 `RunJobs`，无新进展才 park，
  不忙转（progressed 源皆有限：通道缓冲/timer 触发）；None 分支内
  `if idle { continue; }` 已不可达，删除。
- 复现：`w11.mjs`（监听 + 投递，无 timer，修前 hang；`tests/node/worker.rs` 落盒时已修）。
- 推广为铁律：§4.18 铁律的完整形态——结算点之后**到 park 之前**必须保证至少一轮
  `RunJobs`；新增事件域若结算只排 microtask，必走此路径验证（无 timer 用例）。

### 4.47 `newListener` 在监听入表*之前*触发（2026-09-12，Phase 9f-2）

- 症状：迟挂监听（先 post、后 `on('message')`）永远收不到，端口关不掉 hang。
- 根因：`newListener` 事件在监听**入表前**触发（events.js:371 同款语义）——
  处理器里查 `listenerCount('message')` 仍为 0，flush 门控永不开。
- 修法：newListener 处理器内 `queueMicrotask(() => maybeFlush())`，延迟到入表后
  再判（`src/builtins/node/worker.rs` MessagePort 构造器）。
- 推广为铁律：凡用 `newListener` 做"监听到达即…"门控，一律延迟一轮再读表；
  同步读表恒为旧值。

### 4.48 native 重名静默覆盖 + `static` 判重跨会话误报（2026-09-12，Phase 9f-2）

- 症状：`phase4_node_process_argv_env` 挂——`process.env` 的 `Object.keys` 看不见
  新变量、`delete` 删不掉；与 worker 八竿子打不着。
- 根因：worker 环境数据 native 取名 `__wjs_env_get/set`，撞上 `process.env`
  同名 native——`define_all` 同表后注册静默覆盖，get/set 走新表、keys/del 走旧表，
  两张皮。教训：`__wjs_*` 无命名空间校验，全靠自觉。
- 修法：改名 `__wjs_worker_env_*` + `define_all` 的 `web` 表循环加
  `debug_assert` 判重（`src/builtins/mod.rs`，负验证：临时插重复名即 panic）。
- 连环坑：判重集初版用 `static`——`define_all` 每会话跑一次（test 多文件/
  worker 线程），第二会话起全误报。改函数局部 `HashSet`（`#[cfg(debug_assertions)]`）。
- 推广为铁律：新增 `__wjs_*` 前先 grep 有无重名；判重状态一律函数局部，
  禁 `static` 累积（多会话进程必误报）。

### 4.49 worker 早失败必须也发 rendezvous，否则主侧超时 + 事件丢失（2026-09-12，Phase 9f-3）

- 症状：`new Worker("./nope-missing.js")` 卡 30 秒报 `failed to start`；
  若修超时，`WError`/`WExit` 又因先于 JS `attach` 到达被丢弃（exit 事件永不到）。
- 根因：文件读错发生在会话起之前，rendezvous 从未发出（主侧 `recv_timeout`
  干等）；早发的错误事件在目标登记前到达即丢（dispatch 查不到 target）。
- 修法（`src/runtime.rs::run_worker_thread`）：早失败路径先排 `WError`/`WExit`，
  再发 rendezvous（parked 收件箱——接收端已 drop，后续投递即失败不堆积）；
  主侧 spawn 返回后 JS 同步 attach，早于任何分发，事件不丢。
- 推广为铁律：跨线程 rendezvous 的**所有**出口（含失败出口）都必须发一次；
  目标登记晚于事件到达是常态，设计时即保证"先排队、后登记、再分发"时序。

### 4.50 任务尾的资源释放：重构拿掉等待点后即变杀手（2026-09-13，Phase 9g-2）

- 症状：`createBidirectionalStream` 永不 resolve，随后会话无故 `close`。
- 根因：connect 任务尾有 `endpoint.close()`——9g-1 时任务卡在 `closed().await`，
  该行只在会话自然结束后执行，无害；9g-2 改驱动接管守望后任务直达尾部，
  上线即关（实证：quinn `Endpoint::close` 杀其名下活会话）。
- 修法：发起侧 endpoint 移交会话表项保活（`client_ep`），收尾时才关
  （`quic_sess_remove`）；任务尾只 detach（`src/builtins/node/quic.rs`）。
- 推广为铁律：凡"等待点之后"的清理代码，拿掉等待点时必须重审其前提；
  跨任务共享的资源（socket/句柄）归属写进表项，不靠任务尾 drop 顺带。

### 4.51 校验必须待在 `__callNative` 闭包之外（2026-09-13，Phase 9g-2）

- 症状：`cc: "nope"`/`idleTimeout: -1`/`resetStream(-1)` 全报 `ERR_QUIC_ERROR`，
  与黑盒断言的 `ERR_INVALID_ARG_VALUE`/`ERR_OUT_OF_RANGE` 对不上。
- 根因：校验调用写成了 `__callNative(() => native(..., __normCc(v)))` 的实参——
  抛错发生在闭包内，被包装函数一律重包成 `ERR_QUIC_ERROR`。
  同源：`resetStream/stopStream` 传 `String(code)` 给只收 number/bigint 的
  native，直接 `ERR_OUT_OF_RANGE`（`quic_sess_close` 传串是对的——它家 native
  收串，各家约定不一致，调用前先看 native 侧类型）。
- 修法：校验提到 `__callNative` 之外先执行，结果变量再传入
  （`src/builtins/node/quic.rs` `connect`/`listen`/`stopSending`/`resetStream`）。
- 推广为铁律：`__callNative` 闭包内只放纯 native 调用 + 已校验的值；
  踩过一次的"码被重包"（§4.37 症状一同源），新增包装函数时先查。

### 4.52 会话收尾先收半端任务，否则 teardown 噪声变 fatal（2026-09-13，Phase 9g-2）

- 症状：`c.close()` 后进程报 `read failed (connection lost)` exit=1（流无 error
  监听时）；有监听则多一行本不该有的 error。
- 根因：会话关 → 读写任务的 pending 读/写立刻失败 → `StreamError` 先于
  `SessionClose` 派发——用户只关了会话，流 error 属 teardown 噪声。
- 修法：驱动发 `SessionClose` 前先对名下全流 `quic_stream_finish`
  （abort 半端任务，无声），再发事件；分发侧收尾照旧
  （`src/builtins/node/quic.rs` 驱动 `finish` 闭包）。
- 推广为铁律：父域收尾（会话/进程）必须先静默摘除子域任务，再发自己的
  终结事件；顺序反了，子域的临终报错必先到（§4.36 purge 顺序的跨任务版）。

### 4.53 quinn 默认 idle 30s：悬空会话 hang 测试，收尾必须显式关（2026-09-13，Phase 9g-2）

- 症状：黑盒跑 30.9s 才退（平时 4s）；connect 到已关 endpoint 同样 30s 才报。
- 根因：quinn 默认 `max_idle_timeout` 30s——服务端接受了但从不关的会话、
  发往黑洞的握手，全按 30s 结算。`quic_open` 计数如实续命，属正确语义，
  非 bug。
- 修法：测试里服务端 `secure` 即关（用完即走）；需要等失败的用例给短
  `idleTimeout`（如 1500ms）或错配 CA（TLS alert 即时失败）。
  黑盒禁依赖"对端会关"的用例形状。
- 推广为铁律：凡引入带默认超时的轮子（idle/握手），先查清默认值并写入
  测试纪律；hang 30s 整首先怀疑默认超时，其次才是死锁。

### 4.54 试解循环按长度分发，撞长即误判：看 OID 不看坐标（2026-09-13，Phase 9h-1）

- 症状：本仓签的 secp256k1 自验全过，真 Node 的签名恒验不过（`false` 不抛错）。
- 根因：SPKI/PKCS#8 导入按"逐曲线试解"定 curve——P-256 与 secp256k1 坐标同为
  32 字节，P-256 先试先"成功"（SPKI 解析根本不看曲线 OID），验签用了错曲线。
  自签自验全绿是因为加解密同错，属对称性盲区。
- 修法：`__wjs_ec_guess_curve` 读算法参数 OID 直判（SPKI/PKCS#8 通吃；
  P-256=`1.2.840.10045.3.1.7`…k256=`1.3.132.0.10`）+ SEC1 `[0]` 显式 OID；
  openssl 实测向量单测钉住（`ec_curve_name_reads_oid_not_coords`）。
  教训：跨实现交叉验证必须双向（9e-1c 只做了"真机验我"，漏了"我验真机"）。
- 推广为铁律：凡"试解定类型"的导入面，键长/形状重合即视为已撞——必须找
  自描述字段（OID/魔数/版本号）直判；交叉验证永远双向。

### 4.55 k256 验签拒 high-S：验前 normalize_s（2026-09-13，Phase 9h-1）

- 症状：上条修完后，Node 的签名仍验不过——仅当 `s > n/2` 时（对比两边 DER：
  过的是 70B（s 无填充），挂的是 71B（s 首字节 `00` 填充）。
- 根因：OpenSSL 接受可锻造签名（high-S 照验），k256 的验签拒绝 high-S。
- 修法：验签前 `sig.normalize_s()`（inherent 方法，无需 trait；P-* 系无影响，
  低 S 是恒等变换）（`src/builtins/crypto.rs` `ecdsa_verify`）。
- 推广为铁律：密码学"验不过"先按字节比对两边产物的形态差异（填充/长短/
  大小端），再怀疑算法；可锻造性是验签侧必须兼容的语义，不是 bug。

### 4.56 Node PKCS#8 省公钥 y：`y=g^x mod p` 补算（2026-09-13，Phase 9h-1）
- 症状：真 Node 的 DSA 私钥导不进（`Invalid PKCS#8 key`），自家往返全过。
- 根因：Node 的 DSA PKCS#8 只含 `(p,q,g,x)`（公钥 y 可选省略），信封解码硬要 y。
- 修法：`dsa_envelope` 内 y 缺失且 x 在，即补算 `y=g^x mod p`
  （`rsa::BigUint::modpow` 重导出直用，零新依赖）。
- 推广为铁律：外部输入的"可选字段省略"是常态（尤其 OpenSSL 系编码），
  解码器必须按"缺啥补啥"写，而不是按"我家导出形状"收；双向交叉时，
  导入真机产物的用例与导出给真机的用例缺一不可。

### 4.57 跨域 `instanceof Promise` 恒 false + 跨域求值恒异步（2026-09-13，Phase 9i-1）

- 症状：vm 模块顶层 `throw` 后 `evaluate()` 反而 resolve，随后进程报
  `unhandled rejection: Error: boom` exit=1；成功模块的完成值也全是 promise。
- 根因（二连）：① JS 壳用 `r instanceof Promise` 判完成值形态——r 来自 vm
  compartment（CCW），其原型是彼域 `Promise.prototype`，主域 `instanceof`
  恒 false，落定被丢弃、成功靠巧合（回 undefined）、失败变 unhandled；
  ② 跨域（native 内 `AutoRealm` 切 compartment 且 JS 在栈上）的
  `ModuleEvaluate` 恒走异步求值（主域同序列对照亦然，非 compartment 之过；
  `require` 同调用内无切换故同步）——rval 为 promise 是正确语义，不是 bug。
- 修法：JS 侧按 thenable 结构认领（`typeof r.then === "function"`），成功
  认领后置 evaluated 位 + 读 namespace，失败置 errored + 记 `module.error`；
  Rust 侧 promise 路径不预置 evaluated 位（`__wjs_vm_mod_settled` 由壳在落定后
  补记；`vm_mod_ns` 照旧以位为门）。
  二分过程的临时 `vm_dbg_*` natives 用完即删，不进提交（本次删干净，
  `grep vm_dbg` 为空）。
- 推广为铁律：跨 compartment 的值一律按结构判形态（thenable/数组用
  `Array.isArray` 跨域安全），禁 `instanceof`；凡 `evaluate` 族 API 的壳必须
  同时兼容同步完成值与 promise 两种 rval。
- 复现：`tests/node/vm.rs::phase9i_vm_module_boundary`（`m9iB-evthrow` 行修前为
  `OK` + 进程 exit=1）。

### 4.58 迁移排空三件套：offer 留 target + forwarded 排空 + 分发回退（2026-09-13，Phase 9i-2）

- 症状：跨线程端口迁移后，主→worker 方向首条消息必丢（worker→main 方向正常）。
- 根因（三连）：① offer 即摘 target 则竞态消息无处排队；② `forwarded`
  排空时通道里在途的迟到消息晚于排空到达，此时 target 已摘即丢；
  ③ 本引擎 `structuredClone` 不支持 BigInt（`DataCloneError`），"先 clone
  探路"的信封设计与 BigInt 支持互斥；另 `SharedArrayBuffer` 全局不存在，
  `instanceof SharedArrayBuffer` 直接 ReferenceError（非 false）。
- 修法：offer 置 `moved` 停计数但保留 target（竞态消息照常排队）→
  `PortForward` 派发后调目标 `__ev("forwarded")` 经现转发路由排空 →
  分发侧 `PortMsg` 见 target 空而有转发路由即改道（`port_forward_route`
  回退）；可克隆性改 walk 全权判定（`__denyClone` 显式拒绝表）；
  不存在的全局一律 `typeof` 守卫先行。
- 推广为铁律：凡"先摘后建"的跨会话移交，必须回答"在途消息去哪"——排空点
  + 分发回退缺一不可；引擎能力断言（structuredClone/BigInt/SAB）以上手实测
  为准，不抄文档记忆。
- 复现：`tests/node/worker.rs::phase9i_worker_transfer_cross_thread_and_broadcast`
 （`w9i-xfer` 第二项修前为 false）。

### 4.59 CJS 互操作垫片吞掉纯 TLA 的 `.js` 依赖（2026-09-13，Phase 9j）

- 症状：CJS 互操作上线后，`phase2_top_level_await_entry` 挂——入口 `tla.js`
  （顶层 `await` 专属、无 import/export）报 `await is only valid in async
  functions...`，而非走模块重试。
- 根因：`cjs_interop` 用 `is_module`（`has_module_syntax`）判 ESM——纯 TLA
  文件无模块语法即判 CJS，打上 `export default` 垫片；垫片求值期同步
  require，CJS 包装走经典脚本求值，顶层 `await` 即炸。入口经典路径的
  TLA 重试（§4.17）够不着依赖。
- 修法：歧义集（无 type 的 `.js`/`.jsx`）加经典目标试解析
  （`parses_as_script`，`with_module(false)`）：但注意 oxc 在 script goal
  下仍会对无歧义顶层 `await` 置模块升级信号（Babel 式 `sawUnambiguousESM`，
  `set_module_syntax` + 延迟错丢弃）——所以试解析必须同时看
  `!has_module_syntax`，只看"无错"不够（`src/modules.rs`）。
- 复现：`tests/node/require.rs::phase9j_tla_dep_stays_esm`（修前 TLA 依赖进垫片炸）。
- 推广为铁律：oxc `with_module(false)` ≠"无模块信号"——`module_record.
  has_module_syntax` 才是升级真相；任何"经典/CJS 兜底"判定都要先过 TLA
  专属文件这一关（入口 `tla.js` 即现成探针）。

### 4.60 `util.parseEnv` 结果键按 ASCII 排序，非插入序（2026-09-13，Phase 9j）

- 症状：四组真机差分探针前三组全同，第四组（多行引号）仅键序不同——
  `B=1\nA=2` 真机出 `{"A":"1","B":"1"}`，不是插入序 `{"B","A"}`。
- 根因：Node 的 parseEnv 实现按 ASCII 排序输出键（`B=1\nA=2` → A,B，
  大小写敏感大写在前），与 JS 对象插入序直觉相反。
- 修法：解析期 `Map` 收集，组装期 `[...keys()].sort()` 再写入
  （`src/builtins/node/util.rs` `parseEnv`）。
- 复现：`tests/node/util.rs::phase9j_util_parse_env` 首断言
  （`{"A":"1","B":"1"}` 顺；修前为插入序）。
- 推广为铁律：§4.32 教训延续——"顺序"也是语义，真机差分必须连键序一起
  `diff`，逐行 `JSON.stringify` 对拍（本仓即靠它抓到）。

### 4.61 自递归子进程的动作 flag 碰撞：脚本参数禁复用 winterjs 动作名（2026-09-13，Phase 9j）

- 症状：vue 形 `-r dev` 管线黑盒用 `tool --serve` 作 fixture，子进程报
  `specify exactly one action, got: --run, --serve`。
- 根因：9i-10 JS bin 自递归把脚本参数原样透传给自身 `--run <bin>`；
  `--serve` 是 winterjs 已知动作 flag，子进程 clap 即判双动作（§4.26
  全 flag 铁律）。未知 flag（如 `--watch-mode`）因 `trailing_var_arg`
  透传无事——只有**已知动作名**才炸。
- 修法：fixture 改中性参数（`--watch-mode`）；真脚本如需透传动作名，
  走 `--` 分隔（9i-10 语义）。
- 复现：`tests/cli.rs::run_script_vue_dev_shape_through_node_module`
  （`--serve` 形修前必炸）。
- 推广为铁律：黑盒 fixture 的脚本参数不得与 winterjs 动作 flag 同名；
  新增动作 flag 时 grep 测试 fixtures 有无撞名。

### 4.62 stash 期间构建会污染 target，pop 后必须重编再探（2026-09-13，通用构建卫生·非本仓代码坑）

- 症状：`git stash → cargo build → git stash pop` 后，`./target/debug/winterjs`
  探针报旧行为（`'node:module' is not a builtin`），而 `cargo test` 全绿——
  两边结论打架。
- 根因：stash 期间的构建把旧代码编进了 `target/debug/winterjs`；
  `cargo test` 的测试二进制每次现编（新代码），手工探针用的却是 stale 主二进制。
- 修法：pop 后立即 `cargo build` 再探；结论打架时先对 `ls -la target/debug/winterjs`
  时间戳。
- 推广为铁律：凡中途 stash/checkout 换过代码再探，必须重编主二进制；
  `cargo test` 绿 + 手工探针红 ≠ 代码问题，先查二进制新鲜度。

### 4.63 子进程复用自家 CLI 时透传参数必须 `--` 收尾（2026-09-13，9k）

- 症状：scripts `{"v": "node-which --version"}` 经 .bin JS bin 自递归
  （`winterjs --run <bin> --version`）打出 winterjs 版本横幅，bin 本体没跑。
- 根因：本仓已知内置 flag（`--version`/`--help`）在 trailing positional
  收集前就被子进程 clap 拦截；§4.61 的"未知 flag trailing_var_arg 透传"
  只覆盖未知 flag，已知 flag 是它的补集缺口。
- 修法：子递归统一 `--run <bin> -- <args>`，`--` 由 clap 消费、rest 原样落
  `cli.args`（`src/scripts.rs`）。
- 推广为铁律：凡 spawn 自家 CLI 传用户参数，一律 `--` 收尾；每新增一个
  内置 flag，grep 全部这类调用点复核一遍。

### 4.64 成对分隔符夹在中间时 strip_suffix 必空（2026-09-13，9k）

- 症状：stub registry 按 `/<name>/-/<name>-1.0.0.tgz` 剥后缀解析 tarball
  路径，`strip_suffix("-1.0.0.tgz")` 后再 `strip_suffix("/-")` 恒失败——
  剩余串以包名结尾（如 `init-dep-a/-/init-dep-a`），`/-` 在中间不在尾部。
- 修法：`split_once("/-/")` 取左名 + 右文件名全等校验。
- 推广为铁律：路径/串解析先画全形再选 API——分隔符在串中段用 split 家族，
  strip_suffix 只配"真后缀"；stub 请求数对不上时先打印实际 path。

### 4.65 宽松 API 重构到严格语义：先真机实测，再改伪语义断言（2026-09-14，M5）

- 症状：EventTarget 全局化（cac `class CAC extends EventTarget` 前置）并把
  AbortSignal 重构到基类后，`tests/fetch.rs::streams_abort_events` 挂
  （`dispatchEvent requires an Event instance`）。
- 根因：旧 AbortSignal 的 dispatchEvent 收普通对象且手动 dispatch 会置
  aborted 位；新实现按 DOM/Node 标准只收 Event 实例。真 node 26 实测：
  普通对象 → TypeError（`The "event" argument must be an instance of Event`）；
  `dispatchEvent(new Event("abort"))` 返 true 但 aborted **不变**（置位只归
  abort 算法）。旧测试编码的是伪语义，非实现回归。
- 修法：测试按真机改（`new Event("abort")` + 断言 aborted 不置位 +
  普通对象断 TypeError）。
- 推广为铁律：把宽松 API 重构到标准严格语义后必跑全量抓旧测试；断言与
  真机冲突时先实测再改——测试也可能在编码实现的历史偏差（§4.32 测试侧版）。
- 复现：修前 `cargo test --test fetch streams_abort_events` 必挂。

### 4.66 napi-rs 3 的 Promise 转换暗面 + Either 兜底吞真因（2026-09-14，M5）

- 症状：vite build JS API 报 `The function returned \`object\`, but expected
  \`undefined\`.`（stack 空）；CLI 路径则死在 vite 配置加载
  `export declarations may only appear at top level of a module`。
- 根因（二连）：① 报错文案出自 rolldown binding 的
  `Either<Ret, InvalidReturnValue>` 兜底分支——它把一切转换失败吞成统一文案；
  真因是 napi-rs 3 的 `PromiseRaw.then/catch` 会**立即对自家
  `napi_create_function` 产物调 `napi_wrap` 挂 finalizer**，撞上 M3 的
  "仅 define_class 实例" fail-fast（真 Node 的 napi_wrap 本就收任意对象——
  m2 fixture 编码了偏差）。② vite 配置打包链
  （bundleConfigFile→loadConfigFromBundledFile）依赖 `require.extensions` +
  `module._compile`（内存 CJS 产物求值），require 无视钩子直接读盘即炸。
- 修法：napi_wrap 任意对象路进 env 登记表（无 GC 驱动 finalize，
  end_session 收敛 LIFO 触发；ref 出参仅类实例路）；createRequire 系
  require 消费 extensions（`.js` 兜底同 vite loaderExt；cache 先于
  extensions，Node 口径）+ native `__wjs_cjs_compile`（CJS 包装口径与
  require 全同，require 以文件自身为 base）。
- 定位手法（可复用）：文案不可信时给 `napi_call_function` 插 [wdbg]——
  被调函数名 + 匿名函数经 `Function.prototype.toString` 吐源码前段 +
  rval 类型，TSFN 加 resource_name；报错紧跟的最后一条即命中。
- 推广为铁律：napi-rs 3 起 Promise 转换是**有副作用**的（挂 then/catch
  回调 + napi_wrap）；凡 "expected X" 类报错先查 Either 兜底吞没的真因。
  fixture 断言偏离真机语义的，真机口径一锤定音（§4.65 同源）。
- 复现：`cd /tmp/wjs-vite-probe/min-proj && build-probe.mjs`（napi Wrap 前
  必报 expected undefined；改 vite.config.js 前必报 export declarations）。

### 4.67 vite dev 真变更 139：fsevents 回调专属，polling 通（2026-09-14，M5）

- 症状：vite dev（rolldown transform 拉起 + WS 握手完成）后真文件 append 即
  `EXIT:139`（`EXC_BAD_ACCESS 0x4b4b4b4bXXXXXXXX`，栈顶 JIT
  `tryAttachTypedArrayElement`；调用栈 `asyncwork::dispatch(TsfnDrain)` →
  `fse_dispatch_event` → `napi_call_function` → JS 内崩，`CHOK-CHANGE` 前）。
  `hmr:false` 照崩；`usePolling:true` 不崩。
- 隔离矩阵（`/tmp/wjs-vite-probe/min-proj/hmr-min*.mjs`，落盘法读输出——
  进程不退出时管道输出会被吞）：无 WS/手动 emit/仅 fetch/仅 append 全过；
  `fetch(transform)+握手+真 append` 必崩（WS 事先 close 照崩——握手期 state
  已埋雷）；`fetch(@vite/client 静态)+握手+append` 不崩（transform 必需）。
  [wdbg] 实证 napi 实参形态正确（path 字符串/flags 数字/id 数字）。
- 落袋三件（实锤缺口，与崩溃因果未完全钉死但均为 dev 必经面）：
  ① `stream.resume is not a function`（min13 关停路径 unhandled rejection
  实录）→ Socket/IncomingMessage/ServerResponse 补 pause/resume/read/
  setTimeout/cork/uncork 桩（整收口径 read 恒 null）；
  ② `fs.watchFile is not a function`（polling 37 路 rejection 实录）→ 纯 JS
  stat 轮询实现 + `unwatchFile`（零 native；persistent:false/bigint 记档）；
  ③ TSFN dispatch 无 pending 守卫（M4-③ loader 同类，net/worker 系均有
  `failed(cx)` 收敛）→ `TsfnDrain/AsyncDone` 回调后查 pending 即转可读错
  （本次未触发——崩溃在回调**内**，属防御性收敛）。
- 根因已闭环（2026-09-14，见 §4.68：`NapiEnv::trace` 漏标 `tsfns.js_cb`），
  默认 fsevents 路径 `hmr-min9.mjs` 修后 full-reload + `EXIT:0`。
  （此前曾记"未闭环/待深入"——系根因未定时的过程口径，现以 §4.68 为准；
  上方隔离矩阵保留为定位过程记录。）
- 复现：`hmr-min11.mjs` 形（transform fetch + WS 握手 + 真 append，三件齐崩；
  任缺一件即过）；崩溃报告见 `~/Library/Logs/DiagnosticReports/winterjs-*.ips`
 （`0x4b4b4b4b` 高位恒定）。
- 推广为铁律：长驻探针进程一律输出落盘再读（`>file 2>&1` + 定时 kill），
  管道直连超时即丢输出；新事件域 dispatch 先抄 `failed(cx)` 收敛再接线。
- 追补（2026-09-14）：`writeUInt16BE is not a function` 是另一独立缺口——
  HMR 重变换链（sourcemap 编码）直调 Buffer 整数系，本仓全局 Buffer 从未实现
  `read/write*整数/浮点` 全家（`phase9b_buffer_int_rw` 落盒，DataView 直通
  32 方法）。症状是 HMR 推 `{"type":"error"}` 而非 update（有 CHOK 无 update
  即查此面），与 fsevents 139 无关（polling 后端同样先 error 后随补齐转绿）。
  `/var` 下"能侦测不推送"即此缺口所致，非路径/时序问题（教训：先看推了什么
  消息类型再怀疑路径）。

### 4.68 TSFN 的 JS 回调漏标 GC 根：fsevents 真变更 139 根因（2026-09-14，M5）

- 症状：§4.67 的 139——真文件 append 后 `EXIT:139`
 （`EXC_BAD_ACCESS 0x4b4b4b4bXXXXXXXX`，高位恒定、低位浮动；调用栈
  `asyncwork::dispatch(TsfnDrain)` → `fse_dispatch_event` →
  `napi_call_function` → `call_impl` → `JS_CallFunctionValue` → JS 内崩）。
  崩点在回调**内**（`tryAttachTypedArrayElement`/`GetProperty` 都见过——
  崩点是受害现场，不是根因）；napi 实参形态正确、无 pending 遗留。
- 根因：`NapiEnv::trace` 只标 `slots/escape_slots/deferreds/refs/wrap_sym/
  modules`，漏了 `tsfns` 记录里的 `js_cb`。TSFN 的 JS 回调**只被该记录持有**
  （dispatch 截断 `slots` 后无其他根），一次 GC 后回调即悬垂，下次事件
  dispatch 取悬垂槽值进 `fn.apply` 就是 UAF。fsevents 是长驻回调 +
  transform/fetch 制造 GC 压力 → 稳定复现；m3 旧 fixture 全程无压力 → 全绿
  掩盖（§4.40 同源：旧测试从未在回调内存活期外造 nursery 压力）。
- 修法（`src/napi/env.rs`）：trace 加 `for (_, t) in &self.tsfns {
  t.js_cb.trace(trc) }`。回归：`tests/fixtures/napi/m3_tsfngc.c`
  （线程延迟 600ms 投递 3 条，JS 侧 8 轮 × 20 万小对象 ≈ 80MB nursery 压力；
  大对象直进 tenured 触发不了 minor GC，故不用大数组）+
  `tests/napi.rs::phase_napi_m3_tsfngc_roots_callback`。
  Revert-check：注释掉该行即挂（修前 139/修后过；`/tmp/wjs-tsfngc` 手动复现同，
  崩溃报告 `0x4b4b4b4b00000000` 与线上同特征）。
- 复现：`hmr-min9.mjs`（transform fetch + WS 握手 + 真 append）修前 139、
  修后 `full-reload` + `CLOSED` + `EXIT:0`；mimic 探针的 BigInt 是红鲱鱼
  （`Number(id & 0xffffn)` 混用抛 TypeError 触发 fsevents.c 的 assert，
  改纯 Number 探针即过，与 139 无关）。
- 推广为铁律：§4.40 的完整形态——**`Box<Heap>` 定址只保地址稳，trace 才保
  可达，两者缺一不可**；凡新增跨 GC 存活的 JS 值存储（含 HashMap value、
  新 napi 记录），trace 必须同步加，上线前 grep trace 覆盖。FinalizationRegistry
  在本引擎不可靠（真机同为 false），回归探针用"确定性压力 + 投递存活"断言，
  不用 canary（试过，不稳定）。

### 4.69 目录首条目 tarball 炸解包：暂存预建（2026-09-14，M5 vitest 牵引）

- 症状：`winterjs -a vitest` 在 `@types/chai@5.2.3` 必败：
  `cannot unpack @types/chai@5.2.3: unpack failed: failed to create
  node_modules/.staging-<pid>-<rand>`；其前 100+ 包全过。
- 根因（二连）：① 该包 tarball 非 `package/` 布局——条目以 `chai/` **目录条目**
  打头（`tar -tzf` 首行 `chai/`，常规 npm 包首条目即文件、无根目录条目）；
  ② tar 0.4.46 的 `unpack_in` 在父链校验里 `canonicalize(dst)`，首条目为目录
  时本仓又跳过预建父链（只对非目录条目建），暂存尚不存在即
  `failed to create <staging>`。常规包因首个文件条目的预建顺带建了暂存，
  故从未暴露。另：`chai/` 根的回落（暂存内唯一顶层目录即包根）本就写好，
  只是一直没活到那一步。
- 修法（`src/pm/install.rs::unpack_tgz`）：循环前 `create_dir_all(staging)`
  预建暂存，一行。回归：模块单测 `unpack_dir_first_non_package_root`
  （现场打目录首条目 gz，目标为尚不存在的暂存；revert-check 注释预建即挂）+
  黑盒 `phase5_install_dir_first_tarball_stub`（stub 下发→真装→require 出 42）。
  实证：修后 `-a vitest` 一遍过（25 包，`vitest@5.0.0`）。
- 过程教训：中途一次"旧二进制却装成功"系误判——`cargo test --test pm`
  会先编出**带修复的主二进制**（集成测试要 `CARGO_BIN_EXE`），其后对
  `install.rs` 的 revert/恢复编辑只改了 mtime 没改净内容，二进制实际已含修复；
  `ls -la` 的新旧比较证明不了二进制由哪版源码编出。结论打架时先查构建指纹链，
  不只看 mtime（§4.62 姊妹篇）。
- 复现：`curl registry @types/chai/-/chai-5.2.3.tgz | tar -tzf -` 首行即目录；
  修前模块单测必挂。
- 推广为铁律：凡"前 N 个全过、特定包必败"的安装失败，先 `tar -tzf` 看该包
  条目布局（根目录条目/非 package 根/符号链接三件），再怀疑网络与版本。

### 4.70 入口失败 + 开着的句柄 = 事件循环永不收割（2026-09-14，M5 fork 牵引）

- 症状：`fork("/no/such/xxx.mjs")` 父端无 error 无 exit，进程 hang（`bad-arg`
  同步校验正常；裸 eval worker 同样缺失 import 却正常 error+exit 1）。
- 根因：`run_module` 的入口 rejection 收割在 `event_loop` **之后**
  （`src/runtime.rs`）——子会话开着 parentPort（message/close 双监听）时循环
  永不 idle，收割点永不到。裸 worker 无句柄，首轮即 idle，收割正常。
  二分探针：端口在场 + 未捕获顶层拒绝即挂（`p6.mjs`），catch 住即回 END 后
  空转（`p5.mjs`，端口续命本身是正确语义）。
- 修法：`PumpStats` 加 `entry_failed` 旗——`pump_once` 内 RunJobs **之后**
  查 `entry_rejection.is_some()`（与 `process_exited`/`worker_terminated`
  同族检查点、同顺序），`event_loop` 见旗即 `break` 走既有收割上报
  （入口带专用捕获，不进 `unhandled` 表，收尾 `report_unhandled_rejections`
  不受影响；`process.exit` 优先顺序不动）。
- 复现：`tests/node/child.rs::phase9m_child_fork_errors`（修前 hang；
  另带出父断子不断 control 分支漏 `parentPort.close()`，同批修，
  `phase9m_child_fork_ipc` 的 `EXIT-EV 0 0` 覆盖）。
- 推广为铁律：凡"失败只在循环尾收割"的设计，必须回答"循环不退时失败去哪"——
  fatal 类决议（入口错/uncaught）一律检查点提前跳出，不等自然排空。

### 4.71 `Object.assign` 不自建自引用 + 真机 assert 形态先行（2026-09-14，M5）

- 症状：`assert.ok is not a function`（`phase4_node_assert_subset`），连带
  `net_echo_loopback`（服务端 handler 首行即 `assert.ok`，抛错后响应永不结束→
  链式 stall→server 不关→全进程 hang；http 回环 hang 同源，修 assert 即好，
  无 http 侧改动）。
- 根因：M5 把默认导出改可调用时写成 `Object.assign(ok, {...})`——assign 不建
  自引用键，`default.ok` 为 undefined。想当然补 `__default.ok = __default`
  又错第二遍：真机（node 26.8.2 实测）`assert.ok !== assert`，
  `assert.ok === assert.strict.ok`，名分别为 `assert`/`ok`。
- 修法（`src/builtins/node/assert.rs`）：具名 `function assert(value, message)`
  转调 `ok`，再 assign 全方法（`ok/strict/AssertionError` 等）后默认导出——
  五项逐项对真机：`typeof function`×2、自反 false、`ok===strict.ok`、
  双名、`assert(true)` 直调。
- 教训：§4.65 姊妹篇——"真机口径"必须**逐项实测**，不能只验规划的那两项
  （本次若只验 `typeof` + 直调，自引用错就漏网；`console` 黑盒的
  `assert-callable true false` 即真机逐项，修完直接绿）。
- 推广为铁律："与真机同款"断言必须列出全部可观察项并逐项对拍，缺一项即欠账。

### 4.72 fork 单槽监听 + CJS 具名上线后的黑盒同步（2026-09-14，M5）

- 症状一：`phase9m_child_fork_ipc` 修 hang 后报
  `NotSupportedError: ChildProcess event 'w9m-never'`——测试用凭空事件名验
  once/off，而 `on()` 对未知名按设计抛错。
- 修法（测试侧）：占位改 fork 路径永不触发的 `spawn` 事件——不用 error/exit
  是因单槽位 `once` 会顶掉同名常驻监听（`off` 对 exit/close 双重 wrap 也摘不净），
  不用 message/disconnect 是因流程内真会触发。注释写明三选理由。
- 症状二：`phase9j_cjs_interop_default` 的"命名导入必须失败"边界在具名导出
  上线后反绿为红（`import { v }` 成功）。
- 修法（测试侧）：边界翻转为成功断言（`cjs-named 41`），与
  `phase9j_cjs_interop_named` 同口径；旧"缺导出"注释标退役。
  另：`cjs_named` 的 `defaults … function …` 系 fixture/plain-object 与期望
  打架的测试 bug（`typeof` 应为 object），同批改。
- 推广为铁律：功能上线即全 grep 旧边界断言（"必须失败/必须抛"类），上线不改
  旧断言等于埋红；单槽事件设计下，测试占位事件必须选"永不触发 + 无常驻监听"
  的那一个。

### 4.73 线信封循环引用：先序 id + ref marker，两端同序才成立（2026-09-14，M5）

- 症状：vitest/tinypool 跨池消息含循环/共享引用对象——9i-2 信封语义下
  DataCloneError（循环抛错）或共享引用被拷成多份（身份丢失），池消息失真。
- 根因：9i-2 信封走"可克隆性探路 + 拒绝"，不保对象同一性。
- 修法（`worker.rs` `__packValue/__unpackValue`）：容器先序 id——打包侧首访
  `path.set(v, nextId++)`、重访出 `ref` marker；解码侧**同先序、先占位再填子项**
  （`refs[nextId++] = out` 先注册占位，子项回填），祖先后向引用恒可解。
  共享引用保留同一性（真机 structuredClone 口径）。9i-2 黑盒同步翻转
  （`w9i-circular true true true true`：自引用/自同/数组含自身/跨消息不串）。
- 复现：`tests/node/worker.rs` 循环保留用例（M5 升级前必抛 DataCloneError）。
- 推广为铁律：跨端序列化保循环 = "同序编号 + 先占位后回填"两条同时成立，
  漏任一即环解 undefined；编码器语义升级时全 grep 旧"必须抛"断言。

### 4.74 `process.stdout.write("", cb)` 无回调实现 = 等落定的协议永挂（2026-09-14，M5）

- 症状：vitest threads 池 worker 挂起——worker 线程 `flushStdio` 以
  `process.stdout.write("", cb)` 等前序块落定，本仓 write 只写不回调，cb 永不触发。
- 根因：`process_.rs` stdout/stderr 的 write 未实现 Node 的 `write(str[, cb])`
  回调面（Node：flush 后异步触发，即便直写成功也异步回）。
- 修法：write 识别函数实参，直写恒成功，cb 经 `queueMicrotask` 异步回
  （stdout/stderr 双侧）；黑盒 `write("",cb) → fired`（`tests/node/process_.rs`）。
- 推广为铁律：凡实现"带回调的写出面"，回调即使同步完成也必须异步触发
  （microtask），否则调用方"等 flush"的挂起式协议永不解锁。

### 4.75 napi 建面 AB 数据指针必须终身稳定（2026-09-15，M5 终线）

- 症状：vue-project rolldown 载荷下偶发堆腐坏（138/139 随 GC 时序漂移）。
- 根因：`napi_create_arraybuffer/buffer` 走 `JS::NewArrayBuffer`——SM 小 AB 用
  inline 存储、GC 可搬移；Node/V8 契约是 data 指针**终身稳定**，napi-rs 按
  Node 语义持指针跨 GC 读写即腐坏引擎堆。M3"偏差记档：指针须重取"实际不可
  执行——addon 无从得知何时 GC。
- 修法（`buffer.rs` `new_owned_ab`）：napi 建面一律走自有稳定存储（Rust 分配
  对齐 16 + 零填 + `NewExternalArrayBuffer` 桥 + free 回收）；JS 侧建 AB 传
  addon 的指针仍只保回调存活期（偏差记档）。
- 复现：`tests/napi.rs` 全量黑盒 + rolldown 真包载荷。
- 推广为铁律：宿主 AB 与"Node AB 指针稳定"的引擎差异，必须在 napi 建面
  一次性抹平，不得要求 addon 配合（addon 按 Node 契约书写）。

### 4.76 napi_wrap 的 ref 出参两路同发：拒发即 addon 对象无根（2026-09-15，M5 终线）

- 症状：任意对象路 napi_wrap 对非空 `result`（napi_ref 出参）报
  INVALID_ARG——napi-rs 对缓存跨回调复用的函数（PromiseRaw.then/catch 产物）
  恒带此参；拒发后对象无 ref 保活。
- 修法（`class.rs`）：任意对象路同样发初始计数 0 的 ref（类实例路同款）。
- 推广为铁律：Node 语义里"跨 scope 存活的 napi_value"只有 ref 一条正道；
  宿主对 ref 出参拒发 = 逼 addon 裸持 = 悬垂。fail-fast 前先想 addon 有无
  正当用法（§4.66 的 fail-fast 被 M4/M5 实战两次证伪为缺口）。

### 4.77 pin-all（槽位不截断）反例：finalize 链整体死亡（2026-09-15，M5 终线）

- 症状：为根治"悬垂槽位读"试 pin-all（arena 只增不截断）——external/wrap
  对象被槽位永久钉住，永不可达死态，**finalizer 从此永不跑**
  （m2 drain 探针 `free` 恒 0），external 内存无底洞泄漏（rolldown 每
  transform 产 buffer 即中招）。
- 根因：截断不只是"Node 值域契约"，更是 finalize 链的前提——槽位是 GC 根，
  不截断则回调产物永不可达死态（M2 头注早有预言，本轮实证）。
- 修法：回退截断（trampoline/dispatch/scope close 三处恢复），addon 跨窗持有
  走 §4.76 ref；测试侧 `m2_finalize` 改 drain 形（async_work 安全点回吐计数）。
- 推广为铁律：改内存管理语义前先问"finalizer 何时跑"——凡 GC 根面（槽位/表），
  收回 = 析构通道，两头（泄漏 vs 悬垂）都通向事故，正解只有 Node 口径
  （值随 scope、跨 scope 走 ref）。

### 4.78 GC sweep 内禁 addon finalizer：入队 + 安全点排空（2026-09-15，M5 终线）

- 症状（疑点，未单独复现钉死）：`napi_class_finalize` 在 GC sweep 内直调
  addon finalizer——napi-rs 的 finalizer 会经 `napi_delete_reference` 等改
  env 表 + Heap clearing barrier，即 GC 期间写堆，GC 元数据腐坏风险。
- 修法（`class.rs`/`buffer.rs`/`asyncwork.rs`/`lifecycle.rs`）：sweep 内只
  **入队** `pending_finalizers`（纯 Rust push，不碰 JSAPI/不写堆）；
  `asyncwork::dispatch` 入口（`class::drain_pending_finalizers`）/
  end_session 安全点（`lifecycle::run_wrap_finalizers`，JS 线程、无 GC 活动期）排空。
  external AB 的 contents 释放随 cb 一并延迟（external 语义：data 归 addon）。
- 复现：m2_finalize drain 形（GC 期入队 → 安全点全量 free）。
- 推广为铁律：GC 回调（finalize op）内只许纯 Rust 簿记；凡会触 JSAPI/写堆/
  改 GC 根面的 addon 回调，一律队列化到安全点。Node 的 finalizer 上下文限制
  （只许 napi 簿记面）在宿主侧必须由"延迟排空"落实，不能指望 addon 自律。

### 4.79 截断语义下跨回调裸持 napi_value = 悬垂，call_impl 先验 func 形态（2026-09-15，M5 终线）

- 症状：fixture 裸 static 存 `napi_value` 跨回调（无 ref），下一次
  `napi_call_function` 的 func 读出非 object（tag 0xfff9 系）——直接进
  JSAPI 即崩（bad func → JIT/解释器读垃圾对象）。
- 修法：`call_impl` 入口先验 `fn_v.is_object()`（tag 检查不 deref），非 object
  返 INVALID_ARG + last_error（"stale napi_value?"）——Node 同款是 UB，本仓
  给可读错当现形点；fixture 侧改 `napi_create_reference`（Node 正道）。
- 复现：m2_finalize drain 探针（ref 前必现 BAD-FUNC，ref 后干净）。
- 推广为铁律：§4.31 教训的 napi 版——跨 JS/Rust 边界的句柄生命周期，契约
  （scope 存活期/ref）违者宿主要能"可读地死"而非 UB；新增宿主入口先想
  "输入是垃圾时怎么死"。

### 4.80 CJS 包装五连链的裸 JSVal 栈拷贝：GC 搬移即悬垂（2026-09-15，M5 终线②）

- 症状：vitest worker 线程（fork 底座）加载 css-tree 深图必现 138/139，
  exit 1 静默（scripts 父进程把信号死亡映射成 1，`echo $?` 全程骗人）。
  lldb 实锤：`require_cjs_file` 闭包 → `call_one(cur, arg)`，cur/arg 位型
  0xFFF8/0x5800 垃圾，`js::Interpret` 写穿 KERN_PROTECTION_FAILURE。
- 根因：五连柯里化调用链里 `cur`（wrapper 函数）与 exports/require/module/
  filename/dirname 实参全是裸 JSVal 栈拷贝——`call_one` 入口 rooting 只保
  **调用中**，不保**调用间**；链内每次 call_one 都分配 curried 闭包可触发
  GC，nursery 搬移后栈拷贝即悬垂。§4.40/§4.68 完整形态第 N 例。
- 修法：cur 与各实参全程 `rooted!` 槽位、调用点现读现传（`.get()` 后无
  JSAPI 直入 call_one 入口 rooting，窗口为零）。
- 复现：修前 `--run node_modules/.bin/vitest` 必 138；修后全链通。
- 推广为铁律：凡"多次 JS 调用组成的链"（CJS 包装/遍历/reduce 式），链上
  中间值一律 rooted——call_one/call_two 的入口 rooting 不是链的保活凭证。
  另：父进程把子进程信号死亡映射 exit 1 会吞掉整个崩溃类，结论打架先看
  真实退出码（`--run bin` 直跑拿原始 rc）。

### 4.81 require 条件族：resolver 必须按调用方分流（2026-09-15，M5 终线②）

- 症状：`require('magic-string')` 拿到 ESM namespace，`new MagicString()`
  报 is not a constructor（真 node 正常）。
- 根因：resolver 单例写死 `["node","import"]` 条件——require 走 import 条件
  挑了 ESM 入口。真机口径（26.8.2 对拍）：require 严格走 `["node","require"]`，
  imports-only 包直接 `ERR_PACKAGE_PATH_NOT_EXPORTED`（无 import 回落）；
  require(esm) 只作用于"已解析文件是 ESM"的情形。
- 修法：`resolve.rs` 条件族双单例（`Cond::Import/Require`）+ `resolve_require`
  入口，require.rs 四调用点（require_value/resolve/resolve_from/静态名跟随）
  全切。
- 推广为铁律：module resolution 的条件族是调用方属性（import vs require），
  共享 resolver 必须参数化；"require 拿到 ESM 就垫 default"类的互操作补丁
  在双条件包面前全是错药。

### 4.82 语义升级（require(esm)+detect-module）后旧边界断言全翻转（2026-09-15，M5 终线②）

- 症状：require(esm) 上线后 `phase4_require_errors`/`phase9k` 静默挂
  （"ESM 拒绝"断言反绿为红）；`phase9j` 具名发现 fixture（自定义 `__es(...)`
  转出）link 期报缺导出。
- 根因：① 4dbb202 上线 require(esm)/detect-module 时未 grep 旧"必须抛"
  断言（§4.72 二进宫）；② 自定义转出包装真机 cjs-module-lexer 同样不识别
  （实测：静态 import `__es` 形 link 期同败），我们的静态 lexer 行为已对等。
- 修法：断言翻转对真机逐项实测（typeless ESM .js require 成功、__exportStar
  形真机识别——fixture 改该形 + 本地补 helper 定义）。
- 推广为铁律：语义升级的收尾动作 = grep 全部旧断言逐个对真机；fixture 的
  "聪明写法"若真机不认，宁可改 fixture 也不给宿主加超集。
  附：`-r test:unit` 里 hello.test.js（node:test）在 vitest 4 下真机同败
  （"No test suite found"，exit 1）——真机对等即验收，别替 fixture 修世界。

### 4.83 无编码 fs 读必须返回 Buffer：String(buf) = utf8 内容（2026-09-15，M5 终线②）

- 症状：vite PostCSS 配置加载报 `JSON.parse ... column 4` 假错。
- 根因：`readFileSync`（无编码）返回裸 Uint8Array——`String(buf)` 走
  TypedArray join 成 "byte,byte,…"，JSON.parse 隐式 ToString 后把字节列表
  当 JSON（`{` 的字节 123 → 解析出 "123" 后 column 4 报错）。
- 修法：`__fsDecode(encoding=null)` 统一 `Buffer.from(bytes)`（Node 语义：
  isBuffer/toString()/String(buf) 全 utf8 口径）；两调用点（sync + fh 读）
  一处收口。
- 推广为铁律：Node API 返回"Buffer 的地方"必须是真 Buffer（Uint8Array 子类
  不够——toString/String/isBuffer 三面都要对）；裸 TypedArray 与 Buffer 的
  差异隐在隐式转换里，JSON.parse(buf) 是现成探针。

### 4.84 napi_get_cb_info 余槽必须填 undefined（node Args() 契约）（2026-09-15，M6）

- 症状：官方 js-native-api 3_callbacks 在本仓 139——addon 在 `argc==1` 断言
  后照读 `args[1]`（请求 2 槽、实际 1 参）。
- 根因：node 的 `FunctionCallbackWrapper::Args()`（js_native_api_v8.cc）
  把请求槽位余下部分**全填同一 undefined**——这是书面契约外的硬契约，
  官方套件直接依赖；我们的实现只填实际个数，余槽留 addon 栈垃圾 = UB 读。
- 修法：照抄 node——`put(UndefinedValue())` 单槽共享填满请求槽位，`*argc`
  回写实际个数。
- 复现：`tests/napi.rs::phase_napi_m6_official_js_native_api_spot_check`
  （官方 2_function_arguments + 3_callbacks 原文 verbatim，vendored 头编译）。
- 推广为铁律：宿主实现 napi 面时，"官方套件怎么写"本身就是契约的一部分——
  选点回归（M6）不是仪式，是抓这类暗契约的唯一网；遇 addon 读"没给出的
  参数"先查宿主填充语义再骂 addon。

### 4.85 定时器实参从未展开：`fire_due` 直调 + 缓存的 helper 从未被读（2026-09-15，10a）

- 症状：`setTimeout((a, b) => ..., 0, 'x', 7)` 的回调收到 `(cb自身, args数组)`，
  而非 `('x', 7)`——定时器实参透传全灭。
- 根因：`fire_due` 用 `JS_CallFunctionValue(fun, [cb, args])` 直调回调
  （§4.9 禁 `Rooted<ValueArray>` 的语境下写错了调用形状）；而 prelude 的
  `__wjs_call(cb, args)` 展开器虽有缓存位（`RootedState::call_fn`，runtime 起
  即 set + trace），却没有任何读者——set 时没人接线，读侧永远直调。
  旧用例全用闭包传参（`setTimeout(() => resolve(v))`），躲过全部回归。
- 修法：`fire_due` 经 `call_two(cx, global, call_fn, cb, args)` 走 `__wjs_call`
  展开（`src/builtins/timers.rs`；`call_two` 的 UNSAFE-BOUNDARY 覆盖 + 本条）。
- 复现：`tests/builtins.rs::phase10a_immediate_and_timeout_class`
  （`args x 7` 行；修前为 `args <fn> x,7`）。
- 推广为铁律："缓存了" ≠ "用上了"——新增缓存位必须同时提交第一个读者，
  reviewer 按"set 有无 get"逐项对；回调传参形态（闭包 vs 实参）是两种覆盖，
  只测一种等于没测。
- 附带（同批 10a-3）：全局 `setImmediate` 缺失即补（setTimeout(0) 近似，
  check 语义记档）；Timeout/Immediate 改真类（constructor.name 真机口径，
  类本体不出 prelude 块作用域——真机 `globalThis.Timeout === undefined`，
  污染全局即错）；plan.md 9b"裸 number"记档同步勘误（M5 起已返回对象）。

### 4.86 Writable 默认 `autoDestroy:true` 会杀保活连接（2026-09-15，10b）

- 症状：客户端请求发出去服务端永远收不到（`ref-got` 缺席），`_final` 后跟一条
  无来由的 `_destroy`（堆栈：`finish@writable → destroy@http_framing`）。
- 根因：`autoDestroy` 默认 true——upload 一 finish 就自动 destroy，而本仓
  `_destroy` 负责杀 socket（连接未建好即杀，请求死在半路）。
  连带：`emitClose` 默认 true 会在 upload finish 后自动发 req 'close'，
  与 9d 口径（close 在响应收齐后）冲突，造成双 close。
- 修法：`ServerResponse`/`OutgoingMessage` 一律
  `super({ autoDestroy: false })`（销毁只显式：server.close/agent.destroy/
  用户 destroy/对端死亡）；`OutgoingMessage` 另加 `emitClose: false`，
  req 'close' 由 `__finishResponse`/`__onSockCloseEv` 手动在 res 'end' 后发。
- 复现：任意 `http.get`（修前服务端零收到；`tests/node/http.rs` 全挂）。
- 推广为铁律：凡把"连接"装进 stream 壳，`autoDestroy/emitClose` 默认值先按
  连接语义重审一遍——流的生死 ≠ 连接的生死。

### 4.87 增量泵的"等更多数据"必须带空 rest（2026-09-15，10b）

- 症状：GET→POST 交界 flaky hang（约 1/3；单 POST 全过，加日志全过）。
- 根因：`__pumpChunked` 等数据的 `return { done: false }` 不带 `rest`，
  调用方 `st.buf = r.rest` 把缓冲置 `undefined`——下个包一到
  `__concat(undefined, …)` 抛错，外层 catch 直接 `sock.destroy()`。
  包不拆就全到（`done:true` 带 rest），一拆就炸，故 flaky。
- 修法：6 处 `return { done: false, rest: new Uint8Array(0) }`（余字节已进
  `fr.buf` 内部态，调用方覆盖为空即对）；CL 泵本就带 rest，无事。
- 定位手法：可复现的 ping-pong 循环（20 轮，成功打点、卡住即停）比反复跑
  原测试更快——`pp.mjs` 式"窄探针 + 全事件追踪"三轮即抓到（本次还抓到
  `cli-close` 早于服务端完工，顺藤摸到 destroy 链）。
- 推广为铁律：增量解析器的返回形状必须全字段齐备（含"无进展"分支）；
  flaky hang 先怀疑包边界，再怀疑逻辑（`pp.mjs` 留档思路，不入库）。

### 4.88 TLS 写半部 drop 不发 close_notify，双边读端永 block（2026-09-15，10b）

- 症状：https keep-alive 功能全对（复用计数/`reusedSocket` 全绿），但进程
  永不退出（lsof 双边 ESTABLISHED；destroy 全送达、purge 全缺席）。
- 根因：tokio TCP 写半部 drop 自带 FIN，tokio-rustls 写半部 drop 不发
  close_notify——destroy 后双边读端各 block 各的，Close 永不到（9d 纯 TCP
  从未暴露）。
- 修法（`src/builtins/node/net.rs` writer task）：`NetCmd::Close` 路径先
  `w.shutdown().await` 再 break（TLS 发 close_notify + 关 TCP 写；
  TCP 侧与 drop 语义重复，无害）。
- 连带（同案第二漏）：对端 FIN 到达**池化空闲** socket 时只发 End（写端活着
  不收尾），而池 socket 永不会再有人 end——`net.js`/`tls.js` 的 end 处理器
  首行加池检查：`__inPool` 即 destroy（半关不可复用，Node 同样踢出池）；
  Agent 侧 `__release/__acquire` 置 `__inPool` 位；泵层加 `reader_done` 旗，
  writer 退出时读端已走则补 Close（`close_once` 防双发，与既有 EOF 路径收敛）。
- 复现：`tests/node/https.rs::phase10b_https_keepalive_reuse`（修前永 hang；
  http 同形不 hang 是 TCP FIN 掩盖，不是语义对）。
- 推广为铁律：传输换底座（TCP→TLS）后，"关闭收敛"必须重走一遍——drop/
  shutdown/EOF 三者的底座语义各不相同，9d 的 TCP 经验不自动继承。

### 4.89 `end()` 与 connect 竞速：`_final` 时 socket 未就绪则请求永不发出（2026-09-15，10b）

- 症状：`http.get` 后服务端零收到、无报错、无 error，进程空转到 watchdog。
- 根因：`Writable.end()` 同步走完 `_final` 时 socket 尚未连通（连通要过事件
  循环），旧代码只管"连通时发出"，未连通分支置了 `pendingFinal` 旗却无人消费。
- 修法：connect/secureConnect/复用 attach 三处统一收敛——先 `__tryFlush()`
  （holdback 转 chunked），再消费 `pendingFinal`（`__flushFinal`：头未发走 CL
  快捷，已发补 0-chunk；漏后者即 POST 交界 hang，见 §4.87 同源）。
- 推广为铁律：凡"构造即发"（请求/连接）遇"异步就绪"（connect/握手），就绪
  回调必须同时服务"已就绪数据"与"已结束标记"两件，缺一件即半吊子挂起。
  定位时先分清"没发出去"（服务端零收到，curl 对照）还是"没解析出来"。

### 4.90 vm 重跑即炸：sync-in 的重定义必须回落赋值（2026-09-15，10c-3）

- 症状：同 context 第二次 `runInContext` 即 `vm could not define sandbox property`
  ——当前轮新建了全局（function/var 声明）时必发；纯求值重跑无事。
- 根因：每次 run 前 `__syncIn` 把沙箱属性逐个 `JS_DefineProperty` 重打一遍；
  上轮 sync-out 又同步回来的新全局（值为跨 compartment CCW）在重定义时失败。
  纯数据种子（`{x:1}`）重打无事，故 9i 旧测试全绿掩盖（REPL 是首个高频复用者）。
- 修法：`vm_set` 里 define 失败即回落 `JS_SetProperty` 赋值语义（更新值、
  保留既有描述符；双失败才抛）；收敛函数 `set_prop_value` 进 `jsapi_glue`
  （`UNSAFE-BOUNDARY` + 覆盖测试名）。
  附带同案：DONT_CONTEXTIFY 全局跑 `runInContext` 首轮即炸（sync-in 逐个重打
  标准内建）——同修法一并治愈。
- 复现：`tests/node/vm.rs::phase10c_vm_rerun_with_new_globals`（修前第二跑必炸）。
- 推广为铁律：凡"每次调用前全量同步"的设计，必须回答"已存在项怎么办"——
  define-if-absent + set-if-present 两条路，缺回落即二次调用必炸；
  旧测试只跑一次的面，新功能复用即现形（§4.24 多 run 教训的 vm 版）。

### 4.91 turso 懒执行 vs Node prepare eager：校验走只备不步进的 cols（2026-09-15，10d）

- 症状：`db.prepare("NOPE SYNTAX @@")` 不抛（真机 `ERR_SQLITE_ERROR` 在 prepare 期）。
- 根因：turso `conn.query` 只备语句不步进——坏 SQL 要到首次 `next()` 才暴露；
  本仓 prepare 纯 JS 构造，从不碰 Rust，错误自然延迟到 run/get/all。
- 修法：Rust 加 `Cols` op（只调 `query()` 取 `columns()` 元数据，不 `next()`，
  无副作用、INSERT 亦不执行）；`DatabaseSync.prepare()` 内先调一次校验，
  坏 SQL 即抛；`columns()` 复用同一 op。
- 附带同案：hickory `Name.to_string()` 带 FQDN 尾点（`localhost.`）——Node 口径
  无尾点，Rust 侧统一 `trim_dot`（`src/builtins/node/dns.rs`）。
- 复现：`tests/node/sqlite.rs::phase10d_sqlite_errors_boundary`（`prep-err` 行修前缺席）。
- 推广为铁律：宿主底座"懒"的面（prepare/query 构造），对齐 Node eager 语义时
  必须显式加一次无副作用的校验调用；校验调用本身不得有副作用（不步进/不执行）。

### 4.92 RustCrypto 泛型顺序与 AEAD 两侧：ccm 的 M/N 反直觉 + 解密 GHASH over 密文（2026-09-15，10e）

- 症状一：`ccm::Ccm<Aes128, U12, U8>` 编不过（`SealedTag for U12` 不满足）。
- 根因：`Ccm<C, M, N>` 的 M=tag 长、N=nonce 长（文档注记，非直觉顺序；
  例 `Ccm<Aes256, U10, U13>` 是 tag 10 + nonce 13）。
- 修法：分发宏按 `(key → tag → nonce)` 逐级 match（`src/builtins/node/crypto.rs`
  `ccm_crypt`，注释写明顺序）。
- 症状二：GCM-J0 手工路径加密与真机逐字节一致，解密自家密文即拒收。
- 根因：GHASH 的输入写成了解密输出（明文）——tag 是 over **密文**算的，
  加解密两分支必须各取各的输入（`gcm_manual` 的 `(gct, glen)` 分流）。
- 复现：`crypto_gcm_j0_known_answer`（修前解密断言挂；加密向量先绿极具误导性——
  加密绿 ≠ 路径对，AEAD 必须加解密双向断言）。
- 推广为铁律：AEAD 手工路径的测试必须含"自加密自解密 + 真机交叉"双向；
  泛型顺序以库文档注记为准，不按直觉（Ccm/Ctr32BE 这类双长度泛型先查）。

### 4.93 后台 sleep/kill 判 hang 必看退出码（2026-09-15，10e，§4.62/§4.67 姊妹篇）

- 症状：cluster 探针 `sleep 10; kill` 后输出停在中间，误判事件循环 hang。
- 根因：进程早已正常退出（EXIT=0），sleep 到点 kill 扑空——输出截断是杀时
  机问题，不是 hang；直接跑取 `$?` 即见 0。
- 修法：凡判 hang，先直接跑取退出码（+ 超时门）；后台法只用于必现 hang 的
  堆栈/输出采集，且一律落盘读。
- 推广为铁律：无退出码的 hang 结论不可信；自然退出（idle 收敛正确）与 hang
  在输出上看起来一样，区分只认退出码。

### 4.94 park 唤醒集与存活判定集必须分家：unrefed 到点要醒、不续命、不算 progress（2026-09-15，10f timers）

- 症状三连：① unrefd-interval-still-fires 的 1ms unrefed interval 迟到 1s 才触发（睡到 refed 看门 1s 到点）；② unref.js 的 1ms unrefed interval 空转整整 10 秒（单轮 pump ≈1.1ms > interval 1ms，每轮都到期 → `progressed` 永真 → 永不 idle，LONG_TIME mustNotCall 必炸）；③ unrefed-in-callback 永 hang（callback 内 `unref()` 时 entry 已摘表，native noop，重排丢旗 → interval 恒 refed）。
- 根因：park 目标、存活判定、progressed 三个概念混用同一个 `next_deadline`/timers 计数。node 口径：uv poll 超时含 unrefed timer（要醒）、`uv_loop_alive` 只看 refed（要退）、unrefed 触发不算续命（防转），但其回调的 microtask 仍要一轮 RunJobs（§4.18）。
- 修法：`next_wake()`（含 unrefed，只给 park）与 `next_deadline()`（refed-only，给 idle）分家（`src/builtins/timers.rs` + `runtime.rs` park 点）；`PumpStats` 拆 `timers`/`timers_unrefed`，progressed 只数 refed，`idle && unrefed-only` 给**一轮** grace continue 后即 break；unref 旗加 plain 侧账 `unrefed_ids`（触发期 unref 落账，重排时并读）。
- 复现：`tests/builtins.rs::phase10f_timer_face_unref_uncaught`（修前分别迟到 1s/空转 10s/永 hang）。
- 推广为铁律：凡事件源带 ref 语义，"到点唤醒"与"存活判定"必须显式分家；progressed 只统计能续命的源，任何触发的 microtask 都要一轮 RunJobs 兜底——§4.18 完整形态的 ref 版。

### 4.95 宿主调用户回调一律 Reflect.apply，禁走实例属性查找（2026-09-15，10f timers）

- 症状：user-call 套件挂——回调 `fn.call/.apply` 被猴子补丁成字符串后，`self._onTimeout.apply(...)` 报 not a function（node 同套件全过）。
- 根因：实例上的方法查找先命中自有补丁；node 用内部 ReflectApply（内建，无视补丁）。
- 修法：prelude timers 触发改 `Reflect.apply(self._onTimeout, self, self._timerArgs)`（实参也 live 读，套件 unenroll 直改字段同源）。
- 推广为铁律：宿主侧凡"以方法形式"调用用户提供的函数，一律走 Reflect/内建引用；用户对象上的同名属性是它的数据，不是我们的调用机制。

### 4.96 CJS→ESM 移植的 primordials 残留：未定义符号在错误路径换错误类型（2026-09-15，10f timers）

- 症状：promises-scheduler 套件报 `rejects: unexpected throw`——真因是移植稿里 `return PromiseReject(...)`（node primordials 名）未随行，运行时 ReferenceError，assert.rejects 把它当 mismatch 报——两层错掩盖一层。
- 修法：`Promise.reject(...)`；移植面收尾 grep primordials 名单残留（PromiseReject/PromiseResolve/NumberIsNaN/MathMax…）。
- 教训：错误文案不可信（§4.66 Either 兜底同源）；"mismatch" 类报错先打印实际 rejection 值再动断言。

### 4.97 全局单例的方法族必须 this 基；`--run` 完成值回显随 CJS 主模块化消失（2026-09-15，10f timers）

- 症状一：process-tampering（`globalThis.process = {}` 后 setImmediate）——node common 载入期 `const process = globalThis.process` 捕获真身，我们的 on/emit 等方法体读**全局** process 绑定，tamper 后 `undefined["exit"]` 炸。
- 修法：process 监听器族方法一律 `this` 基（`__wjs_emit` 顺带回监听数，供 `__wjs_uncaught` 判"是否已处理"——探针为 0 时 native 保持 pending 原样走 fatal，错误信息不降级）。
- 症状二：`run_file_from_tempdir` 断言 `--run` 回显完成值 `42`——b701cf6 typeless .js 入口走 CJS require 主模块后无 rval，回显消失（HEAD 实测同红，非本轮回归）。真机 `node app.js` 本就无回显——静默才是对等，测试改执行效应断言（§4.82 老断言翻转）。
- 推广为铁律：全局单例的方法族，方法体禁解引用全局绑定（`this` 或载入期捕获二选一）；"回显类"断言在入口语义升级后逐个对真机。

### 4.98 自写编码器不能指望 `encodeURIComponent` 兜底：unreserved 恒放行（2026-09-16，10f url）

- 症状：`pathToFileURL('/foo~')` 出 `file:///foo~`，套件期望 `%7E`；条件表里明明有 `ch === '~'`。
- 根因：`encodeURIComponent` 对 unreserved（`~ ! ' ( ) * - . _` + 字母数字）**恒原样放行**——
  条件命中了，编码函数却吐回原字符。同理 `noEscapeAuth` 逐字符手工编码里 `%XX` 由
  `toString(16)` 生成是安全的，但混用 `encodeURIComponent` 的分支必须逐字符核对其放行集。
- 修法：被收字符里凡 unreserved 者手动给字面量（`out += ch === '~' ? '%7E' : encodeURIComponent(ch)`）。
- 推广为铁律：凡"自选编码集 + 借 encodeURIComponent 执行"的编码器，放行集 = 自己的表
  ∩ encodeURIComponent 的放行集，交集外字符一律手动字面量；新增编码集先对真机
  逐字符 diff（本轮 `~`/`^`/`|`/`[`/`]` 全是套件期望反推出来的）。
- 复现：`test-url-pathtofileurl.js` 尾部 `'/foo\r\n\t<>"#%{}|^[\\~]`?bar'` 用例（修前 `%5C~`）。

### 4.99 `assert.throws` 函数形期望：`instanceof` 必须以 Error 子类为门（2026-09-16，10f url）

- 症状：`assert.throws(fn, (e) => e instanceof URIError)` 报 "unexpected throw"，
  校验器**从未被调用**（打点实证）；同一校验器直接调用全过。
- 根因：旧实现先做 `e instanceof expected` 再回落校验器——箭头函数无
  `prototype`，`e instanceof arrow` 按 OrdinaryHasInstance 取 `C.prototype`
  即抛 TypeError，被外层 `catch { ok = false }` 整体吞掉，校验器永不到达。
  类（有 prototype）不受影响，故旧套件全绿掩盖。
- 修法：`expected.prototype !== undefined && expected.prototype instanceof Error`
  才走 instanceof（node 口径：Error 子类 = 构造器形，其余 = 校验器形）；
  instanceof 失败仍回落校验器调用。
- 复现：`tests/node/url.rs::phase10f_url_parity_suite`（`urierr` 行；修前 REJECTED）。
- 推广为铁律：对"函数既可能是构造器也可能是校验器"的双形态参数，形态判定
  （prototype 链）必须先于使用形态的运算符；`instanceof` 右侧无 prototype
  是抛错不是 false，凡 try/catch 包 instanceof 都要想到这一层。

### 4.100 肉眼同形异码点：探针先核对码点，NFKD/NFKC 跟 UTS46 走（2026-09-16，10f url）

- 症状一：探 `'℀'` 用 `\u2440` 白转一轮（`instanceof URIError` 校验块其实早修好了）——
  U+2100 与 U+2440 打印**一模一样**（都是 ℀），但 NFKD 分解迥异
  （U+2100→`a/c` 真斜杠、U+2440 不分解）；套件用的是 U+2100（hexdump 才实锤）。
- 症状二：IDNA 映射用 NFKD 后 punycode 出 `xn--bucher-xyd`，真机期望
  `xn--bcher-kva`——分解态没做**规范组合**，`u+◌̈` 没回到预组合 `ü`。
- 修法：IDNA 标签走 `normalize('NFKC')`（NFKD 分解 + 组合，两套件关注点都覆盖：
  badIDNA 靠分解段、punycode 形靠组合段）；ignored 码点（软连字符 U+00AD）删除
  后空标签即抛。
- 推广为铁律：凡"同形字符"对比实验（对拍/探针/fixture），先 `hexdump`/码点核对
  再下结论；Unicode 归一化选 NFD/NFKD/NFC/NFKC 不是口味——语义对齐哪个标准
  （UTS46=分解+组合）就用哪个，分解态直接喂下游（punycode/hex 表）必错形。

### 4.101 `exit`/`close` 事件 node 是双参 `(code, signal)`；spawn 默认 stdio 是 pipe（2026-09-16，10f url）

- 症状：`spawnPromisified` 解构 `close(code, signal)` 全收 undefined（`{status:1}`
  透进断言）；`child.stderr.setEncoding` 报 not a function / is null。
- 根因（三连）：① 派发把 `{status, signal}` 单对象当唯一实参（fork 文档曾把它
  合理化为"与 spawn 同形"——两处错互相印证≠对）；② spawn 默认 stdio 写成
  `inherit×3`（node 缺省 `pipe×3`，数组缺项也补 pipe）；③ stdout/stderr 给的是
  Web ReadableStream（无 `on('data')/setEncoding`）——自家黑盒用 `getReader()`
  编码了实现偏差（§4.65 同源）。
- 修法：派发走 `call_two(handler, status, signal)`；fork 路径同翻；访问器 wrap
  改双参落定 exitCode/signalCode；默认 stdio pipe；流面改 legacy Readable
  （`on/once/off/setEncoding/pause/resume/destroy`，data 缺省 Buffer
  （§4.83）、setEncoding 后为串），自家黑盒 2 处 `getReader()` 同步翻转。
- 推广为铁律：事件回调的**实参形状**是跨边界契约（用户代码逐名解构），移植时
  以真机签名逐字对拍，不做"对象打包"的自作主张；旧文档的"同形"引用链要溯源
  到真机，不能拿自家另一处偏差当依据。

### 4.102 `process.emitWarning` 是 nextTick 异步派发（2026-09-16，10f url）

- 症状：`test-url-parse-deprecation` 的"先 `url.parse('foo')` 后
  `expectWarning`（挂监听）"序列在同步派发下警告丢失（监听挂上前已走 stderr）。
- 根因：node 口径 warning 经 nextTick 异步派发（真机实证：emitWarning 后同步
  读收集器为空、setImmediate 后才见）——同步派发是实现偏差；10f timers 的
  `phase10f_timer_face` 黑盒同步收集 `warn []` 也在全量回归现形。
- 修法：`process_.rs` emitWarning 改 `queueMicrotask` 派发（监听列表**现读**，
  microtask 前挂的监听有效）；timers 黑盒收集点同步移到 await 之后（真机
  口径翻转）。
- 复现：`tests/node/url.rs::phase10f_url_parity_suite`（`dep0169` 行）+
  `tests/builtins.rs::phase10f_timer_face_unref_uncaught`（`warn` 行）。
- 推广为铁律：凡 node 文档写明"异步派发/异步回调"的面（warning/写入回调等
  §4.74 同族），即便宿主能同步完成也必须异步触发；同步完成的便捷性不是契约，
  套件时序（先触发后挂监听）就是按异步写的。

### 4.103 Proxy 包 Uint8Array 必须透传 newTarget，否则子类化全灭（2026-09-16，10f buffer）

- 症状：`Readable.fromWeb` 回的 chunk `constructor.name` 是 `Uint8Array` 非
  `Buffer`（`phase9b_stream_web_and_consumers` 的 `f2w 1 Uint8Array`；真机 `Buffer`）。
- 根因：`Uint8Array` 文案桥 Proxy 的 `construct(target, args)` 未收第三参
  `newTarget`，`Reflect.construct(target, args)` 即按基类构造——`class X extends
  Uint8Array`（含内部 `FastBuffer` 的显式 `super(...args)` 与空构造器两形）
  全灭为基类原型。注释还写了"extends 透传不受影响"，想当然，未实测。
- 修法：`construct(target, args, newTarget)` + `Reflect.construct(target, args,
  newTarget)`（`src/builtins/mod.rs`）。
- 复现：`new (class E extends Uint8Array {})(4)` 的 `proto===E.prototype`
  （修前 false；`tests/node/buffer.rs::phase10f_buffer_parity_fixes` 的 `subclass` 行）。
- 推广为铁律：凡包装全局构造器的 Proxy，`construct`/`get` 的后位参
  （`newTarget`/`receiver`）默认透传，不确定即全透；"不受影响"类断言必须配
  子类化探针（`extends` + `new` + `proto===` 三件），不能只测直接构造。

### 4.104 伪 ArrayBuffer 须品牌拒收，不能信 tag（2026-09-16，10f buffer）

- 症状：`Buffer.from(fakeAB)`（`Object.setPrototypeOf(AB, ArrayBuffer)` 伪造）
  报 `incompatible Object` 引擎文案，与套件 `ERR_INVALID_ARG_TYPE / an instance
  of AB` 对不上。
- 根因：`__wjs_bufIsAnyAB` 只看 `instanceof` + `toString` tag——伪造链两者全过；
  真机走 V8 `IsArrayBuffer` 内部槽检查，伪造即拒。直接进 `FromArrayBuffer` 即在
  引擎内抛 incompatible，断言对不上。
- 修法：`Buffer.from` 的 AB 分支先 `void value.byteLength` 试探（真槽可读，
  伪造抛），失败落空到尾部统一 invalid-arg（`__wjs_bufSpecificType` 的
  `an instance of AB` 口径正好对上）（`src/builtins/mod.rs`）。
- 复现：`tests/node/buffer.rs::phase10f_buffer_parity_fixes` 的 `brand` 行；
  套件 `test-buffer-arraybuffer.js`（修前 `incompatible`）。
- 推广为铁律：凡"is-X"判定走 `instanceof`/tag 的，伪造原型链即视为已撞——
  关键入口（`from`/`isUtf8` 等）必须加一次内部槽试探（读 `byteLength`/`slice`
  等）；§4.54"看 OID 不看坐标"的 JS 品牌版。

### 4.105 全局 structuredClone 的 transfer 须 detach，否则 isAscii 视残留为真（2026-09-16，10f buffer）

- 症状：`isAscii`/`isUtf8` 的 detach 段修前 `after false false false`
  （应全 true）——`structuredClone(ab, {transfer:[ab]})` 后 `ab.byteLength` 仍 1。
- 根因：`src/builtins/clone.rs` 的 JSON 中转实现完全忽略 `options.transfer`，
  只克隆不 detach；`__wjs_bufAsU8` 的 detached 容错（视空）永无触发机会。
- 修法：`structured_clone` 入口先走 transfer 列表（`transfer` 数组 + 
  `IsArrayBufferObject` 品牌 + `DetachArrayBuffer`，失败跳过不致命），再走既有
  JSON 中转（返回值仍中转语义，detach 系副作用为准）。
- 复现：`tests/node/buffer.rs::phase10f_buffer_parity_fixes` 的 `detached` 行；
  套件 `test-buffer-isascii.js`/`test-buffer-isutf8.js`（修前 `Expected false
  strictEqual true`，行号恒 `254:53` 系 common 断言包装）。
- 推广为铁律：凡"带选项的克隆/投递"（transfer/neutering），副作用（detach）
  与返回值同等重要；JSON 中转实现上线新选项必须先问"源端状态变了吗"。

### 4.106 池 AB 不可转移：共享要池化、转移要拒收，两件缺一即挂池套件（2026-09-16，10f buffer）

- 症状：`test-buffer-pool-untransferable.js` 修前 `a.buffer !== b.buffer`
  首断言即挂（本仓直接分配，无池）。
- 根因：Node 小串（`< poolSize/2`）走 64KB 池（`fromStringFast` 口径，8 字节对齐，
  满即新池），池 AB 经 V8 `markAsUntransferable` 标记——`postMessage(..., [pool])`
  抛 `DataCloneError(25)`、`pool.transfer()` 抛 `TypeError`，且事后不 detach。
  本仓三件全缺。
- 修法（纯 JS + worker 拒收，零新依赖）：prelude 建池（`__wjs_bufPoolAB`/
  `__wjs_bufPooled: WeakSet` 全局暴露/`__wjs_bufPoolOffset` + 对齐/满转）+
  `fromStringFast` 小串走池（`scratch` 视图写入 + 实长推进）+
  `ArrayBuffer.prototype.transfer` 对池内抛 TypeError +
  worker `__normTransfer` 对池内抛 `DataCloneError(25)`（`__dataCloneErr` 补
  `code=25`；视图取 `buffer` 同判）。
- 复现：`tests/node/buffer.rs::phase10f_buffer_parity_fixes` 的
  `pool-share/post/still/xfer/still2` 五行；套件修前首断言挂、修后 `0/0 ✅`。
- 推广为铁律：凡"性能优化有可观察共享"（池化/缓存/复用），对拍前先查"共享 +
  不可转移/不可变"二元组——只做共享不做拒收，转移套件必挂；`code`（25）与
  `name` 同为契约，补一漏一即红。

### 4.107 `util.inspect` depth -1 空容器显体 + 函数不走 primitives（2026-09-16，10f buffer）

- 症状：`test-buffer-from.js` 的 `{__proto__:null}` 期望
  `[Object: null prototype] {}`，本仓 common  helper 算出 `[Object]`——
  `Buffer.from` 的实际报错是对的（`__wjs_bufSpecificType` 口径），期望串错了，
  两边对不上。
- 根因（二连）：① `inspect2` 的 `depth<0` 一刀切回 `[Array]`/`[Object]`——
  真机空容器仍显体（`[]`/`{}`/`Foo {}`/`[Object: null prototype] {}`），非空才
  显 `[Prefix]`（`[Object]`/`[Foo]`/`[Object: null prototype]`/`[Array]`/
  `[Map]`/`[Set]`/`[Uint8Array]`）；Date/Error/RegExp/Promise/函数照常展开。
  ② `typeof value !== 'object'` 把函数送进 `formatPrimitive` 回 `unknown`——
  Node `formatValue` 明确排除函数（`!== 'object' && !== 'function'`）。
- 修法（`src/builtins/node/internal/inspect.rs`）：`depth<0` 按空/非空分流
  （空走 `prefix+{}`/`[]`/`Map(0) {}`，非空走 `[Prefix]`；`constructorName null`
  即 null-proto）；primitives 门加 `&& !== 'function'`。
- 复现：`tests/node/buffer.rs::phase10f_buffer_parity_fixes` 间接（`from` 门）；
  探针 `inspect({__proto__:null},{depth:-1})` 修前 `[Object]`、修后与真机同串。
- 推广为铁律：`depth` 是"剩余层数"不是"开关"——空与非空在截断点语义不同；
  `typeof` 三态（object/function/primitive）写早退条件必须三态全列，漏 function
  即 `unknown`。

### 4.108 `process.nextTick` 裸 throw 变 unhandled rejection：uncaught 路由须先探监听（2026-09-16，10f diagnostics_channel）

- 症状：`bind-store`/`safe-subscriber-errors` 修前报 `unhandled rejection: Error:
  fail/nope` exit=1，而套件等的是 `uncaughtException` 监听触发后 exit=0。
- 根因：移植稿把 Node 的 `triggerUncaughtException(err)` 转译成
  `process.nextTick(() => { throw err; })`——本仓 nextTick 系 microtask，
  回调内 throw 落进 rejection 表，循环尾按 fatal 收割；真机 nextTick 回调抛错
  走 uncaughtException（有监听即交付）。
- 修法：`__dcUncaught(err)`——nextTick 内先探
  `process.listenerCount('uncaughtException')`，有则 `emit`，无则 throw
  走既有 fatal（`src/builtins/node/diagnostics_channel.rs`，两处 throw 点同改）。
- 复现：`tests/node/diagnostics_channel.rs::phase10f_diagnostics_channel_parity_fixes`
  的 `uncaught` 行（修前 `["sub-boom"]` 缺席 + exit=1）。
- 推广为铁律：凡转译 `triggerUncaughtException`/`_fatalException`，一律经
  "探监听 → emit/throw" 两件套，不许裸 throw 赌运行时语义（timers 的 Rust 侧
  `uncaught_fn` 同族，见 `src/builtins/timers.rs`）。

### 4.109 `AsyncLocalStorage.enterWith` 是文档化主入口，`enter` 只是别名（2026-09-16，10f diagnostics_channel）

- 症状：`run-stores-scope` 首错即 `store.enterWith is not a function`——移植稿只给了
  遗留 `enter`。
- 根因：Node 文档化的是 `enterWith`（`enter` 系遗留），套件只调前者；两者语义同
  （进入 store 直至被 run/exit 切换）。
- 修法：`enterWith` 与 `enter` 同体并列（`src/builtins/node/async_hooks.rs`，
  头注同步）。
- 复现：同上 `phase10f_…_fixes` 的 `enter/restored` 行。
- 推广为铁律：移植"公开面"以真机文档方法名为准，不以内部实现名（`enter`）为准；
  遗留别名与文档主入口并存时两个都要给。

### 4.110 `instanceof` 右侧自定义 `hasInstance` 内禁裸调 `getPrototypeOf`（2026-09-16，10f diagnostics_channel）

- 症状：`tracing-channel-args-types` 的 `tracingChannel({})` 期望
  `Cannot convert undefined or null to object`，本仓出小写 `can't convert…`。
- 根因：`Channel[Symbol.hasInstance]` 内 `Object.getPrototypeOf(undefined)`——
  同一操作两引擎文案不同（SM 小写/V8 大写），而 Node 恰把该引擎错原样抛给用户，
  套件用正则钉住（§4.99 的 `instanceof` 右侧形态门是另一面：那是右值为函数时的
  抛错，本条是 hasInstance 内部操作的文案）。
- 修法：空值（`undefined`/`null`）先行直抛 V8 文案
  `Cannot convert undefined or null to object`（`diagnostics_channel.rs`；
  非空值沿旧路，`getPrototypeOf('')` 等装箱语义两边一致，无需桥）。
- 复现：同上 `t1` 行；探针 `dc.tracingChannel({})` 修前小写、修后与真机同串。
- 推广为铁律：文案桥只桥"套件正则钉住且真机可观测"的位点；桥的触发条件能窄则窄
  （本条仅空值），不全局 patch 引擎内建（`Object.getPrototypeOf` 本体不动）。

### 4.111 运行时从不 emit `unhandledRejection`：tracePromise 无 catch 即 fatal（2026-09-16，10f diagnostics_channel，偏离另案）

- 症状：`tracing-channel-promise-unhandled`（注册 `process.on('unhandledRejection')`
  后故意不接 tracePromise 的拒绝）修前 `unhandled rejection: Error: test` exit=1，
  监听永不触发。
- 根因：本仓 rejection 只在循环尾收割报 fatal（`runtime.rs`
  `report_unhandled_rejections`），从无 `emit('unhandledRejection')` 面；
  `src/` 全 grep 无该事件名。补齐需运行时收割点加"探监听 → emit，无则 fatal"
  （`__wjs_uncaught` 同族的新 `__wjs_unhandled`），属跨切片改动。
- 修法（本切片）：不修，记 🟡偏离（`docs/bun-parity.md` diagnostics_channel 节）；
  不在 dc 侧用"急于 emit"伪造语义（调用方后接 `.catch` 时真机还会有
  `rejectionHandled`，急 emit 即错上加错）。
- 推广为铁律：事件面缺口（emit 点在运行时）不在功能切片内用功能侧补丁绕过；
  绕过能过当前用例、必坏相邻语义（handled/unhandled 双生面）。

### 4.112 阻塞 native 停转事件循环：定制查询改投递+轮询（2026-09-16，10f dns）

- 症状：stub-DNS 套件（multi-channel/resolveany 系）全 hang——查询包明明已发出
  （对端 python 可收），stub 回包/自发包却永不到 JS。
- 根因：定制查询是 microtask 内**阻塞** native（`rx.recv()` 等 helper 线程）——
  JS 线程停转期间 dgram 到包无人分发；待 native 超时返回，测试早已错过收包窗口
  （server 永不 close 即 hang）。§4.46/§4.18 的 DNS 版：10d"同步阻塞与 lookup
  同哲学"在 stub 并发下不成立。
- 修法：投递即返 + 5ms refed 轮询收割——`__wjs_dns_job_start`（spawn 线程跑查询，
  结果进 `dns_jobs` 表）/`__wjs_dns_job_poll`（取走即摘）/`__wjs_dns_job_forget`
  （cancel 摘除）；cancel 纯 JS 即刻合成 ECANCELLED（线程迟归自然沉底，不多等
  一轮超时）；运行时零改动（未动 pump/idle：轮询 interval 自带保活）。
- 复现：`wjs-dns-stub` 探针（修前 `stub got` 缺席 + ETIMEOUT；修后即达）。
- 推广为铁律：凡"查询等回包"与"回包靠本循环分发"同现，查询路径永不阻塞——
  投递/轮询/遗忘三件是最小闭环；"线程迟归自然沉底 + JS 侧即刻合成"是 cancel
  的标准形（强杀线程不可取）。

### 4.113 `assert.throws(fn, Error)` 裸类须先 `instanceof`，门错即全灭（2026-09-16，10f dns）

- 症状：`setlocaladdress` 全线 `throws: unexpected throw`——抛的明明是 Error。
- 根因：§4.99 的门写成 `expected.prototype instanceof Error`（仅 Error 子类为真）——
  裸 `Error` 本体（`Error.prototype` 的原型是 `Object.prototype`）被踢进校验函数分支，
  `Error(e)` 回对象 `!== true` 永假。真机（lib/assert.js）门是
  `expected.prototype !== undefined && actual instanceof expected`，再以
  "是否 Error 构造器族"分流（是则直接不过，不当校验器调）。
- 修法：照抄真机三段（`src/builtins/node/assert.rs` `__checkThrow`；Error 族判定走
  `getPrototypeOf` 链找 `Error`，箭头函数无 prototype 照旧落校验器，§4.99 不动）。
- 复现：`assert.throws(() => { throw new TypeError("x"); }, Error)`（修前不过）。
- 推广为铁律：§4.99 的延续——转译断言库的形态判定必须与 lib 原文逐行对拍，
  "看起来等价"的门（`X.prototype instanceof Error` vs `actual instanceof X`）
  在本体/子类边界上必然分叉；改断言库必跑全量（本轮全绿才敢合）。

### 4.114 双 `Received` 口径：ARG_TYPE 用 helper 形，ARG_VALUE 用 inspect 形（2026-09-16，10f dns）

- 症状：`setservers-type-check`（`Received type string ('x')`）与
  `lookupService`（`Received 'fasdfdsaf'`）对字符串期望互斥——统一即顾此失彼。
- 根因：Node 两处文案源不同：`ERR_INVALID_ARG_TYPE` 用 invalidArgTypeHelper
  （字符串 `type string ('x')`），`ERR_INVALID_ARG_VALUE` 用 kInspect
  （字符串裸 `'x'`）。移植时合写成一个 `__dnsReceived` 即撞墙。
- 修法：`__dnsReceived`（helper 形）与 `__dnsInspectValue`（inspect 形）分家，
  前者供 ARG_TYPE，后者供 ARG_VALUE（`src/builtins/node/dns.rs`）。
- 复现：上两个套件行（修前各对一半）。
- 推广为铁律：同词（`Received`）不同源（helper vs inspect）的文案，初见即分家；
  报错文案的"源头函数"与"调用点"同等重要，抄文案先问出自哪个函数。

### 4.115 移植先读全文件：文件头校验段漏读=返工三轮（2026-09-16，10f 过程教训·非代码坑）

- 症状：`channel-timeout` 修完"主体"仍红——漏读文件头 20 行构造器校验段
  （`{timeout: null/true/...}` 与 `-2/4.2/2**31` 两批）；`resolveany.js` 漏读
  `validateResults` 的 SOA-`type` 键（误作裸对象）；`setlocaladdress` 漏读
  `ERR_INVALID_ARG_VALUE` 全文（自编 `invalid IPv4` 错两遍）。
- 根因：`sed -n '25,60p` 式抽查只看了"主体"，校验段恰在文件头注释之后。
- 修法：以后套件先 `cat` 全文（本轮三个文件皆 <120 行有效段），再列需求表；
  本条即 hunger：需求表（test-dns.js 花了整轮）之后零返工。
- 推广为铁律：点名前通读套件全文（含头 30 行的校验表）；"先跑再读"只适用于
  找崩点，不适用于定需求。

### 4.116 跨域微任务先 AutoRealm 进执行 global 再 RunJSMicroTask（2026-09-17，10f vm）

- 症状：vm 沙箱内 `queueMicrotask`/promise 反应在排空点 SEGV（exit 139）——
  test-vm-script-after-evaluate 探针必现；纯主域微任务从不出事。
- 根因：`RunJSMicroTask` 带 DEBUG assert：任务的执行 global 必须等于
  `cx->global()`。本仓 RustJobQueue glue（§4.7）在**主域**排空整个队列，
  vm 的 cross-compartment 微任务未进任务 realm 即 assert abort（SM 内部队的
  runJobs 从不犯此错——它逐任务进对应 realm）。
- 修法（`src/jobqueue.rs` `run_jobs`）：逐任务先
  `GetExecutionGlobalFromJSMicroTask`（null 则同 SM 内部队 `continue` 跳过），
  `AutoRealm::new` 进执行 global，再 `RunJSMicroTask(realm.raw_cx(), …)`；
  realm 对象随迭代丢弃自动还原。
- 复现：vm 沙箱内 `queueMicrotask(() => …)` 后排空（修前 139，修后 exit=1
  可读错；余下 mustNotCall 文案面属微任务模式偏离，bun-parity vm 节记档）；
  `tests/node/vm.rs::phase10f_vm_parity_sync_and_errors` 同路回归。
- 推广为铁律：§4.1 的微任务版——"之后要调 JSAPI 先回 realm"不只适用于
  evaluate 返回后，**逐任务**的引擎回调同样适用；凡引擎 API 的 DEBUG assert
  写明上下文前提，宿主 glue 照 SM 内部队同款实现即免踩（读引擎源码找 assert
  前提比猜崩点快）。

### 4.117 cjs goal 探测禁包络形：包装会吞语句语义（2026-09-17，10f vm，§4.59 姊妹）

- 症状：CJS 分类的 cjs goal 复核若用 `(function(){ … })` 包络（想顺便容忍
  顶层 return），phase9k require(esm) 的 `export default 2` fixture 被误判
  CJS——export 藏进函数体后 oxc 无错也无模块信号，探测"通过"。
- 根因：包装改变语句的顶层性：包络形里 `export` 不再是顶层语句，oxc 既不报
  错也不置 `has_module_syntax`；顶层 `return` 本就无需包装容忍——
  `SourceType::with_commonjs(true)` 即 node 函数包装语义。§4.59 已钉
  `with_module(false)` ≠ 无模块信号，真相在 `module_record`。
- 修法（`src/modules.rs` `cjs_goal_probe`）：**裸文本** + `with_commonjs(true)`
  解析，`无错 && !has_module_syntax` 才判 CJS；歧义集分类改双值
  `script_goal_probe`（可解析/模块信号分读）——有模块信号判 ESM、可解析判
  CJS、解析失败（顶层 return 等 CJS 体专属语法）才落 cjs goal 复核；require
  侧经 `load_cjs_js`（`src/loader/transpile.rs`，`with_commonjs` 同 goal）转译。
- 复现：`tests/node/require.rs::phase10f_cjs_top_level_return_entry_and_dep`
  （入口/依赖顶层 return 双形 + `export default` 不误判；修前 ESM 进 CJS 垫片）。
- 推广为铁律：解析探测永不包装——包络形会吞语句的顶层性（export 信号消失），
  也会反向吸收体（compileFunction 的 `});` 提前闭合，同轮实证）；"想容忍什么"
  就用对应 goal（`with_commonjs`）表达，不手写包装。oxc 的**信号位**
  （`module_record.has_module_syntax`）才是分类真相，"无错"从来不是。

### 4.118 nextTick 双层调度：原生队列收割，microtask 合并队列语义必反（2026-09-17，10f stream）

- 症状：stream 对拍 10 件 DIFF 全族——pipe-error-unhandled 等 5 件把
  `process.on('uncaughtException')` 期望的同步 throw 落成 unhandledRejection；
  compose post-loop throw 1 件管线提前 clean 收工拆 error 监听（composed 流
  error/close 双丢、toArray 永悬）；修 nextTick 后另现 139（SIGSEGV）。
- 根因（两层）：① node 的 tick/微任务是**两层队列**——同步期入队的 tick 先于
  微任务、微任务期入队的 tick 等**整轮微任务排空**后才跑（V8 checkpoint 原子性，
  RunMicrotasks 不可被 nextTick 插队）；旧实现 `queueMicrotask(() => cb())` 把
  两层并成 FIFO——destroy 的 `nextTick(emitErrorNT)` 抛错变 rejection、
  duplexify 的 finish 信号抢在 generator 续体前（② 的直接受害者）。
  ② 修复时的 batch-drain 把多条目裸 JSVal 攥在 Rust Vec 里横跨回调——回调触发
  GC 即悬垂（§4.80 完整形态 N 连击：fire_due 是逐条摘取立即 rooting，没照抄）。
- 修法：① `process.nextTick(cb, args)` 入**原生队列**
  （`RootedState.next_ticks: Vec<NextTickEntry>`，`Box<Heap>` 定址）；
  `pump_once` 内 `drain → RunJobs → drain` 循环（微任务期入队的 tick 由下一轮
  drain 收割）——node 双层语义自然成立；回调抛错照 fire_due 口径路由
  （探 `__wjs_uncaught_count` → 分发/fatal）。② drain 改**逐条摘取立即
  rooting**（`remove(0)` + rooted! 后再调，§4.80 纪律）。
- 复现：`tests/node/stream.rs::phase10f_stream_parity_tick_scheduler_and_fs_readstream`
  （tick-order 行：`start,micro,tick` 序——microtask 期入队的 tick 恒后于既有
  微任务；修前为 `start,tick,micro` 反转序）；batch 形即 139（pe2 探针 3/3）。
- 推广为铁律：移植 node 异步 API 先问"它排在 tick 队列还是微任务队列"——
  `queueMicrotask` 一把梭对 nextTick/微任务**互相入队**的场景必然语义反转；
  原生队列 + pump 前后收割是最小正解。**§4.80 三进宫**：从队列批量取出的
  JSVal 一律当场逐条 rooting，"先攒一批再逐个处理"在 JS 值上永远不成立。

### 4.119 `__fsCall` 闭包内抛的校验错误被 `__fsErr` 重包成 UNKNOWN（2026-09-17，10f fs）

- 症状：`fs.fchmod(1, '123x')` 期望 `ERR_INVALID_ARG_VALUE`，实测 `UNKNOWN`。
- 根因：`__fsModeNum` 在 `__fsCall("fchmod", ..., () => __wjs_fs_fchmod(fd, __fsModeNum(mode)))`
  闭包内求值，抛出的 JS 错误（已带 code）被 `__fsErr` 按"native 消息无 code"路径
  再包一层 → code 落 UNKNOWN。
- 修法：`__fsErr` 先查 `e.code`——JS 侧已带 code（≠UNKNOWN）的错误直通
  （补 path 后原样重抛）；仅 native report_error 产的无 code 消息走 node 形状重包。
- 复现：`test-fs-fchmod.js`（修前 `throws: unexpected throw`）。
- 推广为铁律：错误包装函数必须区分"JS 侧语义错误（已有 code）"与
  "native io 消息（无 code 需整形）"，后者才重包（§4.37 症状一同源第三例）。

### 4.120 `__cb1` 末参即 cb 的校验次序与 node 相反：f 系 fd/mode 先于 cb（2026-09-17，10f fs）

- 症状：`fs.fchmod(1, '123x')` 期望 `ERR_INVALID_ARG_VALUE`（mode 校验先抛），
  实测 `ERR_INVALID_ARG_TYPE`（callback）——`__cb1` 先验末参为函数，把 `'123x'`
  当 cb 报了错。
- 根因：node 回调 API 的校验次序分两族——readFile 族 cb 先验，f 系（fchmod/
  fchown/fstat/ftruncate/fsync/fdatasync/futimes）fd/mode 实参先验、cb 最后。
- 修法：f 系手写 `__fdCb` 包装（syncFn 先跑、cb 缺省再补 callback 类型错）；
  `__cb1` 保持 cb 先验（readFile 族语义）。
- 复现：`test-fs-fchmod.js` M5；模块探针 `fs.fchmod(1,'123x')` 单独跑必现。
- 推广为铁律：移植 node 校验次序必须逐 API 对 `lib/fs.js` 原文——"统一先验 cb"
  在 f 系全错；黑盒断言 message/code 时先真机实测错误种类再写。

### 4.121 `fs_err` 包装吞 raw errno：io_code 落 UNKNOWN（2026-09-17，10f fs）

- 症状：`fs.mkdirSync(file/sub, {recursive:true})` 期望 `ENOTDIR`，实测 `UNKNOWN`
  （消息里还是 fs_err 的双层 Display）。
- 根因：native 用 `fs_err::create_dir_all`——其 `Error` 的 Display 带自身上下文且
  raw errno 不在 `io_code` 期待的位置。
- 修法：native 改 `std::fs::create_dir_all/create_dir` 直用（report_io 拿到真
  io::Error → raw_os_error → ENOTDIR）；配套 `uv_msg` errno→node 消息表 +
  report_io 产 node 形状（`CODE: <uv msg>, <syscall> '<path>'`）+ `__fsErr` 直通。
- 复现：`test-fs-mkdir.js` 父为文件/父链含文件两件。
- 推广为铁律：新 syscall native 禁引 fs_err 系包装（io_code/uv_msg 依赖
  raw_os_error）；错误消息形状一次性对 node（`CODE: msg, syscall 'path'`），
  JS 层只补属性不重排。

### 4.122 对拍并行跑分未设 TEST_THREAD_ID：全进程共享 `.tmp.0` 互踩（2026-09-17，10f fs）

- 症状：mkdtempDisposable 套件单独跑全过，与其它 fs 件并行跑恒挂——
  teardown 报 `EACCES: rm '.tmp.N'`，残留 0444/0644 目录连锁污染后续轮次。
- 根因：node `common/tmpdir.js` 的目录名 = `.tmp.` + `TEST_SERIAL_ID ||
  TEST_THREAD_ID || '0'`——缺省全部进程共用 `.tmp.0`；某件 chmod 共享目录
  0444 后，其它件的 `tmpdir.refresh()` 全炸，错误又被归因到当前件。
- 修法：跑分脚本 `run1` 按线程注入唯一 `TEST_THREAD_ID`；手工并行探针同样
  显式设；清理残留目录 `chmod -R u+rwx`（u+w 不含遍历位，rm 不掉）。
- 复现：`wjs-10f-par.py` 未注入版并行跑 test-fs 全域（互踩随机现形）。
- 推广为铁律：并行跑 node 套件必须逐进程设 TEST_THREAD_ID/TEST_SERIAL_ID；
  "单独跑过、并行挂"先查共享临时目录，再怀疑代码（§4.41 读全局态姊妹篇）。

### 4.123 命名函数表达式遮蔽外层绑定 → custom promisify 无限递归（2026-09-17，10f fs）

- 症状：`promisify(fs.exists)` 拒绝原因就是 `true`（回调首参当 err）+ 
  `InternalError: too much recursion`。
- 根因：`exists[Symbol.for(...)] = function exists(path) { ... exists(path, resolve) }`
  ——内层命名遮蔽外层导出，自递归；拒绝值恰是 truthy 的 `true`。
- 修法：内层匿名（`function (path) { ... 外层 exists ... }`）。
- 复现：`test-fs-promisified.js`。
- 推广为铁律：给既有函数挂 `promisify.custom` 时内层**禁用同名命名函数表达式**
  ——命名 FE 的名字在其作用域内遮蔽外层绑定， lexically 就地自指。

### 4.124 Buffer/TypedArray 自带 `Symbol.iterator`（吐数字）：视图必须排除在"可迭代 data"分支外（2026-09-17，10f fs）

- 症状：`fh.writeFile(Buffer)` 报 `The "chunk" argument ... Received number`。
- 根因：`typeof data[Symbol.iterator] === "function"` 对 Buffer/TypedArray 也成立
  （迭代出 number）——Buffer 被当逐块可迭代收集，块校验当场拒绝。
- 修法：可迭代分支加 `!ArrayBuffer.isView(data)` 前置门；块校验
  （string/Buffer/TypedArray/DataView）照 node 逐字（`ERR_INVALID_ARG_TYPE`，
  write 系参数名 "buffer"、writeFile/appendFile 系 "data"）。
- 复现：`test-fs-promises-file-handle-writeFile.js` doWriteBuffer。
- 推广为铁律：`Symbol.iterator` 存在性 ≠ "集合类型"判据——TypedArray 全家都是
  iterator；分支判据按"视图先收、迭代器次之"排序，视图门永远在前。

### 4.125 net 可观测表面五连坑（2026-09-17，10f net 五轮）

- 坑一（`JSON.stringify` 吞 undefined）：`JSON.stringify([true,5,undefined])` 呈
  `[true,5,null]`——探针误读为"真机转发 null"，险些把实现写错。修法：形态断言一律
  `typeof` 逐项，禁 `JSON.stringify` 看 undefined（`wjs-10f-par.py` 的 W 行同理，
  首行截断只看形状不看空位）。
- 坑二（`EBADF` 含 `BAD`）：黑盒哨兵 `assert!(!out.contains("BAD"))` 被
  `w1 write EBADF` 误触发（§4.42 自摆乌龙）。修法：哨兵一律带分隔符（`BAD `）。
- 坑三（柄关 vs 柄空是两种状态）：`_handle.close()` 后写 → `write EBADF`，
  `_handle = null` 后写 → `ERR_SOCKET_CLOSED`——同为"柄没了"，错误各异
  （真机 26 实测）。修法：`close()` 只标 `__handleClosed` + 延迟 destroy
  （同步 destroy 会提前置空，两态坍缩为一）；write 内先判空柄（CLOSED）再判
  关柄（EBADF），destroyed 面保持 `__writeErr` 不动。
- 坑四（销后噪声必吞）：destroy 后 Rust 侧 teardown 竞速错（RST/EOF，
  `UNKNOWN` 整形）会以 error 事件迟到——双块并发下 1/3 flaky
  （单块 8/8 绿，§4.122"单独过并行挂"姊妹篇）。修法：`__ev error` 见
  `destroyed` 即吞派发（`__hadError` 照记，close(true) 口径不变）；
  真机同为销后不派发（stream `errorOrDestroy` 语义）。
- 坑五（真机 fresh 柄即 null）：`new net.Socket()._handle === null`
  （预连接打补丁即 TypeError）——本仓构造期建桩是偏差。修法：_handle 只在
  连接存活期非空（`__realConnect/__attach*` 建、`destroy/__ev-close` 置空），
  无柄期的 setNoDelay/setKeepAlive 只缓存不转发。
- 复现：`tests/node/net.rs::phase10f_net_socket_surface`（坑三/四修前 flaky 红）；
  探针 `/tmp/wjs-nv-probe/p2.mjs`（双块并发，修前半数多一条 UNKNOWN error）。
- 附带真机口径（同轮实测）：setKeepAlive ms→s 下取整、缺省位转发 undefined、
  四元组全同跳过；TOS 三文案逐字；autoSelectFamily 默认 true/timeout 500；
  server keepAlive 默认 false/0（`src/builtins/node/net.rs`）。

### 4.126 重负载验证禁令：禁并行压力循环与重复全量子集（2026-09-17，机器约束）

- 背景：为抓 pingpong 并行偶发静默死亡（exit -10），用 10-worker 对拍子集循环
  15+ 遍（每遍 ~200 进程启动）+ 6~7 次全量构建，单轮烧掉 200G+ 内存 IO——
  纯验证方法学开销，非代码泄漏（本轮改动全为纯 JS prelude 小字段），但机器扛不住。
- 铁律：验证只用轻量三档——单文件直跑、小域子集
  （`cargo test --test node net::`）、全量 `cargo test` 一次；对拍全量子集
  （157 文件 ×2）每轮至多跑一遍；**禁并行压力循环**（同文件 ×N 并发、
  后台负载围攻、repro 脚本反复跑、为攒统计样本重复子集）。
- 资源上限（2026-09-17 补）：单轮验证常驻内存不超过 **8G**、
  新增测试产物不超过 **16G**（含 `target/` 增量与 `/tmp` 探针文件）。
  超限先清场再跑：`rm -rf /tmp/wjs-*`、清各测试 `.tmp.*` 残留、
  `cargo clean -p winterjs`（慎用，全量重编更烧）；峰值以
  `/usr/bin/time -l` 的 maximum resident 为准记录。
- flaky 并行崩溃改走证据路线：抓到一份 `.ips` 栈即停手记档，不用"统计显著性"
  去换根因。本次即例：`ready` 发射与 -10 强相关（有 1/8、无 22/22）但机制未定，
  到此为止，不再追。
- 附带技术记档（本轮未闭环三件）：
  ① `emit("ready")`（connect → ready 真机序，已接受端不发）暂不发射——与并行
  静默死亡强相关，待引擎侧闭环；`ready-without-cb` 对拍计数不受影响（见③）。
  ② `.ips` 实证：`net dispatch → with_str_args → JS_CallFunctionValue →
  js::Call` 内 SIGBUS（KERN_PROTECTION_FAILURE），疑存活期/GC 时序旧患
  （§4.40 家族），与本轮纯 JS 改动无直接代码关联，另案。
  ③ 本仓不执行 common mustCall 的 exit 钩——只靠退出码时，"mustCall 未触发"
  类失败恒为假绿（ready-without-cb：不发射也 exit=0）；以后对拍报告注明此局限，
  关键语义必须另写内容断言黑盒（本轮 `phase10f_net_remote_surface` 即此路）。

### 4.127 zlib 轮子 framing 与收尾四坑（2026-09-17，10f zlib 首轮）

- 坑一（brotli 显式 `flush()` 多包 2 字节）：`CompressorWriter` 的 `flush()`
  只吐非终结同步块，终结靠 drop 的 FINISH——`write+flush+drop` 比 Node
  one-shot（单 FINISH）恒多头尾 framing（空输入 3B vs 1B、非空头尾各异、
  中段载荷逐字节一致）。修法：删显式 flush（`src/builtins/node/zlib.rs`
  `zlib_brotli_compress`），非空与真机逐字节一致才算对。
- 坑二（ruzstd 默认 `hash` 特性带 checksum）：`default = ["hash","std"]` 使每帧
  补 4 字节 content_checksum（空帧 13B），Node/libzstd 默认无校验（9B）。
  修法：`ruzstd = { default-features = false, features = ["std"] }`
  （`Cargo.toml`；解码侧有无校验自适应，既有往返不受影响）。
- 坑三（`Z_BLOCK` 是 5 不是 2）：flush kind 集按引擎各异——zlib {0,4,5} /
  brotli {0,1,2,3} / zstd {0,1,2}（真机逐项实测；2 是 `Z_SYNC_FLUSH`，
  别当 `Z_BLOCK`）。另补缺失的 `ZSTD_e_continue/flush/end` 常量（0/1/2）。
- 坑四（zlib `close()` 是撕毁不是 end）：真机 `write+close` 无 data/finish/
  end、只有 close+cb（待刷数据直接丢）。`end` 等效是伪语义。修法：
  close = 置旗 + 无错 destroy + cb 落 close（已销毁只等）；`reset()` 已关闭
  即 `ERR_INTERNAL_ASSERTION`（真机实测，非臆测）。
- 复现：`tests/node/zlib.rs::phase10f_zlib_stream_teardown`（7 套件附带修好；
  `test-zlib-flush` 需真增量状态机，整收架构下另案）。
- 推广为铁律：压缩轮子的"完成"语义（flush vs finish/drop）先对空输入逐字节，
  再对非空——空帧是 framing 的最小探针；改 framing 类输出前先 grep 自家黑盒
  有无精确字节断言（本轮全是相对/往返断言，故零回归）。
- 追补（flush-flags/close-after-write 同轮）：① `flush()` 方法 kind 集与构造
  `flush/finishFlush/fullFlush` 选项集是两套口径（方法 zlib {0,4,5}、选项恒
  0..5），抄串了即把合法的 `Z_BLOCK=5` 判死——选项/方法分开核对；
  ② `ERR_INVALID_ARG_TYPE` 的 property 形（`The "options.X" property …`）直接
  复用 errors 移植 helper，禁手写文案（`Received type string ('x')` 含引号，
  手写必错）。

### 4.128 child 同步族六坑（2026-09-17，10f child 首轮）

- 坑一（自举翻译）：`spawnSync(execPath, …)` 的子进程是 winterjs 自身，
  Node 形 argv（`-e` 脚本/裸文件/shell 串首 `"<execPath>" -e`/`$NODE` 形）
  进全 flag CLI 即死（input/timeout/maxbuf 全灭于此，与实现质量无关）。
  修法：`__selfArgv`（数组形）+ `__selfCmd`（shell 串首形）两道映射到
  `--eval`/`--run`；他家二进制（`cat` 等）原样透传。禁给 CLI 加 `-e` 别名
  （全 flag 铁律 §0.8）。
- 坑二（三处缺省各不同）：spawnSync/execSync/execFileSync 缺省 **Buffer**，
  exec **异步**缺省 utf8——实测与文档字面（utf8）相反。修法：只改同步族；
  动 async 即把 `phase10f_exec_live_handle` 打红（本轮亲测）。旧测试两处
  `.trim()` 伪语义同步翻转（§4.65）。
- 坑三（同步等待管道死锁）：等退出（try_wait 轮询）后才读输出 = 1MB+ 输出
  永挂（子撑满 64K 管阻塞、父永不见退出）。修法：take 出 pipe 即起读出线程，
  等与读并发（`run_command`）。
- 坑四（killSignal 两层）：错型先行 ARG_TYPE，查表落空才 UNKNOWN_SIGNAL
  （初版合一即挂 validation-errors，插桩二分半天才定位到 killsig 段——
  "311:53" 系固定伪位置，禁按行号找断言，改四分文件）。
- 坑五（视图输入）：`new Uint8Array(u16view)` 按元素拷（长度减半+内容错位），
  须 `new Uint8Array(buf, byteOffset, byteLength)`（§4.124 姊妹篇；
  DataView/多字节视图全中）。
- 坑六（`args=null` 吞 opts）：`spawnSync(file, null, opts)` 的重载分流把
  null 当 options 覆盖——四处（sync/async/execFile）同修；另 `error.spawnargs`
  为参数数组（非 undefined）、`syscall` 带命令、起 spaw 失败 `pid` 为 0、
  缺省 SIGTERM 阻塞 wait（旧 SIGKILL/直杀两处伪语义已翻转）。
- 复现：`tests/node/child.rs::phase10f_child_sync_surface`；
  另 `process.argv0` 缺省（runtime 取真 argv[0]）+ `ChildProcess.emit`
  （四位分发）附带补齐（spawn-argv0/execfile 套件）。
- 推广为铁律：子进程族"子是谁"先分类——自身/真程序/shell 串三条路，
  翻译层只动自身路；"311:53" 式固定伪位置出现即改文件二分，不读行号。

### 4.129 crypto 首轮六坑（2026-09-17，10f crypto 首轮）

- 坑一（无 new 调用）：`crypto.Hash/Hmac/Cipheriv/DH` 系真机可无 new 调用，
  class 直出即 TypeError。修法：内类改名 `XImpl` + 同名 `function X(...args)`
  包装 + `prototype` 接线（`instanceof`/方法面不变）；Hash/Hmac 附 DEP0179/
  DEP0181 一次性警告（`deprecate()` once 语义，真机 `length/name` 伪装不抄）。
- 坑二（流式 end 丢 head）：Cipher 系 update 即增量吐块，end 只调 final 即少块
  （`wrong final block length`）。修法：end 拼 update-head + final-tail；
  Hash/Hmac 系 update 无输出，照旧只 final。
- 坑三（copy 无参回默认长）：XOF `copy()` 不带 options 即回默认输出长
  （非保留源长），真机实证。修法：copy 内 XOF 恒经新 native setter
  （`__wjs_crypto_hash_set_len`，覆盖/默认两路）。
- 坑四（超长 update）：输入 ≥2^31-1 真机抛无码 `Trying to add data in
  unsupported state`（nodejs/node#45757，精确边界 2147483646 ok/2147483647 挂）。
- 坑五（Received 裸形）：null/undefined 的 Received 无 type 前缀
  （`Received null/undefined`），`__needStr` 全族统一；Hmac 参数名是 "hmac"
  非 "algorithm"；`__outBuf` encoding 先 `String()` 显式转（用户 toString 抛错透传）。
- 坑六（旧断言翻转三件）：copy 保留源长/DH 数值形抛 NOT_SUPPORTED/shake 负长
  ARG_VALUE——三处旧黑盒全系伪语义，真机实证后翻转（§4.65/§4.82 三进宫）。
- 复现：`tests/node/crypto.rs::phase10f_crypto_round1_parity`（40 断言）；
  对拍 133 件 17→24 绿（`docs/bun-parity.md` crypto 节）。
- 推广为铁律：对拍修文案先探真机逐字（含大小写/标点/Received 形态），
  别凭记忆拼；"全绿旧测试"在语义升级面前先对真机再信。

### 4.130 crypto 二轮八坑（2026-09-17，10f crypto 二轮）

- 坑一（品牌≠原型）：`instanceof` 是纯原型检查，品牌另算——定制
  `hasInstance` 把两者绑死即坏 `spoofed instanceof === true` 断言。
  修法：`instanceof` 保持默认，内部 15 处改显式 `__isKeyObject`
  （WeakMap 品牌 + 手动走链，override 期免疫）；实例零自有属性经
  WeakMap + 原型访问器（131 处 `x.__foo` 文本零改动）。
- 坑二（Group 与实例同形不同命）：`getDiffieHellman` 回 Group Flavor
  （constructor 归 Group、setters 置 undefined），`createDiffieHellman`
  回 DH Flavor——同源不同命，探真机才分得清；另手抄素数丢字节
  即环错，改脚本精确替换。
- 坑三（`Signer::sign` 内 unwrap）：钥短于摘要即 panic→139（非 catchable）。
  修法：先验长度转可读错（`rsa_sign`，`PrivateKeyParts::size` 既有先例；
  `TrySigner` 在所钉版本不存在，别硬引）。
- 坑四（验签形态错须回 false）：空/短签名抛错即挂"mustSucceed 永不到"——
  ed/RSA/EC/DSA 四处预检回 false（真机逐项）；顺带 `sign/verify` 补 callback
  异步形（旧同步独占，回调永不到）。
- 坑五（混合 OAEP 几何）：种子长取 oaep 哈希长（非 mgf）——自交全绿掩盖，
  真机预言机（双候选 EM 投真机解密）一锤定音；双向交叉必做（§4.43 再进宫）。
- 坑六（默认 padding 按方向）：加解密 OAEP/签式 v1.5——统一默认 4 即错半边。
- 坑七（key 对象 encoding 双解码）：`{key: hex串, encoding}` 的 data 串同解码
  （真机实证，非文档臆测）；PEM 装甲裹 Buffer 同嗅探（fixtures 无编码读回）。
- 坑八（旧断言翻转四件）：copy 保留源长/asym throw 面/KeyObject 互传规则/
  export 无参——翻转前逐项真机实测（§4.65 四进宫）。
- 复现：`tests/node/crypto.rs::phase10f_crypto_round2_parity`（34 断言）；
  对拍 133 件 24→38 绿（`docs/bun-parity.md` crypto 节）；x448 无轮子卡
  `key-objects.js`（§0.5 待拍板）。
- 推广为铁律：到了"看起来都对但文件还红"时，停手写新探针——把**原文段**
  整体抽出来跑，交互污染（二分头段全绿、全文件红）只认整体复刻。

### 4.131 http 对拍首轮八坑（2026-09-18，10f http 首轮）

- 坑一（Agent 不是 class）：`http.Agent({...})` 无 new 裸调用是合法面（keepalive
  系四套件）——node 的 Agent 是老式函数（`if (!(this instanceof Agent)) return
  new Agent(options)`）。class 直出即 TypeError。修法：函数 + `__init` 方法 +
  `Object.setPrototypeOf` 双挂（Agent.prototype→EE.prototype、Agent→EE）；
  子类（http/https flavor）同形，函数体内调 `BaseAgent.prototype.__init.call`。
- 坑二（键位就是 API）：agent 池键不是 `host:port` 而是 getName 形
  `host:port:localAddress(:family)`——缺省位仍带分隔冒号（`localhost:80:`），
  测试以 `agent.getName({port})` 命中 `agent.sockets`。修法：acquire/release/
  addRequest 全走 getName；ClientRequest 缺省 host 同步改 `localhost`（否则键
  对不上）。https 的 getName 是 23 字段 TLS 会话键（lib/https.js 全字段追加），
  不是 http 形——照抄勿自创。
- 坑三（createConnection 双形态）：agent 级/request 级 createConnection 的实现
  可能同步回 socket、可能走 cb、可能都做——settled 旗 + `maybe` 回值双收口，
  防双 attach。
- 坑四（ServerResponse 构造首参是 req 形对象）：node 的
  `new ServerResponse(req)` 首参是请求信息（method/httpVersion…），不是 socket
  （standalone 套件）；把它当 socket 存即 assignSocket 首挂误拒
  ERR_HTTP_SOCKET_ASSIGNED。修法：构造器只认 `typeof sock.write === "function"`
  为 socket，其余存 req 形；双拒判定走独立 `__sockAssigned` 旗。
- 坑五（头+首块合并写）：node 的 `_send` 在头未发时把 header 与首块**合并为一次
  socket write**（standalone 套件断言单 chunk 以 body 结尾）——我们头/体分两次
  write 即红。修法：`_final` CL 快捷路径合并（`__headBytes()` 拆出，`__sendHead`
  保留独立写路径）；HEAD/204 等无体形态不合并。
- 坑六（408 计时器不因数据重置）：requestTimeout/headersTimeout 是**逐消息
  deadline**（消息起点到消息完结），部分数据到达**不重置**（interrupted/delayed
  系套件依赖）；ka 计时器只在响应完成后臂——体齐响应未完时挂 ka 会误杀在途
  响应（armIdleTimers 拆 withKa 两相）。
- 坑七（408/400 字节逐字对拍）：`HTTP/1.1 408 Request Timeout\r\nConnection:
  close\r\n\r\n` 与 400 同形精确断言；管线残渣（合法请求后跟 `hello world\r\n`）
  在**行终结时**即 400，不等 `\r\n\r\n`（blank-header 套件）——头未齐也要
  增量校验请求行。
- 坑八（真机 paused 语义先行）：无 data 监听也不 resume 的 res，'end' 不发——
  真机同款（node 26 实测同挂），黑盒想当然 `await end` 即挂死；测试侧
  `res.resume()` 后再等 end。rc 陷阱再进宫（§4.45）：`cmd | tail; echo $?`
  是 tail 的 rc——退出码断言必须 `>file; echo $?` 或 `${PIPESTATUS[0]}`。
- 复现：`tests/node/http.rs::phase10f_http_parity_round1`（13 组）；
  对拍 404 件 95→125 绿（`docs/bun-parity.md` http 节）；https 随行 8→13。
- 推广为铁律：对拍修"缺方法"前先跑真机探针把**属性面**（自有属性有无/键形/
  构造形态 no-new）逐项定型——node 老式函数面（Agent/Server 的 no-new）与
  class 面（IncomingMessage 不可 no-new）混存，凭"都是构造器"猜必翻车。

### 4.132 worker 对拍五轮七坑（2026-09-18，10f worker 二轮起）
- 坑一（wire 字段语义跨界即炸）：端口信封 view 分支存 `byteLength`，解码却把它当
  typed array 第三参（**元素数**）——BPE>1（Int32/Float64 系）全 OOB。且异常在
  worker 内 `workerData` 常量求值期炸出，而该常量位于模块 **class 声明区之前**，
  求值中断即整模块 class 绑定 TDZ，require 命中报 `can't access lexical
  declaration "Worker"`——TDZ 是下游症状，不是根因。修法：wire 存 byteLength、
  解码按 `BYTES_PER_ELEMENT` 折算。教训：信封字段是跨端 ABI，编解码两侧的
  语义（字节/元素）必须成对核对；模块顶部的"数据落地常量"是炸点，能懒则懒。
- 坑二（报错归属三重错位）：worker 内异常经 `Error::Script` 上报，文件名是主脚本、
  行号是内嵌模块行号（`313:53`/`665:59` 跨文件同值即此症）。定位时先认出"行号属于
  内嵌模块源"，再用 `awk`/python 按内嵌源行号切片，别在测试文件里找 665 行。
- 坑三（端口同步收信的三次试错）：node `receiveMessageOnPort` 是同步语义，而事件
  派发是 task 级。JS 直推对端队列+微任务 flush → 微任务链式 ping-pong **饿死
  定时器**（infinite-message-loop 10001 轮）；改 `setTimeout` flush → 与 close
  定时器在轮内不保 FIFO；kick 事件经 pump 收割 → 收割在 RunJobs 前后各一轮仍同轮
  链式。正解：**本地 pair 直入对端 Rust pending 表（纯串无 GC 值），pump 逐轮
  统一派发**——事件节奏回归通道模型，同步收信走表。推广：跨 JS/Rust 的消息面
  改投递节奏前，先画 pump 的收割/RunJobs/timer-check 顺序图，微任务链是
  定时器杀手（§4.46 姊妹篇）。
- 坑四（per-session id 空间相撞）：本地 pair 与 cross 口（parentPort）共用
  各自会话的 `worker_next_id` 计数器——cross 口表项的 `peer` 是 worker id，
  可与本地 pair id 相撞，按 peer 反查对端对象会拿错（甚至自指自旋）。本地路由
  必须 `!peer_is_worker` 双向过滤。
- 坑五（recount 只回差值不落账）：`port_recount` 返回计数净变化，落账靠调用方
  `port_bump`——`port_peer_closed` 置 `peer_closed` 后忘了 bump，对端关后端口
  仍续命循环（hang）。且在 `with_rooted` 闭包内直接调 `port_recount`（同样走
  with_rooted）→ RefCell panic rc=101（§4.14 三进宫）。修法：闭包内只改字段，
  差值闭包外 `port_bump(port_recount(id))`。
- 坑六（undici webidl 文案）：node 26 的 MessageEvent 校验走 undici webidl 层，
  值回显是 `inspect(v, {quotes:'double'})` 再外包一对引号（`1 → ("1")`、
  `"str" → (""str"")`），not-iterable 用裸 inspect——照 V8 惯例猜必错，逐值
  实测；instanceof 门经隐藏槽 `__wjs_MessagePort`（模块求值期登记）判定。
- 坑七（原始值错误跨线程）：worker 顶层 `throw 42` 的 error 事件要收到**原始值
  本身**（`err === 42`、注册 Symbol 同一性）——`Error::Script` 只有文本。修法：
  捕获点对非对象异常打包 `__wjs_prim:{json}` 信封（kind=None 时 message 即信封，
  `worker_error_text` 直通勿加前缀），JS 侧按类还原；Symbol 描述经 prelude
  helper（`JS::ToString` 对 symbol 抛 TypeError，Rust 侧原生路不通）。
  复现：`tests/node/worker.rs::phase10f_worker_*`（修前 Int32Array 跨端静默丢、
  TDZ 级联、receive-message 收尾 hang）。

### 4.133 crypto 四轮 + http2 流式化六坑（2026-09-18，10f crypto/http2 收尾）

- 坑一（`Uint8Array.equals` 不存在）：`__b64urlDec` 回 `Uint8Array`（无
  `.equals`），OKP/EC 的 JWK `x` 比对直调即 `xBytes.equals is not a function`。
  修法：一律 `Buffer.from(x).equals(...)`（`crypto.rs` OKP/EC 两处）。
- 坑二（`ChanBody.done` 初值吞体）：客户端上传 `Data+End` 已入队但 `done` 仍
  `true`，首轮 `poll_frame` 即 `None`，POST 体恒空（服务端 `srv-end ""`）。
  修法：有体即 `done=false`（空体无 trailer 才 `true`）。
- 坑三（`try_recv` 丢 waker 饿死流式）：服务端应答 `ChanBody::poll_frame` 用
  `try_recv` 空转 `Pending` 且忽略 `cx`，后到的体块永不唤醒（`res.write` 后
  客户端零 `data`）。修法：改 `rx.poll_recv(cx)` 注册 waker（首版注释记坑）。
- 坑四（Duplex 基类只读 `closed/destroyed`）：`ClientHttp2Stream extends Duplex`
  后 `this.closed=false` 即 `setting getter-only property "closed"`（state 位图
  只读；旧 EventEmitter 壳无此约束）。修法：自有 aboard 旗用 `__` 前缀，
  `close()` 以 `destroyed` 只读判幂等、`destroy()` 置位（服务端
  `Http2ServerStream` 同口径早已 `__closed`）。
- 坑五（`Object.create(Socket.prototype)` 撞只读 `connecting`）：socket 代理
  `base.connecting=false` 同样 getter-only 抛错。修法：
  `Object.defineProperty(base,"connecting",{writable:true})` 自有遮蔽。
- 坑六（`writeHead` 不发头）：本仓 `writeHead` 仅缓冲、`__sendHead` 在
  `write/flush/end` 才发——双 `writeHead` 不抛（真机发头即抛
  `ERR_HTTP2_HEADERS_SENT`）。测试用 `flushHeaders()` 先发头再断言抛错；
  `write+end` 体会拼接收尾（断言须按全形 `"xgate:..."` 或改 `flush` 形）。
- 复现：`tests/node/http2.rs::phase10f_http2_streaming`（修前 POST 空体/
  流式零 data/`gate false`）；`tests/node/crypto.rs::phase10f_crypto_round4_parity`
  （修前 `xBytes.equals`/`Invalid JWK EC key`）。

### 4.134 crypto 五轮 raw 门六坑（2026-09-18，10f crypto 五轮）

- 坑一（恒定位 `313:53` 系 assert 内部抛点）：套件失败行号恒为
  `assert.js:313:53`（`__checkThrow` 的 `unexpected throw` / `throws` 的
  `Missing expected exception` 抛点），不是套件位置——读行号找断言必错。
  修法：截断二分（按顶层 `}` 块逐段 `head -n` 落临时文件跑，§4.115 四分法
  的机械版）；`hasOpenSSL(3,5)` 在本仓为 true，ml/slh 块全执行（别当跳过）。
- 坑二（预解码洗白字符串门）：`createPublicKey` 的 encoding 预解码把 raw
  字符串 key 先洗成 Buffer，`__parseKeyMaterial` 内的字符串门永不触发。
  修法：预解码限 pem/der 系，raw 系（`raw-private/-public/-seed`）跳过。
- 坑三（门序即语义，三层各异）：`want` 门（`key.format` 无效）→
  字符串门（`key.key` 实例断言）→ akt 链（缺/坏 akt）→ seed/尺寸语义门，
  逐层实测定序，不猜（如 `createPrivateKey({format:'raw-public'})` 报
  format 门而非 akt 错；字符串门却排 akt 链之前）。
- 坑四（ml 展开形 PKCS#8）：`ml_dsa_44_private.pem` 非种子形而是
  OCTET{ SEQ{ OCTET(32 seed), OCTET(2560) } }——种子解析器只认 `[0]` 形即
  `Invalid PKCS#8 key`。修法：`mlkem_pkcs8_seed` 加 SEQ 首子 OCTET 分支
  （Rust 单测钉种子形/展开形/错长三件）。
- 坑五（压缩点别手写 BigInt）：P-384/P-521 的 `b` 凭记忆必错——树内
  p256/p384/p521/k256 全直引（零新增），新 native
  `__wjs_ec_import_compressed` 经 `PublicKey::from_sec1_bytes`（固有方法，
  含解压+上曲线校验）一行落地；JS 侧只做形态分流（02/03→native、
  06/07→04 同道、坏前缀落 `bad()`）。
- 坑六（zsh `===` 展开，§4.3 二进宫）：`echo ===` 裸写即 hemis——本轮
  `grep ... | head` 后误跟裸 `===` 又炸一次；分隔符一律加引号。
- 复现：`tests/node/crypto.rs::phase10f_crypto_raw_seed_parity`（`r5-*` 行；
  另 `test-crypto-key-objects-raw.js` 修前 DIFF 修后 SAME0）。

### 4.135 crypto 六轮 PSS/PBES2 八坑（2026-09-18，10f crypto 六轮）

- 坑一（a1 剥层连翻两次）：MaskGen `[1]` 内容是**完整** `SEQ{OID-mgf1, SEQ{hash}}`——
  首版 `children(it.body)` 得 `[SEQ]` 判长 2 即 null；改"剥外层 SEQ"又写成判长 1，
  还是 null（hexdump 明明对得上）。两翻同源：不动手算，拿 `__derRead` 逐层打印
  （本次 `a1 body: 3018…` 钉死 `[OID, SEQ]` 两元才落定）。教训：DER 嵌套层数
  只认逐层打印，不认"看起来"。
- 坑二（SHA-1 OID 键多拼长度字节）：map 键 `052b0e03021a` 混入 `05` 长度——
  map 键一律 OID **body** hex（`2b0e03021a`），tag/len 剥干净再当键。
- 坑三（门序即真机序）：PSS 约束 salt 先 digest 后（sha1+小 salt 落 salt 错，
  套件 1007 行钉住）；验签缺省 salt 同取键约束值（sign/verify 双缺省互通，
  真机实测钉住：混合键缺省签出 salt 20 非摘要长 64）。
- 坑四（自验绿≠互通）：混合 MGF 自签自验全绿，但真机拒收——缺省 salt 取了
  摘要长。收敛标准：手组密码学一律双向真机交叉（本仓⇄真机互验），自交绿不算数
  （§4.54 对称性盲区再进宫）。
- 坑五（`?? key` 洗白 null）：`key.key ?? key` 把 null/undefined 洗成 options
  对象，JWK 对象门永不触发（"Unsupported JWK kty" 现形）。修法：直透 `key.key`。
- 坑六（`__cryptErr` 全大写门）：`DataError: ...` 小写码被 `^([A-Z]...)` 门漏掉
  落裸文——装载宽容的 catch 须按消息窄匹配，不能按 `e.code`。
- 坑七（getCurves 顺序即契约）：JWK-unsupported-curve 块靠 `find(!supported)`
  首个非支持曲线——本仓仅 NIST 四曲线时 `assert(namedCurve)` 空值，
  属能力偏离（非语义缺口），不造假曲线凑数。
- 坑八（Node 自己都不逐字节）：PSS 导出 round-trip，真机对 sha256/sha512 对
  非逐字节（重编码 params），本仓 stash 原文反而逐字节——"与真机逐字节一致"
  在重编码面不成立，以"解析等价 + 细节稳定"为准。
- 复现：`tests/node/crypto.rs::phase10f_crypto_pss_gates`（`r6-*` 行；
  另 `test-crypto-key-objects.js` 余唯一红块见 bun-parity）。

### 4.136 Zip移植双坑：HideStackFrames漏挂 + 解压背stop缺失（2026-09-18，10f zlib Zip）

- 症状一：`contentSync` 篡改包报 `ERR_ZIP_ENTRY_CORRUPT is not a constructor`。
  根因：新增 `E('ERR_ZIP_*')` 未挂 `HideStackFramesError`，而移植稿按 Node 原文
  解构 `codes.X.HideStackFramesError`——undefined 当构造器即炸。修法：7 ZIP 码 +
  `ERR_INVALID_STATE` 全挂 HideStackFramesError（真机 E 定义逐字对拍）。
- 症状二：伪造小头（declared 50/实际 5000）报 `produced 5000 bytes, expected 50`
  而非 `inflates beyond its declared size of 50`。
  根因：`inflateRaw`/`zstdDecompress`（Sync + 回调）丢 `maxOutputLength`——Zip 的
  declared+1 背stop（compression.js `outputCap`）永不触发，全量解出后才在
  `checkDecoded` 落错；既有仅 brotli 有该面。修法：两路补 maxOut（校验 +
  超限抛 `ERR_BUFFER_TOO_LARGE`，Sync/回调双侧），使失败发生在解压内、
  经 `rethrowDecodeFailure` 转成 corrupt-inflates 口径。
- 复现：`test-zlib-zip-hardening.js` 伪造小头件（修前 rejects: unexpected throw；
  修后 30/30）+ `phase10f_zlib_zip_archive`（corrupt 行）。
- 推广为铁律：新增错误码即 grep 移植稿解构形态（`HideStackFramesError` 有无）；
  新增"限输出"调用方前先查被调解压面是否真 honor 该选项——"传了" ≠ "用了"。

### 4.137 入口失败 + 开着句柄 = 永不收割（eval 路径版）：then 吞 rejection + unhandled 只在循环尾（2026-09-19，10f child 牵引）

- 症状：`--eval 'spawn("sleep",["30"]); throw new Error("x")'`（或 `Promise.reject`）进程
  hang（模块路径 `--run` 同样 hang 在未处理 rejection 形）。§4.70 修了模块入口的
  `entry_failed`，但 eval 路径无捕获、且 unhandled 表只在循环尾收割——句柄开着
  循环永不 idle，两个收割点都永不到。
- 根因（三连）：① eval 的 async IIFE 包装以 `.then(v=>…, e=>{__wjs_error=e})` 收尾
  ——rejection 被 then 吞掉，链式 promise 正常 fulfill，失败信息只落全局变量；
  ② `__wjs_error` 在 `extract_eval_result`（事件循环**之后**）才读；③ 未处理
  rejection 表只在 `event_loop` 尾 `report_unhandled_rejections` 收割（REPL 才有
  逐轮收割）。
- 修法（三件配齐）：① wrapper 的 rejection 处理器重抛（`return Promise.reject(e)`，
  链式 promise 保持 rejected）；② eval 包装求值成功后对 rval（promise）挂
  `entry_native_values()` reactions（镜像 run_module；`entry_rejected_native` 消费
  重抛，无二次 unhandled 噪声）；③ `event_loop` pump 后加检查点：
  `unhandled_pending() > 0` 即 break（tracker Handled 已在表内摘除，checkpoint
  时非空 = 整轮排空后确无处理 = node 的 checkpoint 末 fatal 语义）。检查点放
  event_loop 不放 pump_once——pump 与 REPL 共用，REPL 逐轮收割不退出（node
  REPL 同款），pump 内早退会跳过当轮结算。
- 复现：`tests/node/child.rs::phase10f_entry_failure_open_handle_exit`
  （eval throw/eval reject/module reject 三形；修前 alarm 打不到头，修后 exit 1
  带原文）。
- 推广为铁律：§4.70 完整形态二——凡"失败信息落在循环尾才读的地方"（全局变量/
  表），必须同时有一条**提前跳出的检查点**；eval 包装的 then 处理器不是失败
  通道（吞 rejection），失败必须以 promise 形态回到宿主侧。

### 4.138 net listen 面三坑：字符串端口当 UDS + 无重听守卫 + 柄不随出口清（2026-09-19，10f net）

- 坑一（`listen("0")` 建出套接字文件）：`__doListen` 把一切字符串当 IPC 路径
  ——`listen("0")` 绑出名为 "0" 的 UDS；真机 normalizeArgs 判据是
  `Number.isFinite(+s)`（可解析为有限数的非空字符串 = TCP 端口，"abc" 才是
  path）。listen-options 套件两次 `listen("0")` 第二次 EADDRINUSE 现形。
- 坑二（重听无守卫）：listening 期间再 `listen()` 须同步抛
  `ERR_SERVER_ALREADY_LISTEN`（真机文案 `"Listen method has been called more
  than once without closing."`，逐项实测）。守卫谓词用 `this._handle`（node
  同款），不用 `__listening`——后者只在 listening 事件后才有值，盖不住
  绑定窗口。
- 坑三（柄不随出口清）：`close()` 只 destroy 不清 `_handle`/`__id`；listen 失败
  的 error 派发也不清——"close 后可再听"（套件第三段）与"EADDRINUSE error 后
  可立即重听"（第一段）全卡死。三出口（close/error/成功转移）柄同步即清。
  注意：Server/Socket 各有 `__ev`（同名方法两个类），server 侧清柄别插进
  Socket 的 error case（`__port`/`__udsPath` 仅 server 在 `__doListen` 置位，
  可作判别但首选插对类）。
- 复现：`tests/node/net.rs::phase10f_net_listen_surface` +
  `test-net-server-call-listen-multiple-times.js`/`test-net-server-listen-options.js`
  （修前 DIFF，修后 SAME0）。
- 推广为铁律：凡"listen/打开"族 API，字符串首参先过数字归一化再分流
  （真机 normalizeArgs 为准）；"重听/重开"语义 = 守卫谓词 + 全部失败出口的
  柄清理，两件缺一即卡死或漏抛。

### 4.139 fork 非 silent 的 stdio null 三面：流式初始化别覆盖真机 null 缺省（2026-09-19，10f child）

- 症状：`phase9m_child_fork_ipc` 的 `c.stdout === null` 反绿为红——fork 非 silent
  的 stdout/stderr 变 undefined。
- 根因：10b/10f 给 ChildProcess 补流式面时，`__initStreams` 按 stdio 数组分派
  （pipe→流，否则 null），但 fork 走独立构造路径，旧代码里 `proc.stdout = null`
  显式缺省被删除——非 silent（stdio 继承）分支没人置 null，字段落 undefined。
  真机 26 逐项：非 silent 三面（stdout/stderr/stdin）全 `=== null`（继承无管道
  句柄），silent 才挂管形流。
- 修法：fork 内 `if (!o.silent) { proc.stdout = proc.stderr = null }` 恢复
  （stdin 本就有 `proc.stdin = null`）。
- 教训：§4.86 姊妹篇——"流的面"与"无流时的缺省"是两条路径，改流式初始化时
  grep 该构造器全部创建点（spawn/promisified/fork/死句柄）逐一对缺省值；
  `=== null` 类断言对 undefined 也红（`undefined === null` 为 false），
  反绿为红先查缺省丢失而非语义翻转。
- 复现：`tests/node/child.rs::phase10f_fork_nonsilent_stdio_null`（修前
  `nonsilent false false true`）。

### 4.140 G2 `__closing` relisten 残留：手工过/ cargo 挂 ≠ 环境问题（2026-09-19，欠账轮）

- 症状：全量 `cargo test` 在 `net::phase10f_net_listen_surface` 挂死 **46 分钟**
  （子进程 tokio `park_internal` 睡死，lsof 见 TCP LISTEN 已建立、listening
  回调永不触发）；**同一份 p.mjs 手工 shell 跑 6 秒全过**——结论打架，一度
  误判为"cargo 环境问题"，clean 全量重建后照样复现，排除构建脏状态。
- 根因：G2 的 close-during-listen 窗口旗 `__closing` 在 `close()` 置位后
  **从不重置**——close 后 relisten，新一轮 listening 派发被残留旗吞掉
  （`__ev("listening")` 开头 `if (this.__closing) break;`）。手工 shell 与
  cargo harness 的 bind 回调派发时序不同，恰好掩盖/暴露它——**不是环境问题，
  是状态机残留旗 bug**。
- 修法：`__doListen` 开头 `this.__closing = false`（net.rs）。修后 0.54s 绿。
- 推广为铁律：① 凡"窗口旗"（临时置位拦截某类派发/回调）必须与柄一样随
  close/error/listen 全部出口复位，新增窗口旗时逐出口清单化核对；②
  **"手工过、cargo 挂"≠环境问题**——先查状态机残留/时序型状态污染，两种
  启动方式只是时序不同；先 bisect 到引入提交再下结论（本轮 bisect 实锤
  G2 引入，非 G3/G9-2）。

### 4.141 with_str_args 参数跨分配悬垂：rooted 必须在第一个分配之前（2026-09-19，欠账轮）

- 症状：`net_remote_surface`/`net_unix_socket_roundtrip` 在 cargo harness 下
  SIGBUS（exit=None 信号死亡、stderr 空），手工 3/3 稳定过；崩溃报告
  （`~/Library/Logs/DiagnosticReports/winterjs-*.ips`，**SEGV/SIGBUS 定位
  第一手段**，比 sample/lldb 快且必落盘）栈实锤：
  `net.rs dispatch → with_str_args → call_two → JS_CallFunctionValue →
  js::Call memset_pattern16`。
- 根因：`with_str_args(cx, global, fun, kind, payload)` 的 `global`（裸指针）
  与 `fun`（裸 JSVal）参数**跨 `to_jsval` 分配**——分配可触发 GC 搬移，
  悬垂后进 `JS_CallFunctionValue` 即 SIGBUS。第一次补丁只 root 了 dispatch
  调用点、漏了函数体内跨分配的参数，照样崩——**rooted 必须在任何分配之前
  覆盖全部跨 GC 存活值**（§4.80 第 N 例；dispatch 三处 `net_target()` 返回值
  与 `get_prop_value` 读出的 `__ev` 同批全部入槽）。
- 修法：with_str_args 内 `g`/`f`/`a`/`b` 全部先入 rooted 槽再 to_jsval；
  dispatch 三处 net_target() 返回值立即 `target_r` 入槽。
- 推广为铁律：**新增 Rust→JS 调用 helper 时，函数体第一行先把全部 JS 值
  参数入 rooted 槽，之后才允许出现任何分配型调用**；reviewer 按
  "参数表 → 第一行 rooted"逐项对。

### 4.142 bisect 禁止连续建 worktree：每个 worktree 的 target 都是全量重编（2026-09-19，欠账轮·灾难记录）

- 症状：为 bisect G3 六提交，**连续建 4 个 worktree 且各自 `cargo build`**，
  每个 worktree 独立 target/ 全量重编依赖树（每个 10-20GB），磁盘 36G→0
  急速耗尽，连 ZCode 工具自身的日志都写不下（ENOSPC）——**连"删文件"的
  命令都失效**，只能用户外部手动 rm 恢复；G9-2 检查点构建中途断供作废重来。
- 根因：git worktree 共享 .git 不共享 target/；`cargo build` 在新 worktree
  = mozjs/全部依赖重编。连续建 N 个 = N 份全量。
- 修法/铁律（**拒绝连续建 worktree 压榨空间**）：
  ① **bisect 一律用主仓 `git checkout` + `git stash`**（单一 target，增量
  切换 16 秒），不用 worktree；worktree 只留给"必须并行持有多份完整构建"的
  场景（如双 agent 并行开发），且用完即删（`git worktree remove --force` +
  `rm -rf` 残留 + `git worktree prune`）；
  ② **每建一个 worktree 前先 `df -h` 看余量**（一个 debug worktree 按 20GB
  计）；余量 < 40GB 不建；
  ③ 磁盘 ENOSPC 的第一症状是"工具静默失效/日志写不下"，看到即停手清盘，
  不要继续任何编译类操作。

### 4.143 全量 cargo test 禁套 alarm/timeout（2026-09-19，欠账轮）

- 症状：`perl -e 'alarm 570; exec @ARGV' cargo test` 跑全量——全量（编译全部
  target + 数千测试）20-40 分钟，alarm 到点中途击杀，后台任务永远等不到结果，
  反复轮询超时。
- 根因：alarm 口径（20 秒级）只适用于**单个真机套件冒烟**；全量跑无界。
- 修法：全量跑 `cargo test` 后台直跑（`> file 2>&1`），轮询 `grep -c
  "test result: ok"` 看进度；**单测/冒烟才用 alarm**。同族：shell 里
  `ps aux | grep winterjs` 的 etime 是判断黑盒子进程挂死的硬指标
  （>60s 的单 phase 即挂，正常 0.5-2s）。

### 4.144 手工复现脚本放 /tmp 会被清（2026-09-19，欠账轮）

- 症状：二分/真机对照用的手工脚本放 `/tmp/wjs-*.mjs`，用户清 tmp 后消失，
  "手工过"的结论无法立即复核，差点把已修好的当未修。
- 修法：手工探针脚本每次从测试文件现提取（python re 从 `tests/node/*.rs`
  的 `r#"..."#` 抽 JS 源），或放 `/Users/bemly/probe`（家目录，不随 tmp 清理
  消失）；结论引用脚本时注明来源与生成方式。

### 4.145 跑分包装 exec 失败静默假绿：`exec or die` + glob 路径（2026-09-19，G9-3 轮）

- 症状：6 个 zlib 目标套件"双侧全绿"，实为 winterjs **从未执行**——
  `perl -e 'alarm 20; exec @ARGV' <BIN> --run f.js` 的 exec 对不存在路径
  失败后 perl 继续走完脚本，正常 exit 0（永假绿）；本仓路径含 U+F8FF
  （`/Volumes/ Projects`），工具调用里手敲极易被转义打断成
  `/Volumes//Projects`（no such file），两坑叠加差点把欠账判成已清。
- 修法：跑分包装一律 `exec @ARGV or die "exec failed: $!"`；二进制路径
  一律 `WJS=$(echo /Volumes/*/Projects/winterjs/target/debug/winterjs)`
  glob 解析，禁手敲含特殊字符路径；"全绿得可疑"时先验证二进制真跑过
  （输出非空/无 "no such file" 尾巴）。
- 复现：`perl -e 'exec @ARGV' /nonexistent; echo $?` → 0（die 之前）。
- 推广为铁律：harness 的每条 exec 都必须 or die；结论与常识打架时
  （node 套件不该全绿）先查执行痕迹再信退出码（§4.93 姊妹篇）。

### 4.146 一次性 native 丢 opts 二进宫 + raw 字典必须构造期设（2026-09-19，G9-3 轮）

- 症状三连：① `zstdCompressSync`/`brotliCompressSync` 走专用一次性 native，
  opts（dictionary/pledgedSrcSize）**整体丢弃**——pledged 不抛
  ZSTD_error_srcSize_wrong、brotli dict 静默失效（§4.136"传了≠用了"二进宫）；
  ② brotli 字典 'string' 漏过校验（`__zBytes` 收 string——数据输入合法但
  字典非法），漏到 brotli crate 报 "Decompression failed"；③ raw 族字典流
  inflate 报 "repeated call with bad state"——被动 NEED_DICT 恢复对 raw
  留 Mode::Bad（raw 无 FDICT 头，zlib 语义字典必须在首次 inflate 前设）。
- 修法：① 一次性压缩改走 `__zEngineOnce`/引擎（dict/pledged/错误口径
  单点接线；切前先字节对比——同 quality 下与裸 native 逐字节一致，零回归）；
  ② `__zDictBytes` 严格校验（Buffer/TypedArray/DataView/ArrayBuffer），
  一次性面 + `__zStreamBase` 两收口点共用；③ `RawInflate` 建引擎即
  `set_dictionary`（zlib 族被动 NEED_DICT 恢复保留，dictionary-fail 套件
  依赖其 "Missing/Bad dictionary" 文案）；④ 四个死 native + 注册项 + 孤儿
  `__zCall` 一并删除（`grep` 验空）。
- 教训：选项"传到 JS 函数"≠"传进引擎"；一次性面与流面共用收口点后，
  新选项只接一处，杜绝两套皮漂移。

### 4.147 Web CS/DS 错误落 readable：pipeTo 的 cancel 语义（2026-09-19，G9-3 轮）

- 症状（设计坑，type-error 套件牵引）：DecompressionStream 尾垃圾错误若从
  `write()` 抛，WritableStream 的错误经 pipeTo 走 **cancel 链**——readable
  正常结束，`Array.fromAsync(readable)` 读不到 reject，套件必红。
- 修法：CS/DS 的解码错误（junk/截断）一律 `ctrl.error()` 落 readable 侧；
  done 后再写的 junk 也落 readable（套件 `[valid, empty]` 形），不从 write 抛。
- 真机 26.8.2 逐项对拍：不继承 TransformStream（`instanceof` false，proto
  链独立）、format 枚举 TypeError（"1st argument 'x' is not a valid enum
  value of type CompressionFormat."）、`[object DecompressionStream]` tag、
  junk = TypeError `ERR_TRAILING_JUNK_AFTER_STREAM_END`（node:zlib 引擎
  junk 码同文复用）、无静态 supportedFormats。
- 复现：`test-zlib-type-error.js`（修前 DS 缺失 3 not ok；修后全绿）。
- 推广为铁律：Web 流管道的错误出口看消费侧——readable 的迭代器要 reject，
  错误就必须 `controller.error()`，走 write 拒绝等于把错误送进 cancel 黑洞。

### 4.148 net 尾件五坑：分包解码/end 回调挂点/abort 发射时序/EPIPE 三条件/HE 回落钩（2026-09-19，G6 轮）

- **坑一（large-string）**：Socket `setEncoding` 用 `new TextDecoder(enc)` 逐
  chunk 解码——TCP 分包把多字节序列切断，每碎片各吐一个 U+FFFD（40962 vs
  40960 之谜）。修法：持久 `StringDecoder`（`__dec` 与 `__enc` 同生命周期），
  `__ev end` 时 `end()` 补齐残余。教训：凡"逐 chunk + 字符编码"，解码器必须
  跨 chunk 保态（stream 系同查）。
- **坑二（async-iter）**：`end(cb)` 的回调挂 'close'——node 流语义挂
  **'finish'**（FIN 刷完即发）；半开对端不回 FIN 时 close 永不来，
  `end(resolve)` 卡死。改挂 finish 前先 grep 依赖 close 时点的旧套件。
- **坑三（abort-controller）**：`ac.abort()` 后套件才挂 `once('close')`——
  我们 destroy(err) **同步** emit error/close，抢在 once 挂载前 → 未处理
  error + once 永挂。node 的 destroy 发射是 nextTick。修法：abort 触发的
  destroy 一律 `queueMicrotask`（connect 侧/构造器侧两处）。另：Socket
  构造器此前根本无 signal 分支（查到的是 Server 的——先确认函数归属再改）；
  直调 `addEventListener` 须入 `__etAdd` 侧表，`events.listenerCount` 才可见。
- **坑四（write-after-end-nt/writable）**：对端 FIN 后写 → EPIPE
  'This socket has been ended by the other party'。条件三缺一不可：
  `__peerFin && __ended && !allowHalfOpen`——仅对端 FIN（writable 套件
  'end' 后写合法且无错）、仅本地 end（STREAM_WRITE_AFTER_END 旧形）、
  半开（async-iter 套件 FIN 后写要成功）都不走此路；cb 与 error 事件
  都下一 tick（同步返回 false 时 hasError 仍 false）。回归三板斧：
  writable/write-after-close/blocklist 三件旧绿套件先受累后修复——
  **改 write 路径必跑这三件**。
- **坑五（autoselectfamily-default/blocklist）**：HE 串行回落 = lookup
  `all: <autoSelectFamily 生效值>`（mocked lookup 只在 all:true 给数组）+
  `__heOnErr` 钩吞中间失败（error case 不落用户监听）+ close 后 `__heReset`
  重试（**保留 `__pendW`**——回落期间的用户写带到最终连接）+ 尝试统一走
  `__doConnect` 闭包（blockList 校验每地址生效，直接 `__realConnect` 会
  绕过拦截且无 close 事件→链停摆）。记档：attemptTimeout 竞速未实现。
- 回归：全量 `test-net-*` 159 件对拍——116 绿（+13）/ SAME1 14 / 仅我们红
  29（与 stash 旧二进制红集**逐一相同**，零回归）；black-box 223 全绿。
- 推广为铁律：①改 net write/end 路径，writable/write-after-close/
  write-after-end-nt/async-iter/blocklist 五件是固定回归组；②"事件 X 后
  才挂监听"的套件形状 = 发射必须异步（node destroy 语义）。

### 4.149 G4 fs validators 轮七坑（2026-09-19，欠账 G4 轮）

- **坑一（utimes 数字实参 = 秒，不是 ms）**：`utimesSync(path, 2**31, 2**31)`
  真机把数字当**秒**（y2K38 套件 2^31 s 断言）；本仓旧实现按 ms + 黑盒测试
  也编码了 ms——测试随实现偏差翻转（§4.65 姊妹篇），真机口径一锤定音。
  Date → ms 直传（精度全保）；_toUnixTimestamp 负数回当前秒（真机实测怪形，
  逐字照抄）。lutimes/futimes 同批对齐。
- **坑二（__cb1 回调先于值校验）**：`fchown(1, '')` 无回调——__cb1 先取末参
  当 cb → /callback/ 错误；node 值校验在前 → /uid/。__fdCb 重写：cb 非函数时
  先跑 syncFn（值校验错误原样抛、操作错误让位）再校验 cb；**且不得造孤儿
  rejected promise**（lchown ×7 unhandled rejection 根因：p 已 reject、cb 校验
  又同步抛，无人接）。uid/gid 域 `[-1, 4294967295]`（-1 = 不变更哨兵）。
- **坑三（writeFile opts 在 rest[2]）**：writeFile=(path,data,opts,cb)、
  readFile=(path,opts,cb)——参数表序号照搬 readFile 的 rest[1] 拿到的是
  data 字符串，signal 面静默失效（c1/c2 abort 后仍 success）。**复用包装
  helper 时先画参数表**。
- **坑四（fs_err::read 单阶段丢 syscall 语义）**：目录 readFile——node 是
  open 成功、read 失败（syscall 'read'）；fs_err::read 一把梭报不出阶段。
  open/read 两阶段手写，各报各的 syscall；fs_err 换 std 直用保 raw errno
  （§4.121 三进宫：mkdir/rmdir/read_file 全切）。
- **坑五（TextDecoder latin1 = windows-1252）**：node 'latin1' = 字节直映
  码点；TextDecoder 的 latin1 标签是 win-1252（0x80-0x9F 段不同）——✓/😀
  文本 roundtrip 必挂。latin1/binary 走 Buffer。
- **坑六（TDZ：const 箭头 helper 与导出顺序）**：`export const lchown =
  __fdCb(...)` 写在 `const __fdCb` 定义**之前** → 模块求值 ReferenceError →
  **整模块绑定全未初始化**（表象是"can't access lexical declaration"）。
  const 箭头 helper 必须先于全部使用点；function 声明无此问题。
- **坑七（模块级报错行号不可信）**：套件报 `xxx.js:346:53` 而文件仅 58 行
  ——错误位置映射失真时，靠 `console.log` 插桩（CK/TI/AV 标记）定位到
  用例级，不猜。
- 回归：全量 `test-fs-*` 355 件 166 绿（净 +20：constants/stat-bigint/stat/
  statfs/readfile/rename-type-check/null-bytes/options-immutable/mkdir-mode-
  mask/rmdir-throws/truncate/timestamp-parsing/lchmod/lchown×2/fchown/utimes/
  y2K38/append-file-sync/write-file-sync/write-file/roundtrip）；残件：
  roundtrip 末段 async_hooks FSREQCALLBACK 资源面（另案）、write-stream/cp/
  watch 簇（G8/大簇）；black-box fs 13/13、冒烟 5/5。
- 推广为铁律：①改时间戳 API 先对真机量纲（秒/ms/µs）+ 全 grep 旧测试的
  量纲假设；②包装 helper（__cb1/__fdCb 族）新增变体时列出 node 的完整
  校验顺序（值 → callback → 操作）+ 孤儿 promise 检查。

### 4.150 edit 工具可能静默吞行：oldString 跨行即 diff 复核（2026-09-20，G5 轮·工具约束）

- 症状：两次 edit 后相邻整行消失——`__normExecOpts` 的 maxBuffer 行、
  `__normForkOpts` 的 killSignal 行；oldString 里根本没含那两行，编辑却"成功"，
  maxBuffer 套件全灭（err null + 全量输出）才现形。
- 根因：未深究（疑似多行 oldString 的模糊匹配吞了中间行；§4.28 姊妹篇）。
- 修法：补回整行（与原文逐字节同）；此后凡多行 edit，提交前必
  `git diff | grep "^-"` 逐条核对删除行是否全部有意（本轮即靠此抓到第二处）。
- 推广为铁律：edit 的删除行默认全部可疑——无 `-` 行是意外之喜，有即逐条认领；
  大 edit 拆小步 + 步步 `git diff`（§4.28 验落盘的 diff 版）。

### 4.151 外层模板字符串内禁写内层模板字面量（2026-09-20，G5 轮）

- 症状：`node:child_process` 模块整体加载失败，
  `child_process:1486:77: Expected a semicolon...`（全套件全灭，非单点红）。
- 根因：`__FORK_CHILD_SRC` 本身是外层模板字符串（反引号界定）；在其内部新写的
  `process.send` 校验用了内层模板字面量（反引号 + `${}` 插值）——内层反引号把
  外层提前闭合，`${v}` 在父作用域求值/残文变垃圾，oxc 解析期即炸。注释行里的
  反引号/`${}` 同样顶破外层（1486 行即注释行）。
- 修法：fork 子源块内一律字符串拼接（`"'" + v + "'"`），注释亦禁反引号与插值
  写法；提交前 grep 块内反引号/`\${` 计数归零（本轮 `inner backticks: 0`）。
- 推广为铁律：凡"JS 生成 JS"的模板块（fork 子源/worker eval 串），内层禁一切
  模板字面量；注释是代码，同样禁。

### 4.152 静默窗防抖在持续写下饿死：fs.watch 改前沿触发（2026-09-20，G8 轮）

- 症状：1ms/10ms 写循环套件（test-fs-watch.js/encoding/promises-watch）全 hang；
  单写/偶写套件全过。同一机制下只有目录自身元事件能出来，文件事件全丢。
- 根因：`debounce_loop` 是 300ms **静默窗**（到期才刷、同键刷新 deadline）——
  持续写使窗口永不到，事件饿死。旧设计为 `test --watch` 的"首事件赢"抄来的，
  但语义错了：node/libuv 无静默窗，事件即时流。
- 修法：前沿触发 + 同键 300ms 抑制窗——首事件立即刷（Create+Modify 的 rename
  先到先赢，§4.27 诉求保留），窗内同键丢弃，窗后首事件再刷（`src/builtins/node/fs.rs`
  `debounce_loop`；testrun 自有 debouncer-mini，不受影响）。
- 复现：10ms 写循环 watch（修前 foo.txt 永不到，修后即达；
  `tests/node/fs.rs::phase10f_fs_watch_rapid_and_rewrite`）。
- 推广为铁律：凡"等安静再动"的设计，必须回答"一直不安静时怎么办"—— perpetual
  busy 下静默窗 = 饿死；要么前沿触发，要么加最大等待上限。

### 4.153 notify 回调线程的 TLS state 是错表：分类移分发侧（2026-09-20，G8 轮）

- 症状：Create 二判据（seen 表 + birthtime）上线后，预存文件重写仍首报 rename——
  调试打印 `seen=false`，而种子明明已标记同一路径。
- 根因：notify 回调跑在 **notify 线程**，`state::with_plain/with_rooted` 读的是该线程
  的 TLS state——与 JS 线程的表完全是两张皮，mark/has 跨线程hello对不上（静默错，
  不 panic）。此前回调内无 state 访问，故从未暴露。
- 修法：回调只做纯数据搬运（生 kind + 展示名 + 全路径键）；seen/birthtime 终分类
  移到 `dispatch`（事件循环 = JS 线程，TLS 正确）（`src/builtins/node/fs.rs`）。
- 推广为铁律：§6 线程模型的反面——Rust 侧多线程经 channel 回 JS 线程**之后**才能碰
  TLS state；回调线程内如需查表，一律把生数据送过界、在分发侧判定。review 时按
  "回调跑在哪个线程"逐项对。

### 4.154 `process.exit` 哨兵被用户 catch 即覆盖：首个码赢（2026-09-20，G8 轮）

- 症状：`test-fs-realpath-pipe.js` 挂——自举子进程 `try{exit(2)}catch{exit(1)}`
  的 rc=1（应为 2）。
- 根因：exit 经 JS throw 实现，用户 `catch` 能吞掉哨兵；`process_exited` 旗无条件
  覆盖，第二次 exit(1) 把第一次的 2 洗掉。真机 exit 即终结，catch 永不触发。
- 修法：`process_exit` native 只在旗为空时落账（first-wins；`src/builtins/node/process_.rs`），
  `run()` 的旗检查本就优先，一处改全链对（ESM/CJS 双入口黑盒钉住）。
- 复现：`tests/node/process_.rs::phase4_process_exit_codes` 的 first.mjs/first.cjs 行。

### 4.155 首轮特例的无条件 return 会吞真变迁（2026-09-20，G8 轮）

- 症状：`test-fs-watch-file-enoent-after-deletion.js`（watch 后秒删）必挂；
  心跳探针证明事件循环活着、50ms 轮询 timer 正常、手动复刻同逻辑却能触发。
- 根因：`__statPoll` 的"缺席首轮发 (zero,zero)"分支 `return` 无条件——首轮恰为
  (null,real) 真变迁（unlink-then-poll 形）也被吞，随后 (null,null) 恒跳过，
  永静默。手动复刻"碰巧"走了另一条时序故能过，极具误导性。
- 修法：仅"首轮且双 null"走零值分支并返回，其余一律落正常路径
  （`src/builtins/node/fs.rs` `__statPoll`）。
- 推广为铁律：凡"首轮/首包特例"分支，默认写成"特例命中才返回"，特例不命中
  必须落回正常路径；特例分支的返回条件与触发条件逐字同写，禁大包围 return。

### 4.156 内部别名 normalize 只认裸形：`node:` 前缀形落空（2026-09-20，G11 联调缺口）

- 症状：http 全域 15 个黑盒齐挂 `Error: 'node:_http_common' is not a builtin`
  （fs/cluster 等全绿，具有误导性——以为 http 栈坏了）。
- 根因：G11 只给裸形（`_http_agent`）写了 normalize 臂；自家 http.rs 用
  `node:_http_common` 前缀形导入（套件直引与内部互引两形并存），前缀形走
  `strip_prefix("node:")` 后无臂命中即 None。INTERNALS 表项一直在，只是够不着。
- 修法：四别名臂并入 `strip_prefix` 后的 match（`src/builtins/node/mod.rs`
  `normalize_spec`），裸/前缀两形同归一。
- 推广为铁律：新增内部别名必须双形验证（裸 `require('_x')` + `node:_x`
  导入各跑一次）；"表里有" ≠ "够得着"，注册链（映射→表→source）逐段断言。

### 4.157 错误包装函数必须幂等：`__fsErr` 剥前缀致嵌套重包（2026-09-20，G8 轮）

- 症状：`tests/permissions.rs::phase8_permissions_fs` 挂——allow-list 读
  `/etc/hosts` 得 `Error/UNKNOWN`（应 `PermissionError`），而裸 `--allow-read`
  写拒绝分支正常。
- 根因：`__fsErr` 的 PermissionError 分支用 `m.slice(prefix)` 剥掉前缀后重建
  Error；读路径经 `__fsCall("open")` 包 `__fsReadWhole` 包 `statSync` 内层
  `__fsErr`——同一错误过包装函数**两次**，第二次前缀已失认，落通用分支重包成
  `UNKNOWN …, open '…'`。写路径只过一次，故正常（§4.37/§4.119 同源第三例：
  包装函数须区分"已整形"与"待整形"）。
- 修法：前缀保留原文重建（`new Error(m)` + 改名），二次进入同分支直通，
  天然幂等（`src/builtins/node/fs.rs` `__fsErr`）。
- 推广为铁律：错误包装函数默认会被嵌套调用（`__fsCall` 层层包）——构造的错误
  必须能无损地再过一次本函数（幂等）；凡 `slice/replace` 去特征头的写法，
  先问"第二次进来还认得吗"。

### 4.158 稀疏检出缺 fixtures 即污染对拍基线（2026-09-21，fs 残簇轮）

- 症状：`test-fs-cp-sync-copy-file-to-directory-error` 等在真机 `node` 下 rc=1
  （`ENOENT`，`fixtures.path('copy/kitchen-sink/README.md')` 不存在），与本仓
  SAME 对齐成"双红"，另有数件从 DIFF 变 SAME，全是假信号。
- 根因：`git sparse-checkout` 只取了 `test/parallel/test-fs-*` + `test/common/*`，
  漏了 `test/fixtures/copy/`——`kitchen-sink/` 以空目录存在，不报错只缺内容。
- 修法：`sparse-checkout add 'test/fixtures/copy'` 后重取真基线（本轮 129 件
  70/59 → 96/33，一夜变天全是 fixtures 的功劳）。
- 推广为铁律：对拍前先断言 fixtures 完备（`ls kitchen-sink/README.md`）；
  "真机自家套件挂"第一反应是环境缺件，不是 Node 有 bug。

### 4.159 mustNotMutateObjectDeep 的 Proxy 断 WeakMap 身份键（2026-09-21，fs 残簇轮）

- 症状：`mustNotMutateObjectDeep({ signal })` 包过的信号一读 `.aborted` 即
  `can't access property "aborted", __wjs_abortState.get(...) is undefined`；
  `addEventListener` 则在 `st.get` 直接炸（st 为 undefined）。
- 根因：该 helper 递归 Proxy 包裹（get 转发、set/define 直接 fail）——WeakMap
  的精确身份键遇 Proxy 即断裂；且经 Proxy 加监听记的是代理身份，真触发 miss。
- 修法：AbortSignal 状态双轨——WeakMap + symbol 自有属性（构造/触发期写真实
  对象，读经 `??` 回落；读穿透 Proxy 只因 get 转发，永不触发 set 陷阱）；
  `addEventListener` 无表决建表不抛（代理监听 miss 即 benign，abort 竞速由
  aborted 轮询门覆盖）（`src/builtins/mod.rs`）。
- 推广为铁律：凡 WeakMap 键存宿主内部态，先问"用户传个 Proxy 进来还认得吗"——
  读路径一律 symbol 回落；只读不写是穿透 Proxy 的唯一安全形。

### 4.160 sync 底座流的双重早退：同步终结派发 + 无句柄续命（2026-09-21，fs 残簇轮）

- 症状：`createWriteStream(f); s.end(); s.on('close', …)` 的 close 永不到，
  且无 timer 时进程 rc=0 直接退出（连 `process.on('exit')` 都不跑——后者系本仓
  另案不支持，干扰项）；先挂监听再 end 则全收到。
- 根因（二连）：① base 在无积压时同步调 `_final`，同步 cb 即同步派发
  finish/close，用户后挂监听全 miss（真机全异步）；② sync 底座无原生句柄，
  循环见全零即退，close 的异步尾巴永不到（§4.34 同族：IO 面的生命周期缺口必现
  为 hang/丢事件）。
- 修法：`_final` 完成 + `open` 派发改 `queueMicrotask` 递延（fd 同步建不受影响）；
  `fs_stream_open` 计数（state + 双 native + idle 门，`UNSAFE-BOUNDARY` 双标签）：
  构造持有 → close 释放，autoClose 关时 finish/end 静默释，未用流 A 段微任务自释
  （否则 patch-open 子进程形永不退出）；`autoDestroy` 随 `autoClose`（closed 语义）。
- 复现：`tests/node/fs.rs::phase10f_fs_stream_lifetime`（`w-fin/w-close/r-end` 行）。
- 推广为铁律：sync 底座的流/句柄，上线即回答"谁让循环等我"——无原生句柄即配
   计数器；"构造即完成"的同步链一律递延派发终结事件。

### 4.161 require 的 make_fn 裸值窗口 + 文件名假相关二分法（2026-09-21，dgram 轮）

- 症状：`test-dgram-async-dispose.mjs` 稳定 138（8/8），而字节相同的改名拷贝
  次次过；`rm + cp` 重建后好一次又坏；stash 旧码 3/3 干净——一度误判"文件名相关"。
- 根因：`require_cjs_file` 内 `make_fn`（`get_prop_value` 裸 JSVal）横跨
  `to_jsval` 字符串具现（可触发 GC 搬移），栈拷贝悬垂后 `call_one` 读垃圾即
  SIGBUS（§4.80 修链时漏了此窗，链下半的 rooted 全但上半没盖）。文件名/内容
  长度只改变 nursery 分配序列从而改变 GC 触发点——确定性假相关，非因果。
- 修法：入 `rooted!` 槽后再做一切分配型调用（`src/builtins/node/require.rs`，
  5 行；修后 3/3 直通）。
- 二分手法（可复用）：尺寸探针（等量死代码）→ hunk 累积二分（每次验 `Compiling`
  行，3 秒"构建"多为 no-op，行为才是真相）→ Rust/JS 分离杂交 → 最小翻转子。
  另：`from_std` 前必 `set_nonblocking(true)`（UDP 无握手安全），否则 tokio
  直接 panic（bindSync 首版现形）。
- 推广为铁律：新增 Rust→JS 调用点，函数体第一行先把全部 JS 值参数入槽
  （§4.141 的 require 版）；"改名即好"的结论默认不可信，先问分配序列。

### 4.162 fork env 丢失即指数 fork 炸弹（2026-09-21，child 轮）

- 症状：`net-reuseport` 探针打出数百个 `parent listening`（端口递增）+
  `parent closing` 交织，进程数爆炸；套件 hang。
- 根因：fork 只验 env（\0）不透传（"值忽略"记档），`new Worker(src, …)`
  未带 env——子复走父分支（`isWorker` 缺失）再 fork，指数爆炸。
  同源：net `Server.listen({reusePort})` 的 direct 路径丢选项（只
  BoundSocket-adopt 路径透传），双绑即 EADDRINUSE。
- 修法：`__normForkOpts` 存 env 拷贝 → `new Worker(src, { env: o.env })`
  （缺省 Worker 侧快照继承，等价真机缺省）；net `__doListen` 加
  reusePort 形参直通 native 第 4 参（`o.reusePort` 两分支同传）。
- 复现：reuse2.mjs（修前炸弹，修后 worker 单次 listening + exit 0）。
- 推广为铁律：凡"子复用父文件"（fork/self-spawn）的开关量（env/argv），
  丢失即自指递归——接线完备性按"子能否区分自己"逐项验。

### 4.163 Rust Stdout 块缓冲吞常驻输出（2026-09-21，child 轮）

- 症状：子进程 `write('x')` 后等 stdin，父永收不到，双边 hang；
  直接跑同代码（无 interval）却正常（"xtrue" 现形）。
- 根因：`std::io::stdout().write_all` 经块缓冲（管道无换行即滞留）；
  进程即退时 exit 刷出掩盖，常驻即永滞。console.log 带换行故无事，
  精确制导到 `process.stdout.write` 无换行 + 循环存续才现形。
- 修法：`stdout_write/stderr_write` 逐次 `flush()`（Node 写无缓冲；
  stderr 本无缓冲，对称 no-op 防后人误抄）。
- 复现：`--eval 'process.stdout.write("x"); setInterval(...)'` 管道接
  （修前零输出，修后即时见 x）。
- 推广为铁律：宿主侧一切"写 fd"面，默认逐次刷——缓冲是传输优化，
  不是语义；"退出即对、常驻即错"的分裂是块缓冲的指纹。

### 4.164 数字信号 0 无枚举值，落空即 SIGKILL（2026-09-21，child 轮）

- 症状：`kill(0)`（存在性检查）直接杀掉目标（exit null SIGKILL）。
- 根因：`Signal::try_from(0)` 无对应变体 → `unwrap_or(SIGKILL)`。
- 修法：`raw == "0"` 短路 `kill(target, None)` 纯验活（nix 口径）；
  JS 侧 `killed=true` 保留（真机同款）。
- 复现：kill0.mjs（修前误杀，修后存活到显式 kill）。
- 推广为铁律：`try_from(x).unwrap_or(默认)` 写法先问"落空值是不是合法
  输入"——0/空串类哨兵值落空即灾难，哨兵短路永远先于转换。

### 4.165 UDS listen 异步绑与同步 cp 的竞态（2026-09-21，fs 轮）

- 症状：`cp-async-socket` 报 ENOENT（应 `ERR_FS_CP_SOCKET`），自带 listen
  回调的探针却正常——同一 socket 文件时有时无。
- 根因：UDS bind 在 Rust task 内异步落定；套件 `listen(path)` 后同步
  `cp()`，文件尚未出现。真机 pipe bind 在 listen() 返回前同步完成。
- 修法：探活（connect 通即 EADDRINUSE）/清残留/bind/chmod 全改同步
  （本地 syscall，无等待），task 只接管已绑 listener 跑 accept；
  `from_std` 前照旧 nonblocking（§4.161）。
- 复现：`test-fs-cp-async-socket.mjs`（修前 ENOENT；修后 0）。
- 推广为铁律：凡"创建即同步用"（listen 后 stat/cp、bind 后 connect），
  创建语义必须是同步落定——异步落定的创建面配合同步消费必竞态；
  改时保留原错误面（探活/chmod/错误事件逐项搬，不删逻辑只换线程）。

### 4.166 宿主 mock 可见性：内部调用须经默认导出（2026-09-21，fs 轮）

- 症状：`write-stream-err` 补丁 `fs.write/fs.close` 永不触发（第二块 BAM
  丢、`close` mustCall 悬空）；`change-open` 的补丁 close 不调回即 hang。
- 根因：WriteStream 内部直调本地 `write/close` 绑定与 native，而套件
  `require('fs')` 补丁落在默认导出对象（`__api`）上——直调即绕过。
- 修法：`_write/_final` 经 `__api.write/__api.close` 调用（同一对象，
  补丁可见；无补丁即原函数，零行为差）；`close` 不等回调即走
  （补丁不调回是真机同款 fire-and-forget）；`_write` 补写 position 跟踪
  （fd 形 `start: 0` 覆盖写，autoclose-option 现形）。
- 复现：`test-fs-write-stream-err.js`（修前第二块丢；修后全绿）。
- 推广为铁律：凡"用户可 mock 的面"（fs/dns 等），内部调用一律经默认
  导出对象、不直调本地绑定——自测时顺手打一个"补丁补丁是否生效"的
  探针（直调绕过是静默的，功能全对时最难发现）。

### 4.167 fifo 双 open 死锁：数据必经已开 fd 读（2026-09-21，fs 轮）

- 症状：`test-fs-read-stream.js` 修校验后由红转 hang（TIMEOUT）；二分定位到
  fifo 子段（mkfifo + 自家 exec 写者 + `{end: 1}` reader）。
- 根因：`__doOpen` 先 `openSync` 建 fd（与写者会合成功，写者写完退出），
  再调 `read_file` 按径**二次 open**——写者已走，二次 open 等新写者永挂。
  sample 实锤：主线程 858/858 采全卡 `fs_read_file → File::open → open(2)`
  （初判"park 空转"系误读——grep 截断了下半栈，见 §4.168 手法）。
- 修法：`__doOpen` 经已开 fd 全量读（`readSync` 循环，position null 游标推进），
  不再按径重开（`src/builtins/node/fs.rs` `__doOpen`）。
- 复现：`test-fs-read-stream.js` fifo 段（修前 TIMEOUT，修后 `END "xy"`）。
- 推广为铁律：凡"创建即同步用"的会合型资源（fifo/pipe/socket），open 与读写
  必须同一句柄——按径二次打开是自杀（§4.165 的读侧版）。

### 4.168 旧 SAME1 掩盖 + 跑分两手法（2026-09-21，fs 轮·过程教训）

- 症状：本轮 5 个"新红"（read 系 ×4 + handle-read）——旧跑分全是 SAME1
  （node=1 wjs=1 双红），新跑分 node=0 wjs=1。
- 根因：旧跑分时 `test/fixtures` 尚未取全（`elipses.txt/x.txt` 缺失，node 自家
  套件也挂），双红对齐成假 SAME；fixtures 补齐（§4.158 后续）后真机转绿，
  宿主缺口现形。非本轮回归（校验/读盘路径与在修代码无交集也佐证）。
- 手法二则：① sample 读栈禁 grep 截断——`sample PID 1 | grep 关键词` 会把
  下半栈（真正的 JS→native 调用链）截掉，先看全栈再过滤，本轮因此误判
  park 一次；② `perl -e 'alarm N; exec @ARGV'` 不加 `or die` 即 §4.145 翻版——
  本轮亲手复现（相对路径二进制 exec 失败，perl 正常 exit 0，空输出当通过），
  此后跑分一律 `exec @ARGV or die` + 绝对路径。
- 推广为铁律：SAME1（双红）≠正确——fixtures/环境补齐后必重跑基线；
  "单独跑过、并行挂"查共享 tmp（§4.122），"以前双红、现在单红"查 fixtures。

### 4.169 仅 error 监听的流够不着懒 open + 补丁分支置空（2026-09-21，fs 轮）

- 症状：`createReadStream(missing)` 只挂 error 监听时无 error、无退出码异常——
  进程静默 0 退出（真机是异步 error，无监听则抛）。
- 根因：`__doOpen` 懒在 `_read`，无 data 监听即无流动、`_read` 永不跑，
  错误永不发现（node 构造后即异步 open，不依赖消费）。
- 修法：A 段微任务内先开（`__doOpen` 前置），开败即就地 `_read()` 递送
  （走既有 `__openErr` 分发；成功仍等流动消费）；补丁分支微任务内同步置空
  bytes（否则 `_read` 回落见 `__bytes===null` 误真开，patch-open 破功）
  （`src/builtins/node/fs.rs` `__holdStream`）。
- 复现：error-only 探针（修前静默退出，修后 `async-error ENOENT`；无监听形
  与真机同为 unhandled error exit=1）。
- 推广为铁律：凡"构造后即生效"的宿主语义（open/error），触发点不得绑在消费
  侧（`_read`/data）——无消费者的形状（纯 error 监听）是天然反例。

### 4.170 tower-http 的 `not_found_service` 恒改写 404 + 非 GET/HEAD 缺省 405（2026-09-21，plan4 T1）

- 症状：`--serve --handler` 下 handler 明明跑了（body 对），但 GET 状态恒 404、
  POST 恒 405 空体（handler 永够不着）。
- 根因（轮子源码实锤，`tower-http 0.7.1 serve_dir/mod.rs`）：① `not_found_service`
  把 fallback 包进 `SetStatus<_, 404>`——文档原话"always respond with 404"，
  fallback 的状态被恒改写（body 保留）；② 非 GET/HEAD 缺省不调 fallback
  直接 405（`call_fallback_on_method_not_allowed` 缺省 false）。
- 修法：`serve_dir.call_fallback_on_method_not_allowed(true).fallback(js_fallback)`
  （`fallback` 文档原话"status will not be altered"；`src/serve.rs`）。
- 复现：`curl GET /<缺失>`（修前 body 对 + 404）+ `curl -X POST /echo`
  （修前 405 空体；修后 201 回声）。
- 推广为铁律：凡"名字像兜底"的轮子 API（not_found/fallback），先读源码确认
  状态改写语义再选；"静态优先、动态兜底"路由上线即验 GET/POST 双方法。

### 4.171 serve 停机 Wake + 响应构造快照边界（2026-09-21，plan4 T1）

- 症状一：空闲 `--serve --handler` 收 SIGTERM 后恒等 10s 才退，
  日志 `serve JS session did not drain in time open=0`（在飞为零仍 warn）。
- 根因：停机旗只在 quiescent 路径检查，`serve_loop` 空闲时 park 在通道上，
  无事件到来即 10s 收尾超时（`src/runtime/serve_session.rs`）。
- 修法：`ServeEvent::Wake` 无副作用事件（`serve_bridge.rs` dispatch 直返 Ok），
  收尾先置旗再投 Wake 打断 park，SIGTERM 亚秒级退出（`src/serve.rs`）。
- 症状二：handler `new Response(readableStream)` 报
  `Response: unsupported body type`（500）。
- 根因：prelude `Response` 构造是快照语义（`__wjs_normBody` 只收
  string/U8/AB/null，与 fetch 客户端共享；`http.rs:187`），与 undici
  可收流不同——属共享语义边界，非 serve 桥 bug。
- 修法（T1 范围）：构造期快照不动，`__wjs_serve_send_resp` 推送时 64KB 分片
  （多 Chunk 通道 + 单 native 拷贝封顶；`serve.rs:54` serve 驱动内，零外溢）。
  真流式构造（收 ReadableStream）留待另案（需动共享 `bodyUsed`/text 全家）。
- 复现：`POST 2MB 回声逐字节一致` + `GET 5MB 分带校验` + `SIGTERM 亚秒退出`
  （探针 `/tmp/wjs-serve-t1b-probe` 形；黑盒 `phase11_serve_large_body_streaming`）。

### 4.172 H3 半关闭 FIN + h3-axum 请求体整收（2026-09-21，plan4 T3）

- 症状：H3 建连/ALPN/h3-build 全过，`send_request` 后服务端静默、客户端
  30s `ConnectionError(Timeout)`（服务端日志停在 `H3 request accepted`）。
- 根因：h3 client `send_request` 只发 HEADERS 不带 FIN；
  `h3-axum::serve_h3_with_axum` 先收齐 body（`recv_data → None`）再调 router——
  client 不 `finish()` 即半关闭死锁（curl 等真客户端自动 FIN，只坑手写 harness）。
- 修法：harness `send_request` 后即 `stream.finish().await` 再读响应
  （`tests/serve.rs::phase11_serve_h3_same_router`）。
- 附带轮限：h3-axum 请求体整收后才调 router（H3 大上传内存 = 体大小），
  与 H1/H2 边收边泵不对等；T3 只验回声，上传流式对等留另案。
  另：本机 curl（SecureTransport 版）无 `--http3-only`，H3 以 harness 验收。
- 复现：去 `finish()` 即 30s Timeout；诊断法：服务端 debug 埋点看停在
  accepted 还是进 axum（本次停 accepted 即 FIN 面）。

### 4.173 T4 WS 五坑：握手归属/GUID 记忆/构建盲区/自动应答/101 表达（2026-09-21，plan4 T4）

- 坑一（hyper 已握手后再 `accept_async` 永挂）：hyper 接管 101 后流上只有 WS 帧，
  `accept_async` 等一个永不到的 HTTP 握手。修法：
  `WebSocketStream::from_raw_socket(up, Role::Server, None)` 直接接管
  （async 仅构造，infallible；`src/serve.rs::run_server_socket`）。
- 坑二（GUID 凭记忆必错）：自拼 `258EAFA5-E648-…` 系虚构，真值
  `258EAFA5-E914-47DA-95CA-C5AB0DC85B11`（tungstenite `handshake/mod.rs::WS_GUID`）；
  python/实现双绿掩盖（自交一致，§4.54 对称性盲区再进宫）。修法：密码学常量
  逐字节对轮子源码 + RFC 向量单测钉死（`ws_accept_key_rfc_vector`）。
  附带教训：单测写完即跑——本次若早跑，错向量当场红，不必绕 python 一圈。
- 坑三（`cargo build` 不编 `cfg(test)`）：bridge 加字段后 build 绿、集成测试绿，
  但 `cargo test --bin` E0063（`state/mod.rs` 测试 helper 旧构造体）。
  修法：改结构体必跑 `cargo test --bin <name>`（集成测试只链接二进制成品，
  不编 bin 的 `cfg(test)`）。
- 坑四（tungstenite 自动回 Close）：对端 Close 到达时库内已排队回帧，应用层
  手动再发被拒（ClosedByPeer），直接 break 即回帧滞留缓冲 → RST。
  修法：收 Close 后 `sink.flush()` 推出再结算（tests/ws.rs stub 回帧同族不同术）。
  附带：client 侧（`ws.rs`）同形缺 flush（对端先关即 RST），既有测试全绿未暴露，另案。
- 坑五（101 表达弃用）：初版 `new Response(null, {status: 101})` 被 prelude
  `RangeError`（status 限 200-599，undici 同口径，不动共享语义）。
  修法：handler 直接 `return socket`（`__wjs_wskState.server` 品牌位）即接受；
  Response 即 Decline。无新模块形状，fetch 契约不变。
- T4 语义记档（三件，另案）：① Decline 双 fetch（offer + HTTP 各跑一次，
  升级请求专属）；② H3 上传整收（§4.172）；③ 通道 unbounded（背压另案）。
- 复现：`phase11_serve_ws_echo`（去 flush 即 RST；错 GUID 即 tungstenite 客户端
  握手失败）；`phase11_serve_ws_bad_handshake/static_first`（400×3/101+RFC 键）。

### 4.174 全并行全量偶发 mozilla mutex 解锁失败（2026-09-21，观察中·未闭环）

- 症状：`cargo test` 全并行跑到 `--test node` 时
  `util::phase9a_util_promisify_callbackify_deep_equal` 报
  `mozilla::detail::MutexImpl::unlock: pthread_mutex_unlock failed: Invalid argument`
  后 abort；同用例单跑 0.59s 过，整 `--test node` 套件 51s 234/234 全绿。
- 现状：与当轮改动（serve 系）零交集，判定并行负载型 flake，非回归；
  根因未深究（引擎内部锁，另案）。再现两次即升级为必查。
- 推广：全量红先单跑 + 整套件跑两档复核，再定回归/flake（§4.62 姊妹篇）。

### 4.175 fifo 黑盒全并行负载下挂死：写者缺席读端零 CPU 睡眠（2026-09-21，观察中）

- 症状：全量 `cargo test`（默认并行）在 `fs::streams::phase10f_read_stream_fifo_end`
  卡死（`running for over 60 seconds`；读端 winterjs 进程 `S` 睡眠、7 分钟仅
  0.21s CPU；写者 `sh` 不存在）。同域测试（26 并行）两次 7s 过，单跑 0.59s 过。
- 根因（未完全钉死）：重负载下 `child_process.exec` 的写者 shell 缺席，
  读端 `open(O_RDONLY)` 永阻塞。域拆分纯搬移（JS/Rust 双字节恒等已验），非回归。
- 修法：`cargo test --test node -- --test-threads=4` 全 node 域 248 绿（90s）；
  其余 19 target 全绿。全并行卡死先查该用例（`ps` 见读端 `S` + 无写者即此坑）。
- 复现：全并行跑到该用例即卡；降并行即过。

### 4.176 URLPattern 专项四坑（2026-09-21，plan3 §5）

- 坑一（组序是哈希桶 artifact）：多组 `groups` 键序真机同名集异序可得异序
  （`(a,b,c)→[b,c,a]`、`c,b,a→[b,a,c]`，跨进程稳定但无声明规则；映射本身全对）。
  根因：Node 内部名表迭代序非声明序（疑固定种子哈希表桶序，无源码实锤）。
  修法：`serde_json::Map`（BTree）天然字典序 + 代码注释记档为确定性偏离；
  套件只钉单组（无多组序断言），黑盒显式断言排序后形状。教训：先验跨进程
  稳定性（稳定≠可复刻），不稳定才谈对齐、稳定但无规则即记偏离。
- 坑二（base 失效吞掉）：`new P("https://example.com", null, null)` 应抛
  INVALID_URL_PATTERN，初版回成功——Rust 把非法 base（`"null"` 解不出）
  当缺席（`and_then(parse)`）。修法：parse 路 `Some(b)+解不出` 即抛；
  test/exec 路维持吞错（真机回 false/null，套件钉住）。
- 坑三（`r#"` 撞 `"#frag"`）：测试 JS 含 `"#frag"`，`r#"` 裸串被提前闭合，
  全文件编译炸（§4.44 家族：`format!`/`r#` 与 JS 同现一律换定界/落盘）。
  修法：载荷块改 `r##"..."##`（断言串内无 `"##` 即安全）。
- 坑四（concat 同域双导出）：`url_pattern.js` 与 `url_legacy.js` 各写一次
  `export URLPattern` 即 `Duplicated export`（concat 是同一模块作用域）。
  修法：定义与导出分家——pattern 块只留 `const`（置首位供 default 对象求值），
  具名/default 双导出全收进 legacy 块。
- 附带：轮子两入口宽严不一——`parse_constructor_string("[")` 抛，
  `parse(init{pathname:"["})` 过（真机后者不抛）。单测走错入口即红，
  入口按调用形状选（串形/字典形各归各）。
- 复现：`tests/node/url.rs::phase11_urlpattern_*` + 真套件三件双侧 rc=0。

### 4.177 dgram 余簇五件：校验序/端口序/fork 宽容/伪语义翻转/挂死归属（2026-09-21）

- 校验序：已连接态 `send(23)` 应报 ARG_TYPE buffer 而非 IS_CONNECTED——
  msg 形态校验先行，再判连接态，最后 offset/length 越界（真机序；
  `send(buf,1234,addr)` 的地址串在位置 2 即判连接，同序）。
- 端口序：未连接 `(buf,0,6)` 报 BAD_PORT 而非地址错——`validatePort` 先于
  地址形态校验（`(buf,6,0)` 之类三参形按 msg/port/address 解，地址数错另案）。
- fork 宽容：`cluster.fork("str")` 真机不抛（env 展开语义，fork 从不校验
  env 类型，见 lib/internal/cluster/primary.js；42 同理照走）。
  自家黑盒旧断言 `fork(42)` 抛错系伪语义——§4.65 翻转（黑盒改不断言抛、
  改跑真 fork + worker 自退）。
- 挂死归属：unref-in-cluster 实现正确（res-check `[]`）但套件抖动时，
  先抓"停在哪端"（本轮：P-worker-exit 永缺席 = cluster 退出 race，
  G6 worker 投递/cluster 协议同族），再定回归/flake；stash 旧树对照若
  缺 API（恒红）即无效对照，不如直接读退出事件。
- 复现：`tests/node/dgram.rs::phase11_dgram_*` + `test-dgram-send-bad-arguments`
  修前 `Missing expected exception`（端口进队列未同步校验）/修中
  `unexpected throw`（连调定位法：CAUGHT 打实际值，见本轮）。

### 4.178 node:test Slice A 四坑（2026-09-21，plan3 test API 轮）

- 坑一（CJS 包装头垫行，栈行号 +1）：`t.assert.ok` 失败补调用点源码行时，
  ESM 按栈行号读文件精确命中，CJS 恒差一行（require 五连柯里化包装头垫一
  行，`require.rs:244` 实锤）。修法：`__callerLine` 窗口向上回扫（行号起向下
  5 行），首个含 `ok(` 的行即调用点，落空才回精确行
  （`src/builtins/node/testmod.rs`）。
- 坑二（async 吞同步校验）：`t.waitFor` 校验写在 `async` 函数体内，
  `t.assert.throws` 同步调用够不着——抛错变 rejection（另附 4 条 unhandled）。
  修法：校验提同步段先执行，通过后再进异步轮询（`__waitFor`/`__waitForRun`
  分家）。推广：凡"同步抛 + 异步跑"双形态 API，校验一律同步段，
  §4.37 症状一的 async 版。
- 坑三（竞速输家 timer 续命）：`waitFor` 的 `Promise.race([attempt, sleep])`
  输掉的 `sleep(60000)` 不清——测试全过、小结已打，进程续命 60s
  （`polls`/`limits` 套件，`timeout: 60000` 现形；真机同为 cancel 语义）。
  修法：race 落定即 `clearTimeout` 睡眠端。推广：凡带超时的 race，
  落定即清输家，否则"全过但不退"（§4.93 的 hang 反面：输出齐、退出码无）。
- 坑四（CJS 看不见 ESM 具名导出）：`require("node:test")` 取默认导出本体，
  `assert`/`getTestContext` 等纯具名导出即 undefined（`test/suite` 因挂在
  test 函数上才可见，`run is not a function` 同源）。修法：具名同步挂载
  `test.getTestContext/test.assert`（`run/mock/snapshot` 随各片）。
- 附带：`describe` 无 fn 即抛是偏差——真机 `createSubtest` 非函数 fn 即
  noop（空 suite 合法），改静默 noop；`strictEqual` 缺省文案改真机逐字
  （`Expected values to be strictly equal` 前缀，custom-assertions 套件钉住，
  既有黑盒无文案依赖）。
- 复现：11 目标套件（修前 DIFF 修后 SAME0）+ `tests/node/testmod.rs`
  `phase10f_test_*` 四件。

### 4.179 全并行 2 挂再现（2026-09-21，§4.174 家族）

- 症状：全并行 `cargo test` 在 `--test node` 挂 2 件——
  `child::exec::phase10f_child_exec_shell_self_and_timeout`（`envself` 行缺失）
  + `fs::sync::phase10f_file_handle_read_empty`（`MutexImpl::~MutexImpl:
  pthread_mutex_destroy failed: Resource busy`，§4.174 同款）。
- 定性：单跑双绿 + `--test node -- --test-threads=4` 257 全绿——并行负载型
  flake，非回归（本轮改动：testmod/assert，与 child-env/fs 零交集）。
- 推广：全量红先单跑 + 降并行整域两档复核（§4.174 纪律）；`| head` 后
  `echo $?` 取的是 head 的码，黑盒/探针判活一律文件落盘 + `${PIPESTATUS[0]}`
  或重定向后取码（§4.45 三进宫：本轮二分 wait-for 时亲手复现一次）。

### 4.180 node:test 钩子归属 + MockTracker（2026-09-21，plan3 test B1 轮）

- 症状：`local mocks are auto restored` 在 mock 恢复后报
  `Expected [Function bar] notStrictEqual [Function bar]`（afterEach 把已复原
  的 bar 又判成"仍 mock"）。
- 根因：想当然"钩子跑在 owner 身上"——真机 Test.run 跑的是
  `this.parent.hooks.*`（钩子归属是父，参数传子 ctx）：父 afterEach 只在子
  身上跑一次（继承执行），owner 自身结束时跑的是**祖父**的钩子（多为空）。
  探针 `hook.cjs` 钉住：无子测试的 `t.beforeEach/afterEach` 永不跑；
  `t.before` 在首个子测试时跑一次；子身上 getTestContext 见 owner 名。
- 修法：`__runOne` 钩子段重排——before（套件 runOnce + 测试 owner 首子一次）、
  beforeEach（套件由外向内 + owner 的，子 ctx）、afterEach（owner 先 + 套件
  由内向外，注册序）、owner 自身 `after` 照旧；测试级钩内 getTestContext
  改推 owner ctx（`__runTestHook(fn, ownerCtx, argCtx)`）。
  旧模型（自身跑自身 Each）在 Slice A 全绿下掩盖，新钩子一用即现形。
- MockTracker 移植要点：Proxy target 即原函数（name/length/descriptor 全透；
  `bind` 的 this 透传；construct 经 `ReflectConstruct(impl, args, proxy)`
  故 `instanceof` 归原函数）；method 经原型链找描述符、自有属性安装；
  `restore` 判 `methodName !== undefined`（真机仅判 string，symbol 复原是其
  漏口）；`times`/once 下标门逐字（validators 现成）。
- 引擎偏离（记档不修）：`mocks a constructor` 末断言要 V8 私有字段文案，
  SM 文案不同且 JS 层无拦截点（mocking.js 55/56，文件级仍 DIFF）。
- 复现：`tests/node/testmod.rs::phase10f_test_mock_*` 两件 +
  `test-runner-mocking.js`（修前 `target undefined` 全灭）。

### 4.181 mock.timers 四件（2026-09-21，plan3 test B2 轮）

- 补丁面先探再写：`node:timers` 命名空间冻结（`setTimeout is read-only`）——
  其具名补丁整体跳过（文档化偏差）；`scheduler.wait` 经影子赋值可补
  （原型有实现，赋后自有、删即复原）；`globalThis.Date/setTimeout` 直接
  赋值可补。探针 `patch{,2,3}.mjs` 三件先行。
- 测试结束必须 `mock.reset()`（含 timers），只 `restoreAll()` 即跨测污染——
  node Test 收尾即此口径（`test.js:1526`），本轮 testmod finally 同改。
- `Date.toString()` 套件钉 V8 单行串——SM 原生多行，直接回真机可观测串
  （与 §4.110 同类文案桥，就地注释）。
- 复现：`tests/node/testmod.rs::phase10f_test_mock_timers_*` 两件 +
  真套件 date/scheduler 双转绿（SAME0 17→19）。

### 4.182 run(none) 五坑（2026-09-21，plan3 test C 轮）

- 坑一（import 期 pump 打架）：内层文件 `test()` 直接调 `__pump`，与 run 的
  drain 形成双排空循环共吃队列，顺序全乱。修法：`__innerActive` 旗，泵卫拒
  内层 drain（`__pump` 直接 return），run 走 `__drainLoop` 直调（§4.24 多 run
  教训的同进程版：隔离边界 = 状态快照/复原 + 可重入 drain）。
- 坑二（suite 回调早于 before）：真机 Suite 构建先跑父 before 钩、再调 suite
  回调（order-probe 钉住：直跑/run 同序；异步 before 不阻塞回调）。
  修法：describe 建套件即 kick 父 before；kick 内联跑同步前缀、遇异步挂链；
  测试起跑 await 落定（毒化照旧）。
- 坑三（后注册 before 永不到）：套件级 fired 旗太粗——后载入文件的根 before
  在首 kick 之后注册即漏。修法：逐钩 runOnce（已跑集合 + 每次 kick 补跑新增，
  全串行；落定清槽以便下轮内联）。
- 坑四（钩子归属再确认）：钩子 `this`/参数 = 运行中测试 ctx，`getTestContext`
  = owner；before/after/suite 回调的 `this`/参数 = owner；根名 `<root>`
  （no-isolation 夾具 `this.name` 逐项钉住）。修法：全部调用点改
  `fn.call(argCtx, argCtx)` / `fn.call(ownerCtx, ownerCtx)` 两族。
- 坑五（only 批量误伤）：旧批量过滤把整批压成 only，套件内非 only 本该跑。
  修法：删批量，改 applyFilters 逐项门（祖先标记 + 父门；`only:false` 显式
  即 noop）。另：无 before 套件的 after 被 beforeFired 门吞——补 `_ran` 位。
- 复现：`tests/node/testmod.rs::phase10f_test_run_none_and_plan_gates` +
  真套件 no-isolation ×2/enqueue/test-id/tags-validation（修前 DIFF）。

### 4.183 run(process) 五坑（2026-09-21，plan3 test D 轮）

- 坑一（loader 目标回退重复求值）：抛错文件的动态 import 被 ESM→CJS 回退各
  求值一次，注册翻倍（todo-skip 双跑现形；成功文件单次无事）。
  修法：双管齐下——① skip 套件不跑回调（真机：跳过即不构建，遂无抛错），
  ② 失败导入回滚本批注册（队列/注册表截断）。
- 坑二（合成错被 expectFailure 回吞）：意外通过合成的 expectedFailure 错在
  catch 又被"期望失败→pass"分支吞掉。修法：catch 按
  `failureType !== "expectedFailure"` 分流（`§4.51 __callNative` 闭包错码重包
  的同源教训：包装层必须识别已整形错误）。
- 坑三（skip/todo 是置旗不是抛）：`t.skip()` 后 body 继续、skip 优先、message
  回显到事件互斥键（真机探针钉住三条）。修法：throw 改置旗 + 终局判定
  （抛错仍粘滞失败）；静态 todo 改跑 body（失败仍失败）。
- 坑四（cwd 径带 `..` 全等失败）：`process.cwd()` 非规范化，filetest 的 file
  全等断言挂。修法：`resolve(cwd, given)` 规范化（`node:path` 现成）。
- 坑五（监听抛错被吞即假绿）：`_emit` 内 try/catch 把 mustNotCall 断言吞掉，
  文件空绿。修法：去吞（真机 EventEmitter 口径）+ `__runFilesAsync` 的 catch
  改异步重抛走 uncaught（文件可见失败）。
- 复现：`tests/node/testmod.rs::phase10f_test_run_process_and_expect_failure` +
  真套件 expect-error ×2/todo-skip/filetest（修前 DIFF）。

### 4.184 run 语义深化六坑（2026-09-21，plan3 test E 轮）

- 坑一（文件级 enqueue 在监听前丢失）：`run()` 同步调 `__runFilesAsync`，
  首事件先于调用方 `.on` 发出（test-id 套件现形：dequeue/start 有、enqueue
  无）。修法：执行体递延一轮 microtask（监听先挂）；worker 内同理
  （同函数复用，harness 的 `.on` 同样后挂）。
- 坑二（同文件二次 import 命中缓存）：黑盒两次 run() 同一探针文件，第二次
  空转零事件。真机同款语义（模块缓存；worker 跨线程则天然隔离），黑盒改
  双探针文件（§4.168 SAME1 掩盖姊妹篇：缓存使"跑过"恒真）。
- 坑三（helper 改签名丢参数）：`__runOneWorker(given, abs, stream)` 重构丢了
  `options` 形参，体内 `options.timeout` 全变 ReferenceError（全部 process
  用例一夜回红；栈 `__runOneWorker/<` 指认）。修法：改签名必须 grep 全部
  调用点 + 被调体内全部标识符（§4.182 坑五同源；本轮现形）。
- 坑四（包装与显式发射二选一）：`__emitPass`（内发 pass+complete）上线后，
  外层残留的显式 `complete` 致每测试双 complete（test-id 计数现形）。
  修法：包装函数与显式发射二选一，grep 全调用点去重。
- 坑五（skip/todo 置旗三语义）：`t.skip()` 后 body 继续、skip 优先、message
  回显到事件互斥键（真机探针三条钉住）；静态 todo 跑 body（失败仍失败，
  通过记 todo）。修法：throw 改置旗 + 终局判定（§4.183 坑三的 E 轮落实）。
- 坑六（loader 回退双求值）：抛错文件的动态 import 被 ESM→CJS 各求值一次，
  注册翻倍（todo-skip 双跑现形；成功文件单次无事）。修法见 §4.183 坑一
  （skip 套件不跑回调 + 失败导入回滚注册；本轮探针钉死）。
- 附带：legacy done 回调（streaming 套件现形）；plan 子计数；stopTest 超时
  竞速 + TestPlan wait；tag 过滤子集；entryFile 转发戳；调用点文件归属；
  种子洗牌（PRNG 逐字 + 延迟兄弟队列）；run coverage 选项校验（码逐字）。
- 复现：`tests/node/testmod.rs::phase10f_test_run_semantics_*` +
  `phase10f_test_run_tag_filter_and_randomize` + 真套件 plan/tags/entry/
  randomize（修前 DIFF 修后 SAME0）。

### 4.185 http TIMEOUT 轮八坑（2026-09-22，plan3 G11）

- 坑一（defer-to-connect）：`req.setTimeout(1000)` 同步覆写把已建连 socket 的
  超时也改成构造期值。根因：覆写直写 socket，connect 前后未分。修法：connect
  前只记请求级值（socket 事件仍见构造期值），connect 时落地（`framing_agent.js`
  `setRequestSocket`）。复现：`client-set-timeout`（修前 socket 事件即 1000）。
- 坑二（`_last` FIN 递延）：GET 管线超 max 的 503 恒丢（POST 靠 pacing 碰巧能到）。
  根因：FIN 排在 cont（re-feed）之前，已读管线字节的错误响应先被 FIN 截断
  （write-after-end 丢失）。修法：FIN 递延一轮排在 cont 之后
  （`framing_outgoing.js`）。复现：`tests/node/http/timeout.rs::p3`（修前 GET 无 503）。
- 坑三（capture 接线）：`captureRejections` 形 error 经 socket 透传丢失。
  根因：capture destroy 的 err 未进 socket 错误通道。修法：capture destroy 带 err
  透传（有监听才发，无监听仅 aborted）。复现：`outgoing-message-capture-rejection`
  （修前 DIFF 修后 SAME）。
- 坑四（ready 解禁）：raw-socket 套件（keep-alive-pipeline-max-requests 等）写饿死。
  根因：§4.126 暂缓 `ready` 不发射，真机 `onconnection` 后同步触发写。修法：connect
  后同步发射（`net` 侧；暂缓作废，欠账清零要求语义到位）。复现：上套件（修前 hang）。
- 坑五（池复用 FIN 竞态）：`Connection: close` 响应入池，次请求复用撞 FIN 报
  ECONNRESET。根因：回池只门 freeSockErr。修法：`__release(poolable)`——close 响应
  即销毁（`framing_agent.js`）。复现：`get-pipeline-problem`（修前 DIFF）。
- 坑六（空闲判定漏 res）：`closeIdleConnections`/看门狗误杀在途响应。
  根因：空闲只看 req 侧。修法：判定补 `st.res` 三处（close/closeIdle/看门狗）。
  复现：在途响应形（修前被关）。
- 坑七（abort 门控）：服务端无 error 监听时 abort 抛错。根因：error 同步发抢在
  aborted 之前。修法：aborted 同步恒发，error 有监听才发且递延
  （不抢 res 侧 PREMATURE_CLOSE）。复现：`aborted` 块 + `timeout.rs::a2`。
- 坑八（`_ended` 订正）：响应中 `setTimeout` 全被 noop。根因：门控误用请求 finish
  置位（`__reqFinished`）。真机 `_ended` 置于 responseOnEnd。修法：门控改
  `res.readableEnded`，删请求置位。复现：`client-timeout-with-data`（修前 hang）。
- 附带方法学二则（旧坑再现）：① 脏二进制打架两次（6 秒构建误判；§4.62/§4.145
  姊妹）——后一律 `ls -la` 对时间戳 + 行为验证；② `| head` 后 `$?` 是 head 的码
  （§4.45 三进宫）——判活一律文件落盘取码。

### 4.186 服务端 destroy 失声 + 半开续命：两处 hang 一次清（2026-09-22，plan3 G11）

- 坑一（写端等读端 EOF 即死锁）：`NetCmd::Close` 只 shutdown 写端，`Close`
  事件要等读端 EOF——对端半开（allowHalfOpen 客户端）永不 FIN，读端在
  `read()` 永驻，`server.__sockets` 残留 1，循环永不 idle（`__sockets.size`
  探针实锤；客户端兜底 destroy 即退是同一根因的反证）。
  修法：写端收 `Close` 即发 `Close`（`close_once` 防与读端 EOF/错路径双发），
  不等读端（读端后到 EOF 只发 End）（`net_pumps.rs` 写端 `Close` 臂）。
- 坑二（收 FIN 半开仍续命）：坑一修后服务端干净（`sockets=0`）仍 hang——
  半开客户端（`net_open=1`）续命，而真机照常退出（k7/k9/k12 逐项实测：
  半开且 `ref()` 也留不住；读停转后空闲句柄不 ref 循环是 libuv 层事实，
  写侧仍可用、`write-cb ok` 照常）。
  修法：`NetEntry.holding` 位（初值 true）+ `net_halfhold` native——`__ev end`
  内 allowHalfOpen 未销毁即递延一轮 microtask，稳定半开（监听内无同步
  destroy/auto-end 动作）才摘续命；`net_open` 只数 `refed && holding`；
  `set_ref` 摘除后只翻位、`purge` 按位结算，防双减（`net_halfhold_balance`
  单测钉住全部转移）。
- 证伪记录（勿复踩）：半开判定不能下在 End 派发时——正常全关舞蹈的
  End→（microtask auto-end）→Close 链中间会出现"无进展 + 计数零"的轮次，
  直接摘会抢在 Close 到达前退出（Close 在途由 park 的通道唤醒兜住，但
  写端 shutdown 中的无消息窗口盖不住）。只摘"JS 已结算无动作"的稳定态。
- 复现：`test-http-server-keep-alive-timeout`（修前 TIMEOUT 修后 SAME0）+
  `tests/node/net.rs::phase11_net_halfopen_releases_loop`（8s unref 守卫，
  回归只红不挂）；`drain-writable-length` 仍 TIMEOUT（outputData 缓冲模型，
  G3 既定另轮专项，不属本坑）。
- 附带：`server.close-idle-wait-response` 同批转 SAME0；`server-request-
  timeout-keepalive` 真机自挂（node 142，超跑分 alarm），非我方回归；
  dd3/kadbg  park 偶发未复现（4/4 确定性触发 kaT，疑为同族残留计数所致）。

### 4.187 http 升级流八坑（2026-09-22，plan3 G11 upgrade 轮）

- 坑一（升级判定缺 connection 门）："带 Upgrade 头即升级"系伪语义——真机需
  connection token `upgrade` + Upgrade 头双全（advertise case2/3 钉住；llhttp
  同款）。修法：双门（token 大小写不敏感逗号切）。
- 坑二（无监听回落 vs 销毁三形态）：无回调 + 无监听 → 回落 request（advertise
  末段/`upgrade-server` no-listener 形 200）；回调放行 + 无监听 → 销毁
  （TrueWithoutHandler 形 ECONNRESET）；回调否决 → request。旧"无监听即销毁"
  系伪语义。修法：三向分流（`framing_outgoing.js` 升级块）。
- 坑三（对形 headers）：客户端 `headers: [[k,v],...]` 在 errors 内部空错炸
  （扁平形才通）。真机双形同发头。修法：首元数组即对形分支（`framing_outgoing.js`
  ClientRequest 构造器）。
- 坑四（spill 重入无限递归）：spill 经 `sock.emit("data")` 重入服务端同表监听
  → `__feedUpgraded` 自递归（700+ 次才爆栈）。修法：`__spillGuard` 守卫。
- 坑五（直调前双发）：native `__ev` 的 `emit("data")` 与 spill 同表——用户收到
  原始体 + spill 双份。修法：升级后 native 改 `__srvFeed` 直调喂体，用户只收
  spill（`net_socket.js` data 臂）。
- 坑六（服务端监听占数吞 spill）：服务端自有 data 监听使 `listenerCount ≥ 1`
  恒成立，spill 提前冲刷给空（unread 套件 'upgrade head' 丢）。修法：升级/
  CONNECT 接管即 `off` 摘除服务端监听（native 已直调，残留无用）。
- 坑七（迟挂监听丢字节）：101 先到、data 监听后挂（unread 套件 10ms）即丢——
  真机缓冲至读。修法：`__dataBuf` 暂存 + `newListener` 递延冲刷（入表后，
  §4.47）；直发改先暂存后冲刷，保序（`net_socket.js`）。
- 坑八（destroy(err) 同步抛）：同步 `emit("error")` 把 uncaught 语义压成同步
  异常（body-error 套件）。真机 `emitErrorNT` 走 nextTick。修法：`process.
  nextTick` 异步发（tick 回调带 uncaught 路由；microtask 落 rejection 走 fatal，
  不可用）。
- 复现：6 目标套件（修前 5 TIMEOUT + 1 DIFF，修后 SAME0）+
  `tests/node/http/upgrade.rs::phase11_http_upgrade_faces`（15s unref 守卫）。

### 4.188 http 头面 batch5 九坑（2026-09-22，plan3 G11 头面轮）

- 坑一（数字头名过 token 门）："3840" 全数字是合法 token 字符，
  `TOKEN_RE.test(String(name))` 对数字名恒过。真机数字名即
  `ERR_INVALID_HTTP_TOKEN`。修法：`typeof name !== "string"` 先判即抛
  （set/append 双侧，`framing_head.js` + `framing_agent.js`）。
- 坑二（奇长数组错码）：`writeHead(200, ['a','b','c'])` 真机
  `ERR_INVALID_ARG_VALUE 'headers'`，旧实现错抛 ARG_TYPE。修法：改码。
- 坑三（writeHead 无发头门）：已发头再 writeHead 真机即 HEADERS_SENT，
  旧实现无入口检查直接覆写。修法：入口加门。
- 坑四（writeHead 不覆写拼写）：`setHeader('test')` 后
  `writeHead({Test})` 真机 wire 为 'Test'——首写优先仅 setHeader 之间，
  writeHead 恒覆写。修法：合并分支无条件赋值 `__headerNames`。
- 坑五（220 短语 undefined）：未知码真机短语 'unknown'（属性与 wire 同），
  旧实现属性 undefined、wire 空串。修法：`STATUS_CODES[sc] ?? "unknown"` 双处。
- 坑六（数组同键塌缩）：`writeHead([a,1,a,2])` 旧实现后值覆写前值丢一行。
  真机逐行保留。修法：首触覆写、再触累积（`__touched` 集）。
- 坑七（对形 writeHead 不认）：`writeHead(200, [[k,v]])` 真机合法（ClientRequest
  构造器双形同源），旧实现当扁平判奇长即抛。修法：首元数组即逐对取 [0]/[1]
  归一扁平（`["b"]` 对即 value undefined 走 INVALID_HEADER_VALUE，超长元忽略，
  真机逐项实测）。
- 坑八（Host 恒省略缺省端口）：旧实现 `port===80` 即省（flavor 缺省），真机
  （lib/_http_client.js 源码）比较的是**显式配置** defaultPort（缺席即
  undefined，`80 !== undefined` 恒拼）。修法：`__cfgDp`（options.defaultPort ??
  agent.defaultPort）+ 恒拼 + IPv6 双冒号加框（单冒号 'foo:1234' 不加框）。
- 坑九（拒写检查进 _write 毒化流）：`_write` 内同步抛使 writing 态永驻，
  后续 `end()` 永挂。修法：检查提 `write()`/`end()` 包装层（Node 本体亦在
  OutgoingMessage 层），`_write` 保持纯净；连带 `ERR_HTTP_BODY_NOT_ALLOWED`
  新码 + 服务端选项透传（`rejectNonStandardBodyWrites` 缺省 false，
  1xx/204/304/HEAD 无体判据，空串亦抛，真机矩阵实测）。
- 复现：28 件头面对拍（修前 5 红 + 旧 11 件，修后 SAME0；`header-overflow`
  的 `socket.push` 系既定另轮）+
  `tests/node/http/surface.rs::phase11_http_header_face_batch5`。

### 4.189 http TIMEOUT 深水第一铲：host/auth/CONNECT 隧道九坑（2026-09-22，plan3 G11）

- 坑一（hostname/host 取反）：`url.parse` 对象同时带 `host: "h:port"` 与
  `hostname: "h"`，旧实现取 host 当主机名连过去即 ECONNRESET。真机
  （lib/_http_client.js 源码）`hostname` 优先。修法：两处（ClientRequest
  构造器 + agent 建连 opts）同改。
- 坑二（auth 丢失）：`options.auth` 从未转 `Authorization: Basic`（真机 551 行
  口径）；URL userinfo 经 `urlToHttpOptions` 进 auth（decode 双侧）+ IPv6 去框。
  修法：`normalizeRequestArgs` 补 auth + 构造器补 Basic（显式头恒赢）。
- 坑三（CONNECT 补斜杠）：`path` 无条件补 `/` 把 authority-form 改成
  `/target:443`。真机 293-295 行 CONNECT/OPTIONS * 豁免。修法：双豁免。
- 坑四（CONNECT Host 取错）：Host 取连接主机，真机 546 行取 path 本体。
  修法：`method === "CONNECT" && options.path` 即 `String(path)`。
- 坑五（隧道不 detach）：隧道建立后两端挂满请求侧监听（client connect 1/
  data 1/end 2/close 2/error 1/timeout 1，server close 2/error 1/timeout 1），
  真机两端皆 end:1 其余 0。修法：具名存根（connect/secureConnect/agent 单例/
  net conns/error/close/timeout）+ 双端 detach（client 留 agent
  onReadableStreamEnd 恰一 end，server 留 end；`_httpMessage=null` + 摘池 +
  req destroyed/close，socket 不动）+ server FIN 守卫（`__connectHijacked`，
  升级形不动）。
- 坑六（server timeout 无条件挂监听）：`sock.on("timeout")` 在 `if` 之外，
  缺省 timeout=0 仍占数。修法：进 `if` + 存根。
- 坑七（socket 无 HWM）：`net.Socket` 无 `writableHighWaterMark`（真机 65536，
  与 ServerResponse 默认对齐）——旧背压默认 16KB 一并改 64KB（黑盒无
  write-false 依赖，实测零回归）。
- 坑八（基类无 setTimeout/protocol）：`new OutgoingMessage().setTimeout` 即
  not a function；`req.protocol` 缺席。修法：基类 `setTimeout`（无 socket 等
  'socket' 事件，**用事件实参**——手工 emit 形下 this.socket 恒 null）+
  `this.protocol = flavor.protocol`。
- 坑九（后块同步抛掩盖前块 hang）：多 server 文件里 B1 的 handler 抛（吞进
  400 通道即静默 hang）与后块 protocol 同步抛竞速——后块赢即 rc=1（前块 hang
  被掩盖），protocol 修好后前块 hang 现形。教训：多 server 文件定级只看
  rc 会误判"后块全过"，必须分块二分（本轮拆 5 段钉死 B1）。
-  deferred（另轮专项，不在本铲）：handler 抛进 400 通道即静默 hang（真机
  crash；`uncaught-from-request-callback` 为关键套件，改动 blast radius 覆盖
  全 http 域，另立单元）+ `writableLength` 精确记账（headers/帧头计入，
  `len+8` 形；G3 outputData 专项同源）。
- 复现：11 件转 SAME0（url.parse×5/auth×2/CONNECT×3/settimeout）+
  `tests/node/http/surface.rs::phase11_http_timeout_deep_host_auth_connect`；
  `outgoing-properties` 仍红（HWM 已对齐，余 wl 记账专项）。

### 4.190 http TIMEOUT 深水第二铲：server 选项面三坑（2026-09-22，plan3 G11）

- 坑一（选项类被无视即 handler 抛吞 hang）：`createServer({IncomingMessage:
  MyIM})` 下 handler 调 `req.getUserAgent()` 在默认类上不存在 → 抛错吞进
  400 通道即静默 hang（§4.189 deferred 同源）。真机无选项校验（任意值照收）。
  修法：server 存 `IncomingMessage/ServerResponse` + 请求期当构造器用
  （`new (self.IM ?? IM)(hwmOpts)`/`new (self.SR ?? SR)(sock)`；子类无显式
  构造器即透传；裸 `http.Server()` 本就可调，无事）。
- 坑二（createConnection 丢选项）：`net.createConnection` 以 `new Socket()`
  无参构造再 connect，`readableHighWaterMark` 等流选项永不到构造器。
  修法：首参对象即透传进 `new Socket(__o)`（构造器只读自家键，其余忽略）。
- 坑三（res HWM 不同步）：`res.readableHighWaterMark` 恒 Readable 缺省，
  真机跟 socket 走（1024 用例）。修法：客户端 res 构造传
  `{highWaterMark: sock.readableHighWaterMark}`（Readable 原生键，server 侧
  同款）；socket 侧补 `__rhwm`（readableHighWaterMark/highWaterMark 逐级，
  缺省 65536）+ 双 getter。
- 附带真机口径（同轮实测）：socket 读写 HWM 缺省双 65536（旧背压默认
  16KB 一并改 64KB；黑盒无 write-false 依赖）；定制只改对应侧
  （readable 定制不碰 writable）。
- 复现：3 件转 SAME0（server-options-incoming-message/
  server-options-server-response/incoming-message-options）+
  `tests/node/http/surface.rs::phase11_http_server_options_surface`。

### 4.191 splitting 一件：ERR_INVALID_CHAR 缺 `["key"]` 后缀（2026-09-22，G11）

- 症状：`writeHead(200, {foo: "bar\r\nbaz"})` 码对文案错（缺 `["foo"]`）。
- 根因：`E('ERR_INVALID_CHAR')` 定死裸串；`__checkOutboundHeaderValue` 不收键。
  真机（lib/_http_outgoing.js 664/692/756 行 + internal/errors.js 1486 行）：
  `(name='header content', field)` 双参，field 在场即拼后缀——set/append/
  writeHead 三路全带键。
- 修法：E 改 `(field = undefined)` 函数形（无参回裸文案，旧调用零回归）+
  `__checkOutboundHeaderValue(validation, value, name)` 全调用点传键；
  trailer 私有 `__validateHeaderValue` 保持无键（套件未点名）。
- 复现：`test-http-response-splitting`（修前 DIFF 修后 SAME0；附带
  validators/value-relaxed/mutable/multiple/invalidheaderfield×2 零回归）+
  `tests/node/http/surface.rs::phase11_http_invalid_char_key`。

### 4.192 response 双件：write-after-end 毒化终结块 + 状态码门未注册（2026-09-22，G11）

- 坑一（同步 extra 写吞终结块）：`write/end/同步write` 三连只差一步——异步
  extra 写（100ms 后）终结块正常，同步即丢（`5\r\nDATA.\r\n` 后无 `0\r\n`）。
  根因：基类见 errored 压住在途 `_final`，而终结块只活在 `_final` 里；
  holdTimer 路径（发头+体）照走，终结无人补。修法：`write()` 包装层先行拦截
  已 end 的写（自发 error + 回 false，不进基类不置 errored），终结块走正常
  `_final`；优先级 end > 拒写旗（204 先 end 后写仍 WRITE_AFTER_END，真机实测）。
  附带：类内曾有两个 `write()` 定义（旧拦截引未定义的 `__writeAfterEnd`，
  被后者遮蔽零生效）——删死代码时把夹在中间的 `__isNoBodyStatus` 一并带走，
  编译不报错（JS 方法悬空引用只在调用时炸），靠 grep 现形。教训：删遮蔽方法
  必 grep 体内标识符。
- 坑二（`ERR_HTTP_INVALID_STATUS_CODE` 从未注册）：`codes.X` 无 Proxy 兜底，
  未注册即 undefined，`new` 即构造器 TypeError（码错）→ handler 抛吞 hang。
  修法：E 注册 `'Invalid status code: %s'` RangeError + 调用传原值；门按 Node
  原文 `statusCode |= 0` 后判（字符串 '1000' 照收越界才抛；`%s` 遇对象走
  inspect——{}→'{}'；writeInformation 同换 E 形，门不动）。
- 复现：`test-http-res-write-after-end`/`test-http-response-statuscode`
  （修前双 TIMEOUT，修后 SAME0；head-throw 零回归）+
  `tests/node/http/surface.rs::phase11_http_response_gates`。
- 未竟：`response-cork`（cork 真缓冲 + socket 镜像计数 + end 排空三件，流控
  手术另单元）。

### 4.193 G11 收尾轮：cork 面双 CRLF + uncaught 吞错 + 小面四件（2026-09-23，plan3 G11）

- 坑一（chunk 帧双 CRLF，整条流错位）：`__frame` 粒度对齐真机 `_send` 链时
  把尺寸行 hex 写成 `len + "\r\n"`、又独立发一个 `__CRLF`——每 chunk 尺寸行
  后双 CRLF，客户端 chunked 解析整体错位（首 chunk 吞字节、后续 size 行全歪
  → 400/静默 hang；同会话回环全灭而跨进程双向皆绿——真实 node 客户端当
  裁判才定位到"流错位"而非"泵停摆"）。真机 `_send` 链：hex **不含 CRLF**
  （`_send(len)` 后 `_send(crlf_buf)` 独立一发）。教训：对齐"写调用粒度"时
  逐 send 核对字节内容，CRLF 属于哪一发要看真机 crlf_buf 的使用点。
- 坑二（catch 一刀切吞用户 throw）：`__sockOnData` 的 catch 把一切异常
  `destroy(e)`——用户 response 监听里的 throw 被吞成 req 销毁，uncaught
  永不触发（uncaught-from-request-callback 套件 hang）；服务端
  `emit("request")` 同病（handler throw 进 400 通道静默 hang，§4.189
  deferred）。修法：解析错带旗（`__hpe` 加 `__parseErr`）走原 destroy 通道，
  用户 throw `process.nextTick(() => { throw e; })` 重抛（tick 回调带
  uncaught 路由，§4.188 坑八同源）。推广：吞错 catch 必须区分"实现内部错"
  与"用户代码异常"，后者永远上抛——node 语义解析错走返回值通道、用户
  throw 原样冒泡。
- 坑三（options 原型链陷阱）：套件在 `Object.prototype` 装 getter 陷阱，
  我们的 ClientRequest 直读用户 options 走原型链即触发；真机构造器入口
  `ObjectAssign({__proto__: null}, input, options)` 先拷 null-proto 再读。
  修法照抄（`Object.assign({ __proto__: null }, options)`——own 枚举拷贝
  不触发原型 getter）。推广：对接外部 options 的 API，读属性前先 null-proto
  拷贝隔离。
- 坑四（同一解析器两种超限口径）：客户端响应头超限 = **静默截断**
  （node parserOnHeaders "stop collecting"，maxHeaderPairs 上限后不再收集、
  响应照常完成）；服务端请求超限 = 抛 HPE_HEADER_OVERFLOW 走 clientError。
  `__parseHead` 加 `__trunc` 旗按调用方分流。教训：max-headers-count 套件
  的 expected=20 就是截断口径的铁证，"抛错"与"截断"两套件各钉一面。
- 坑五（sweep alarm 量纲误判"真机自挂"）：`server-request-timeout-keepalive`
  套件 requestTimeout 5s × 1.5 defer，全程 ~18s——15s sweep alarm 双边掐死
  被记成"真机自挂（node 142）"（§4.186 的误判）；25s alarm 实证双边绿。
  推广：TIMEOUT 分类前先算套件自身时长（platformTimeout × 倍数 + 余量），
  alarm 必须 ≥ 套件最坏时长；"真机自挂"结论必须换 alarm 档复核。
- 本轮转 SAME0：response-cork / response-drain-cork / outgoing-end-cork
  （cork 面三件：机构 cork 滞留 + socket 镜像计数 + end 强制全开 + 写粒度
  对齐）/ uncaught-from-request-callback / test-http-1.0（_send 面 +
  sendDate=false 不补 Date）/ null-prototype-options / max-headers-count
  （客户端截断）/ response-multi-content-length（客户端拒多 CL，
  HPE_UNEXPECTED_CONTENT_LENGTH 'Duplicate Content-Length'）。
- 黑盒：`phase11_http_cork_faces`（镜像/背压/粒度/end 全开 10 断言）+
  `phase11_http_uncaught_throws`（cli/srv 双向 throw 原文到 uncaught）。

### 4.194 基建轮：socket.push + 服务端解析错 + 写侧流式化（2026-09-23，plan3 基建）

- 坑一（`expectsError` 无 mustCall 即空转）：header-overflow/destroy-socket
  系套件的 socket-error 断言用裸 `expectsError`（不查调用次数）——实现缺失
  时恒假绿，输出对、rc=0。本轮加 socket-error 递送后 validator 才真跑，
  首跑即钉住三件（code/bytesParsed/rawPacket）。推广：对拍"绿"先问断言是
  否执行过——无调用计数的错误断言一律视为假绿嫌疑，宿主侧以"validator 实跑"
  为收敛标准（§4.126 ③的 expectsError 版）。
- 坑二（rawPacket=当片非累计）：'FOO / HTTP/1.1' 整头与 '123…' 首字节 '1'
  的 rawPacket 矛盾——前者整头、后者 1 字节——真相是 llhttp rawPacket=触发
  本次解析的数据片（multiple-client-error 的 unshift 把 '1' 独立成片）。
  修法：`__lastPkt` 存根（空 re-feed 不覆盖）+ 缺席补齐；bytesParsed=片内
  偏移（方法分叉点/全消费=片长；TE/CL 重门未被点名，记档近似取头长）。
  另：补齐须在 clientError emit **之前**（有监听分支直接返回，事后补即漏）。
- 坑三（方法匹配是候选集不是 token 表）：token 门把 'FOO' 当合法（全大写
  token），llhttp 却报 HPE@1——方法是已知表增量匹配（首字节 A-Z + 逐字节
  前缀候选，分叉即偏移；空格终结未知词即词长，CR 终结同；'GE' 悬置等数据）。
  7 探针钉住（FOO→1/Oopsie→1/GETX→3/老小写→0/'*'→0/GE 悬置超时）。
  连带修好 socket-error-listeners 的 hang（'*' 旧口径合法致 clientError 永不发）。
- 坑四（数组头在存不在发）：double-CL 套件 wire 单行 '1,2'——`__emitOne`
  早就会数组分行，真凶是 `__lowerHeaders` 存值 `String(v)` 预洗。修法只改
  存（数组原样），校验仍按合并串（同结果），writeInformation 模板 join 恒等
  零回归。推广：发散路径（存→发）断链时先查存，不动发。
- 坑五（流 buffering 吞同步计数）：`res.write('asd')` 后同步读 length 仍是旧值——
  第二个 _write 还没跑（流一次只派发一个 _write，次块等 microtask）。
  修法：记账上移到 write/end 包装层（同步），_write 内去重（end 块经内部
  _write 直调不走 write 包装，由 end 包装层补计）。教训：凡"同步读"口径
  （writableLength），计数点必须与用户调用同 tick，流派发节奏不可信。
- 坑六（write 覆写的 socket-null 早拒）：管线队列上线后 `write` 覆写的
  `__sock===null→false` 把入列写全拒（`while(write)` 零块即停，needDrain 永不立）。
  修法：null 分两种——入列（__queued）走流机构→_write park，独立构造维持旧
  false。§4.192"删遮蔽方法必 grep 体内标识符"姊妹篇：改守卫先数清有几种
  null（独立/入列/已销毁三种）。
- 坑七（CL 快捷与同步头渲染互斥）：early-render 头即杀 end-only-data 的 CL 快捷
  （`!__headSent` 门）。修法：dry-run 计数（快照→渲染→取值→还原，Date 同长
  恒等）+ 落盘递减 + _final 兜底清零——渲染时机零改动，只加记账。真值覆盖
  （CL 快捷）天然对齐（终态清零），预测偏差不出终态。
- 本轮转 SAME0（13 件）：read-in-error/header-overflow（push 面）/
  server-client-error/invalid-te/double-content-length/
  server-reject-chunked-with-content-length/socket-error-listeners（HPE 面）/
  outgoing-properties（131/139 记账）/outgoing-drain-writable-length（队列+drain）/
  附带 1.0-keep-alive/pipeline-flood/pipeline-outgoing-destroy（eager 队列连带）/
  catch-uncaughtexception（destroy(e) 递送 uncaught 通道连带）。
  黑盒 `phase11_http_socket_push_and_server_parse_errors` +
  `phase11_http_outgoing_writable_length_faces` +
  `phase11_http_pipelined_outgoing_queue_faces`。
  残：reuse-drained（process.report 缺失，另域）/ execPath spawn ~18（待拍板）/
  parser 内省 ~4（记档偏离）。

### 4.195 socket 写错透传四坑：异步确认计数 + 递送顺序 + end 取已记错（2026-09-23，剩余轮 outgoing 面）

- 坑一（mock 确认异步一跳）：socket 写错收集初版同步读 box——mock Duplex
  的写确认经微任务到（自家 _write 异步一跳），同步读恒空，排空递送 null，
  随后错才到（writable-finished 套件 `null !== {}` 顽固）。修法：pend 计数
  + `__afterSockFlush` 等全部 ack（同步全回即下一拍，异步 mock 等确认；
  确认永不到即 socket 违约，node 同款挂起）。
- 坑二（显式递送 + 带 err destroy 双发 error）：`_write/_final` 显式 cb(err)
  后再 `destroy(err)`——destroy 无 errored 去重（destroy.rs 实锤：有 err 即
  排 emitError），mustCall(1) 形得 2 次。修法：显式递送（同一 err 对象，
  strictEqual 同一性）→ `destroy()` 收尾（不带 err：只做 close + socket
  清理；`__failFlush` 注）。顺序再有一层：递送 → destroy 置位 → 终结回调
  （destroyed 门禁二次 emit；终结回调在 destroyed 流上仍触发用户 endCb，
  仅压住 emit——onFinish 无 destroyed 门，实证）。
- 坑三（end 短路恒 STREAM_DESTROYED）：`end()` 在已销毁流上恒回
  STREAM_DESTROYED，end-again 形（失败后再次 end）与真机（回已记错）不符。
  修法：`state.errored ?? STREAM_DESTROYED`（writable_flow endWritable；
  其余两处同形早已如此）。
- 坑四（trailer/基类门三件）：OM 基类缺 `setHeader`（子类各有，基类直调即
  not a function——proto 套件现形）；trailer 名/值标签与 header 不同
  （"Trailer name"/"trailer content"，`ERR_INVALID_CHAR` 加 label 次参，
  旧单参调用零改）；`write` 覆写的子类分流误伤外来 this
  （`constructor.name !== OM` 即走 super——fake-this 形须按
  `instanceof` 判 standalone 先验块形态）。
- 附带翻转（§4.65）：round1 p11 旧静默缓冲系伪语义（真机 proto 抛
  NOT_IMPLEMENTED）——改 stub 后断言 + OM standalone `writableLength`
  走 outputSize。
- 本轮转 SAME0：outgoing-proto/outgoing-buffer（上轮预建，本轮收尾）/
  outgoing-writableFinished/outgoing-finished（res close-on-finish +
  willEmitClose OM 臂，见 §4.196）/outgoing-destroyed（silent-destroy +
  errored-undefined，flaky hang 见 §4.197）。

### 4.196 TLS 无 connecting 面 + 销毁响应禁回池（2026-09-23，剩余轮）

- 坑一（TLS 误判已连通）：mock 识别用 `connecting !== true`——TLS socket
  根本无 `connecting` 面（tls.rs 实锤零命中），新建 TLS 全走"已连通"分支
  提前 flush，握手未成就发明文头，首请求即 hangup（https 全域红）。
  修法：判据加原生柄（`__id` 缺席才是 mock；真新建含 TLS 皆有 __id，
  照旧等事件）。
- 坑二（destroy 的 res 回池即 hang）：客户端 `res.destroy()` 后 socket 按
  正常收齐回池（ESTABLISHED 常驻、unref 不靠）、服务端永不见 FIN——pipe
  形（outgoing-destroyed block3）服务端零感知挂死。真机 destroy 即销 socket
  （不可复用）。修法：`__finishSock` 首门——res destroyed 即销毁不回池
  （+ `agent.__noteClosed` 记账）；正常 end+close 才可池化（keepalive
  复用零回归）。
- 推广为铁律：凡"已连通"判定，先问"哪些真 socket 缺该面"（TLS/UDS/自定义
  底座逐个核）；凡"复用/回池"路径，先问"销毁态到这了吗"（destroyed 进池
  即泄漏 + 对端挂死）。

### 4.197 同一失败的二次投递：socket 迟到错 + flaky 定级（2026-09-23，剩余轮）

- 症状：mock socket 写失败同时走 cap（递送写回调）与 'error' 事件（兜底）
  ——res 已因本次失败销毁且记错后，迟到的 socket 错又进兜底，
  `throw e` 即 uncaught（writableFinished block3 `forced write failure` 实录）。
- 修法：`__httpSockOnError` 首门——res destroyed 且 errored 有值即吞
  （已走 res 通道递送）；无错销毁仍 throw（升级/空闲形旧口径）。
- flaky 定级法（本轮沉淀）：单块过 + 整文件挂 ≠ 块间污染——先量化（单块×3/
  整文件×3），再 instrument（分块标记 + lsof 看残留对端 + FIN 流向），
  最后 master 基线裁决（stash + 同条件跑）。本轮 destroyed 整文件挂即按
  此法定为 block3 管道收尾缺口（§4.196 坑二），非调度 flake。
- 附带卫生：开工先 `git status`（本轮工作区有前人未提交的 proto/buffer
  半成品，交接未提及——`git diff` 认领归属后再动手）；探针脚本放
  `/tmp/wjs-*` 用完即清（`__wjs_node_compat` 同族纪律）。

### 4.198 readable closed 随 close 发射翻位（2026-09-23，剩余轮）

- 症状：`test-http-client-incomingmessage-destroy` 首跑即挂——`res.destroy(err)`
  后同步读 `res.closed` 得 true（套件 26 行要 false，close 监听 29 行要 true），
  且随后 `server.close()` 挂死（开着的 res 致 in-flight unref 续命，另案）。
- 根因：`destroy.rs onDestroy` 给读写双侧同步置 `kClosed`——node 口径读写有别
  （writable 同步翻，readable 随 close 发射翻；套件三段即铁证）。
- 修法：`onDestroy` 只置 w 侧，r 侧移到 `emitCloseNT`（bit 先置后 emit，
  监听内恒 true；emitClose=false 形同样翻位，只是无事件）。
- 推广为铁律：凡"销毁后同步读"口径（closed/destroyed/errored），读写双侧
  分开对真机——读写的销毁时序在 node 从来不是对称的。

### 4.199 incomingmessage-destroy 双件 + error/close 分排（2026-09-23，剩余轮）

- 症状：`test-http-client/server-incomingmessage-destroy.js` 双 TIMEOUT——
  服务端 `req.destroy(err)` 外发 req 'error'（uncaught mustNotCall 形本应静默），
  且无监听 error 抛 uncaught 会吞掉同 tick 的 close（客户端形挂死）。
- 根因二连：① IncomingMessage 无自有 `_destroy`，基类 destroy(err) 必排
  error 发射；② `onDestroy` 把 error/close 嵌套排（`emitErrorCloseNT`）——
  无监听 error 的 throw 直接吞掉 close。
- 修法：① `IncomingMessage._destroy`：错误经 socket 递（服务端级联杀连接→
  客户端 hangup ECONNRESET；客户端经请求 error 照常 uncaught）、本体 `cb()`
  吞错；无错仅未收齐（`readableEnded !== true`）才销 socket——正常收齐后
  的自动 destroy 不碰（keep-alive 复用/响应在途；loopback 实锤杀了即 hangup）。
  ② `onDestroy` 改 error/close 分开排（各下一 tick；uncaught 抛错不再吞 close）。
- 附带：`writeContinue(cb)` 旧实现吞回调（write-callbacks 套件挂死）——补
  落盘后 microtask 触发；standalone `write` 已销毁形回
  ERR_STREAM_DESTROYED 进回调（outgoing-destroy 套件，不同步抛）。
- 推广为铁律：凡"错误 + 终结"双事件设计，排期必须独立（嵌套排即谋杀)——
  uncaught 的 throw 是控制流，会吞掉同回调内的一切后继。

### 4.200 abort 与 destroy 的错误分流 + 孤儿连接守卫（2026-09-24，剩余轮）

- 症状：`test-http-abort-before-end.js` 报 error（mustNotCall）——`req.abort()`
  后 `req.end()` 走出 ECONNRESET；堆栈 `destroy ← abort`，错在 destroy 内合成。
- 根因：`ClientRequest.destroy` 无响应即合成 ECONNRESET（abort-destroy 套件
  要的），abort() 经同一 destroy 即误合成。abort-destroy 套件三段即铁证：
  abort 中途无错 / destroy 中途（有响应）无错 / destroy 事前才 ECONNRESET。
- 修法：合成门加 `__aborted !== true`（abort 恒先置旗），回池门同加
  （abort 不回池）；connect/secureConnect 到达发现已销毁即杀孤儿 socket
  不刷盘（abort 后连接才到形；否则半截请求 RST 回 ECONNRESET 且 server
  零收到被破）。
- 附带同批绿：client-abort3（同源 throw）。
- 推广为铁律：凡 destroy 内合成错误的面，必须区分调用源（abort/signal/
  用户 destroy/内部错误销毁）——合成是 destroy 的语义，不是 abort 的。

### 4.201 req.signal 早夭 + res-close 排序重构（2026-09-24，剩余轮）

- 症状三连：`request-signal` 要 server req.signal（AbortSignal，早夭 abort、
  正常永不）；`req-res-close` 要 res-close 在 req-close 前 + 双 destroyed；
  `content-length` 的 end-with-data 走 chunked（应 CL:11）。
- 根因：① signal 面从零开始（惰性 AbortController + 早夭标记滞后补）；
  socket-close 时 res 未完（或 req 未完）即 abort（正常收齐看 res end，
  真机探针钉住）；② res-close 排序：node 是 finish→destroy→close 且 req
  end 被 res 收尾唤醒（暂停流无 end；真机 finish→close→end→close 序实锤）——
  本仓 res 从不 destroy + req 永暂停；③ `_write` holdback 暂存使 `_final`
  滞后，timer 先刷不认 CL 捷径（end 侧已落 `__contentLength`）。
- 修法：① signal getter + `__abortReq`/socket-close 早夭 abort（含
  `__signalAborted` 滞后）；② res finish-hook 手动 destroy（流机构 auto
  在 finish 链中重入即 b5 hang；microtask 排，finish 时 destroyed 仍 false）
  + `_destroy` 干净 detach/显式杀分流 + `_destroy` 内 resume req；
  ③ `__tryFlush` 认已落定 `__contentLength`。
- 推广为铁律：流机构 autoDestroy 的 destroy 时机不可控（finish 链中重入）——
  要时序即手动排；"暂停流无 end"是天然门控，唤醒点与收尾点同放。

### 4.202 对拍提速三件套（2026-09-24，效率专项·①已落地②③待做）

- 背景：http 剩余 32 件（9 hang + 23 文案），一周实测约三成时间花在机械活上。
  本条是施工计划，不是复盘——三件全落地前，对拍效率未达最优，不许再称"已最优"。
- ① 断言 mapper 报实际值：✅ 2026-09-23 落地（`tests/node/helpers.rs`
  `run_suite_mapped` + 包装壳模板）。用法：
  `WJS_MAP_SUITE=<套件名> cargo test --test node phase_mapper_locate_suite
  -- --ignored --nocapture`（套件名按 /tmp/wjs-node-test/test/parallel 解析；
  定位器非闸门，红绿不进门；门禁两件 `phase_mapper_locates_async_callsite`/
  `phase_mapper_passthrough_on_success`）。实现：包装壳 require 套件 +
  `uncaughtException` 监听，AssertionError 自带 `actual/expected/operator/code`
  （JSON 可序列化，无需解析消息文本），栈里首个套件文件帧即真实调用点，
  一次输出三行：`[mapper-actual]`（JSON.stringify 截 200 字）/`[mapper-callsite]`
  （帧 + 折算物理行 + `>>` 源行 ±2 节选）/`[mapper-expected]`（期望值）。
  验收：raw-headers → 物理 110 rawHeaders 断言、mutable-headers → 物理 187
  数组头 join，均一击定位零二分。实测钉住三件引擎事实：
  （a）无壳输出的位置是 assert SOURCE 内部行号（`node:assert:9:5`/prelude
  `424:53`）安着套件文件名——包装位置是骗的，真实调用点只在 `.stack` 里；
  （b）栈帧行号 = 物理行 + CJS 包装前奏行数（`.js` 恒 +1，6 处实验一致；
  `.mjs` 无包装记 0），mapper 按此折算；
  （c）`unhandledRejection` 监听拦不到（引擎自有收割先走，§4.137 路径），
  `process.on('exit')` fatal 路径不触发——rejection 形失败回落引擎默认输出，
  mapper 只覆盖 uncaught 形；TIMEOUT 件由 helper 20s 看门兜住不挂 cargo。
- ② flake 先分类再动手：✅ 2026-09-25 落地（`scripts/flake-classify.py`）——
  新红先自动跑 3 遍（整文件×3 + 手抽单块 repro×N），GREEN / FLAKY(k/N) /
  RED-DETERMINISTIC / NODE-FLAKY 四分流——flaky 走定级法（§4.197），必现才
  instrument。禁把 flake 当回归深挖（destroyed 整文件挂误判块间污染，实为
  block3 管道缺口——§4.196 坑二教训）。dogfood 首件：dump-req-when-res-ends
  判 FLAKY(0,142,142，挂死型而非红绿互跳)。跑分纪律沿用 §4.145（exec or
  die + glob 绝对路径）；与常驻 sweep 并跑时 `--thread-id/--port-base` 错开
  （sweep 默认 3599/29999，classify 默认 3601/29999）。
- ③ sweep 常驻后台：✅ 2026-09-25 落地（`scripts/sweep-bg.py`）——双 fork
  脱离会话后台直跑、`status/wait/tail/stop` 轮询、工件落
  `~/.wjs-sweep/<tag>/`（status.json/results.log/debug/，家目录 §4.144）；
  失败行带 stderr 首行 + 整份落 debug/（本次排障一击即中）；killpg 连
  suite 子进程一起收；全量禁套 alarm（§4.143），单套件 subprocess timeout。
  与 sweep4 家族 409 件基线可比：`.js`/`.mjs` 都进——前缀过滤只认 `.js`
  会静默丢 6 件 mjs（口径先对齐再比较）。
- 推广为铁律：机械开销（定位/分类/等数）用工具换，推理开销（hang 根因）
  用人换——前者不投半天，后者永远被前者拖慢；工具先行，啃数随后。

### 4.203 请求级 createConnection 错误被吞：oncreate 只认 socket 不认 err（2026-09-23，G11 TIMEOUT 轮）

- 症状：`test-http-createConnection.js` TIMEOUT 且零输出。套件六块插桩定位——
  四个成功块全过（SRV-HIT 各一），async 错误块
  （`createConnectionAsyncError`：`process.nextTick(cb, new Error('async'))`）
  永不决议；sync throw 块（E1）反而正常 reject。
- 根因：请求级 createConnection 的
  `oncreate = (err, s) => { settled = true; if (s) this.__attach(s, false); }`
  只认 socket、完全无视 err——async cb 错被吞，请求既不挂 socket 也不发
  error，promise 永悬（挂死）；sync throw 形靠"异常穿透构造器"侥幸走到
  assert.rejects（真机是 try/catch 收进 emitErrorEvent，永不同步抛——机制
  不同结果碰巧同）。
- 修法（真机 `_http_client.js` 591-607 行逐字）：oncreate err 臂
  `process.nextTick(() => this.emit("error", err))`——无监听经 EE
  rethrow 原错落 uncaughtException（真机 emit('error') 无监听语义，本仓
  EE 已逐字）；sync throw `try/catch → oncreate(err)` 收进同路；
  `settled` 门前置防双投（`createConnectionBoth1/2` 的 cb+return 双形，
  node 用 `once()` 同义）。
- 复现：套件修前 rc=142 修后 0；黑盒
  `tests/node/http/parity.rs::phase11_http_create_connection_error_routing`
  （async cb 错 / sync throw 不同步穿出 / 无监听 uncaught 原文 三形）。
- 教训：① cb(err, s) 双参回调的 err 臂"暂时用不上"也必须路由——`if (s)`
  单臂回调是 hang 制造机；套件四个成功块全绿掩盖了错误块，块标记插桩
  十分钟定位（§4.202-① mapper 覆盖 uncaught 形，TIMEOUT 件仍走插桩）。
  ② "结果碰巧对"（sync throw 穿透）不等于"机制对"——换一个调用形状
  （async cb）即现形；对真机要对机制，不只对结果。

### 4.204 http 欠账清扫轮七坑（2026-09-23，plan3 G11 sweep8 红件簇）

- 坑一（重复头 join 缺省反转）：真机 `_http_incoming _addHeaderLine` 是
  **表驱动**——joinable 表 + 未知头缺省恒 `', '` 合并，19 头单值表才首个赢
  （matchKnownFields 无前缀名单：age/host/from/etag/referer/expires/server/
  location/user-agent/content-type/max-forwards/authorization/last-modified/
  content-length/if-modified-since/proxy-authorization/if-unmodified-since/
  content-encoding/x-forwarded-host），cookie `'; '`、set-cookie 数组不受旗控；
  joinDuplicateHeaders 旗**只压单值表**。旧"未知头首个赢"系 authorization
  单值头行为被错误推广——对真机要对机制不只对结果（§4.203 教训实例）。
- 坑二（查询面只见用户头）：node 查询面（getHeader/hasHeader/getHeaderNames/
  getHeaders/getRawHeaderNames）读 `[kOutHeaders]` 用户头——自动头（Date/
  Connection/Keep-Alive/自动 CL/TE）对查询不可见。修法 `__isAutoKey` 谓词
  （date/connection/keep-alive 旗 + CL/TE 无用户拼写名），内部状态机读
  `__headers` 不受影响。
- 坑三（GET+用户 TE 裸体）：请求侧 `__sendHead` 缺真机 _storeHeader 的 TE
  值扫描——用户 TE 含 chunked（整词）须置 `__chunked`，否则体裸发 + 终结块
  照发（双机构打架），服务端 'bad chunked body' → 挂死。
- 坑四（TE 整词 + 冒号空格，走私向量）：① chunked 判定必须逗号切分整词
  全等——`chunkedchunked` 不得命中；TE 在场非 chunked = teInvalid 分型：
  请求派发（handler ×1）但 data/end 永不发，体字节到达即 HPE → 400 + close。
  ② strict 模式拒收冒号前空格（RFC7230 §3.2.4；lenient 放行）。
- 坑五（parser 全局池）：真机 parser 来自 `_http_common` 全局 freelist
  （回收复用），**不是每连接**更不是每请求——parser-free 套件 maxSockets=1
  串行 100 请求恒同一对象；free（res end）字段置空回池，attach 回填
  onIncoming/joinDuplicateHeaders。
- 坑六（res 侧 timeout 桥 × 监听数契约）：socket 超时双路——req 侧走
  req.setTimeout 的 timeoutCb 独立通路；res 侧 responseOnTimeout 打 **res**
  （真机 1055 行）。**恒挂会多占 EE 监听数**（set-timeout-after-end 套件
  `listenerCount('timeout')===1` 钉住——真机 net 单例不是 EE 监听，我们的
  是）；res.setTimeout 后置形由 IM.setTimeout 桥自武装（complete 哑/close 摘）。
  stash 基线定级（§4.197）实锤回归后回修——改超时路由必跑 timeout 家族全量。
- 坑七（出局归类纪律）：红件先定性再动手——`--expose-gc/--expose-internals`
  flag 门控 + internals 模块（reused-gc/leaky/keepalive-req-gc）、
  process.report 另域（reuse-drained）出局；domain 异步路由（§1 记档）、
  Atomics.wait 引擎面（ka-race）书面偏离；sweep 红绿互跳件先重扫再定级
  （chunk-extensions-limit 两次扫描间自绿 = flake 族）。
- 复现/回归：本轮 15 件转绿（multiheaders×5/mutable-headers/abort-keep-alive-
  destroy-res/override-global-agent/parser-free/smuggling/te-repeated/
  write-information/optimize-empty/chunk-extensions-limit/response-timeout/
  dump-req-when-res-ends），timeout 家族 6 件守卫零回归；node 域 285 绿 +
  冒烟 5/5 ×4 轮。
- 追补（2026-09-25 二批五件）：① server 兜底 `Server[nodejs.rejection]`
  （_http_server 716 行逐字：未发头清头+500 / 已发头 destroy）+ TLSSocket
  双路 `_secureEstablished` + ServerResponse.setTimeout + IM 桥 timeout 带
  socket 实参 + server 连接级**无条件**三路转发（req 未完结/res/server）——
  capture-rejections/url.parse-https.request/set-timeout-server 前四块转绿；
  ② HPE 门序：TE+CL/重复 CL 门必须**先于** requireHost（llhttp 解析期校验
  先行——TE+CL 缺 Host 形 clientError 先到且无 400 直写）；
  ③ 头串 latin1 编码（逐 charCode 低 8 位）——`'binary'` 形非 ASCII 头值
  按 latin1 字节上网（__storeHeader/writeInformation/__sendHead 三处）；
  ④ **原型链桥接禁用**：`setPrototypeOf(ServerResponse.prototype,
  OutgoingMessage.prototype)` 会改道 super.write/end/cork/destroy 全链
  （cork 面实锤 writableCorked 错 0）——身份语义改走
  `OutgoingMessage[Symbol.hasInstance]` 品牌判定（`Object.defineProperty`
  ——`Symbol.hasInstance` 只读直赋即 throw）+ `ServerResponse.__omBrand`。
- 残件定性（第三批）：outgoing-message-capture-rejection（res 写错 capture
  通路）、should-keep-alive（1.0/1.1 × Connection 判定矩阵）、no-read-no-dump
  （socket 'pause' 事件 + POST 背压 + 管线，§4.148 throttle infra 族）。
  set-timeout-server 末段 exit-hold（paused client FIN 的 EOF 急切检测/
  readStop——真机 readStop 后循环放行，本仓按需读永不见 EOF）归 G6 infra
  残件族。本批 5 件转绿（capture-rejections/url.parse-https.request/
  reject-chunked/non-utf8-header + set-timeout-server 前四块），cork 家族
  4 件 + timeout 家族 6 件守卫，node 域全绿 + 冒烟 5/5 ×3 轮。

### 4.205 统一 runner 给 node 也带 `--run`：假红全表 + 过滤丢件（2026-09-25，②③工具轮）

- 症状一（node 假红全表）：smoke sweep 9/9 全 DIFF 且 `wjs=0 node=1`，2.7 秒
  跑完 18 个进程——真套件不可能这么快。node stderr 一行：
  `Can't find package.json for directory /private/tmp/.../parallel`。
  根因：统一 `run_one(binary, path)` 给两个二进制都硬编码 `--run`——node 22+
  的 `--run` 是"跑 package.json scripts"命令（node --run <name>），套件目录
  无 package.json 即报此错 rc=1。wjs 侧 `--run` 是本仓动作 flag，同名不同义。
- 症状二（409 基线缩水）：前缀过滤只认 `.endswith(".js")`，静默丢 6 件
  `test-http-*.mjs`——与 sweep4 家族 409 件基线不可比。
- 排障路径（值得记：三类对照全做完才定位）：① 环境二分（env -i 最小环境 +
  逐变量加回）排除环境；② python 前台复刻（同 env 同 cwd 同 capture）绿；
  ③ 内联双 fork 复刻绿——最后靠"失败行落 stderr 首行"一击命中。教训：
  **失败行的 stderr 是最短路径，走复制粘贴式复刻对照是弯路**——工具先行
  落 stderr 捕获，比人肉二分快一个量级。
- 附带实测：上一轮 sweep 挂死留下的孤儿 winterjs（`--expose-gc`/`--expose-internals`
  套件滞留数小时）会占端口/状态污染基线——两个工具 start 前均 pgrep 预警。
- 修法：`run_one(..., prefix)`——wjs 传 `("--run",)`，node 传 `()`；文件过滤
  `.js`/`.mjs` 双认。修后 smoke6 upgrade 9/9 SAME0 与在册记录一致。
- 推广为铁律：① 包装两个相似 CLI 的统一 runner，实参差异必须参数化而非
  取交集默认——同名 flag 语义不同（本仓 `--run` vs node `--run`）是重灾区；
  ② 新跑分工具首跑必抽查 2-3 件已知绿套件对表，elapsed 异常短（<200ms/件）
  即"根本没跑起来"的信号（§4.145"全绿得可疑"的姊妹篇：全红得可疑同理）。

### 4.206 服务端流控/计时三面：pause 事件、dump 机制、headers 计时归属（2026-09-25，http 尾巴轮）

- 坑一（服务端无体背压）：req 缓冲超 HWM 不停读、socket 不发 'pause'——
  no-read-no-dump 套件（handler 借 'pause' 触发 res.end + 客户端才续发体）
  整链挂死。修法四件联动：①体泵 backpressured 旗（push 返 false）；②__feed
  停读 + sock.pause()；③net Socket pause/resume 发事件（转换沿守卫、异步，
  node emitPauseStreamEvent 口径）；④req._read 钩消费即解暂停。
- 坑二（res 完成清 framing）：__onDone 无条件 st.req/st.framing 双清——体
  在途时后续体字节被当新请求头解析（HPE_INVALID_METHOD → 断连）。修法：
  体未完保留 framing 到体完（node dump 口径按帧处置）。**已试并回退**：
  node _dump 机制（removeAllListeners('data') + resume() + 续推）——本仓流
  端口 flowing 排空节奏与 node 有差（push 在 flowing 态仍缓冲累积、返
  false），dump 后二次背压 → 二次 'pause' → 二次 res.end → write-after-end；
  msg=null 直通丢弃泵又致泵停摆体完不了。**结论：dump 机制需先修流端口
  flowing 排空语义（readable_flow push/flow），独立轮另做**；维持孤儿弃收
  （st.req=null）+ framing 存活口径，dump-req-when-res-ends 维持挂死定级。
  同场钉死：`_consuming` 必须在 `_read` 钩置位（node 258 行），放公共
  read() 会被流机构内部 read(0) 污染（resume_ → read(0)）。
- 坑三（计时归属三混）：①Host 校验在升级检测之前——node parserOnIncoming
  头部对 upgrade 请求 return 0，host 400 只属 pipeline 路径（Host-less
  GET+Upgrade 形不得 400）；②劫持（CONNECT/upgrade）不撤 request/headers
  计时——408 打进已劫持 socket；③headersTimeout 当空闲计时用——node 模型：
  开于连接建立（首消息未启，408 可先于首字节）+ 新消息首字节（残头未齐），
  请求完成即撤；空闲 keep-alive 归 keepAliveTimeout 专管（headers-timeout-
  keepalive 套件：空闲 1.5×headersTimeout 无 408、残头超时 408 照发）。
- 附：killed/中断后台任务后必 `pkill -f winterjs` 清孤儿再跑基线（§4.205
  pgrep 预警的动版）；grep -c 退出码 1（计数 0）会断 `&&` 链——构建检查用
  `|| true` 收尾。
- 坑四（背压两形态不可兼得的解法）：初版"停读+续喂"在两套件间必顾此失
  彼——no-read 要停读事件，flush-drain 要泵永不停（体一次性到齐形，残段/
  终结段扣 fr.buf 等再喂而包不会再有）。终解：**泵不停读不中断，事件改状
  态驱动**（缓冲 ≥HWM 发 'pause' 转换沿、落回 HWM 内发 'resume'，缓冲有界
  =体长）；chunked 泵背压 early-return 撤销（8c 版二分三段实锤元凶）。
  推广：流控事件与流停读是两个正交面，事件可状态驱动，停读必须回答"残段
  谁再喂"。
- 坑五（分离 HEAD 提交）：二分 checkout 后直接 commit 落在 detached HEAD
  （父=旧提交，缺后续修复）——cherry-pick 回 master 解。推广：bisect/checkout
  后先 `git status` 看 HEAD 归属，提交前必 `git log --oneline -1` 核父。
- 本轮战果：no-read-no-dump / server-request-timeout-upgrade / server-
  headers-timeout-keepalive / outgoing-flush-drain / upgrade-large-body-unread
  五件转绿 + should-keep-alive / outgoing-message-capture-rejection 两件
  （见各自提交）；黑盒四件新增，node 域 288 全绿（fifo 按 §4.175 剔除），
  冒烟 5/5，sweep8 终局 379/16/8（五件零红）。
### 4.207 `TMPDIR` 放 U+F8FF 卷即黑盒假红：`URL.pathname` 是百分号编码（2026-09-25，D1 轮）

- 症状：`TMPDIR=/Volumes//wjs-data/tmp cargo test --test node child::` 挂 2 件
  （`envself/fileself false`、`fork-exit false`），默认 `TMPDIR` 全绿。
- 根因：外置盘卷名含 U+F8FF；探针用 `new URL("x.mjs", import.meta.url).pathname`
  取路径——pathname 是百分号编码（`/Volumes/%EF%A3%BF/...`），文件找不到。
  软链 `~/wjs-data` 也躲不过（入口 URL 经 canonicalize，§4.12）。node 同款语义，非实现 bug。
- 修法：`cargo test` 维持系统默认 `TMPDIR`（临时文件小且即删）；只有 node 套件检出 /
  sweep 工件 / 探针等大数据放外置盘（plan3 §0.5 已改）。
- 推广铁律：换数据盘/临时目录前先跑一次含 `import.meta.url` 的黑盒域；路径含非 ASCII
  时 URL→路径一律 `fileURLToPath`。
### 4.208 require 把用户异常转串重抛：位置恒 prelude 424:53、NodeError 空文案、原对象丢失（2026-09-25）

- 症状：`--run x.js`（node 套件几乎全是 CJS）任何未捕获错误都报 `Error: x.js:424:53: …`
  （行列是 prelude `__wjs_require_main` 的调用点）；NodeError 报 `Error: x.js:424:53: `
  空文案；`try { require('./m') } catch (e)` 拿到的是新 `Error`（丢类/code/stack）。
- 根因：`require_cjs_file` 用户代码失败时 `pending_message` 消费异常取文案，native 入口
  `report_error` 新抛一个 Error——异常栈变成重抛点；NodeError 的 message 是 `super()` 后
  defineProperty 的属性，引擎报告里的 message 槽为空。另：CJS 包装头占第 1 行，栈行号恒 +1。
- 修法：用户代码异常留 pending（`KEEP_PENDING` 哨兵 → native 直接 `return false`，吞错
  调用方显式 `take_pending_exception` 清场）；包装编译起始行 0（行号 = 物理行，mapper 去 +1）；
  报错点 `jsapi_glue::fill_message` 从 `message` 属性回填；入口报错抛点不在入口文件时如实
  报其文件名。`__wjs_cjs_compile` 链顺带 §4.80 rooting 修复（cur 裸 JSVal 跨调用）。
- 复现：`tests/node/require.rs::phase11_require_rethrows_original_exception`。
- 推广铁律：宿主转发用户异常一律"留 pending 原样透传"，禁转串再抛；需要文案时读属性
  而非只信引擎报告槽。
### 4.209 node 旗前缀放行 = 自 spawn 无限递归，fork 链吃光内核致系统 panic（2026-09-25，D1 回归）

- 症状：全域基线 sweep（3 并发）跑到 70% 时整机卡死、WindowServer 看门狗超时，
  随后内核 panic 重启（`userspace watchdog timeout: no successful checkins from configd`）。
  panic 快照：323 个 `winterjs` 进程，其中 321 个是**单链**（每个父进程只等一个子进程），
  链根 12:50:12 起、ppid=1（孤儿）；内核 zone `VM map entries 10G`，winterjs 常驻合计 7.4GB。
- 根因：D1 把 `--unhandled-rejections=` 等**前缀族**当 node 旗放行剥除。
  `test-promise-unhandled-flag.js` 用 `spawnSync(execPath, ['--unhandled-rejections=foobar', __filename])`
  验证非法值启动即拒（node exit 9）；本仓剥除后照跑**同一文件**——该文件再 spawnSync 自己，
  无限递归。sweep 超时只杀直接子进程，孙辈成孤儿继续繁殖约 20 分钟，直到内核 VM 耗尽。
- 修法（三道防线）：① 旗名单改**精确表**（`src/cli_node_flags.rs`，取自 `node --help`，剔除
  改执行模式的旗与 winterjs 自有同名旗如 `--allow-ffi`），必须带值的旗只认 `--k=v`，值非法即
  node 同款 exit 9；② 自 spawn 深度闸：起自身时子进程环境 `WINTERJS_SPAWN_DEPTH` 逐层 +1，
  超 32 即拒（变量对 JS 的 `process.env` 不可见）；③ sweep 每件独立进程组，超时/收尾 `killpg`
  连孙辈一起收，daemon 设 `RLIMIT_NPROC = 现有进程数 + 200`。
- 复现：`tests/cli.rs::self_spawn_depth_guard_stops_recursion`（链止于 33 层）+
  `node_runtime_flags_self_spawn_forms`（非法值 exit 9）；原套件现 rc=0。
- 推广铁律：**会让"同一文件再跑一次"的兼容翻译，必须同时回答"非法输入时会不会自递归"**；
  跑任何会 spawn 自身的批量任务前，先给进程数封顶（`ulimit -u` / `RLIMIT_NPROC`），超时一律杀进程组。
### 4.210 rustls/webpki 拒收 X.509 v1 证书：服务端绕 keys_match，客户端验签兜底（2026-09-25，P1）

- 症状：node 套件的 `agent*-cert.pem`（v1）在 `tls/https/http2.createServer` 即报
  `bad key/cert pair (… UnsupportedCertVersion)`；客户端 `ca:` 校验同码失败。基线 37 件同因。
- 根因：webpki 只解析 v3；`ServerConfig::with_single_cert` 经 `CertifiedKey::from_der` 做
  keys_match 时解析终端证书即拒。OpenSSL 照收 v1。
- 修法：服务端 provider 严格加载私钥后以固定解析器直出（`FixedCert`，不做 v1 解析）；
  客户端 `ca:` 路径包一层 `V1FallbackVerifier`——标准 WebPki 报 `UnsupportedCertVersion`
  时手工校验（issuer Name 全等 + 复用 `x509_verify_impl` 验签 + 有效期 + CN 主机名），
  TLS 1.3 握手签名经 SPKI 走 `verify_tls13_signature_with_raw_key`（1.2 无原始公钥变体，记档）。
- 复现：`tests/node/tls.rs::phase11_tls_x509_v1_certificates`（fixtures 用 LibreSSL
  `/usr/bin/openssl x509 -req` 生成——OpenSSL 3 缺省出 v3）。
- 推广铁律：轮子"拒收合法旧格式"时，先确认拒收点是**解析**还是**校验**；解析拒收可在
  出示侧绕开，校验侧兜底必须保住签名/有效期/主机名三件，不许退化成空校验。
### 4.211 修一个结算点会放出一串假绿：beforeExit 缺失 + process.emit 吞错 + 致命错不发 exit（2026-09-26，P2）

- 症状：base13 较 base12 净 −75——`exit` 事件修好后 mustCall 核对首次生效，143 件旧假绿翻红；
  其中 `beforeExit` 监听 9 件整簇不触发、`beforeExit` 抛错被静默吞掉、未捕获异常后 `exit` 监听不跑。
- 根因：① 事件循环排空即退，从未派发 `beforeExit`；② 公开 `process.emit` 与宿主内部派发
  共用吞错的 `__wjs_emit`，监听抛错全被 `catch {}`；③ 致命错直接 `?` 上抛到 main 渲染，
  node `triggerUncaughtException` 尾段（exitCode=1 → 派发 exit → 按 exitCode 退出）缺失。
- 修法：idle 点投递 `beforeExit`（经 nextTick，抛错走 uncaughtException/fatal 同一路由；
  每次排空只发一次，监听排新任务才复位）；公开 `emit` 改 EventEmitter 口径（抛错上抛、
  无监听 `'error'` 即抛），内部派发仍走 `__wjs_emit`；runner 各致命出口经 `fatal_exit`：
  先就地渲染错误（main 登记配色后才启用，testrun/worker 原错透传）再发 exit、按 exitCode 退出。
- 复现：`tests/node/process_.rs::phase11_before_exit_and_fatal_exit_event`。
- 推广铁律：**基线数字下跌先分"假绿现形"与"真退化"**（按红因聚类：`MUSTCALL` 计数不符 vs
  `HANG` 挂死自报）；修结算点类 bug 后必须全量重跑，旧绿数不可信。宿主内部派发与用户可见
  `emit` 必须分开，前者可吞错，后者照 node 上抛。
### 4.212 "同步底座 + 特判"的流实现一改就碎：fs 流改逐字移植，底座时序补两处（fs 回调微任务、setImmediate 钳 1ms）（2026-09-26，P2）

- 症状：fs 流 18/39 件红（mustCall 簇）：mock `fs.read/fs.close`、`options.fs` 自定义、
  `ReadStream.prototype.open` 补丁、destroy(err) 的 error/close 顺序全不对。旧实现是"快照读 +
  live-follow + 续命计数"的特判堆（933 行里 500 行），每条补一个套件。
- 根因：流不经 `this[kFs]` 调 fs（用户 mock 不可见），自带 open/读/关生命周期而非 node 的
  `_construct/_read/_destroy` + kIsPerformingIO/kIoDone；为弥补同步底座又加了续命计数器。
- 修法：`lib/internal/fs/streams.js` 逐字移植（kFs 缺省 = `node:fs` 默认导出对象），删续命
  natives。移植后暴露两处底座时序偏差并修正：① fs 回调 API 在微任务里完成——整条读链在一个
  检查点内跑到 EOF，定时器插不进（node 在后续轮次 poll 相送达）→ 改 `setImmediate` 派发；
  ② `setImmediate` 走 setTimeout 的 1ms 钳——每个 immediate ≥1ms，链式读被 1ms 写端甩开 →
  delay 恰 0 的非 interval 定时器不钳（`fire_due` 快照到期集，回调内新排的顺延一轮 = check 相）。
- 复现：`test-fs-read-stream-pos.js`（写端 1ms 追加，读端跟随）；`tests/builtins.rs` immediate 吞吐用例。
- 推广铁律：**有 node 原文的模块，优先逐字移植 + 修底座，而不是在自写实现上逐套件打补丁**；
  移植后新红多半是底座时序（微任务 vs 宏任务、定时器钳制）偏差，按 node 事件循环相位去对。

### 4.213 io_code 漏 ECONNRESET：拆链收尾错全落 UNKNOWN（2026-09-26，P2-tls-b）

- **症状**：wrap-econnreset 断言 `e.code === 'ECONNRESET'` 拿到 `UNKNOWN`；net 错误形状
  （errno -54、`connect ECONNRESET <target>`）全错。
- **根因**：`fs::io_code` 按 errno 原值映射，只录了常见文件/连接错误——macOS ECONNRESET=54、
  Linux=104 未录（ENOTCONN 57/107 同漏）。RST 类错误是 net/tls 收尾常态，不是边缘。
- **修法**：补 `54 | 104 => "ECONNRESET"`、`57 | 107 => "ENOTCONN"`。
- **复现**：net server `c.end()` + 客户端半关读，读端报错落 UNKNOWN。
- **铁律**：新增 io 错误映射必须**双侧平台 errno**（macOS/Linux）成对录入；新增网络
  断言面（e.code/errno）前先 grep io_code 是否覆盖目标错误。

### 4.214 listen 字符串参误判 UDS path：address().port 全 undefined（2026-09-26，P2-tls-b）

- **症状**：`tls.Server.listen(0, "127.0.0.1")` 后 `address().port === undefined`，全量
  tls/https 黑盒与套件 listen 形崩（连接拨到 `localhost:NaN`）。
- **根因**：给 listen 加 UDS path 支持时，把"非纯数字串"一律当 path（`udsPath = a`）——
  字符串 host（`"127.0.0.1"`、`"localhost"`）被误判；`__udsPath` 残留使 address() 回串。
- **修法**：node isPipeName 口径——**含 "/" 的串才是 path**，否则按 host；
  `address()` 的 UDS 分支只在 `listen(path)` 显式给出时激活。
- **复现**：`server.listen(0, "127.0.0.1", cb); server.address().port` → undefined。
- **铁律**：JS 层多形态参数判别（host vs path vs port）必须引 node `isPipeName` 原文
  判据，禁用"看起来像不像"的宽松启发；改动后须即跑既有黑盒（同域）再继续。

### 4.215 rustls 对 FIN-无-close_notify 严格报错，node/OpenSSL 视为干净 EOF（2026-09-26，P2-tls-b）

- **症状**：对端裸 destroy（RST/FIN 无 TLS close_notify）后，读端泵报
  `Error [UNKNOWN]: peer closed connection without sending TLS close_notify`，
  无监听即崩（test-tls-socket-close / -on-empty）。
- **根因**：tokio-rustls 读端把 bare FIN 映射为 `UnexpectedEof`（io_code 不识 →
  UNKNOWN）；node/OpenSSL 同场景按 TCP EOF 处理——'end' 无 error。
- **修法**：`TlsCleanEof` 读端适配器（tls.rs）——`UnexpectedEof` 且消息含
  `close_notify` → 映射 `Ok(0)`（干净 EOF）；三处 spawn_pumps 读端统一包。
- **复现**：net server `c.end()` 后客户端继续读 TLS 流。
- **铁律**：rustls 严格性与 OpenSSL/node 的宽容语义相悖处（close_notify、X.509 v1、
  hostname 大小写）必须逐一适配层收敛，禁让引擎差异漏到可观察面。

### 4.216 watch 过滤复用 test 表：serve 改 html/css 不触发重启（2026-09-27，CLI --watch 轮）

- 症状：`--serve pub --watch` 跑起后改 `index.html`，无 `restarting` 行、同端口一直回旧内容；
  改 `app.js` 才触发重启。
- 根因：`--serve --watch` 直接复用了 test watch 的 `watchable`（只认代码/JSON 后缀）——
  静态 serve 的被监视物恰恰是 html/css/图等非代码资源，过滤表与监视目标错配。
- 修法：`watch.rs` 拆两层——`ignored()`（node_modules/.git/target + 点文件，三处共用）+
  `watchable()`（test/run 用，后缀表不变）/ `watch_any()`（serve 用，只去噪音不卡后缀）；
  `watch_with(roots, filter)` 可注入，serve 传 `watch_any`。
- 复现：`tests/cli.rs::serve_watch_restarts_child_on_static_change`（改 html 断同端口新内容）。
- 推广铁律：**监视过滤表必须按"被监视物的语言"选，不按"已有表的语言"复用**；新增 watch
  调用点先问"目标目录里什么文件会变"，再定过滤函数。

### 4.217 按调用编译 Regex::new 是启动慢放：CJS 发现 8 正则现场编译烧 1.7s（2026-09-27，F2 轮）

- 症状：vite build-only（179 模块）`cjs_static_names` 累计 1688ms/48 调用（35ms/次），
  `nearest_pkg_type` 45ms/650 调用（逐级读 `package.json` + JSON 解析）；release 同构
  仍是除 prepare/compile 外最大单项（cjs-names 100ms）。
- 根因：① `Regex::new` 在函数内每次调用编译 8 个 pattern，递归每层再编
  （debug 放大，release 亦然）；② `nearest_pkg_type` 无缓存，每文件每轮都 walk。
  XDR 字节码同轮证伪：sm-compile 上限仅 118ms（release）且需新 unsafe，不如修这里。
- 修法：8 正则 `OnceLock` 进程级预编译（`precompiled!`）；pkgtype 按 `package.json`
  路径缓存（mtime 失效，Miss 也缓存 + 存在性校验，watch 安全，原语义逐字保留）；
  `require.rs` 990→1062 行超限，按算法族拆 `require_cjs.rs`（原位重导出）。
  memchr 锚点预检试过——大文件锚点全中、慢文件逐项耗时分毫不差，总量差落噪声带，
  整段回退（不留投机优化）。
- 复现：探针 `WINTERJS_TIMING=1 … --run build-only`（临时，已删；计数见 F2 commit）；
  单测 `nearest_pkg_type_table`（改包文件即验 mtime 失效）；vite dist 哈希断行为一致。
- 推广铁律：**热路径禁现场 `Regex::new`（一律进程级预编译）；纯 fs 判定函数配 mtime
  目录缓存；投机优化必须有同负载前后计数，无 measurable 差即回退**。

### 4.218 concat ESM 具名导出≠默认导出：新类只挂具名即用户面 undefined（2026-09-27，P2-crypto）

- 症状：`test-crypto-classes.js` 报 `invalid 'instanceof' operand crypto[clazz]`、
  `test-crypto-sign-verify.js` 报 `Sign is not a function`——`typeof crypto.Sign`
  实测 `undefined`，而 `Hash/Cipheriv` 正常。
- 根因：本仓 `node:crypto` 由 9 片 JS `concat!` 成同一模块——`crypto_sign.js` 的
  `class Sign` 只存在于模块作用域，`export { Sign }` 挂在 `crypto_pqx509.js`，
  但用户面 `require("crypto")` 拿到的是 `__api` **默认导出表**，该表漏了 `Sign/Verify`
  两门。具名导出绿了，默认表没跟上。
- 修法：`__api` 补 `Sign, Verify`（一行）；`Sign/Verify` 同步改 legacy 函数形
  （无 new 直调，Cipheriv 同款，4.129 坑一再进宫）。附带 `verify-failure` 同根转绿。
- 复现：`tests/node/crypto/asym.rs::p2_crypto_sign_verify_nonew`；
  对拍三件 `getcipherinfo`/`classes`/`verify-failure` 转绿 0。
- 推广铁律：**concat 模块新增可导出的类/函数必须双挂（具名 export + `__api`
  默认表），落地前用真机 `TEST_CASES` 键表逐项 `typeof` 点名**，缺一即此症。

### 4.219 原型污染 setter 探针：native 内部写 JS 层 grep 不到即结构性偏离（2026-09-27，P2-crypto）

- 症状：`test-crypto-sign-verify.js:57` 在 `Object.prototype` 挂 `library` setter
  （写即抛），`createSign('sha1').sign(badPem)` 真机抛 `bye, bye, library`，
  我方抛 `Invalid PKCS#1 key`。
- 根因：真机探针（setter 内打栈）定位写入点在 `node:internal/crypto/sig:147`
  即 `this[kHandle].sign(...)` **native 调用内部**；`grep library internal/crypto/*`
  零命中——写发生在 C++ 层（OpenSSL provider 语境），JS 移植面无此概念，
  RustCrypto 底座亦无对应物，逐字复刻等于伪造实现细节。
- 修法：不修，记档（本条）；该套件后续另需 `Sign` 真流式（`s.end()`），与
  STREAM-PIPE 4 件并案记档 P2-stream。
- 复现：`node -e 'Object.defineProperty(Object.prototype,"library",{set(){...}}); …'`
  逐段二分（create/update 不触发、sign 触发）。
- 推广铁律：**"真机抛 X"先问"写 X 的主体在 JS 还是 native"**——setter 探针打栈，
  栈底落 native 且 JS 全仓 grep 不到同名写，即判结构性偏离（记档不追），
  禁在 JS 层硬塞 dummy 写去"骗过"断言。

### 4.220 自家抛错与真机逐字同形时禁"修正"，修上游误喂（2026-09-27，P2-repl）

- 症状：10 件 `test-repl-*` 报 `input.on is not a function`，第一反应是改自家
  `createInterface` 的抛错分支（改成 ERR_INVALID_ARG_TYPE 等"规范"码）。
- 根因：真机实测 `createInterface({})` 抛的正是无码 `TypeError: input.on is not a function`
  ——逐字同形。错不在抛错，在上游把坏 input 喂了进来（位置形吞参、options 直构、
  双缺未缺省 stdio 三形态）。
- 修法：抛错分支一字不动；修三处上游（Interface 构造器 options 归一、legacy 位置形、
  双缺 stdio）。复验 8 件自绿。
- 复现：任何"自家文案可疑"处，先跑真机同输入再动。
- 推广铁律：**改抛错文案/码前必须真机同输入对照；逐字同形即无罪，往调用链上游找**。

### 4.221 同流自回显即真机亦无限递归：repl 黑盒入出分离（2026-09-27，P2-repl）

- 症状：新黑盒 `p2_repl_legacy_positional` 用同一 PassThrough 既当 input 又当 output，
  输出回显又被当输入求值（每轮添引号），`cargo test` 挂死 150s+（4.140 家族）。
- 根因：输出写入同流即触发 data→求值→输出正反馈；真机同构亦然（recoverable 套件
  靠 noop-write 的 ArrayStream 避开）。
- 修法：黑盒入出分离（位置形 duplex 走 `{ stdin, stdout }` 映射）；判 hang 只认退出码，
  长驻探针输出落盘（4.45/4.93 重申）。
- 复现：`tests/node/repl.rs::p2_repl_legacy_positional` 初版（已改）。
- 推广铁律：**REPL/流黑盒入与出恒分离；同 duplex 必须 noop-write 或断言侧单向**。

### 4.222 补全分支劫持含引号成员行：成员→路径→等号段→拒答→bare（2026-09-27，P2-repl）

- 症状：`obj["one"].toFi` 一直回空，而无引号形正常；`nonExisting.f` 回出
  `nonExisting.fetch`（bare 穿透）。
- 根因：① 未闭合串分支的正则在成员行末引号处误命中（`["one"]` 的关引号被当
  开引号），劫持整行走 fs；② 成员求值失败后穿透到 bare，残段当前缀乱配全局键。
- 修法：分支重排——成员（成形求值失败即拒，不穿透）→路径→等号段→拒答集→bare；
  bare 仅无点行（有点即拒）。
- 复现：`test-repl-tab-complete-computed-props` 全红转全绿；`nonExisting.*` 回空。
- 推广铁律：**多分支派发按"结构确定性"降序排；失败分"解析不成"（另寻他路）与
  "求值不成"（即拒），后者禁穿透**。

### 4.223 allowBlockingCompletions 是 fs 补全面开关，无之回空（2026-09-27，P2-repl）

- 症状：本机探针 `complete('fs.readFileSync("../fixtures/x')` 回空，套件内同调用却有值。
- 根因：套件经 helper 传了 `allowBlockingCompletions: true`；无此旗真机 fs 面回
  `[[], null]`（defer）。另：既存目录如内列子项裸名且 completeOn 置空（反直觉，
  实测为准）。
- 修法：fs 分支镜像真机表（目录→子项裸名+空 completeOn；同级前缀过滤；坏径空）。
- 复现：`test-repl-tab-complete-files`。
- 推广铁律：**探针复刻套件必须连 helper 的透传 options 一起抄**（`startNewREPLServer`
  的 `terminal: true` + `allowBlockingCompletions: true` 皆是行为开关）。

### 4.224 TUI 行编辑替换三坑：管道分流/prompt 拼接/Display 单行（2026-09-28，REPL C 档）

- 症状：rustyline→reedline 后，① 管道黑盒（`tests/repl.rs`）行为漂移；
  ② 提示符打出 `❄ ❄>` 双份；③ 有栈错误仍只打一行。
- 根因：① 旧代码靠 `Editor::new` 失败才退化逐行，新底座 `create()` 常成功，
  不显式分流即把管道当 TTY；② reedline 渲染 `left + indicator` 拼接，
  两边各写一份 `❄> ` 即翻倍；③ `Error` 的 `Display` 是一行格式，
  node 形多行只在 `render()` 里（非 TTY 走 `render_script_node_style`）。
- 修法：`stdin().is_terminal()` 为 false 直接 stdin 逐行（无 ANSI，黑盒原断言不动）；
  `left="❄"` + `indicator="> "` 拼出 `❄> `；REPL 出错走
  `Error::script_with_kind(...).render(color)` 并丢弃退出码（只打印不退出）。
- 复现：`tests/repl.rs::repl_error_prints_stack`（`at f (repl.js:` 帧）；
  `printf ... | winterjs --repl` 管道对照。
- 推广铁律：**换行编辑底座必须三查**：非 TTY 显式 `is_terminal` 分流（禁依赖构造失败退化）、
  prompt 按"左+指示器"拼接规则拼（禁两边各写全形）、报错走 `render()` 不走 `Display`。

### 4.225 读行线程持 raw mode 时他线程直写终端：多行 LF 阶梯 + prompt 竞争（2026-09-27，REPL 渲染修复）

- 症状：TTY REPL 里 ① 打函数体/多行返回值时每行阶梯状右移；② miette 错误框
  碎片散布（框线行首错位、尾部悬空 `┌──`）；③ 报错文本贴在下一轮 prompt 后
  （`❄> winterjs::js::uncaught_exception`）。pty 抓字节证实：多行输出行尾裸 LF 无 CR。
- 根因：读行线程 `read_line` 阻塞时终端处 crossterm raw mode（OPOST/ONLCR 关），
  主循环（求值/console/错误渲染）此时直写终端——LF 只下移不回车即阶梯；且
  readline 线程 send 行后立即回环渲染下一轮 prompt（还发 DSR 光标查询），与慢
  一拍的求值输出竞争。
- 修法：两层。① **哨兵协议**：readline 线程发行后 drain 积压旧哨兵再
  `blocking_recv` 等"本轮输出完毕"哨兵，期间不进 `read_line`（raw 已退，ONLCR
  正常 + prompt 不再抢先）；主循环在行处理轮的 pump/rejection 收尾后发哨兵
  （`pending_flush` 旗，tick 轮不发）。② **CRLF 化**：REPL TTY 会话置
  `REPL_TTY_OUTPUT` 旗，用户输出（console emit）与 REPL 自有打印（`repl_out`/
  `print_completion`/`render_string`）统一裸 `\n`→`\r\n`——覆盖哨兵之后的
  异步窗口（timer 回调里 console.log 多行），ONLCR 开时多出的 `\r` 视觉无害。
  附带：SIGINT 置忽略（哨兵窗口 ISIG 开会直接杀进程，reedline 读时才捕获）。
- 复现：`~/wjs-data/probe/repl_pty.py`（pty 驱动 + DSR 应答，字节比对 CR）；
  黑盒 `tests/node/repl.rs::p2_repl_options_surface`（R4 测试同流 input/output
  踩 4.221 挂死，改分离流复验）。
- 推广铁律：**凡"独占线程持 raw mode 行编辑 + 他线程产输出"的 TUI，输出面
  必须过同步协议或 CRLF 化，禁裸直写终端**；判定用 pty 抓字节（数 CR），
  文本流比对看不出阶梯（LF 在管道渲染里天然对齐）。

### 4.226 全局 console 改原生为 JS 包装后补全文档回落 + trace 空消息冒号（2026-09-28，§7-②）

- 症状：`p2_repl_cli_complete_bridge` 的 `dot-empty` 由 true 翻 false——
  `console.trace` 文档摘要从手写表 `(...data) — stderr + stack` 变成通用
  `trace(...args)`；另 `console.trace()` 无参时自拼 `Trace: ` 与真机 `Trace`
  （无冒号）差一字符。
- 根因：① `__sigDesc` 先抽 `toString` 形参、非空即回落通用形，手写表只在
  形参为空（原生函数）时命中——trace/assert 由原生改 JS 闭包 `(...args)` 后
  命中分支改变，表中 `stderr` 文案永久不可达；② 本引擎
  `Error.captureStackTrace` 不合成 `Name: message` 首行（plain object 上
  stack 为空，见 §7-②探针），trace 首行须自拼，而 V8 空消息时省略 `: `。
- 修法：补全侧——新暴露的 7 个 console 方法进 `__wjsReplSig` 表（原生/空形参
  面仍命中），trace 的桥断言改为通用回落形 `startsWith("trace(")`（桥文档对
  JS 实现面本就按此规则）；trace 侧——`msg === "" ? "Trace" : \`Trace: ${msg}\``。
- 复现：`__wjs_cli_complete("console.")` 的 trace 项；`console.trace()` 双侧对照。
- 推广铁律：**改某内建的实现形态（原生↔JS）时，同步 grep 其 toString 消费者**
  （补全签名/错误文案快照类测试），形态变则文档分支变，旧断言多为过渡态 incidental。

### 4.227 补全手写文档表被 toString 提取永久遮蔽（2026-09-28，§7-②跟进）

- 症状：`console.` 补全浮窗里主方法全显示通用 `log(...args)`，手写表
  `(...data) — stdout` 等通道信息不可见——用户报"补全没有文档"。
- 根因：`__sigDesc` 先抽 `toString` 形参、非空即返回；09-25 起 console 主方法
  是 JS 包装（自带 `(...args)`），表项永久不可达。4.226 只改了测试断言，
  没动优先级，治标。
- 修法：精确 dotted 路径（`console.log`）的表查询提到的 toString 之前；
  Ctor/裸名回落仍在 toString 之后——用户自有同名方法（如 `o.assign(a,b)`）
  不受内建表遮蔽（探针实证）。`dot-empty` 断言回到通道信息形。
- 复现：`__wjs_cli_complete("console.")` 对比改前/后摘要列。
- 推广铁律：**"精确表优先、启发式在后"**——凡手写映射表与运行时反射并存，
  精确键查询永远放反射提取之前；反射只做兜底，否则任何重构（原生→JS、
  改名、包装备案）都会静默降级文档。

### 4.228 嵌进 JS prelude 的文案含撇号：整仓 prelude 解析失败（2026-09-28，`.doc` 轮）

- 症状：`Document's` 进签名表后，`__wjs_prelude.js:5222 SyntaxError`，
  TLA/补全等所有依赖 prelude 的面全挂（黑盒一片红，极易误判为引擎回归）。
- 根因：表值是 JS 单引号字符串字面量，文案里的 `'` 未转义即截断字符串；
  Rust 侧编译期无感（`r#"` 原样收），炸点在运行时首个 prelude 求值。
- 修法：值内 `'` → `\'`（`\\` 先行）；提交前跑冒烟即现形（本轮即冒烟抓包）。
- 复现：任意含撇号文案进 `__wjsReplSig` 后 `--eval '40 + 2'`。
- 推广铁律：**凡进 prelude/loader 内嵌 JS 的外部文案（文档/错误文案/i18n），
  落盘前必须过引号转义检查**；跨语言搬运（MDN→JS 字面量）默认视为脏输入。

### 4.229 具名函数表达式赋给 globalThis 属性不建词法绑定（2026-09-28，本体化轮）

- 症状：补全核心下沉 prelude 后，`__wjs_cli_complete("globalThis.Object.assign")`
  经 `--eval` 对、经 `--run` 文件即空集（`sig[0][0] is undefined`），且
  `import "node:repl"` 与否无关——极易误判为 node 壳冲掉本体。
- 根因：核心以 `globalThis.__wjs_repl_default_complete = function __defaultComplete(...)`
  注册，函数名只在函数体内可见；桥里裸调 `__defaultComplete(...)` 即
  ReferenceError，又被桥内 `try/catch` 吞掉回空集。`--eval` 对是因为当时
  二进制仍是旧构建（未重编），非语义差异——stash/checkout 换代码必重编再探
  （4.62/4.69）同族。
- 修法：桥内经属性取后调用（`const __core = globalThis.__wjs_repl_default_complete`，
  判函数形再调）；定位靠"core 直调对、桥调错"的同文件双探针逐段二分。
- 复现：`--run` 文件内 core/bridge 同参连调（本轮 `/tmp/wjs-probe-o4.mjs` 口径）。
- 推广铁律：**`globalThis.X = function Name` 后一律经 `globalThis.X` 调用**，
  裸 `Name` 只在函数声明（`function Name(){}`）后可用；吞错的 `try/catch`
  桥内，空结果先疑调用点绑定错，不疑被调用方。

### 4.230 prelude 求值期 `process` 尚不存在：快照即死引用（2026-09-29，命名空间轮）

- 症状：`Deno.env.set("K","1")` 后 `process.env.K` 取不到（`undefined`），
  而 `Deno.env.get("K")` 自洽——双 store 分裂。
- 根因：主 PRELUDE 求值先于 NODE_PRELUDE（`process` 挂载在后），顶层
  `const store = process.env || {}` 快照到 `{}` 死对象；之后 `process.env`
  是另一对象。`--eval` 单测若只验自洽（set 后 get）全绿，跨面一读即穿帮。
- 修法：凡读 `process`/`require("node:*")` 的 prelude 逻辑一律懒求值——
  每次调用经 `globalThis` 现场取（`__wjs_ns_envstore`），`require` 只在函数
  体内调，不在顶层。
- 复现：`Deno.env.set("WJS_NS_T","1")` 后读 `process.env.WJS_NS_T`。
- 推广铁律：**prelude 顶层只放纯数据与函数定义，任何宿主对象一律调用期
  现场取**；自洽绿≠跨面绿，断言必须跨面读。

### 4.231 补全要二级展开的值成员禁 getter（2026-09-29，命名空间轮）

- 症状：`Deno.version.` Tab 恒 `NO RECORDS`，而 `Deno.env.`（数据对象）正常；
  冻结与否无关（冻数据对象可补全）。
- 根因：R3 补全核心对成员链逐步求值，getter 一律拒入（防副作用）。
  `version/args/argv` 等写成 getter 即二级死胡同——值对但不可补全。
- 修法：值型成员一律数据属性；`process` 侧活值经 `__wjs_ns_sync()` 在
  NODE_PRELUDE 尾刷新（`node/mod.rs` 调 `__wjs_` 内部面，§7 顺向；用户代码
  之前，无覆盖之忧），`Deno` 刷新后才冻结。标量 getter（`pid` 等）无二级
  可展，不必改。
- 复现：`__wjs_cli_complete("Deno.version.")` 修前 `[]`、修后非空。
- 推广铁律：**凡要 `.x.` 二级补全的值，先问是不是 getter**；补全面只认
  数据属性，活值走启动同步，不走 getter。

### 4.232 工具回"成功"≠落盘：edit 后必 grep 复核再跑构建（2026-09-29，REPL 文档轮）

- 症状：`edit` 連報成功、单测 2 passed，数分钟后 `git status` 不见
  `src/repl_doc.rs` 修改、新建 11 页语料变空目录；`eval` 行为回到旧版。
- 根因：未定位到单一元凶（并行会话同仓改 `Cargo.*`、U+F8FF 卷路径、
  工具/终端两条路径所见不一致皆有可能）；凡"成功了但像没改"，一律按
  未落盘处理——以 `grep`+`git diff --stat` 为准，不以工具回执为准。
- 修法：逐个 `edit` 后当即 `grep` 复核；`git status` 在提交前再看一遍；
  大改分批提交（本轮路由与语料分两提交）；build 前 `git diff --stat`
  确认改动仍在。
- 复现：本轮 `wjs_group_lookup` 三处改动"成功→通过→消失"，重做一次即稳。
- 推广铁律：**构建/提交前先 `git diff --stat` 看改动还在不在**；状态机残留
  先查工作区，不先怪环境（§4.140/4.197 同族）。

### 4.232 编码器色型断言在 nounwind 上下文即 abort（2026-09-29，image 轮）

- 症状：`WinterJS.image.encode(px, "pnm")` 整进程 abort（`Invalid buffer length:
  expected 32 got 16` + `panic in a function that cannot unwind`），非可捕获异常。
- 根因：`image` 各编码器用 `assert_eq!` 校验（通道数×尺寸），色型不匹配
  （ppm/Pixmap 要 RGB 却喂 RGBA、farbfeld 要 16 位、exr 只要 f32）即 panic；
  native 经 `unsafe extern "C"` 进 JS 引擎，panic 不可 unwind，直接 abort。
- 修法：调编码器前**先按目标色型转换**（ppm→RGB、farbfeld→u16、exr→f32），
  把断言变成不可达；凡调第三方 `encode` 系，先 grep 其 `assert` 再定转换。
- 复现：2x2 RGBA 调 pnm/ppm（修前 abort，修后 23 字节 P6）。
- 推广铁律：**进 `unsafe extern "C"` 的第三方调用，`assert`/`panic` 路径
  一律前置校验转干净错误**；黑盒必须含 panic 路径用例（§0.7），且先跑通
  再提交——abort 不留现场。

### 4.233 include_dir 语料加页不触发重编（2026-09-29，文档轮）

- 症状：`winterjs-content/` 加了 4 页后构建"成功"但 `.doc` 仍出父页——新页不在二进制里。
- 根因：`include_dir!` 编译期读目录，但 cargo 只盯源文件 mtime；新增的
  非跟踪文件不触发重编，"成功"的是旧物。
- 修法：加页后 `touch src/repl_doc.rs`（或对应引用页）再编；冒烟里加一条
  新页 `.doc` 即现形。
- 复现：加页 → 直接编 → `__wjs_doc_summary(新主题)` 回父页/None。
- 推广铁律：**凡编译期嵌目录（include_dir!/include_str! 指向目录）加文件，
  必 touch 引用处再编**；`git status` 见新页 + 二进制行为不变先疑此条。

### 4.234 站 luoli 炸了部署不拦（2026-09-29，文档轮）

- 症状：`Failed to load pages/cli-zh.luoli: unmatched }` +
  `pages/api-zh.luoli: reserved word 'native'`——站全白，但 Actions 全绿。
- 根因三合一：① `native` 是 CoffeeScript 保留字（数据数组名撞了）；
  ② cli-zh actions 行尾多 ` }]`（与英文版逐字对即现形）；③ 部署工作流
  只做静态上传，零校验，带病上线。
- 修法：数组改名 `wjs`；括号与英文版逐字对；`pages.yml` 加部署门
  （`scripts/check-luoli.js`，原版 coffeescript 编 coffee: 段，exit 非零即拦）。
  template:/style: 段是 luolita 预处理方言，裸 pug 验不了（缩进基不同），
  只验 coffee + diff 评审。
- 复现：`NODE_PATH=… node scripts/check-luoli.js`（旧文件必 FAIL）。
- 推广铁律：**无校验的部署链等于没有门**；前端 DSL 进仓即配校验脚本，
  今后推站前先等 Actions（用户令）再看站。

### 4.236 async 形态的参数校验必须同步抛（2026-09-29，P2-crypto）

- 症状：`generateKeyPair(type, badOptions, mustNotCall())` 不抛——校验写在
  `queueMicrotask` 回调里，`assert.throws` 抓不到同步异常（MISSING-EXCEPTION 簇）。
- 根因：真机口径是"校验同步抛、生成才排队"（实测 `generateKey('hmac',{length:-1},cb)`
  为同步 `ERR_OUT_OF_RANGE`，callback 根本不背锅）；旧实现把校验和生成一起丢进 microtask。
- 修法：入口先跑全套同步头检（type/options/编码形/算法参数；`__checkKeyPairHead/
  TypeKnown/Encs/RsaKeyOptions`），再 `queueMicrotask` 只做生成。sync/async 双入口
  共用同一套校验函数（`__genPairSync` 内再检一次防直调）。
- 复现：keygen 67/86/303 行（async 校验三连）；改后 sync/async 同形全过。
- 推广铁律：**凡 async 双形态 API，参数校验一律同步抛，microtask 内只留必成功的体力活**；
  写 async 包装时先问"校验在哪"——在回调里即错。

### 4.235 业务层 f32→u8 重解释禁手写 from_raw_parts（2026-09-29，media 轮）

- 症状：`src/builtins/media.rs set_rval_f32` 用
  `unsafe { std::slice::from_raw_parts(out.as_ptr() as *const u8, out.len() * 4) }`
  把 `&[f32]` 重解释成 `&[u8]`——业务逻辑层出现 `unsafe`，违反 AGENTS §6
  （业务层禁 unsafe）与 §0.6（存量只减不增）；注释只有 SAFETY，无 UNSAFE-BOUNDARY 标签。
- 根因：§6 三问第①问即证伪：`bytemuck` 已在 `Cargo.lock` 内（1.25.2，
  `image`/`jxl-oxide`/`resvg` 带入），`bytemuck::cast_slice(out)` 是同语义 safe 写法
  （对齐/长度由类型保证，零拷贝视图）；不愿加依赖时 `to_ne_bytes` 拼 `Vec` 亦可（多一次拷贝）。
- 修法：直引 `bytemuck = "1"`（零新增传递，见 `docs/dependencies.md` §16-6 跟进），
  该行改为 `let bytes: &[u8] = bytemuck::cast_slice(out);`，删业务层 `unsafe`；
  保留的 `TypedArray::<Uint8>::create` 那块 `unsafe` 是 §6 引擎边界（不可去），注释写明。
- 复现：`rg "from_raw_parts" src/builtins/media.rs` 修前命中 1 处，修后 0 处；
  `sample/media/basics.js` 解码能量断言照常绿。
- 推广铁律：**标量切片重解释（f32/u16/i16↔u8）一律走 `bytemuck::cast_slice`，
  禁手写 `from_raw_parts` + 裸指针强转**；凡在锁内已有的纯 Rust 轮子，直引即零成本，
  不要为"省一个直接依赖"造业务层 unsafe。

### 4.237 单例头表勿凭名字猜，须逐行对 matchKnownFields 前缀（2026-09-30，P3-http）

- 症状：`content-encoding`/`x-forwarded-host` 重复头被当单例首个赢，真机却是
  `', '` 合并（`gzip, br` / `a, b`）。
- 根因：`matchKnownFields` 以返回有无 `\u0000` 前缀区分单例/可合并——
  `content-encoding`/`x-forwarded-host` 返回 `\u0000…`（joinable），仅 18 项无前缀
  才是单例；旧表凭"看起来单值"手写，多收两项。
- 修法：单例表删两项并注释列出 18 项来源；`~/wjs-data/probe/dup.mjs` 双侧对拍
  （`authorization:1` 首个赢对照）。
- 复现：raw socket 发双 `Content-Encoding` + 双 `X-Forwarded-Host`，修前丢第二个，
  修后与真机同串。
- 推广铁律：**移植 node 头表一律逐行对 `lib/` 原文前缀，不凭语义猜**（§4.115）。

### 4.238 ServerResponse 缺 OM 品牌位即 finished 不等 close（2026-09-30，P3-http）

- 症状：`test-http-outgoing-finished` 第二个 `finished(res)` 在 `close` 前回调，
  `closed===false`（真机 `finish→close→FIN`，我方 `finish→FIN→close`）。
- 根因：`ServerResponse extends Writable` 未继承 `OutgoingMessage` 品牌四件套
  （`_closed/_defaultKeepAlive/_removedConnection/_removedContLen` + `_sent100`），
  `isOutgoingMessage` 恒 false → `willEmitClose` 恒 false → `finished` 在 `finish`
  即回（真机 `ServerResponse` 无 `_writableState`，走 `!state && isServerResponse` 为 true 等 close）。
- 修法：构造期补五项（初值与 `OutgoingMessage`/`_http_server.js` 同源），
  `writeContinue` 置 `_sent100=true`；`isClosed` 走 `wState` 路径不变，其余消费者仅
  `willEmitClose`（意图内）。
- 复现：`probe/fin.mjs`（`finish→FIN→close` 修前，`finish→close→FIN` 修后，与真机同序）。
- 推广铁律：**凡 `extends Writable` 的 node 消息类，构造期先对 `lib/` 原文品牌字段**；
  `finished` 时序红先查 `willEmitClose`（`_closed` 四件套 + `_sent100`），不先调时序。

### 4.239 argon2 的 async 校验必须同步抛 + 缺参/文案逐字对（2026-09-30，P2-crypto）

- 症状：`test-crypto-argon2` 首败 `Missing expected exception`（9 条坏向量同步全抛，
  异步坏参却进回调不抛）；修后连环三败（缺参错码/文案/算法缺参错码）。
- 根因三合一：① async 把参数校验丢进 microtask（与 4.236 同族：真机校验同步抛，
  生成才排队）；② `Number(undefined)=NaN` 致缺参落区间门（OUT_OF_RANGE），真机为
  INVALID_ARG_TYPE；③ `passes` 下限手写 0（真机 ≥1）+ nonce 短文案自造 +
  算法缺参/错值不分码（真机缺参 INVALID_ARG_TYPE、错值 INVALID_ARG_VALUE）。
- 修法：JS 层 `passes` 改 1 起；`intArg` 首检 `undefined` 即 INVALID_ARG_TYPE；
  `nonce` 文案改 `The value of "parameters.nonce.byteLength" is out of range…`；
  算法分两支（非 string 即 TYPE、错值即 VALUE）；async 先同步跑全套
  `__argon2Args` 再验回调（顺序先参数后回调，真机坏参+无回调即 OUT_OF_RANGE）。
- 复现：`probe/argon2async.mjs`（修前 async 坏参 NO-THROW，修后与真机同码）；
  全坏向量 `argon2bad2.mjs` 双侧 9/9 同码；`run1.sh test-crypto-argon2.js` 0。
- 推广铁律：**KDF/密钥系 async 包装先问"校验在哪"（4.236）；区间门前先拦
  `undefined`（NaN 会偷渡错码）；文案/分码一律真机逐项实测，不凭记忆拼**。

### 4.240 Cipher 系输出编码门缺失：首编码粘住 + 未知即抛（2026-09-30，P2-crypto）

- 症状：`test-crypto-encoding-validation-error` 四断言全 NO-THROW（换编码/final
  异编码/坏编码名全吞）。
- 根因：`__outBuf`（hash 侧复用）未知编码吞回 Buffer，而 cipher 真机口径是独立
  `getDecoder` 状态机：首个非 buffer 输出编码粘住（`utf-8` 归一 `utf8`），再换即
  `ERR_INVALID_ARG_VALUE`（`cannot be changed from 'utf8'`），未知即
  `ERR_UNKNOWN_ENCODING`，`buffer`/缺省不粘。
- 修法：cipher 侧新 `__cipherOut(inst,…)` + 实例 `__decoder`（Cipher/Decipher 四处
  `update/final` 全换；hash 侧 `__outBuf` 不动——digest 真机即吞码口径，见既有记档）。
- 复现：`probe/encval.mjs` 四项双侧同码；`run1.sh` 0。
- 推广铁律：**同名辅助跨域复用先对真机口径**——hash 与 cipher 的"非法编码"语义
  相反（吞 vs 抛），复用即错；新事件/状态门一律实例级存放，不放模块级。

### 4.241 pkcs8 加密导出须走 PBES2 + AKP JWK + DER 嗅探三件套（2026-09-30，P2-crypto）

- 症状：`test-crypto-pqc-encrypted-pkcs8` 三连败——①导出 `PRIVATE KEY` 明文/传统
  PEM（真机 `ENCRYPTED PRIVATE KEY`）；② fixture JWK `Unsupported JWK kty`
  （`AKP` 未实现）；③ 自家 DER 加密体导入 `Invalid PKCS#8`（DER 无标签不嗅探）。
- 根因：pkcs8+口令导出误复传统 PEM 路径（那只属 pkcs1/sec1）+ DER 完全不走
  cipher；AKP（`{kty,alg,priv,pub}`）是 PQC 新面；导入只认 PEM 标签。
- 修法：新 `__pbes2Encrypt`（PBKDF2-SHA256/2048/8B 盐 + AES/DES + PRF 带 NULL，
  与 `__pbes2Decrypt` 对称，fixture/OpenSSL 互解）；pkcs8 私钥 +cipher 的
  der/pem 同走 PBES2（pem 标签 `ENCRYPTED PRIVATE KEY`；pkcs1/sec1 传统路径不动）；
  AKP 分支（alg 表 6 集 + 种子形 PKCS#8 自拼 + 既有展开派生比对 pub，零新 native；
  文案 `Unsupported JWK AKP "alg"`/`Invalid JWK AKP key`/无 priv 三形逐字对真机）；
  DER 显式 pkcs8 + 口令 + PBES2 OID 嗅探先解密（`__sniffPbes2`）。
- 复现：自回环 pem/der 双绿 + fixture 双绿 + 错口令 `ERR_OSSL_BAD_DECRYPT`
  （与真机同码）；`run1.sh` 0。
- 推广铁律：**加解密对必须同批落地验双向**（解密先行、导出后补是半拉子）；
  新 `kty`/`alg` 面先查 fixture 再写码；DER 无标签面一律配嗅探，不只认显式 type。

### 4.242 async 包装调两次生成器即公钥私钥错配（2026-09-30，P2-crypto-R4）

- 症状：crypto3 keygen-async 簇十余件同红——x-JWK 公私钥错配、RSA 解密失败、
  DSA 超时，表象各异。
- 根因：异步 `generateKeyPair` 回调里调了**两次** `__genPairSync`（公钥取自
  第一次结果、私钥取自第二次）——两对毫不相干的键；另附带双倍慢（DSA 2048
  两次生成即超时）。
- 修法：microtask 内单次生成再分发编码（`const out = __applyEncoding(
  __genPairSync(...)); cb(null, out.publicKey, out.privateKey)`）；R4 一并转绿 22 件。
- 复现：`generateKeyPair('ed25519', {jwk/jwk}, cb)` 对比 `publicKey.x ===
  privateKey.x`（修前恒 false，修后 true）。
- 推广铁律：**凡"一次调用产一对"的 async 包装，生成器只调一次**；同簇多件同红
  先疑共享上游（本轮另例：jwk 免 type/paramEncoding 翻正，一改带走 6 件）。

### 4.243 全仓机械改名 checklist（2026-09-30，winterjs→winterjs2）

- 症状：改名后构建/测试多处挂——`include_str!` 路径（assets/svg、content 语料目录
  先搬）、env 前缀裸串（`with_prefix("WINTERJS")` 无下划线被 pattern 漏过）、
  前缀长度硬编码（`slice(11)` 随 `__wjs_prim:`→`__wjs2_prim:` 变长失效）、
  fixture 数据被改名（签名向量绑定的消息串）、`CARGO_BIN_EXE_<旧名>`/insta
  快照名随包名变。
- 根因：① 改名分多遍跑时 pattern 缺"已改名"守卫会叠加（`__wjs2`→`__wjs22`）；
  ② `rg` 不支持 lookahead——"零残留"校验必须换写法（字符类），不可信其空输出；
  ③ 占位符本身含可匹配词会被二次改名。
- 修法：单遍脚本（ alternation + 全守卫：`__wjs(?!2)` 等）+ 占位符用无意义串；
  校验用 `rg -e "X([^2]|$)"` 字符类写法；改名前列禁区（上游 vendor/Bun/Deno/MDN
  语料、历史文档、第三方链接/域名/标准格式名）。
- 复现：本轮 `__wjs222`/`WinterJS22` 三连击 + `rg` lookahead 静默失败。
- 推广铁律：**批量改名前先列"禁区 + 守卫 pattern + 校验命令"三件套**；fixture
  数据与前缀长度常量逐个过目，不进机械替换。

### 4.244 改名后工具链默认二进制路径漏网：sweep 跑的仍是旧名 stale 件（2026-10-03，P2-process）

- 症状：process 首簇 `--eval` 探针全绿，`run1.sh` 五件却全红且报错全是旧行为
  （`can't convert undefined to BigInt`、`process.release is undefined`）——
  探针与套件用了两个二进制。
- 根因：改名只改了仓内码，`scripts/sweep-bg.py` 默认 `target/debug/winterjs`
  （glob 精确路径）+ `~/wjs-data/probe/run1.sh` 默认同路径仍指旧名；
  `target/` 下新旧两份 308M 共存，旧件 09-30 02:59 一直被 sweep/run1 命中；
  proc1（21/82）即旧二进制跑出的无效基线。
- 修法：sweep-bg.py 默认路径/pgrep/pkill 全 `winterjs→winterjs2` +
  run1.sh 默认路径/pkill 同改 + 删 stale `target/debug/winterjs`（含 `.d`）；
  重跑 proc2（`target/debug/winterjs2`）21→25/82 方为有效基线。
- 复现：`ls -lh target/debug/winterjs*` 双二进制共存即中招；`status.json` 的
  `wjs` 字段是唯一真相（proc1 指向旧名）。
- 推广铁律：**改名后先 `ls target/debug` 看双二进制，再跑任何 sweep/探针**；
  工具链默认路径与 pgrep/pkill 模式进改名 checklist（4.243）必查项。

### 4.245 macOS 无 RUSAGE_THREAD + errors 端口无 RangeError 子构造（2026-10-03，P2-process-R2）

- 症状：`threadCpuUsage` 在 macOS 无 `RUSAGE_THREAD`（libc apple 只有
  SELF/CHILDREN），`ERR_INVALID_ARG_VALUE.RangeError` 在本仓 errors 端口为
  `undefined`；另 `THREAD_BASIC_INFO` 是 i32 而 flavor 要 u32。
- 根因：① Darwn 线程 CPU 须走 Mach `thread_info`（libc 有 `thread_basic_info`/
  `mach_thread_self`/`thread_info`，独缺 `mach_port_deallocate`）；② 本仓 errors
  宏只建主构造，node 的 `.RangeError`  flavor 不存在。
- 修法：`thread_info(mach_thread_self(), THREAD_BASIC_INFO)` 取 user/system_time
 （`as u32` + 小 extern 块补 `mach_port_deallocate`，§6 三问注释；Linux 走
  `RUSAGE_THREAD`，其余回零记档）；范围错按 `RangeError` 名 +
  `ERR_INVALID_ARG_VALUE` 码 + 真机文案逐字手拼（`The property 'x' is invalid…`）。
- 复现：`run1.sh test-process-threadCpuUsage-main-thread.js` 0；
  `process.cpuUsage({user:-1,system:2})` 与真机三元组逐字同。
- 推广铁律：**拿轮子先 grep 目标符号在该平台真有**（registry 源码为准，不抄文档）；
  errors 新 flavor 先 `--eval typeof` 探存在性，不存在即手拼码名文案三件。

### 4.246 返回码命名空间撞车：EPERM 本体即 1（2026-10-03，P2-process-R3）

- 症状：`seteuid('nobody')`（本机存在，uid 4294967294）报
  `ERR_UNKNOWN_CREDENTIAL` 而非 EPERM；连 `root` 都"不存在"。
- 根因：native 返回码 0=成功/1=未知身份/正数 errno 三义共用一 int——EPERM
  的 errno 本体就是 1，非 root 下所有 set*id 调用的 EPERM 全被 JS 读成"未知身份"；
  探针逐段无辜（JSON 层、getpwnam 均对，`getpwnam("nobody")` 非空），错在编码层。
- 修法：成功 0 / 未知 1 / 失败 `-errno`（setgroups/initgroups 早就是负 errno，
  统一；JS 侧 `r < 0` 取反成错）。
- 复现：`process.seteuid('nobody')` 修前 UNKNOWN 修后 EPERM，与真机逐字同。
- 推广铁律：**凡"成功/分类失败/errno"三义通道，errno 恒走负值**（EPERM=1、
  EINVAL=22 等正数会撞分类码）；先 `id nobody` 确认环境，再疑代码。

### 4.247 inspect 转义表照抄要逐项对 meta（2026-10-03，P2-process-R6 附带）

- 症状：`process.execve(path, ['123', 'abc\0cde'])` 校验文案差一个转义——我方
  `Received 'abc\0cde'`，真机 `Received 'abc\x00cde'`。
- 根因：inspect 转义表 0/7/11/27 四项与真机 `meta` 表不同（我方 `\0`/`\a`/
  `\v`/`\e`，真机 `\x00`/`\x07`/`\x0B`/`\x1B`）；转义是"看起来对"的重灾区，
  肉眼 review 过不了。
- 修法：逐项对 node `lib/internal/util/inspect.js` 的 `meta` 表改四项；
  黑盒 `phase11_inspect_control_escapes` 四断言钉住。
- 复现：`node -e "console.log(require('util').inspect('\0'))"` 即出 `\x00`。
- 推广铁律：**凡"表驱动"的移植（转义/信号/errno/状态码），表按原文逐项 diff**，
  不凭记忆手写；配黑盒逐项钉表。

### 4.248 带 pending 进 JS 调用非法：分发前须 take（2026-10-03，P2-process-R7）

- 症状：入口分发接上后，无接管的入口抛错渲染成 `undefined`（无文案无栈）——
  有接管路径全绿，裸错路径全崩。
- 根因：`dispatch_entry_throw` 里 `call_one` 跑在 pending 未清的状态下；
  SpiderMonkey 带 pending 进 `JS_CallFunctionValue` 非法（静默吞错），后续
  `error_info` 读空。
- 修法：分发内 take-调-放回三段：先 `take_pending_exception` 再调
  `__wjs2_uncaught`；无人接则 `set_pending_exception` 放回，原 fatal 重读
  （与未分发字节一致，stash 对照实锤）。
- 复现：`--run throw-new-Error.js` 修前渲染 `undefined`，修后 `Error: …` + 栈；
  capture 路径 `cap foo` + rc=0 不变。
- 推广铁律：**任何 JS 调用前先确认 pending 已清**（take 即清场语义）；
  新分发点必配"无人接"裸错渲染对照（stash 二进制 40 秒即得）。

### 4.249 Rust set_var 遇空键 panic：node 口径是静默忽略（2026-10-03，P2-process-R8）

- 症状：env.js 全程崩（rc=139）：`failed to set environment variable '""' to
  '""'`，native 帧直接 abort（panic in nounwind 区，连栈都抓瞎）。
- 根因：套件自带 `process.env[''] = ''`（还有 `TEST=` 一行）；Rust `set_var`
  遇空键/`=`/NUL 即 panic，真机（libuv）静默无操作——也不是 throw。
- 修法：native 先拦（空/`=`/NUL，含值 NUL）→ 回 undefined 不写；裸赋值在严格
  模式下亦不抛（套件即裸写后读 undefined）。
- 复现：修前 rc=139，修后 rc=0；定位靠给 native 加临时 eprintln 看调用序列
  （复现后即删；JS 侧包 `__wjs2_env_set` 抓栈亦可，见 R8 过程）。
- 推广铁律：**凡 Rust 标准库会 panic 的前置（set_var/remove？unwrap），native
  入口一律先拦**；`env['']=x` 这类"合法 JS、非法 OS"键，真机行为是忽略不是抛。

### 4.250 SpiderMonkey 时区缓存清不动（2026-10-03，P2-process-R8，记档）

- 症状：`process.env.TZ='Europe/Amsterdam'` 后 `Date` 仍旧区；启动前置 TZ
  则生效（首用即缓存）。
- 根因：SM 另有引擎侧时区缓存（启动/首 Date 即定）；`tzset` 只刷 C 库，
  对 SM 无效；mozjs/mozjs_sys 均无时区重置口（grep 空），ICU 动刀超出边界。
- 修法：env-tz 记档（引擎边界）；`tzset` 保留（POSIX hygiene，无害）。
- 复现：`TZ=... --eval Date` 生效 vs 运行期置 TZ 不生效，两行即判。
- 推广铁律：**"启动生效、运行期不生效" = 引擎侧缓存**，先 grep 绑定层有无
  重置口，无则记档不动（§6 mozjs 是墙）。

### 4.251 SM 删不可配置属性的文案与 V8 不同：Proxy 拦 deleteProperty 回真机形（2026-10-03，P2-process-R9）

- 症状：exit-code-validation 套件 `delete process.exitCode` 断言正则
  `/Cannot delete property 'exitCode' of #<process>/`，本仓抛
  `property "exitCode" is non-configurable and can't be deleted` 即红。
- 根因：exitCode 按真机 `configurable:false` 后，delete 错文案是引擎实现定义——
  SM 与 V8 逐字不同；断言锁的是 V8 形。
- 修法：`globalThis.process` 包一层 Proxy，只拦 `deleteProperty`（exitCode 即抛
  真机形文案），余下默认透传（get/set 经 target；PROTO_FIXUP 的 setPrototypeOf
  亦透传；回归 prototype/ppid/title 全绿）。
- 复现：`--eval 'try{delete process.exitCode}catch(e){console.log(e.message)}'`。
- 推广铁律：**凡断言锁引擎报错文案的，先对真机逐字抠，SM 形不同即在边界层
 （Proxy/包装）对齐，不动引擎**。

### 4.252 uncaught 三段路由：monitor 先行 + 监听再抛即 exit 7 + _fatalException 置空即 6（2026-10-03，P2-process-R9）

- 症状：monitor 套件 stdout 空（仅 monitor 监听时 count=0 致 drain 跳过）；
  exit-code 套件 exitWithThrowInUncaughtHandler 期 7 得 0（旧 `__wjs2_emit`
  吞监听抛错）、exitWithUndefinedFatalException 期 6 得 1。
- 根因（execution.js 原文）：`process.emit('uncaughtExceptionMonitor', er, type)`
  恒先行（throwing）；capture 次之；`emit('uncaughtException')` 抛错冒泡；
  `_fatalException` 置空即 C++ 回落默认 exit 6；monitor/监听抛的新错替代原错
  exit 7。旧实现三处偏离：count 只数 uncaughtException、__wjs2_emit 吞错、
  fatal_exit 无条件盖 1（连预设 99 一并盖掉后又连 7 一并盖掉）。
- 修法：`__wjs2_uncaught_count` 含 capture/monitor；`__wjs2_uncaught` 先 throwing
  版 emit monitor，再 capture，再 throwing 版 emit uncaughtException；
  `_fatalException in p && === undefined` 即置 exitCode 6 回 false；
  Rust 分发抛错（call None + 新 pending）即置 7 并保留新 pending（不恢复原错）；
  fatal_exit 保留 6/7、余下盖 1（含预设码覆盖，exitWithOneOnUncaught 点名）。
- 复现：monitor1/2 fixture 真机 rc=1/7 对拍；`p._fatalException=undefined` 黑盒断 6。
- 推广铁律：**宿主吞错（__wjs2_emit）与用户可见抛错（emit）是两条路，分发链
  上按 node 原文逐段选用**；fatal 收尾的置码须列出保留集（6/7），不得无条件覆盖。

### 4.253 本仓 builtinModules 双形表 vs 真机混合表 + ESM 无 default 即 import.default 失配（2026-10-03，P2-process-R9）

- 症状：get-builtin 套件连环三红：`getBuiltinModule(1)` 走 `String(id)` 前缀拼出
  `node:1` 抛错（应 ERR_INVALID_ARG_TYPE）；`getBuiltinModule('test')` 回模块
  （真机 builtinModules 无裸 `test`，应 undefined）；`node:timers/promises` 的
  `import().default` 为 undefined（无 default 导出）而 require 有值。
- 根因：本仓 `builtin_modules_json` 对每 canonical 发裸名+`node:`双形，真机是
  裸名（老模块）/`node:`专形（test/sqlite/sea/ffi/quic/vfs 新模块）混合；
  `getBuiltinModule` 照抄 require 别名语义即越界；timers/promises 刻意无 default
  致 ESM/CJS 双面不等。
- 修法：`getBuiltinModule` 非串先 ERR_INVALID_ARG_TYPE，裸 test/sqlite/quic/sea/
  ffi/vfs 与 `internal/*` 直回 undefined（require 别名不动）；双形表对新风格
  六件只发前缀形；timers/promises 补 `export default __api`（timers 的
  `import * as promises` 同步改 default 引用，deepStrictEqual 不散）。
- 复现：自写 probe 扫 51 裸名 `getBuiltin===import.default`（修前 1 坏，修后 0 坏）。
- 推广铁律：**"经 require 可达"≠"真机 builtin"**，凡涉及模块名单（builtinModules/
  getBuiltinModule/isBuiltin）以 `node -e builtinModules` 实表为准，不以自家
  注册表为准。

### 4.254 同源双实例状态分叉：internal 门面须重导出锚定公开实例（2026-10-03，P2-stream-R1）

- 症状：`enabledHooksExist` 已在 `node:async_hooks` 导出，eos 内部分支与三个
  finished 套件仍全假——`require('internal/async_hooks')` 读到空状态。
- 根因：`node:async_hooks` 与 `node:internal/async_hooks` 同 SOURCE 字符串但
  各自求值，模块级状态（ALS Map/enable 集）两份分叉；eos 的 AsyncResource
 （公开实例）快照的 ALS 上下文，internal 实例永远看不见。
- 修法：`node:internal/async_hooks` 改门面源（重导出公开实例的
  `enabledHooksExist`，状态锚定一处）；eos 垫片 `internal/async_hooks` 改引
  真门面（移植体 `require('internal/async_hooks')` 原文不动，只改胶水映射）。
- 复现：ALS 测试修前 `false !== true`，修后绿；门面改回同源即复现。
- 推广铁律：**凡模块级 JS 状态（Map/Set/计数器），`node:X` 与
  `node:internal/X` 同源注册即分叉**——后来者一律门面重导出，不再同源注册。

### 4.255 internal 连字符/下划线双拼写：精确优先、失配回落（2026-10-03，P2-stream-R1）

- 症状：套件直引真机形 `internal/streams/end-of-stream` /
  `internal/streams/add-abort-signal`，本仓表内 `end_of_stream` /
  `add_abort_signal`，两件"未映射"红。
- 根因：本仓 internal 表为 Rust 标识符友好用下划线，真机全连字符；
  `normalize_internal` 只做精确匹配。
- 修法：精确命中优先，失配且含 `-` 再试下划线形（zip 系原生连字符精确命中，
  不受影响）；单测断言两例回落。
- 复现：修前两件 `unmapped internal require`，修后绿。
- 推广铁律：**凡"本仓为实现方便改名"的注册表，对外规范名须双向可达**——
  精确优先 + 回落，而非改名即断。

### 4.256 E() 吞变体类：`ERR_X.TypeError` 须逐类挂载（2026-10-03，P2-stream-R2）

- 症状：iter 面 `new ERR_INVALID_STATE.TypeError(...)` 报
  `ERR_INVALID_STATE.TypeError is not a constructor`。
- 根因：自家 `E(sym, val, def, ...otherClasses)` 只处理了
  HideStackFramesError 标记，其余变体类（TypeError/RangeError/URIError）
  被忽略；真机逐类挂 `ErrClass[Clazz.name]`。
- 修法：循环挂载（标记类走 HideStackFrames 分支，余下按名挂载）。
  纯加法：旧 `codes[sym]` 基类不变，既有 `instanceof` 断言不受影响。
- 复现：`new (require('node:internal/errors').codes.ERR_INVALID_STATE.TypeError)('x')`。
- 推广铁律：**移植"注册器"函数（E/defineProperty 循环）须逐行对原文**，
  丢分支即整族面静默缺失（87 个 E() 共用，无单件可观察）。

### 4.257 ESM 环的新边：移植体顶层 import 即建边，懒 require 也救不了（2026-10-03，P2-stream-R2）

- 症状：classic 引入后 `import('node:stream/iter')` 报
  `can't access lexical declaration '"default"'`，栈顶在 duplex:67
  `require('internal/streams/readable')`。
- 根因：classic 为懒函数配了顶层 ESM import（readable/writable），新建
  classic→readable 边；DFS 到达 compose→duplex 时 readable 尚在环中，
  duplex 顶层解构即 TDZ。旧图无此边故一直绿——新边是唯一变量。
- 修法：classic 改 `__reg` 运行时解析（duplex/duplexify 同款），并给
  readable/writable 补 `__reg.set` 尾（加法）；移植体懒函数体不动。
- 复现：直引 classic 即现；摘掉两 import 即消（对照实锤）。
- 推广铁律：**新移植文件的顶层 import 即新边**——凡原文有 lazy-require
  注释（"defer the require"/"avoid circular"）处，一律走 `__reg`
  运行时解析，不建静态边。

### 4.258 同名双实例之外：`instanceof` 跨 realm 恒 false，ArrayBuffer 系须结构判（2026-10-03，P2-stream-R2）

- 症状：cross-realm 套件 `fromSync(crossRealmAB)` 抛
  `Received an instance of ArrayBuffer`——判定说不是，文案说认识。
- 根因：`isArrayBuffer/isAnyArrayBuffer/isSharedArrayBuffer/isDataView/
  isTypedArray` 用 `instanceof`，跨 compartment 恒 false（4.57 本例）。
- 修法：改 `Object.prototype.toString` tag 比对（DataView/TypedArray 同理；
  伪造 tag 误判记档，真机品牌检查无此问题）。
- 复现：`vm.runInNewContext` 造 AB 调 `isAnyArrayBuffer`。
- 推广铁律：**凡 `internal/util/types` 的形态判定，默认写跨 realm 安全形**
  （tag/isView），`instanceof` 出现即 suspicious。

### 4.259 自家黑盒 stale handler：uncaught 监听不摘即交叉开火（2026-10-03，P2-stream-R2）

- 症状：http upgrade 黑盒 `'sim err' !== 'cb boom'`，干净 HEAD 同败，
  真机跑同文件亦败——与实现无关，测试设计缺陷。
- 根因：u4/u6 两块各 `process.on('uncaughtException')` 永不摘除；
  第二个未捕获到时两监听全跑，先注册的断言先炸（真机 emit 同样不捕获，
  照样败）。
- 修法：两处 `on` 改 `once`（意图不变：各收一次即撤）。
- 复现：双运行时同败即实锤测试问题（4.65 先实测再定责的用例）。
- 推广铁律：**一个文件内多个 uncaughtException 断言必须 `once` 或手动摘**；
  新红先双运行时对照，再动手（本轮另两例 node 侧红亦同理记档）。

### 4.260 真机文案 `No such built-in module`：CJS/ESM 同文，裸名/前缀双形（2026-10-03，P2-stream-R2）

- 症状：iter-disabled 套件要求无旗下 `require("node:stream/iter")` 报
  `No such built-in module`，自家报 `Cannot find module ... is not a builtin`。
- 根因：真机 CJS（ERR_UNKNOWN_BUILTIN_MODULE）/ESM 解析器同文案；
  自家三处（resolve/prepare/require）各写各的旧文案；另裸名形仍
  `Cannot find module`（真机同，双形并存）。
- 修法：`node:` 前缀三处统改新文案（`bun:` 沿旧）；`require()` 内
  `node:` 先拦（外层 "Cannot find module" 包裹仍在，正则子串命中）；
  旧黑盒两处按 4.65 翻转。
- 复现：`node -e 'require("node:path/nope")'` 首行即文案。
- 推广铁律：**"经 require 可达"≠文案一致**——缺失路径的文案须对真机逐字抠，
  前缀形/裸形分开断言。

### 4.261 transform 要的是句柄协议不是算法：缓冲式 shim 只译协议（2026-10-03，P2-stream-R3a）

- 症状：`zlib/iter` 全灭（`internalBinding is not defined`，transform 顶层直调）。
- 根因：transform 经裸 `internalBinding('zlib')` 取 C++ 流式句柄（init/
  write/writeSync/close + writeState 双槽 + processCallback）；轮子只有算法
  无此协议层（crates.io 无 Node 私有 ABI 轮子，预期内）。
- 修法：新 `iter_zlib_binding` 模块实现同协议（攒输入、FINISH 整包同步压、
  writeState 分次吐、`processCallback` 微任务回；`ZSTD_e_flush` 按 PROCESS 攒；
  常量取真机值，缺失档补 `zlib.js`）；transform 体逐字不动，头补局部
  `internalBinding` 映射（无全局污染）；另补 `ERR_ZSTD_INVALID_PARAM` +
  `ZSTD_c/d_*` 族（coverage 按名定界）。
- 复现：gzip 回环 31B↔"hello world"；transform×5 + interop/to-readable 转绿。
- 推广铁律：**"轮子只管算法，协议层手写适配"**（tls wrap 同构）；körper 用量
  先数协议动词（init/write/close/回调），再估行数——本例约 200 行。

### 4.262 end-again 归属错文件：OM 行为安到 Writable 头上（2026-10-03，P2-stream-R3b）

- 症状：writable-destroy（node 套件原文）要恒 DESTROYED，与既有
  `state.errored ?? DESTROYED` 偏离行冲突。
- 根因：9 月 http 轮把 OM（ClientRequest/ServerResponse）的 end-again
  同值语义修进了共享的 `Writable.prototype.end`；node 原文该行无条件
  DESTROYED（套件即证）；OM 的 end-again 走自家 `__omErrored` 路（且 http
  已冻结，writableFinished 系既有红另案）。
- 修法：writable_flow 回滚逐字；OM 侧不动（冻结域）。
- 复现：双套件对打即现（一方要恒值一方要已记错，必有一方是错文件）。
- 推广铁律：**共享基类函数的"特例分支"必须有inctance 判据跟行**（本例
  `this.__omErrored`）；裸改共享行即跨域互斥——先问"谁也在调它"。

### 4.263 console 无回调即无 tick：复用 errorHandler 才是真机形（2026-10-03，P2-stream-R3b）

- 症状：samecb-singletick 要 1 次 TickObject init，实测 0 次。
- 根因：自家 Console 调 `stream.write(text)` 无回调（nop 路 needTick 恒假）；
  真机传复用 errorHandler（同对象百次）→ 首写 1 次 nextTick + 合批计数。
- 修法：实例复用空回调（错误仍走 emit，不拦截，行为不变）。
- 复现：包 process.nextTick 计数器，真机 5 写 1 tick。
- 推广铁律：**"无回调"与"复用空回调"在合批语义下不等价**——凡涉及
  nextTick 合批的调用点，回调同一性是可观测行为。

### 4.264 BOM 默认剥：WHATWG 与 fs/StringDecoder 分家（2026-10-03，P2-stream-R3b）

- 症状：preprocess 套件 `readFileSync(..., 'utf8')` 丢 BOM（要保留）。
- 根因：`__fsDecode` 直调 WHATWG TextDecoder（默认剥）；真机 fs 经 Buffer
  路不剥。流式侧同理（StringDecoder 内建 TextDecoder 默认剥）。
- 修法：两处加 `ignoreBOM: true`（readFileSync 侧 + StringDecoder 构造）。
- 复现：BOM 文件 `read(1)` 真机 `"\uFEFF"`，修前 `"a"`。
- 推广铁律：**凡"读文件/流转串"的解码点，先问 BOM 留不留**（WHATWG 默认
  与 Node fs/stream 默认相反）。

### 4.265 process.exit 裸传当回调即 receiver 错位（2026-10-04，base16）

- 症状：cluster-net-listen（worker 内 `net.createServer().listen(process.exit)`）
  `TypeError: this.reallyExit is not a function`（base15 绿 → base16 红）。
- 根因：自家 `exit()` 方法体走 `this.reallyExit`；裸传后 this=server。
  真机 process.exit 与 receiver 无关（C++ 绑定）；此前绿因 worker 内 server
  根本起不来（R 前 listen 即死 → 进程正常退出），stream R 修好 server 后现形。
- 修法：`exit()` 内 receiver 守卫（有 reallyExit 用 this，否则回落
  `globalThis.process`；§4.97 二选一；mock reallyExit 照常走同对象）。
- 复现：`http.Server.listen(process.exit)` 最小形；黑盒 `p2_process_exit_detached_receiver`。
- 推广铁律：**"修好 A 即现形 B"是常态**——base 轮后转红件先问"是不是之前死太早"。

### 4.266 throwDeprecation 同步抛是伪语义：真机 nextTick 异步走 uncaught（2026-10-04，base16）

- 症状：process-warning test4 `assert.fail('Unreachable')` 被触发。
- 根因：自家 `throwDeprecation` 分支同步 `throw`；真机 `node -e` 实测无同步抛，
  警告经 nextTick 异步抛 → uncaughtException 交付。
- 修法：改 nextTick 异步抛（`this.nextTick`，§4.97 this 基）。
- 复现：`node -e` 三行探针；黑盒 `p2_process_warning_throw_deprecation_async`。
- 推广铁律：**"抛"有同步/异步两种，真机探针先定是哪种**（uncaught 类面默认疑异步）。

### 4.267 注释写的"真机实测"与套件矛盾时以套件为准（2026-10-04，base16）

- 症状：crypto-keygen-eddsa（`generateKeyPair('ed25519', cb)` 无 options）
  base15 绿 → base16 红；代码注释称"真机二参即抛"。
- 根因：R4 校验凭一次手误实测写死注释；套件本身在真机绿（`node -e` 三秒可证）。
- 修法：options 缺省即 `{}`；注释按 §4.65 翻转（黑盒 `p2_crypto_keygen_no_options`）。
- 复现：`node -e "require('crypto').generateKeyPair('ed25519',cb)"`。
- 推广铁律：**注释不是证据**——与套件/实测矛盾时注释是错的，先翻转注释再改码。

### 4.268 http2.connect 无视 lookup + 缺 promisify.custom（2026-10-04，base16）

- 症状：promisify-connect-error（自定义 lookup 回错）base15 绿 → base16 红，
  自家报 UNKNOWN DNS 文案（Rust 直拨无视 lookup）。
- 根因两件：① `__start` 直调 `__wjs2_h2_connect`，options.lookup 从未读；
  ② 缺 node internal/http2/core.js 末尾的 `connect[promisify.custom]`
 （once error→reject），致 session error 无人接变 unhandled。
- 修法：`__start` 内 lookup 优先（错原样 error、成拨解析地址）+ 逐字补
  promisify.custom（黑盒 `p2_http2_lookup_and_promisify_custom`）。
- 复现：套件本体即最小形。
- 推广铁律：**"直拨 native"即绕过 node 选项层**——凡 native 直拨面，逐项核对
  options 透传表（lookup/signal/custom promisify 三件最易漏）。

### 4.269 TLS 服务端同字节双派发：单 st 双 __feed（2026-10-04，base16·记档未修）

- 症状：https 迭代四件（default-port/request-agent/url.parse-https.request/
  set-default-ca-precedence-empty）`ERR_HTTP_HEADERS_SENT`——同一请求 handler
  进两次，第二次 writeHead 炸。
- 根因（定位到界为止）：connection=1（单 st），userland data 事件=1，
  但 `__srvDataListener→__feed` 跑两次（第二次 buf 已消费完又拼回同字节重解析；
  plain-http 单派发正常，TLS 服务端独有；JS 层 ingest/flush/listener 皆单投，
  疑 Rust `tls_listen` 派发层同明文推两次）。
- 复现：`probe/dbg/p9-count.js`（3/3 稳定：nc=1 nr=2）。
- 推广铁律：**"userland 只见一次"≠"只派一次"**——复现探针要同时数三层
  （connection/request/userland-data），差值即分层定界。
- 状态：未修（Rust 派发深水 + https/timeout 件另案），本轮记档。

### 4.270 修好即多活：DIFF 快败翻 TIMEOUT 挂死（2026-10-04，base16）

- 症状：base16 新增 TIMEOUT 107，其中 105 在 base15 是 DIFF（`wjs=1` 快败 →
  `wjs=142` 挂死），聚集 cluster×26/http2×40/tls×24。
- 根因：P2 把 server/socket 做"更对"（worker 内 http 可 listen、TLS 握手可过），
  套件多活到"等一个永不到的事件"（cluster 'listening' 中继本就未实现，
  见 cluster.rs 头注；此前 worker 早死 → exit 断言快败）。
- 修法：不修（形态翻转，仍是红；队列口径不变）；真新 hang 仅 2 件
  （dns-channel-timeout/http-catch-uncaughtexception，其中 dns 系 FLAKY）。
- 复现：base15/base16 results.log `TIMEOUT∩DIFF` 交集脚本。
- 推广铁律：**全域基线对比先算"形态翻转矩阵"再算涨跌**——DIFF→TIMEOUT 不是回归，
  是修好的副作用；真回归只看绿→红。
