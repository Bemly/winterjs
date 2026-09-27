// node:util (+ node:sys alias, node:util/types): format / inspect / promisify.
// 工具：格式化、检视、回调转 Promise（sys 为同实例别名）。
// Run / 运行: winterjs --run sample/util/basics.js
import util, { format, inspect, promisify } from 'node:util';
import sys from 'node:sys';
import { isMap, isPromise } from 'node:util/types';

console.log('[util] format:', format('%s=%d %j', 'a', 1, { x: 1 }));
console.log('[util] inspect depth:', inspect({ a: { b: { c: 1 } } }, { depth: 1 }).includes('[Object]'));
console.log('[util] sys alias:', sys === util);

function addCb(a, b, cb) {
  setImmediate(() => cb(null, a + b));
}
console.log('[util] promisify:', (await promisify(addCb)(20, 22)) === 42);
console.log('[util] types:', isMap(new Map()) && isPromise(Promise.resolve()));
console.log('[util] deprecate callable:', typeof util.deprecate(() => 1, 'old')());
