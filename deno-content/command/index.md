---
title: "Deno.Command"
slug: Deno/Command
---

Create a child process.
If any stdio options are not set to "piped", accessing the corresponding field on the Command or its CommandOutput will throw a TypeError.
If stdin is set to "piped", the stdin WritableStream needs to be closed manually.
Command acts as a builder. Each call to Command.spawn or Command.output will spawn a new subprocess.

## Syntax

```ts
export class Command;
```
