// WinterJS.AsyncLocalStorage: context through timers.
// WinterJS.AsyncLocalStorage：跨定时器的上下文。
// Run / 运行: winterjs --run sample/wcover/asynclocalstorage.js
const als = new WinterJS.AsyncLocalStorage();
console.log('[als] run:', als.run('v', () => als.getStore()) === 'v');
