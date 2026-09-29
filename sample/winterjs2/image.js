// WinterJS2.image: decode to RGBA8 pixels, encode back (all offline).
// WinterJS2.image：解码成 RGBA8 像素、再编码回去（全离线）。
// Run / 运行: winterjs2 --run sample/winterjs2/image.js
const red2x2 = { data: new Uint8Array([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255]), width: 2, height: 2 };

console.log('[image] formats:', WinterJS2.image.formats().filter((f) => f.encode).map((f) => f.name).join(','));

// PNG round-trip keeps pixels byte-identical.
const png = WinterJS2.image.encode(red2x2, 'png');
const back = WinterJS2.image.decode(png);
console.log('[image] png:', back.format === 'png' && back.width === 2 && back.data.join(',') === red2x2.data.join(','));

// Quality params ride through: jpeg quality, png compression+filter, gif repeat.
const jpg = WinterJS2.image.encode(red2x2, 'jpeg', { quality: 90 });
console.log('[image] jpeg:', WinterJS2.image.decode(jpg, 'jpeg').format === 'jpeg');
const best = WinterJS2.image.encode(red2x2, 'png', { compression: 'best', filter: 'paeth' });
console.log('[image] png-opts:', best.length > 0);
const gif = WinterJS2.image.encode(red2x2, 'gif', { speed: 10, repeat: 0 });
console.log('[image] gif:', WinterJS2.image.decode(gif).format === 'gif');

// Vector: inline SVG rasterizes (scale 2 = 8x6), info() reads dims without pixels.
const svg = new TextEncoder().encode('<svg xmlns="http://www.w3.org/2000/svg" width="4" height="3"><rect width="4" height="3" fill="red"/></svg>');
const v = WinterJS2.image.decode(svg, 'svg', 2);
console.log('[image] svg:', v.format === 'svg' && v.width === 8 && v.height === 6 && v.data[0] === 255);
console.log('[image] info:', JSON.stringify(WinterJS2.image.info(svg)));
