// node:string_decoder: decoding split multi-byte sequences.
// 跨包多字节解码。
// Run / 运行: winterjs2 --run sample/codecs/string-decoder.js
import { StringDecoder } from 'node:string_decoder';

const dec = new StringDecoder('utf8');
const euro = Buffer.from('€');
console.log('[codecs] split decode:', dec.write(euro.subarray(0, 2)) + dec.end(euro.subarray(2)) === '€');
console.log('[codecs] incomplete-tail:', new StringDecoder('utf8').write(euro.subarray(0, 1)) === '');
