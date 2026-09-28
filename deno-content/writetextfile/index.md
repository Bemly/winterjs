---
title: "Deno.writeTextFile"
slug: Deno/writeTextFile
---

Write string data to the given path, by default creating a new file if needed, else overwriting.
The data is written to the file and the file is closed, but this does not guarantee that the contents have been flushed from the operating system's buffers to the physical storage device. If you need such a durability guarantee (for example before signalling that a write has been committed), open the file with Deno.open and call Deno.FsFile.sync before closing it.
Requires allow-write permission, and allow-read if options.create is false.

## Syntax

```ts
export function writeTextFile( path: string | URL, data: string | ReadableStream<string>, options?: WriteFileOptions, ): Promise<void>;
```
