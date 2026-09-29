---
title: "WinterJS2.tcp"
slug: WinterJS2/tcp
---

The **`WinterJS2.tcp`** property provides Promise TCP sockets over `__wjs2_net_*` (no `node:`): `connect`/`listen`, byte streams are AsyncIterables.

## Syntax

```js
const s = await WinterJS2.tcp.listen(0, "127.0.0.1")
```
### Parameters

- `port`
  - : The port (0 = ephemeral).
