// WinterJS2.domain: synchronous error routing.
// WinterJS2.domain：同步错误路由。
// Run / 运行: winterjs2 --run sample/wcover/domain.js
const d = WinterJS2.domain.create();
let ran = false;
d.run(() => { ran = true; });
console.log('[domain] run:', ran === true);
