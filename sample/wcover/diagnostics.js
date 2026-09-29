// WinterJS2.diagnostics: publish/subscribe channels.
// WinterJS2.diagnostics：诊断通道发布订阅。
// Run / 运行: winterjs2 --run sample/wcover/diagnostics.js
const ch = WinterJS2.diagnostics.channel('wcover-sample');
ch.subscribe((m) => console.log('[diagnostics] message:', JSON.stringify(m) === '{"a":1}'));
ch.publish({ a: 1 });
