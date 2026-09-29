---
title: "Deno.cwd"
slug: Deno/cwd
---

Return a string representing the current working directory.
If the current directory can be reached via multiple paths (due to symbolic links), cwd() may return any one of them.
Throws Deno.errors.NotFound if directory not available.

## Syntax

```ts
export function cwd(): string;
```
