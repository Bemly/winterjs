// WinterJS2.mime：扩展名查 MIME（未知后缀回落 octet-stream）。
// WinterJS2.mime: extension-to-MIME lookup (unknown falls back to octet-stream).
// Run / 运行: winterjs2 --run sample/wsys/mime.js
console.log('[mime] png:', WinterJS2.mime.lookup('a.png') === 'image/png');
console.log('[mime] html:', WinterJS2.mime.lookup('index.HTML') === 'text/html');
console.log('[mime] fallback:', WinterJS2.mime.lookup('file.nopeлят') === 'application/octet-stream');
