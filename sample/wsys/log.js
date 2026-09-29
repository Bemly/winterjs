// WinterJS.log：结构化日志（debug/info/warn/error 四档；非法档位即抛）。
// WinterJS.log: structured logging (debug/info/warn/error; bad levels throw).
// Run / 运行: winterjs --run sample/wsys/log.js
WinterJS.log.debug('wsys-sample-debug');
WinterJS.log.info('wsys-sample-info');
WinterJS.log.warn('wsys-sample-warn');
WinterJS.log.error('wsys-sample-error');
console.log('[log] bad-level-throws:', (() => { try { WinterJS.log('verbose', 'x'); return false; } catch { return true; } })());
