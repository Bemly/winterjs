---
title: "WinterJS2.dns"
slug: WinterJS2/dns
---

The **`WinterJS2.dns`** property provides Promise DNS over `__wjs2_dns_*`: `lookup` (getaddrinfo) and `resolve` (record query).

## Syntax

```js
await WinterJS2.dns.lookup("localhost")
```
### Parameters

- `host`
  - : The hostname.
