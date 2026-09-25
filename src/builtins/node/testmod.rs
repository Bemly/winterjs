//! `node:test` 起步（纯 JS，无 natives）：`test/describe(suite)/it` + skip/todo/only +
//! `before/after/beforeEach/afterEach`（套件级 + 测试级 `t.*`）+ `t.test` 子测试 +
//! `t.plan`/`t.waitFor`/`t.assert`（全 assert 键 + snapshot/fileSnapshot 桩 +
//! 模块级 `assert.register` 自定义断言）+ `t.tags`（校验/小写规范/父优先并集/冻结）+
//! `getTestContext`（当前上下文栈，串行泵内跨 setImmediate 有效）+ 名过滤
//! （`WINTERJS_TEST_NAME_PATTERN`：子串或 `/re/flags`，不命中即 skip）。
//! 串行泵，失败记数并置 `exitCode=1`，队空打印小结。
//! 口径（文档记录）：名过滤跳过测试本体与 Each 钩子，before/after 照跑；套件
//! `after` 在整轮末尾按深度由内向外跑；hook 抛错：before 毒化其套件，Each 只
//! fail 当个，after 计 fail。子测试 depth-first 即时跑，父等子齐（`pending`）；
//! 子失败父亦 fail。`suite` 回调同步执行并收 SuiteContext。plan 不匹配即 fail。
//! `run({isolation:"none"})` 同进程文件加载 + 事件流（enqueue/dequeue/start/
//! pass/fail/complete + testId；process 隔离另轮）；`t.mock` 全家经
//! `node:internal/test/mock`（MockTracker + MockTimers）。
//! 超出范围（另轮）：run process 隔离、CLI `--test`、TAP reporter、coverage、
//! top-level only/skip/todo/expectFailure。

/// 内嵌 ESM 源（§0.9 按域分块：`testmod_core.js` 状态/套件/上下文与 runner +
/// `testmod_run.js` 事件流/run/发现，concat 字节恒等；调用点零改）。
pub const SOURCE: &str = concat!(
    include_str!("testmod_core.js"),
    include_str!("testmod_core_runafters.js"),
    include_str!("testmod_run.js"),
);
