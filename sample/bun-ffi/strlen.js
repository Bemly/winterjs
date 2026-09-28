// bun:ffi: dlopen libc and call strlen (platform-guarded).
// FFI：打开系统 libc 并调用 strlen（按平台区分）。
// Run / 运行: winterjs --run sample/bun-ffi/strlen.js
import { dlopen, ptr } from 'bun:ffi';
import { platform } from 'node:os';

const plat = platform();
const libName = plat === 'darwin' ? 'libSystem.B.dylib' : plat === 'linux' ? 'libc.so.6' : null;
if (libName === null) {
  console.log('[ffi] skip: no system libc mapped for', plat);
  process.exit(0);
}
const lib = dlopen(libName, { strlen: { returns: 'u64', args: ['ptr'] } });
console.log('[ffi] strlen:', lib.symbols.strlen(ptr('hello-ffi')) === 9);
