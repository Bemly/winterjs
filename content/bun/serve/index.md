---
title: "Bun.serve"
slug: Bun/serve
---

Bun.serve starts a high-performance HTTP server with built-in routing. Routes can be static responses, handler functions, or per-method handler objects, with type-safe path parameters.

## Syntax

```ts
function serve<WebSocketData = undefined, R extends string = never>( options: Serve.Options<WebSocketData, R>, ): Server<WebSocketData>;
```

### Parameters

- `options`
  - : Server configuration options
