---
title: "Deno.args"
slug: Deno/args
---

Returns the script arguments to the program.
Give the following command line invocation of Deno:
Then Deno.args will contain:
If you are looking for a structured way to parse arguments, there is [parseArgs()](https://jsr.io/@std/cli/doc/parse-args/~/parseArgs) from the Deno Standard Library.

## Syntax

```ts
export const args: string[];
```
