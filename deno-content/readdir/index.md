---
title: "Deno.readDir"
slug: Deno/readDir
---

Reads the directory given by path and returns an async iterable of Deno.DirEntry. The order of entries is not guaranteed.
Throws error if path is not a directory.
Requires allow-read permission.

## Syntax

```ts
export function readDir(path: string | URL): AsyncIterable<DirEntry>;
```
