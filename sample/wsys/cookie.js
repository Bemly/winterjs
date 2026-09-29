// WinterJS2.cookie：Cookie 解析与序列化。
// WinterJS2.cookie: cookie parsing and serialization.
// Run / 运行: winterjs2 --run sample/wsys/cookie.js
console.log('[cookie] parse:', JSON.stringify(WinterJS2.cookie.parse('a=1; Path=/')) === '{"name":"a","value":"1"}');
console.log('[cookie] serialize:', WinterJS2.cookie.serialize('a', '1', { path: '/', httpOnly: true }) === 'a=1; HttpOnly; Path=/');
console.log('[cookie] bad-name-throws:', (() => { try { WinterJS2.cookie.serialize('bad name', '1'); return false; } catch { return true; } })());
