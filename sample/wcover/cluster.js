// WinterJS.cluster: primary/worker faces.
// WinterJS.cluster：主/工角色面。
// Run / 运行: winterjs --run sample/wcover/cluster.js
console.log('[cluster] isPrimary:', typeof WinterJS.cluster.isPrimary === 'boolean');
console.log('[cluster] fork:', typeof WinterJS.cluster.fork === 'function');
