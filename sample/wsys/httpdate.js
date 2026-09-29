// WinterJS.httpdate：HTTP 日期解析与格式化（IMF-fixdate 往返）。
// WinterJS.httpdate: HTTP-date parse and format (IMF-fixdate roundtrip).
// Run / 运行: winterjs --run sample/wsys/httpdate.js
const ms = WinterJS.httpdate.parse('Sun, 06 Nov 1994 08:49:37 GMT');
console.log('[httpdate] parse-ms:', ms === 784111777000);
console.log('[httpdate] roundtrip:', WinterJS.httpdate.format(ms) === 'Sun, 06 Nov 1994 08:49:37 GMT');
