// Timers + microtasks: setTimeout / setInterval / setImmediate / queueMicrotask ordering.
// 定时器与微任务：setTimeout / setInterval / setImmediate / queueMicrotask 顺序。
// Run / 运行: winterjs --run sample/web/timers.js
const order = [];
order.push('sync');

setTimeout(() => order.push('timeout0'), 0);
setImmediate(() => {
  order.push('immediate');
  // Nested immediate runs after the current one, still before the 10ms timer.
  setImmediate(() => {
    order.push('immediate2');
    clearInterval(iv);
    console.log('[timers] order:', order.join(' > '));
  });
});
queueMicrotask(() => order.push('microtask'));
Promise.resolve().then(() => order.push('promise'));

const iv = setInterval(() => order.push('interval-tick'), 5);
setTimeout(() => order.push('timeout10'), 10);

// clearTimeout on an already-invalid id is a no-op (no throw).
clearTimeout(9_999_999);
console.log('[timers] timeout id type:', typeof setTimeout(() => {}, 1000));
