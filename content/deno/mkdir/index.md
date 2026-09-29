---
title: "Deno.mkdir"
slug: Deno/mkdir
---

Creates a new directory with the specified path.
Throws if the directory already exists, unless recursive is set to true.
Requires allow-write permission.

## Syntax

```ts
export function mkdir( path: string | URL, options?: MkdirOptions, ): Promise<void>;
```
