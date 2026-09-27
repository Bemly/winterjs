// node:assert (+ strict): ok / equal / deepEqual / throws / rejects.
// 断言：常用断言与异步断言。
// Run / 运行: winterjs --run sample/assert/basics.js
import assert, { strict as strictAssert } from 'node:assert';

assert.ok(true);
assert.strictEqual(1 + 1, 2);
assert.deepStrictEqual({ a: [1, 2] }, { a: [1, 2] });
assert.throws(() => {
  throw new TypeError('nope');
}, TypeError);
assert.doesNotThrow(() => JSON.parse('{"a":1}'));
await assert.rejects(async () => {
  throw new Error('async-boom');
}, /async-boom/);
strictAssert.equal('1', '1');
console.log('[assert] all assertions held');
