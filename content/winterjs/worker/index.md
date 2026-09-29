---
title: "Worker"
slug: WinterJS/worker
---

The **`Worker`** global runs a file in a new thread (`__wjs_worker_*`
natives): `postMessage` sends JSON-text messages, `onmessage` receives
decoded values, `terminate` stops the thread. Inside the worker, use
`node:worker_threads`' `parentPort` (post JSON text back).

## Syntax

```js
const w = new Worker("worker.mjs")
```

### Parameters

- `src`
  - : A file path, or code with `{ eval: true }`.
