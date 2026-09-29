// URLPattern: route-style matching with named groups.
// URLPattern：带命名分组的路由形匹配。
// Run / 运行: winterjs --run sample/web/urlpattern.js
const pat = new URLPattern({ pathname: '/users/:id' });
console.log('[urlpattern] test:', pat.test('https://example.com/users/123') === true);
const m = pat.exec('https://example.com/users/123');
console.log('[urlpattern] group id:', m?.pathname.groups.id === '123');
console.log('[urlpattern] no-match:', pat.test('https://example.com/other') === false);
