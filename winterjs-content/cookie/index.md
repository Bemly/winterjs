---
title: "WinterJS.cookie"
slug: WinterJS/cookie
---

The **`WinterJS.cookie`** property provides cookie codec (cookie backend): `parse` reads the first pair, `serialize` builds a Set-Cookie value.

## Syntax

```js
WinterJS.cookie.serialize("a", "1", { path: "/" })
```

### Parameters

- `name`
  - : The cookie name.
