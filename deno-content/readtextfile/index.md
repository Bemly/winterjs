---
title: "Deno.readTextFile"
slug: Deno/readTextFile
---

Asynchronously reads and returns the entire contents of a file as an UTF-8 decoded string.
The returned promise rejects if the operation fails, for example with Deno.errors.NotFound if the file does not exist, Deno.errors.IsADirectory if path refers to a directory, or Deno.errors.PermissionDenied if the required permission has not been granted.
Requires allow-read permission.

## Syntax

```ts
export function readTextFile( path: string | URL, options?: ReadFileOptions, ): Promise<string>;
```
