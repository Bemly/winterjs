---
title: "Deno.serve"
slug: Deno/serve
---

Serves HTTP requests with the given handler.
The below example serves with the port 8000 on hostname "127.0.0.1".

## Syntax

```ts
export function serve( handler: ServeHandler<Deno.NetAddr>, ): HttpServer<Deno.NetAddr>;
```
