// Event: type/target plus CustomEvent, MessageEvent and CloseEvent.
// Event：类型与目标，含 CustomEvent/MessageEvent/CloseEvent。
// Run / 运行: winterjs --run sample/web/event.js
const e = new Event('build');
console.log('[event] type/bubbles:', e.type === 'build' && e.bubbles === false);
const ce = new CustomEvent('data', { detail: { n: 1 } });
console.log('[event] custom detail:', ce.detail.n === 1 && ce.type === 'data');
const me = new MessageEvent('message', { data: { hello: 1 } });
console.log('[event] message data:', me.data.hello === 1 && me.type === 'message');
const closed = new CloseEvent('close', { code: 1000, reason: 'done' });
console.log('[event] close code:', closed.code === 1000 && closed.reason === 'done' && closed instanceof Event);
