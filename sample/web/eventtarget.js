// EventTarget: subscribe, dispatch and once.
// EventTarget：订阅、分发与 once。
// Run / 运行: winterjs --run sample/web/eventtarget.js
const et = new EventTarget();
const seen = [];
et.addEventListener('data', (e) => seen.push(`on:${e.detail.n}`), { once: true });
et.dispatchEvent(new CustomEvent('data', { detail: { n: 7 } }));
et.dispatchEvent(new CustomEvent('data', { detail: { n: 8 } }));
console.log('[eventtarget] once:', JSON.stringify(seen) === '["on:7"]');
const errTarget = new EventTarget();
errTarget.addEventListener('error', (e) => console.log('[eventtarget] error event:', e instanceof Event));
errTarget.dispatchEvent(new Event('error'));
