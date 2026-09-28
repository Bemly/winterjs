---
title: "Bun.sleepSync"
slug: Bun/sleepSync
---

Block the thread for a given number of milliseconds.
Internally, it calls [nanosleep(2)](https://man7.org/linux/man-pages/man2/nanosleep.2.html)

## Syntax

```ts
function sleepSync(ms: number): void;
```
