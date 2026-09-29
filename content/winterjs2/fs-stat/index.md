---
title: "WinterJS2.fs.stat"
slug: WinterJS2/fs-stat
---

The **`WinterJS2.fs.stat(path)`** method returns `{ isFile, isDirectory, isSymlink, size, mtimeMs }` for a path. Same object as global `fs.stat`.

## Syntax

```js
await WinterJS2.fs.stat(path)
```

### Parameters

- `path`
  - : A non-empty string path.
