---
title: "WinterJS.serve"
slug: WinterJS/serve
---

The **`WinterJS.serve`** property provides static-free HTTP serving over `node:http` (Deno.serve-shaped bridge): `(opts, fetch)` → `{ addr, shutdown }`.

## Syntax

```js
await WinterJS.serve({ port: 0 }, handler)
```
### Parameters

- `port`
  - : The port (0 = ephemeral).
