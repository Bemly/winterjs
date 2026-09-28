---
title: "WinterJS.udp"
slug: WinterJS/udp
---

The **`WinterJS.udp`** property provides Promise UDP sockets over `__wjs_dgram_*`: `bind`/`send`/`close`, messages are AsyncIterables.

## Syntax

```js
const u = await WinterJS.udp.bind(0, "127.0.0.1")
```
### Parameters

- `port`
  - : The port (0 = ephemeral).
