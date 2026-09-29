// WinterJS2.db: embedded SQL database (memory).
// WinterJS2.db：内嵌 SQL 数据库（内存库）。
// Run / 运行: winterjs2 --run sample/wcover/db.js
const db = WinterJS2.db.open(':memory:');
db.exec('CREATE TABLE t(a)');
console.log('[db] insert:', JSON.stringify(db.run('INSERT INTO t VALUES (7)', [])));
console.log('[db] query:', JSON.stringify(db.query('SELECT * FROM t', [])));
db.close();
console.log('[db] closed:', true);
