---
title: "Bun"
slug: Bun/index
---

The **`Bun`** global object is the Bun runtime namespace for Bun-native APIs
(file, subprocesses, HTTP server, shell, hashing). Web standards stay on their
own globals; Node compatibility lives under `node:` modules.

## Syntax

```js
Bun.version
Bun.serve({ fetch(req) { return new Response("hi"); } })
```

### Parameters

- `version`
  - : The current Bun version string.
