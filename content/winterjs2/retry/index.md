---
title: "WinterJS2.retry"
slug: WinterJS2/retry
---

The **`WinterJS2.retry`** property provides retry utilities with computed backoff: `delay` computes the wait, `run` awaits an async function with `setTimeout` waits.

## Syntax

```js
WinterJS2.retry.delay("exponential", 0, { minMs: 100 })
```

### Parameters

- `kind`
  - : One of `constant`, `fibonacci`, `exponential`.
