---
title: "Deno.open"
slug: Deno/open
---

Open a file and resolve to an instance of Deno.FsFile. The file does not need to previously exist if using the create or createNew open options. The caller may have the resulting file automatically closed by the runtime once it's out of scope by declaring the file variable with the using keyword.
Alternatively, the caller may manually close the resource when finished with it.
Requires allow-read and/or allow-write permissions depending on options.

## Syntax

```ts
export function open( path: string | URL, options?: OpenOptions, ): Promise<FsFile>;
```
