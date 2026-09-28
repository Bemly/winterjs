---
title: "WinterJS.fs.stat"
slug: WinterJS/fs-stat
---

The **`WinterJS.fs.stat(path)`** method returns `{ isFile, isDirectory, isSymlink, size, mtimeMs }` for a path. Same object as global `fs.stat`.

## Syntax

```js
await WinterJS.fs.stat(path)
```

### Parameters

- `path`
  - : A non-empty string path.
