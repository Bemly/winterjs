// node:test runner: describe / it / subtests (runs under --run and --test).
// 内置测试运行器：分组、用例与子测试。
// Run / 运行: winterjs --run sample/test-runner/basics.js
// Also / 也可: winterjs --test sample/test-runner/
import { describe, it } from 'node:test';
import assert from 'node:assert';

describe('math', () => {
  it('adds', () => {
    assert.strictEqual(20 + 22, 42);
  });
  it('async works', async () => {
    assert.strictEqual(await Promise.resolve(7), 7);
  });
});
console.log('[test] suite registered');
