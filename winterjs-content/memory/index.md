---
title: "WinterJS.memory"
slug: WinterJS/memory
---

The **`WinterJS.memory()`** method returns the observable memory info of the
current process: `rss` (via sysinfo), the allocator kind (`smmalloc` on
desktop, `talc` on mobile), and zeroed heap fields (cross-engine numbers are
not comparable, same as `process.memoryUsage`).

## Syntax

```js
WinterJS.memory()
```

### Parameters

- `memory`
  - : Takes no parameters; returns `{ rss, allocator, heapTotal, heapUsed }`.
