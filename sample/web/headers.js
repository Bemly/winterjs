// Headers: case-insensitive multi-value map.
// Headers：大小写不敏感的多值表。
// Run / 运行: winterjs2 --run sample/web/headers.js
const h = new Headers({ 'content-type': 'text/plain', 'X-A': '1' });
console.log('[headers] get:', h.get('Content-Type') === 'text/plain' && h.get('x-a') === '1');
h.append('x-a', '2');
console.log('[headers] appended:', h.get('x-a') === '1, 2');
console.log('[headers] has/delete:', h.has('x-a') === true && (h.delete('x-a'), h.has('x-a') === false));
console.log('[headers] entries:', [...h.entries()].length === 1);
