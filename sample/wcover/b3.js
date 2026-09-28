// WinterJS.command/terminal/repl：子进程、终端与 REPL。
// Run / 运行: winterjs --run sample/wcover/b3.js --allow-run
const r = await WinterJS.command.run(process.execPath, ['--eval', '40 + 2']);
console.log('[wcover] run:', r.code === 0 && new TextDecoder().decode(r.stdout).trim() === '42');
const h = WinterJS.command.spawn(process.execPath, ['--eval', '40 + 2']);
console.log('[wcover] pid:', h.pid > 0);
console.log('[wcover] wait:', (await h.wait()).code === 0);
console.log('[wcover] terminal:', typeof WinterJS.terminal.createInterface === 'function');
console.log('[wcover] repl:', typeof WinterJS.repl.start === 'function');
