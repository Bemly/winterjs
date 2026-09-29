// WinterJS2.AsyncLocalStorage: context through timers.
// WinterJS2.AsyncLocalStorage：跨定时器的上下文。
// Run / 运行: winterjs2 --run sample/wcover/asynclocalstorage.js
const als = new WinterJS2.AsyncLocalStorage();
console.log('[als] run:', als.run('v', () => als.getStore()) === 'v');
