---
title: "Bun.sleep"
slug: Bun/sleep
---

Returns a Promise that resolves after the given number of milliseconds, or at the given Date. Like setTimeout, except it returns a Promise.
minimum; it may take longer. Pass a Date to sleep until that time is reached.
## Sleep for 1 second ## Sleep for 10 milliseconds ## Sleep until Date
Internally, Bun.sleep is the equivalent of Bun.sleep and the imported sleep function are interchangeable.

## Syntax

```ts
function sleep(ms: number | Date): Promise<void>;
```

### Parameters

- `ms`
  - : milliseconds to wait before resolving the promise. This is a
