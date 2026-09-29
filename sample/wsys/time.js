// WinterJS2.time：毫秒时钟、ISO 解析与 strftime 风格格式化（UTC）。
// WinterJS2.time: millisecond clock, ISO parsing, strftime-style format (UTC).
// Run / 运行: winterjs2 --run sample/wsys/time.js
console.log('[time] now-ms:', WinterJS2.time.now() > 1700000000000);
console.log('[time] parse:', WinterJS2.time.parse('2026-01-02T03:04:05Z') === Date.parse('2026-01-02T03:04:05Z'));
console.log('[time] format-date:', WinterJS2.time.format(0, '%Y-%m-%d') === '1970-01-01');
console.log('[time] format-datetime:', WinterJS2.time.format(0, '%Y-%m-%d %H:%M:%S') === '1970-01-01 00:00:00');
