// 原生覆盖 B2：WinterJS.tcp/udp/dns/tls（Web 风 Promise，loopback 自回环）。
// Run / 运行: winterjs --run sample/wcover/b2.js
const s = await WinterJS.tcp.listen(0, '127.0.0.1');
const c = await WinterJS.tcp.connect('127.0.0.1', s.address().port);
c.write('ping');
for await (const sock of s) {
  for await (const chunk of sock) { sock.write(chunk); sock.end(); break; }
  break;
}
let back = '';
for await (const chunk of c) back += new TextDecoder().decode(chunk);
console.log('[wcover] tcp:', back === 'ping');
c.destroy();
s.close();
const addrs = await WinterJS.dns.lookup('localhost');
console.log('[wcover] dns:', addrs.some((a) => a.address === '127.0.0.1'));
const u = await WinterJS.udp.bind(0, '127.0.0.1');
const v = await WinterJS.udp.bind(0, '127.0.0.1');
u.send('hi', v.address().port, '127.0.0.1');
for await (const m of v) {
  console.log('[wcover] udp:', new TextDecoder().decode(m.data) === 'hi');
  break;
}
u.close();
v.close();
