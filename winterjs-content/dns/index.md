---
title: "WinterJS.dns"
slug: WinterJS/dns
---

The **`WinterJS.dns`** property provides Promise DNS over `__wjs_dns_*`: `lookup` (getaddrinfo) and `resolve` (record query).

## Syntax

```js
await WinterJS.dns.lookup("localhost")
```
### Parameters

- `host`
  - : The hostname.
