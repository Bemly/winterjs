---
title: "Deno.readLink"
slug: Deno/readLink
---

Resolves to the full path destination of the named symbolic link.
Throws TypeError if called with a hard link.
Requires allow-read permission.

## Syntax

```ts
export function readLink(path: string | URL): Promise<string>;
```
