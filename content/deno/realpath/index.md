---
title: "Deno.realPath"
slug: Deno/realPath
---

Resolves to the absolute normalized path, with symbolic links resolved.
Requires allow-read permission for the target path.
Also requires allow-read permission for the CWD if the target path is relative.

## Syntax

```ts
export function realPath(path: string | URL): Promise<string>;
```
