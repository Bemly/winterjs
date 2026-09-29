---
title: "WinterJS.tcp"
slug: WinterJS/tcp
---

The **`WinterJS.tcp`** property provides Promise TCP sockets over `__wjs_net_*` (no `node:`): `connect`/`listen`, byte streams are AsyncIterables.

## Syntax

```js
const s = await WinterJS.tcp.listen(0, "127.0.0.1")
```
### Parameters

- `port`
  - : The port (0 = ephemeral).
