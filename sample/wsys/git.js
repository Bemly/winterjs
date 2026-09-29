// WinterJS2.git：只读 git（rev 解析与 log；需在 git 检出目录下运行）。
// WinterJS2.git: read-only git (rev parsing and log; run inside a git checkout).
// Run / 运行: winterjs2 --run sample/wsys/git.js
console.log('[git] revParse:', /^[0-9a-f]{40}$/.test(WinterJS2.git.revParse('.', 'HEAD')));
const entries = WinterJS2.git.log('.', 'HEAD', 2);
console.log('[git] log:', Array.isArray(entries) && entries.length > 0 && typeof entries[0].sha === 'string' && typeof entries[0].title === 'string');
