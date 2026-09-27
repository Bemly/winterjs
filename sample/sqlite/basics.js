// node:sqlite + bun:sqlite: create / insert / query (in-memory).
// SQLite：建表、插入与查询（内存库，双形态）。
// Run / 运行: winterjs --run sample/sqlite/basics.js
import { DatabaseSync } from 'node:sqlite';
import { Database } from 'bun:sqlite';

const db = new DatabaseSync(':memory:');
db.exec('CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)');
db.prepare('INSERT INTO t (name) VALUES (?)').run('alice');
console.log('[sqlite] node rows:', db.prepare('SELECT name FROM t').all()[0].name === 'alice');
db.close();

const bun = new Database(':memory:');
bun.exec('CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)');
bun.query('INSERT INTO t (name) VALUES (?)').run('bob');
console.log('[sqlite] bun rows:', bun.query('SELECT name FROM t').all()[0].name === 'bob');
bun.close();
