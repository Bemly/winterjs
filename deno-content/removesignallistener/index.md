---
title: "Deno.removeSignalListener"
slug: Deno/removeSignalListener
---

Removes the given signal listener that has been registered with Deno.addSignalListener.
_Note_: On Windows only "SIGINT" (CTRL+C), "SIGBREAK" (CTRL+Break), "SIGTERM", "SIGQUIT", "SIGHUP", and "SIGWINCH" are supported.

## Syntax

```ts
export function removeSignalListener( signal: Signal, handler: () => void, ): void;
```
