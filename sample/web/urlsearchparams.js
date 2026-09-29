// URLSearchParams: query string building and reading.
// URLSearchParams：查询串的构造与读取。
// Run / 运行: winterjs2 --run sample/web/urlsearchparams.js
const sp = new URLSearchParams('?a=1&a=2&b=%E4%B8%AD');
console.log('[urlsearchparams] getAll(a):', sp.getAll('a').join(','), 'get(b):', sp.get('b'));
sp.append('c', '3');
console.log('[urlsearchparams] serialized:', sp.toString());
console.log('[urlsearchparams] has/size:', sp.has('c') === true && sp.size === 4);
sp.delete('a');
console.log('[urlsearchparams] after-delete:', sp.toString());
