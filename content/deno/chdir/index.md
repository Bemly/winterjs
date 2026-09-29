---
title: "Deno.chdir"
slug: Deno/chdir
---

Change the current working directory to the specified path.
Throws Deno.errors.NotFound if directory not found.
Throws Deno.errors.PermissionDenied if the user does not have operating system file access rights.
Requires allow-read permission.

## Syntax

```ts
export function chdir(directory: string | URL): void;
```
