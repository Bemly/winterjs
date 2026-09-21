//! tests/node/testmod.rs — 对齐 src/builtins/node/testmod.rs（node:test）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase4_node_test_runner() {
    // 通过/失败/跳过计数 + 小结 + 失败 exitCode=1。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str("import { test, describe } from \"node:test\";\nimport assert from \"node:assert\";\ndescribe(\"math\", () => {\n  test(\"adds\", () => assert.strictEqual(1 + 1, 2));\n  test(\"fails\", () => assert.strictEqual(1, 2));\n  test.skip(\"skipped\", () => {});\n});\n").unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("not ok - math > fails"), "runner: {stdout}");
    assert!(
        stdout.contains("# pass 1, fail 1, skip 1, todo 0"),
        "summary: {stdout}"
    );
    dir.close().unwrap();
}

#[test]
fn phase10f_test_suite_alias_and_ctx() {
    // suite 别名 + SuiteContext 回调 + fullName 嵌套 + t.test 子测试。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        "import { test, suite } from \"node:test\";\nimport assert from \"node:assert\";\nassert.strictEqual(test.suite, test.describe);\nassert.strictEqual(suite, test.describe);\nsuite(\"outer\", (sctx) => {\n  assert.strictEqual(sctx.fullName, \"outer\");\n  assert.strictEqual(sctx.attempt, 0);\n  sctx.diagnostic(\"hi\");\n  test(\"inner\", async (t) => {\n    assert.strictEqual(t.fullName, \"outer > inner\");\n    await t.test(\"leaf\", (c) => {\n      assert.strictEqual(c.fullName, \"outer > inner > leaf\");\n    });\n  });\n});\n",
    )
    .unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "suite ctx: {stdout}");
    assert!(stdout.contains("# pass 2, fail 0, skip 0, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase10f_test_t_assert_and_register() {
    // t.assert 全键 + ok 源码行 + assert.register 自定义（含覆盖与 this)。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        "import { test, assert as ta } from \"node:test\";\nimport assert from \"node:assert\";\nta.register(\"isOdd\", (n) => { assert.strictEqual(n % 2, 1); });\nta.register(\"context\", function () { return this; });\ntest(\"keys\", (t) => {\n  const keys = Object.keys(t.assert).sort();\n  assert.ok(keys.includes(\"strictEqual\"));\n  assert.ok(keys.includes(\"snapshot\"));\n  assert.ok(keys.includes(\"fileSnapshot\"));\n  assert.ok(!keys.includes(\"AssertionError\"));\n  t.assert.throws(() => t.assert.ok(1 === 2), /t\\.assert\\.ok\\(1 === 2\\)/);\n});\ntest(\"custom\", (t) => {\n  t.plan(2);\n  t.assert.isOdd(5);\n  assert.throws(() => { t.assert.isOdd(4); }, { code: \"ERR_ASSERTION\" });\n});\ntest(\"this\", (t) => {\n  assert.strictEqual(t.assert.context(), t);\n});\n",
    )
    .unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "t.assert: {stdout}");
    assert!(stdout.contains("# pass 3, fail 0, skip 0, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase10f_test_options_tags_plan_waitfor() {
    // options 归一（name/fn 覆盖、单 options 形）+ 超时/并发校验 + tags 继承 +
    // plan 计数 + waitFor 轮询 + getTestContext。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        "import { test, describe, getTestContext } from \"node:test\";\nimport assert from \"node:assert\";\nassert.strictEqual(getTestContext(), undefined);\nassert.throws(() => test({ timeout: \"1\" }), { code: \"ERR_INVALID_ARG_TYPE\" });\nassert.throws(() => test({ timeout: -1 }), { code: \"ERR_OUT_OF_RANGE\" });\nassert.throws(() => test({ concurrency: 0 }), { code: \"ERR_OUT_OF_RANGE\" });\ntest({ timeout: 5 });\ndescribe(\"outer\", { tags: [\"db\"] }, () => {\n  test(\"child\", { tags: [\"DB\", \"x\"] }, (t) => {\n    assert.deepStrictEqual(t.tags, [\"db\", \"x\"]);\n    assert.strictEqual(Object.isFrozen(t.tags), true);\n    const ctx = getTestContext();\n    assert.strictEqual(ctx.name, \"child\");\n  });\n});\ntest(\"overrides\", { name: \"real\", plan: 1, fn: (t) => { t.assert.ok(true); } }, () => { throw new Error(\"shadow\"); });\ntest(\"waiter\", async (t) => {\n  let n = 0;\n  const v = await t.waitFor(() => { if (++n < 3) throw new Error(\"no\"); return \"yes\"; }, { interval: 1, timeout: 5000 });\n  t.assert.strictEqual(v, \"yes\");\n  assert.throws(() => { t.waitFor(5); }, { code: \"ERR_INVALID_ARG_TYPE\" });\n});\n",
    )
    .unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "options: {stdout}");
    assert!(stdout.contains("fail 0"), "summary: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase10f_test_skip_todo_after_hook() {
    // 运行时 skip/todo + 测试级 after（含零子测试）+ plan 失配 fail。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        "import { test } from \"node:test\";\nimport assert from \"node:assert\";\ntest(\"skipped\", (t) => { t.skip(\"later\"); });\ntest(\"todoed\", { todo: true }, () => {});\ntest(\"hooked\", (t) => {\n  t.after(() => { globalThis.__hooked = true; });\n});\ntest(\"plan-bad\", (t) => { t.plan(2); t.assert.ok(true); });\n",
    )
    .unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(1), "plan must fail: {stdout}");
    assert!(stdout.contains("todo - todoed"), "todo: {stdout}");
    assert!(stdout.contains("not ok - plan-bad"), "plan: {stdout}");
    assert!(
        stdout.contains("# pass 1, fail 1, skip 1, todo 1"),
        "summary: {stdout}"
    );
    dir.close().unwrap();
}

