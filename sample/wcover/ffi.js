// WinterJS2.ffi: bun:ffi alias face.
// WinterJS2.ffi：bun:ffi 别名面。
// 偏离 Deviation: require('bun:ffi') is unavailable in this build, so
// WinterJS2.ffi stays null — import from 'bun:ffi' directly (needs --allow-ffi
// for live dlopen; mirrors sample/bun-ffi/strlen.js).
// Run / 运行: winterjs2 --run sample/wcover/ffi.js --allow-ffi
import { dlopen, ptr } from 'bun:ffi';
import { platform } from 'node:os';

console.log('[ffi] alias-null:', WinterJS2.ffi === null);
const plat = platform();
const libName = plat === 'darwin' ? 'libSystem.B.dylib' : plat === 'linux' ? 'libc.so.6' : null;
if (libName === null) {
  console.log('[ffi] skip: no system libc mapped for', plat);
  process.exit(0);
}
const lib = dlopen(libName, { strlen: { returns: 'u64', args: ['ptr'] } });
console.log('[ffi] strlen:', lib.symbols.strlen(ptr('hello-ffi')) === 9);
