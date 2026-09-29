// WinterJS2.httpdate：HTTP 日期解析与格式化（IMF-fixdate 往返）。
// WinterJS2.httpdate: HTTP-date parse and format (IMF-fixdate roundtrip).
// Run / 运行: winterjs2 --run sample/wsys/httpdate.js
const ms = WinterJS2.httpdate.parse('Sun, 06 Nov 1994 08:49:37 GMT');
console.log('[httpdate] parse-ms:', ms === 784111777000);
console.log('[httpdate] roundtrip:', WinterJS2.httpdate.format(ms) === 'Sun, 06 Nov 1994 08:49:37 GMT');
