---
title: "Deno.connect"
slug: Deno/connect
---

Connects to the hostname (default is "127.0.0.1") and port on the named transport (default is "tcp"), and resolves to the connection (Conn).
Requires allow-net permission for "tcp".

## Syntax

```ts
export function connect(options: ConnectOptions): Promise<TcpConn>;
```
