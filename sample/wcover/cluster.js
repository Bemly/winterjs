// WinterJS2.cluster: primary/worker faces.
// WinterJS2.cluster：主/工角色面。
// Run / 运行: winterjs2 --run sample/wcover/cluster.js
console.log('[cluster] isPrimary:', typeof WinterJS2.cluster.isPrimary === 'boolean');
console.log('[cluster] fork:', typeof WinterJS2.cluster.fork === 'function');
