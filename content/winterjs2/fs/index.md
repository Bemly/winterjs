---
title: "WinterJS2.fs"
slug: WinterJS2/fs
---

The **`WinterJS2.fs`** property is the own file surface of winterjs2, separate
from `node:fs` (separate implementation, no Node error codes). It is the same object
as the global `fs`.

## Syntax

```js
await WinterJS2.fs.writeFile("a.txt", "hello")
await WinterJS2.fs.readTextFile("a.txt")
```

### Parameters

- `path`
  - : A non-empty string path.
- `data`
  - : A string, `Uint8Array`, or `ArrayBuffer`.
