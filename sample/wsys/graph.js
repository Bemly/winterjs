// WinterJS.graph：有向图（节点/边、拓扑排序、计数；用完 free）。
// WinterJS.graph: directed graphs (nodes/edges, toposort, counts; free when done).
// Run / 运行: winterjs --run sample/wsys/graph.js
const g = WinterJS.graph.create('directed');
const a = WinterJS.graph.addNode(g, 'a');
const b = WinterJS.graph.addNode(g, 'b');
WinterJS.graph.addEdge(g, a, b);
console.log('[graph] toposort:', JSON.stringify(WinterJS.graph.toposort(g)) === JSON.stringify([a, b]));
console.log('[graph] counts:', JSON.stringify(WinterJS.graph.counts(g)) === '[2,1]');
WinterJS.graph.free(g);
console.log('[graph] done:', true);
