// WinterJS 第二批：shell/hex/time/retry/graph/git/oauth/transpile/log/mime/cookie/httpdate。
// WinterJS batch 2: pure-function wheels under one namespace.
// Run / 运行: winterjs --run sample/wsys/basics.js
console.log('[wsys] shell:', WinterJS.shell.expand('~/x') !== '~/x');
console.log('[wsys] hex:', WinterJS.hex.encode('hi') === '6869', Array.from(WinterJS.hex.decode('6869')).join(',') === '104,105');
console.log('[wsys] time:', WinterJS.time.now() > 1700000000000, WinterJS.time.format(0, '%Y-%m-%d') === '1970-01-01');
console.log('[wsys] retry:', WinterJS.retry.delay('constant', 2, { minMs: 100, maxMs: 1000 }) === 100);
console.log('[wsys] graph:', (() => { const g = WinterJS.graph.create('directed'); const a = WinterJS.graph.addNode(g, 'a'); WinterJS.graph.free(g); return a === 0; })());
console.log('[wsys] git:', /^[0-9a-f]{40}$/.test(WinterJS.git.revParse('.', 'HEAD')));
console.log('[wsys] oauth:', (() => { const a = WinterJS.oauth.authorizeUrl({ authUrl: 'https://ex.com/auth', clientId: 'c', redirectUri: 'https://app/cb', scope: 'read', state: 's' }); return a.state === 's'; })());
console.log('[wsys] transpile:', WinterJS.transpile('const x: number = 1;').includes('const x = 1'));
WinterJS.log.info('wsys-sample');
console.log('[wsys] mime:', WinterJS.mime.lookup('a.png') === 'image/png');
console.log('[wsys] cookie:', WinterJS.cookie.serialize('a', '1', { path: '/' }).length > 3);
console.log('[wsys] httpdate:', WinterJS.httpdate.format(WinterJS.httpdate.parse('Sun, 06 Nov 1994 08:49:37 GMT')) === 'Sun, 06 Nov 1994 08:49:37 GMT');
