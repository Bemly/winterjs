---
title: "WinterJS.localStorage"
slug: WinterJS/localStorage
---

The **`WinterJS.localStorage`** property is the same object as the global
`localStorage`: the sync Web Storage shim over the project turso file
(`--storage-path` selects the file). Keys live under a reserved prefix and
never mix with the async `storage` keys.

## Syntax

```js
WinterJS.localStorage.setItem("theme", "dark")
```

### Parameters

- `key`
  - : Any string key.
