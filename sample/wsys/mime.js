// WinterJS.mime：扩展名查 MIME（未知后缀回落 octet-stream）。
// WinterJS.mime: extension-to-MIME lookup (unknown falls back to octet-stream).
// Run / 运行: winterjs --run sample/wsys/mime.js
console.log('[mime] png:', WinterJS.mime.lookup('a.png') === 'image/png');
console.log('[mime] html:', WinterJS.mime.lookup('index.HTML') === 'text/html');
console.log('[mime] fallback:', WinterJS.mime.lookup('file.nopeлят') === 'application/octet-stream');
