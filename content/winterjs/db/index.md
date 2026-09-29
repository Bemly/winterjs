---
title: "WinterJS.db"
slug: WinterJS/db
---

The **`WinterJS.db`** property provides sync turso databases over `__wjs_nsqlite_*` (same base as `node:sqlite`): `open/exec/run/query/close`.

## Syntax

```js
WinterJS.db.open(":memory:")
```
### Parameters

- `path`
  - : The database file (`:memory:` works).
