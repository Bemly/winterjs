---
title: "Deno.stdout"
slug: Deno/stdout
---

A reference to stdout which can be used to write directly to stdout. It implements the Deno specific https://jsr.io/@std/io/doc/types/~/Writer | Writer, https://jsr.io/@std/io/doc/types/~/WriterSync | WriterSync, and https://jsr.io/@std/io/doc/types/~/Closer | Closer interfaces as well as provides a WritableStream interface.
These are low level constructs, and the console interface is a more straight forward way to interact with stdout and stderr.

## Syntax

```ts
export const stdout:;
```
