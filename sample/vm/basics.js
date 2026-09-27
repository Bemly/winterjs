// node:vm: Script + runInNewContext sandboxing.
// 虚拟机：脚本编译与沙箱求值隔离。
// Run / 运行: winterjs --run sample/vm/basics.js
import vm from 'node:vm';

const script = new vm.Script('a * b + 1');
console.log('[vm] run:', script.runInNewContext({ a: 20, b: 2 }) === 41);

const ctx = vm.createContext({ x: 1 });
vm.runInContext('x += 41', ctx);
console.log('[vm] context mutated:', ctx.x === 42);
console.log('[vm] host not leaked:', vm.runInNewContext('typeof process') === 'undefined');

const fn = vm.compileFunction('return seed + 1', [], { parsingContext: vm.createContext({}) });
console.log('[vm] compileFunction callable:', typeof fn === 'function');
