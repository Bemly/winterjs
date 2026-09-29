// WinterJS.domain: synchronous error routing.
// WinterJS.domain：同步错误路由。
// Run / 运行: winterjs --run sample/wcover/domain.js
const d = WinterJS.domain.create();
let ran = false;
d.run(() => { ran = true; });
console.log('[domain] run:', ran === true);
