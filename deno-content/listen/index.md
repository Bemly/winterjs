---
title: "Deno.listen"
slug: Deno/listen
---

Listen announces on the local transport address.
Requires allow-net permission.

## Syntax

```ts
export function listen( options: TcpListenOptions & { transport?: "tcp" }, ): TcpListener;
```
