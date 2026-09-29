---
title: "WinterJS2.udp"
slug: WinterJS2/udp
---

The **`WinterJS2.udp`** property provides Promise UDP sockets over `__wjs2_dgram_*`: `bind`/`send`/`close`, messages are AsyncIterables.

## Syntax

```js
const u = await WinterJS2.udp.bind(0, "127.0.0.1")
```
### Parameters

- `port`
  - : The port (0 = ephemeral).
