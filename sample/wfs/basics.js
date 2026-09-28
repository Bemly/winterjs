// WinterJS.fs:本体文件面（与 node:fs 分离，直用 fs-err）。
// WinterJS.fs: own file surface (separate from node:fs).
// Run / 运行: winterjs --run sample/wfs/basics.js
await fs.writeFile('.tmp-wfs-hello.txt', 'hello wfs');
console.log('[wfs] text:', await fs.readTextFile('.tmp-wfs-hello.txt') === 'hello wfs');
await fs.writeFile('.tmp-wfs-bin', new Uint8Array([1, 2, 255]));
console.log('[wfs] bin:', Array.from(await fs.readFile('.tmp-wfs-bin')).join(',') === '1,2,255');
console.log('[wfs] stat:', (await fs.stat('.tmp-wfs-hello.txt')).isFile === true);
await fs.mkdir('.tmp-wfs-dir', { recursive: true });
await fs.writeFile('.tmp-wfs-dir/a.txt', 'a');
console.log('[wfs] dir:', JSON.stringify(await fs.readdir('.tmp-wfs-dir')).includes('a.txt'));
console.log('[wfs] winterjs-ns:', WinterJS.fs === fs);
await fs.rename('.tmp-wfs-hello.txt', '.tmp-wfs-renamed.txt');
console.log('[wfs] rename:', (await fs.exists('.tmp-wfs-renamed.txt')) === true);
await fs.copyFile('.tmp-wfs-renamed.txt', '.tmp-wfs-copy.txt');
console.log('[wfs] copy:', (await fs.readTextFile('.tmp-wfs-copy.txt')) === 'hello wfs');
await fs.remove('.tmp-wfs-dir', { recursive: true });
await fs.remove('.tmp-wfs-renamed.txt', {});
await fs.remove('.tmp-wfs-copy.txt', {});
await fs.remove('.tmp-wfs-bin', {});
console.log('[wfs] gone:', (await fs.exists('.tmp-wfs-dir')) === false);
