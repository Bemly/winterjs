// Events: EventTarget / Event / CustomEvent / MessageEvent / AbortController.
// 事件：目标、分发、自定义事件与中止信号。
// Run / 运行: winterjs --run sample/web/events.js
const et = new EventTarget();
const seen = [];
et.addEventListener('data', (e) => seen.push(`on:${e.detail.n}`), { once: true });
et.dispatchEvent(new CustomEvent('data', { detail: { n: 1 } }));
et.dispatchEvent(new CustomEvent('data', { detail: { n: 2 } })); // once: ignored
console.log('[events] once seen:', seen.join(','));

const c = new AbortController();
c.signal.addEventListener('abort', () => console.log('[events] abort reason:', String(c.signal.reason)));
c.abort('done');

const me = new MessageEvent('message', { data: { hello: 1 } });
console.log('[events] message data:', me.data.hello, 'type:', me.type);

const errTarget = new EventTarget();
errTarget.addEventListener('error', (e) => console.log('[events] error event:', e instanceof Event));
errTarget.dispatchEvent(new Event('error'));
