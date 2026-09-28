---
title: "Deno.listenDatagram"
slug: Deno/listenDatagram
---

UNSTABLE**: New API, yet to be vetted.
Listen announces on the local transport address.
Requires allow-net permission.

## Syntax

```ts
export function listenDatagram( options: UdpListenOptions & { transport: "udp" }, ): DatagramConn;
```
