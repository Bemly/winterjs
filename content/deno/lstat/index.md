---
title: "Deno.lstat"
slug: Deno/lstat
---

Resolves to a Deno.FileInfo for the specified path. If path is a symlink, information for the symlink will be returned instead of what it points to.
Requires allow-read permission.

## Syntax

```ts
export function lstat(path: string | URL): Promise<FileInfo>;
```
