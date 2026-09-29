---
title: "Bun.file"
slug: Bun/file
---

[Blob](https://developer.mozilla.org/en-US/docs/Web/API/Blob) powered by the fastest system calls available for operating on files.
This Blob is lazy: it does no work until you read from it.
- size is not valid until the contents of the file are read at least once. - type is auto-set based on the file extension when possible

## Syntax

```ts
function file(path: string | URL, options?: BlobPropertyBag): BunFile;
```

### Parameters

- `path`
  - : The path to the file (lazily loaded). If the path starts with `s3://`, the file behaves like {@link S3File}
