// WinterJS2.udp: datagrams over loopback.
// WinterJS2.udp：回环 UDP 数据报。
// Run / 运行: winterjs2 --run sample/wcover/udp.js
const u = await WinterJS2.udp.bind(0, '127.0.0.1');
const v = await WinterJS2.udp.bind(0, '127.0.0.1');
u.send('hi', v.address().port, '127.0.0.1');
for await (const m of v) {
  console.log('[udp] datagram:', new TextDecoder().decode(m.data) === 'hi');
  break;
}
u.close();
v.close();
