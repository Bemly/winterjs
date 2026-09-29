---
title: "Bun.gc"
slug: Bun/gc
---

Manually trigger the garbage collector
This does two things: 1. It tells JavaScriptCore to run the garbage collector 2. It tells [mimalloc](https://github.com/microsoft/mimalloc) to clean up fragmented memory. Mimalloc manages the heap not used in JavaScriptCore.

## Syntax

```ts
function gc(force?: boolean): void;
```

### Parameters

- `force`
  - : Synchronously run the garbage collector
