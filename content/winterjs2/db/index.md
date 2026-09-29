---
title: "WinterJS2.db"
slug: WinterJS2/db
---

The **`WinterJS2.db`** property provides sync turso databases over `__wjs2_nsqlite_*` (same base as `node:sqlite`): `open/exec/run/query/close`.

## Syntax

```js
WinterJS2.db.open(":memory:")
```
### Parameters

- `path`
  - : The database file (`:memory:` works).
