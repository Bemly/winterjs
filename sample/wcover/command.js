// WinterJS.command: run and supervise child processes.
// WinterJS.command：运行与监管子进程（输出可收集可流式，需 --allow-run）。
// Run / 运行: winterjs --run sample/wcover/command.js --allow-run
const r = await WinterJS.command.run(process.execPath, ['--eval', '40 + 2']);
console.log('[command] run:', r.code === 0 && new TextDecoder().decode(r.stdout).trim() === '42');
const h = WinterJS.command.spawn(process.execPath, ['--eval', '40 + 2']);
console.log('[command] pid:', h.pid > 0);
console.log('[command] wait:', (await h.wait()).code === 0);
