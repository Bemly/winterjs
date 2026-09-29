---
title: "WinterJS2.tls"
slug: WinterJS2/tls
---

The **`WinterJS2.tls`** property provides Promise TLS client over `__wjs2_tls_connect` (system roots by default): same socket shape as `WinterJS2.tcp`.

## Syntax

```js
await WinterJS2.tls.connect({ host, port })
```
### Parameters

- `host`
  - : The server hostname.
