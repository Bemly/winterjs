// WinterJS.cookie：Cookie 解析与序列化。
// WinterJS.cookie: cookie parsing and serialization.
// Run / 运行: winterjs --run sample/wsys/cookie.js
console.log('[cookie] parse:', JSON.stringify(WinterJS.cookie.parse('a=1; Path=/')) === '{"name":"a","value":"1"}');
console.log('[cookie] serialize:', WinterJS.cookie.serialize('a', '1', { path: '/', httpOnly: true }) === 'a=1; HttpOnly; Path=/');
console.log('[cookie] bad-name-throws:', (() => { try { WinterJS.cookie.serialize('bad name', '1'); return false; } catch { return true; } })());
