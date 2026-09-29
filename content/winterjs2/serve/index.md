---
title: "WinterJS2.serve"
slug: WinterJS2/serve
---

The **`WinterJS2.serve`** property provides static-free HTTP serving over `node:http` (Deno.serve-shaped bridge): `(opts, fetch)` → `{ addr, shutdown }`.

## Syntax

```js
await WinterJS2.serve({ port: 0 }, handler)
```
### Parameters

- `port`
  - : The port (0 = ephemeral).
