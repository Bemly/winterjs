// 原生覆盖 B5：WinterJS.os/path/db/inspect/tty。
// Run / 运行: winterjs --run sample/wcover/b5.js
console.log('[wcover] os:', WinterJS.os.platform(), WinterJS.os.arch());
console.log('[wcover] path:', WinterJS.path.join('a', 'b'));
const db = WinterJS.db.open(':memory:');
db.exec('CREATE TABLE t(a)');
console.log('[wcover] db:', JSON.stringify(db.run('INSERT INTO t VALUES (7)', [])));
console.log('[wcover] query:', JSON.stringify(db.query('SELECT * FROM t', [])));
db.close();
console.log('[wcover] inspect:', WinterJS.inspect.evaluate('1+1') === 2);
console.log('[wcover] tty:', typeof WinterJS.tty.isTTY() === 'boolean');
