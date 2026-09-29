---
title: "WinterJS.tls"
slug: WinterJS/tls
---

The **`WinterJS.tls`** property provides Promise TLS client over `__wjs_tls_connect` (system roots by default): same socket shape as `WinterJS.tcp`.

## Syntax

```js
await WinterJS.tls.connect({ host, port })
```
### Parameters

- `host`
  - : The server hostname.
