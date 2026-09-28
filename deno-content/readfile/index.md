---
title: "Deno.readFile"
slug: Deno/readFile
---

Reads and resolves to the entire contents of a file as an array of bytes. TextDecoder can be used to transform the bytes to string if required. Rejects with an error when reading a directory.
Requires allow-read permission.

## Syntax

```ts
export function readFile( path: string | URL, options?: ReadFileOptions, ): Promise<Uint8Array<ArrayBuffer>>;
```
