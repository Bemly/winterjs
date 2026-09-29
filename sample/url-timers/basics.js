// node:url legacy + node:timers/promises: parse/format/resolve, sleep/once.
//  legacy URL 与 Promise 化定时器。
// Run / 运行: winterjs2 --run sample/url-timers/basics.js
import legacy, { parse, format, resolve } from 'node:url';
import { setTimeout as sleep, setImmediate as immediate } from 'node:timers/promises';

const p = parse('https://example.com:8080/a/b?x=1#h', true);
console.log('[url-timers] legacy host/path/query:', p.host, p.pathname, p.query.x);
console.log('[url-timers] format:', format({ protocol: 'https:', host: 'example.com', pathname: '/a' }));
console.log('[url-timers] resolve:', resolve('https://example.com/a/b', '../c'));
console.log('[url-timers] Url class:', new legacy.Url().protocol === null);

const t0 = Date.now();
await sleep(30);
await immediate();
console.log('[url-timers] slept>=25ms:', Date.now() - t0 >= 25);
