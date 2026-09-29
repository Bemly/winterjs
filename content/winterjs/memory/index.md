---
title: "WinterJS.memory"
slug: WinterJS/memory
---

The **`WinterJS.memory()`** method returns the observable memory info of the
current process: `rss` in bytes, the allocator name, and zeroed heap
fields (heap numbers depend on the engine and are not comparable, same as
`process.memoryUsage`).

## Syntax

```js
WinterJS.memory()
```

### Parameters

- `memory`
  - : Takes no parameters; returns `{ rss, allocator, heapTotal, heapUsed }`.
