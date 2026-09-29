// WinterJS2.graph：有向图（节点/边、拓扑排序、计数；用完 free）。
// WinterJS2.graph: directed graphs (nodes/edges, toposort, counts; free when done).
// Run / 运行: winterjs2 --run sample/wsys/graph.js
const g = WinterJS2.graph.create('directed');
const a = WinterJS2.graph.addNode(g, 'a');
const b = WinterJS2.graph.addNode(g, 'b');
WinterJS2.graph.addEdge(g, a, b);
console.log('[graph] toposort:', JSON.stringify(WinterJS2.graph.toposort(g)) === JSON.stringify([a, b]));
console.log('[graph] counts:', JSON.stringify(WinterJS2.graph.counts(g)) === '[2,1]');
WinterJS2.graph.free(g);
console.log('[graph] done:', true);
