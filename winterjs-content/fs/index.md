---
title: "WinterJS.fs"
slug: WinterJS/fs
---

The **`WinterJS.fs`** property is the own file surface of winterjs, separate
from `node:fs` (direct `fs-err`, no Node error codes). It is the same object
as the global `fs`.

## Syntax

```js
await WinterJS.fs.writeFile("a.txt", "hello")
await WinterJS.fs.readTextFile("a.txt")
```

### Parameters

- `path`
  - : A non-empty string path.
- `data`
  - : A string, `Uint8Array`, or `ArrayBuffer`.
