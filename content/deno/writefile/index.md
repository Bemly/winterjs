---
title: "Deno.writeFile"
slug: Deno/writeFile
---

Write data to the given path, by default creating a new file if needed, else overwriting.
Requires allow-write permission, and allow-read if options.create is false.

## Syntax

```ts
export function writeFile( path: string | URL, data: Uint8Array | ReadableStream<Uint8Array>, options?: WriteFileOptions, ): Promise<void>;
```
