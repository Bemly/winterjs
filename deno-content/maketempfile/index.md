---
title: "Deno.makeTempFile"
slug: Deno/makeTempFile
---

Creates a new temporary file in the default directory for temporary files, unless dir is specified.
Other options include prefixing and suffixing the directory name with prefix and suffix respectively.
This call resolves to the full path to the newly created file.
Multiple programs calling this function simultaneously will create different files. It is the caller's responsibility to remove the file when no longer needed.
Requires allow-write permission.

## Syntax

```ts
export function makeTempFile(options?: MakeTempOptions): Promise<string>;
```
