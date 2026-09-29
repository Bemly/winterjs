---
title: "Deno.remove"
slug: Deno/remove
---

Removes the named file or directory.
Throws error if permission denied, path not found, or path is a non-empty directory and the recursive option isn't set to true.
Requires allow-write permission.

## Syntax

```ts
export function remove( path: string | URL, options?: RemoveOptions, ): Promise<void>;
```
