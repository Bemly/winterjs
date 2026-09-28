---
title: "Deno.exit"
slug: Deno/exit
---

Exit the Deno process with optional exit code.
If no exit code is supplied then Deno will exit with return code of 0.
In worker contexts this closes the current worker using Deno's internal worker close operation. It does not call the current self.close property.

## Syntax

```ts
export function exit(code?: number): never;
```
