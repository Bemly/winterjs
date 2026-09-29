---
title: "WinterJS.retry"
slug: WinterJS/retry
---

The **`WinterJS.retry`** property provides retry utilities with computed backoff: `delay` computes the wait, `run` awaits an async function with `setTimeout` waits.

## Syntax

```js
WinterJS.retry.delay("exponential", 0, { minMs: 100 })
```

### Parameters

- `kind`
  - : One of `constant`, `fibonacci`, `exponential`.
