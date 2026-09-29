// fetch: network requests (offline: data: URLs).
// fetch：网络请求（离线用 data: URL）。
// Run / 运行: winterjs2 --run sample/web/fetch.js
const res = await fetch('data:text/plain,hello-fetch');
console.log('[fetch] status/ok:', res.status === 200 && res.ok === true);
console.log('[fetch] text:', await res.text() === 'hello-fetch');
const json = await (await fetch('data:application/json,{"n":7}')).json();
console.log('[fetch] json.n:', json.n === 7);
