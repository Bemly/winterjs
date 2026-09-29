---
title: "Deno.makeTempDir"
slug: Deno/makeTempDir
---

Creates a new temporary directory in the default directory for temporary files, unless dir is specified. Other optional options include prefixing and suffixing the directory name with prefix and suffix respectively.
This call resolves to the full path to the newly created directory.
Multiple programs calling this function simultaneously will create different directories. It is the caller's responsibility to remove the directory when no longer needed.
Requires allow-write permission.

## Syntax

```ts
// TODO(ry) Doesn't check permissions. export function makeTempDir(options?: MakeTempOptions): Promise<string>;
```