#[test]
fn phase10f_test_mock_fn_and_method() {
    // mock.fn 调用记录/覆盖实现/复原 + mock.method 间谍/复原 + 自动复原。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        "import { test } from \"node:test\";\nimport assert from \"node:assert\";\ntest(\"spy\", (t) => {\n  const sum = t.mock.fn((a, b) => a + b);\n  assert.strictEqual(sum(3, 4), 7);\n  assert.strictEqual(sum.mock.calls.length, 1);\n  const call = sum.mock.calls[0];\n  assert.deepStrictEqual(call.arguments, [3, 4]);\n  assert.strictEqual(call.result, 7);\n  assert.strictEqual(call.error, undefined);\n  assert.strictEqual(call.target, undefined);\n  assert.strictEqual(call.this, undefined);\n  assert.strictEqual(sum.mock.callCount(), 1);\n});\ntest(\"impl\", (t) => {\n  const fn = t.mock.fn((a) => a + 1, (a) => a * 2);\n  assert.strictEqual(fn(3), 6);\n  fn.mock.mockImplementation((a) => a * 3);\n  assert.strictEqual(fn(3), 9);\n  fn.mock.resetCalls();\n  assert.strictEqual(fn.mock.callCount(), 0);\n  fn.mock.mockImplementationOnce((a) => a * 10, 0);\n  assert.strictEqual(fn(1), 10);\n  assert.strictEqual(fn(1), 3);\n});\ntest(\"method\", (t) => {\n  const obj = { p: 5, m(a) { return a + this.p; } };\n  t.mock.method(obj, \"m\");\n  assert.strictEqual(obj.m(1), 6);\n  assert.strictEqual(obj.m.mock.calls[0].this, obj);\n  obj.m.mock.restore();\n  assert.strictEqual(obj.m(1), 6);\n  assert.strictEqual(obj.m.mock, undefined);\n});\ntest(\"auto-restore-check\", () => {\n  assert.ok(true);\n});\n",
    )
    .unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "mock fn/method: {stdout}");
    assert!(stdout.contains("# pass 4, fail 0, skip 0, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase10f_test_mock_timers_date() {
    // mock.timers Date 面：替换/推进/复原 + 未启用门。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        "import { test } from \"node:test\";\nimport assert from \"node:assert\";\ntest(\"date\", (t) => {\n  t.mock.timers.enable({ apis: [\"Date\"] });\n  assert.ok(Date.isMock);\n  assert.strictEqual(Date.now(), 0);\n  assert.strictEqual(new Date().getTime(), 0);\n  t.mock.timers.tick(100);\n  assert.strictEqual(Date.now(), 100);\n  t.mock.timers.setTime(500);\n  assert.strictEqual(Date.now(), 500);\n  assert.strictEqual(Date(), new Date(500).toString());\n});\ntest(\"after-reset\", () => {\n  assert.strictEqual(Date.isMock, undefined);\n  assert.ok(Date.now() > 1000);\n});\ntest(\"gates\", (t) => {\n  assert.throws(() => t.mock.timers.setTime(1), { code: \"ERR_INVALID_STATE\" });\n  assert.throws(() => t.mock.timers.enable({ now: -1 }), { code: \"ERR_INVALID_ARG_VALUE\" });\n  assert.throws(() => t.mock.timers.enable({ now: \"x\" }), { code: \"ERR_INVALID_ARG_TYPE\" });\n});\n",
    )
    .unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "mock timers date: {stdout}");
    assert!(stdout.contains("# pass 3, fail 0, skip 0, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase10f_test_mock_timers_scheduler() {
    // mock.timers scheduler.wait 面：tick 推进落定 + 中止拒绝。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.mjs");
    file.write_str(
        "import { test } from \"node:test\";\nimport assert from \"node:assert\";\nimport { scheduler } from \"node:timers/promises\";\ntest(\"wait\", async (t) => {\n  t.mock.timers.enable({ apis: [\"scheduler.wait\"] });\n  const p = scheduler.wait(4000);\n  t.mock.timers.tick(4000);\n  assert.strictEqual(await p, undefined);\n});\ntest(\"abort\", async (t) => {\n  t.mock.timers.enable({ apis: [\"scheduler.wait\"] });\n  const c = new AbortController();\n  const p = scheduler.wait(2000, { signal: c.signal });\n  t.mock.timers.tick(1000);\n  c.abort();\n  await assert.rejects(() => p, { name: \"AbortError\" });\n});\n",
    )
    .unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "mock timers scheduler: {stdout}");
    assert!(stdout.contains("# pass 2, fail 0, skip 0, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase10f_test_run_none_and_plan_gates() {
    // run({isolation:"none"}) 文件加载 + 事件配对 + plan 校验门。
    let dir = assert_fs::TempDir::new().unwrap();
    let probe = dir.child("probe.test.mjs");
    probe
        .write_str(
            "import { test } from \"node:test\";\ntest(\"p-one\", () => {});\ntest(\"p-two\", () => {});\n",
        )
        .unwrap();
    let file = dir.child("t.mjs");
    let script = "import { test, run } from \"node:test\";\nimport assert from \"node:assert\";\ntest(\"driver\", async () => {\n  const stream = run({ files: [\"PROBE\"], isolation: \"none\" });\n  const passes = [];\n  stream.on(\"test:pass\", (d) => passes.push(d.name));\n  let sawStart = 0, sawComplete = 0;\n  stream.on(\"test:start\", () => sawStart++);\n  stream.on(\"test:complete\", () => sawComplete++);\n  for await (const _ of stream);\n  assert.deepStrictEqual(passes.sort(), [\"p-one\", \"p-two\"]);\n  assert.strictEqual(sawStart, 2);\n  assert.strictEqual(sawComplete, 2);\n});\ntest(\"plan-gates\", (t) => {\n  assert.throws(() => { t.plan(1, null); }, { code: \"ERR_INVALID_ARG_TYPE\" });\n  assert.throws(() => { t.plan(1, { wait: \"x\" }); }, { code: \"ERR_INVALID_ARG_TYPE\" });\n  assert.throws(() => { t.plan(\"x\"); }, { code: \"ERR_INVALID_ARG_TYPE\" });\n  assert.throws(() => { run({ testTagFilters: [42] }); }, { code: \"ERR_INVALID_ARG_TYPE\" });\n});\n"
        .replace("PROBE", probe.path().to_str().unwrap());
    file.write_str(&script).unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "run none: {stdout}");
    assert!(stdout.contains("# pass 2, fail 0, skip 0, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase10f_test_run_process_and_expect_failure() {
    // run({isolation:"process"}) worker 传输 + expectFailure 布尔反转。
    let dir = assert_fs::TempDir::new().unwrap();
    let probe = dir.child("probe.test.mjs");
    probe
        .write_str(
            "import { test } from \"node:test\";\ntest(\"w-pass\", () => {});\ntest(\"w-fail\", { expectFailure: true }, () => { throw new Error(\"boom\"); });\n",
        )
        .unwrap();
    let file = dir.child("t.mjs");
    let script = "import { test, run } from \"node:test\";\nimport assert from \"node:assert\";\ntest(\"driver\", async () => {\n  const stream = run({ files: [\"PROBE\"] });\n  const passes = [];\n  let fails = 0;\n  stream.on(\"test:pass\", (d) => passes.push(d.name + \":\" + String(d.expectFailure === true)));\n  stream.on(\"test:fail\", () => fails++);\n  for await (const _ of stream);\n  assert.deepStrictEqual(passes.sort(), [\"w-fail:true\", \"w-pass:false\"]);\n  assert.strictEqual(fails, 0);\n});\n"
        .replace("PROBE", probe.path().to_str().unwrap());
    file.write_str(&script).unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "run process: {stdout}");
    assert!(stdout.contains("# pass 1, fail 0, skip 0, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase10f_test_run_semantics_timeout_plan_wait() {
    // 超时失败 + plan wait 语义（快时钟）。
    let dir = assert_fs::TempDir::new().unwrap();
    let probe = dir.child("probe.test.mjs");
    probe
        .write_str(
            "import { test } from \"node:test\";\ntest(\"slow\", { timeout: 50 }, async () => {\n  await new Promise((r) => setTimeout(r, 5000));\n});\ntest(\"waiter\", async (t) => {\n  t.plan(1, { wait: true });\n  setTimeout(() => { t.assert.ok(true); }, 10);\n});\ntest(\"wait-false\", async (t) => {\n  t.plan(1, { wait: false });\n});\n",
        )
        .unwrap();
    let file = dir.child("t.mjs");
    let script =
        "import { test, run } from \"node:test\";\nimport assert from \"node:assert\";\ntest(\"driver\", async () => {\n  const stream = run({ files: [\"PROBE\"], isolation: \"none\" });\n  const fails = [];\n  stream.on(\"test:fail\", (d) => fails.push(d.name + \":\" + String(d.details.error.failureType)));\n  stream.on(\"test:pass\", (d) => assert.strictEqual(d.name, \"waiter\"));\n  for await (const _ of stream);\n  assert.deepStrictEqual(fails.sort(), [\"slow:testTimeoutFailure\", \"wait-false:testCodeFailure\"]);\n});\n"
            .replace("PROBE", probe.path().to_str().unwrap());
    file.write_str(&script).unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "timeout/wait: {stdout}");
    assert!(stdout.contains("# pass 1, fail 0, skip 0, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase10f_test_run_tag_filter_and_randomize() {
    // 标签过滤 + 随机种子顺序。
    let dir = assert_fs::TempDir::new().unwrap();
    let probe_src = "import { test, describe } from \"node:test\";\ndescribe(\"g\", { tags: [\"db\"] }, () => {\n  test(\"t1\", () => {});\n  test(\"t2\", { tags: [\"x\"] }, () => {});\n});\ntest(\"plain\", () => {});\ntest(\"parent\", (t) => {\n  t.test(\"a\", () => {});\n  t.test(\"b\", () => {});\n  t.test(\"c\", () => {});\n});\n";
    // 两次 run() 用不同文件（同文件二次 import 命中缓存，真机同款语义）。
    let probe = dir.child("probe.test.mjs");
    probe.write_str(probe_src).unwrap();
    let probe2 = dir.child("probe2.test.mjs");
    probe2.write_str(probe_src).unwrap();
    let file = dir.child("t.mjs");
    let script =
        "import { test, run } from \"node:test\";\nimport assert from \"node:assert\";\ntest(\"filter\", async () => {\n  const stream = run({ files: [\"PROBE\"], isolation: \"none\", testTagFilters: [\"db\"] });\n  const passes = [];\n  stream.on(\"test:pass\", (d) => passes.push(d.name));\n  stream.on(\"test:fail\", () => assert.fail(\"no fail\"));\n  for await (const _ of stream);\n  assert.deepStrictEqual(passes.sort(), [\"g\", \"t1\", \"t2\"]);\n});\ntest(\"shuffle\", async () => {\n  const stream = run({ files: [\"PROBE2\"], isolation: \"none\", randomSeed: 1 });\n  const order = [];\n  stream.on(\"test:pass\", (d) => { if (d.name === \"a\" || d.name === \"b\" || d.name === \"c\") order.push(d.name); });\n  for await (const _ of stream);\n  assert.deepStrictEqual(order, [\"b\", \"a\", \"c\"]);\n});\n"
            .replace("PROBE2", probe2.path().to_str().unwrap()).replace("PROBE", probe.path().to_str().unwrap());
    file.write_str(&script).unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "filter/shuffle: {stdout}");
    assert!(stdout.contains("# pass 2, fail 0, skip 0, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}

#[test]
fn phase10f_test_mock_property_and_top() {
    // mock.property 访问记录/复原 + 顶层 mock + 校验报错两件。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("t.cjs");
    file.write_str(
        "const { test, mock } = require(\"node:test\");\nconst assert = require(\"node:assert\");\ntest(\"prop\", (t) => {\n  const obj = { foo: 42 };\n  const prop = t.mock.property(obj, \"foo\", 100);\n  assert.strictEqual(obj.foo, 100);\n  assert.strictEqual(prop.mock.accessCount(), 1);\n  assert.strictEqual(prop.mock.accesses[0].type, \"get\");\n  obj.foo = 200;\n  assert.strictEqual(prop.mock.accesses[1].type, \"set\");\n  prop.mock.restore();\n  assert.strictEqual(obj.foo, 42);\n});\ntest(\"top\", () => {\n  const fn = mock.fn((a) => a + 1, (a) => a - 1);\n  assert.strictEqual(fn(3), 2);\n  mock.reset();\n  assert.strictEqual(fn(3), 4);\n});\ntest(\"errors\", (t) => {\n  assert.throws(() => t.mock.method({ a: 0 }, \"nope\"), { code: \"ERR_INVALID_ARG_VALUE\" });\n  assert.throws(() => t.mock.fn(() => {}, { times: 0 }), /out of range/);\n  assert.throws(() => t.mock.property({}, \"nope\", 1), { code: \"ERR_INVALID_ARG_VALUE\" });\n});\n",
    )
    .unwrap();
    let out = winterjs().arg("--run").arg(file.path()).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "mock prop/top: {stdout}");
    assert!(stdout.contains("# pass 3, fail 0, skip 0, todo 0"), "summary: {stdout}");
    dir.close().unwrap();
}
