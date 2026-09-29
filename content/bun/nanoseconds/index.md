---
title: "Bun.nanoseconds"
slug: Bun/nanoseconds
---

Returns the number of nanoseconds since the process was started, measured with a high-resolution monotonic system timer.
JavaScript numbers are IEEE 754 doubles, which represent integers exactly only up to 2^53 - 1 (Number.MAX_SAFE_INTEGER). After about 14.8 weeks of uptime the nanosecond count exceeds that, so the returned value keeps counting but loses precision.

## Syntax

```ts
function nanoseconds(): number;
```
