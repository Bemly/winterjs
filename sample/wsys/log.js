// WinterJS2.log：结构化日志（debug/info/warn/error 四档；非法档位即抛）。
// WinterJS2.log: structured logging (debug/info/warn/error; bad levels throw).
// Run / 运行: winterjs2 --run sample/wsys/log.js
WinterJS2.log.debug('wsys-sample-debug');
WinterJS2.log.info('wsys-sample-info');
WinterJS2.log.warn('wsys-sample-warn');
WinterJS2.log.error('wsys-sample-error');
console.log('[log] bad-level-throws:', (() => { try { WinterJS2.log('verbose', 'x'); return false; } catch { return true; } })());
