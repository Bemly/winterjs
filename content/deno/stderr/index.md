---
title: "Deno.stderr"
slug: Deno/stderr
---

A reference to stderr which can be used to write directly to stderr. It implements the Deno specific https://jsr.io/@std/io/doc/types/~/Writer | Writer, https://jsr.io/@std/io/doc/types/~/WriterSync | WriterSync, and https://jsr.io/@std/io/doc/types/~/Closer | Closer interfaces as well as provides a WritableStream interface.
These are low level constructs, and the console interface is a more straight forward way to interact with stdout and stderr.

## Syntax

```ts
export const stderr:;
```
