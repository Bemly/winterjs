---
title: "Bun.write"
slug: Bun/write
---

Use the fastest syscalls available to copy from input into destination.
If destination exists, it must be a regular file or symlink to a file. If destination's directory does not exist, it is created by default.

## Syntax

```ts
function write( destination: BunFile | S3File | PathLike, input: Blob | NodeJS.TypedArray | ArrayBufferLike | string | BlobPart[] | Archive | ReadableStream, options?: { /** * If writing to a PathLike, set the permissions of the file.;
```

### Parameters

- `destination`
  - : The file or file path to write to
- `input`
  - : The data to copy into `destination`
- `options`
  - : Options for the write
