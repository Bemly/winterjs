---
title: "WinterJS2.storage"
slug: WinterJS2/storage
---

The **`WinterJS2.storage`** property is the WinterCG async key-value store for
the current project (turso single file, `--storage-path` selects the file).
It is the same object as the global `storage`.

## Syntax

```js
await WinterJS2.storage.set("k", v)
await WinterJS2.storage.get("k")
```

### Parameters

- `key`
  - : A non-empty string of at most 1024 characters.
- `value`
  - : Any JSON-serializable value, or a `Uint8Array`.
