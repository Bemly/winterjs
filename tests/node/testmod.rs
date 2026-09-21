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
