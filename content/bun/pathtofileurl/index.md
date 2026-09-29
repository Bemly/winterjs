---
title: "Bun.pathToFileURL"
slug: Bun/pathToFileURL
---

Convert a filesystem path to a file:// URL.
Internally, this function uses WebKit's URL API to convert the path to a file:// URL.

## Syntax

```ts
function pathToFileURL(path: string): URL;
```

### Parameters

- `path`
  - : The path to convert.
