// TransformStream: chunk-in/chunk-out mapping.
// TransformStream：块进块出的映射。
// Run / 运行: winterjs --run sample/web/transformstream.js
const upper = new TransformStream({
  transform(chunk, c) {
    c.enqueue(String(chunk).toUpperCase());
  },
});
const w = upper.writable.getWriter();
w.write('hello ');
w.write('stream');
w.close();
let text = '';
for await (const chunk of upper.readable) text += chunk;
console.log('[transformstream] transformed:', text === 'HELLO STREAM');
