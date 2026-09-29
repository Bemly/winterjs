---
title: "Deno.copyFile"
slug: Deno/copyFile
---

Copies the contents and permissions of one file to another specified path, by default creating a new file if needed, else overwriting. Fails if target path is a directory or is unwritable.
Requires allow-read permission on fromPath.
Requires allow-write permission on toPath.

## Syntax

```ts
export function copyFile( fromPath: string | URL, toPath: string | URL, ): Promise<void>;
```
