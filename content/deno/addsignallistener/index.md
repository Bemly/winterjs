---
title: "Deno.addSignalListener"
slug: Deno/addSignalListener
---

Registers the given function as a listener of the given signal event.
_Note_: On Windows only "SIGINT" (CTRL+C), "SIGBREAK" (CTRL+Break), "SIGTERM", "SIGQUIT", "SIGHUP", and "SIGWINCH" are supported.

## Syntax

```ts
export function addSignalListener(signal: Signal, handler: () => void): void;
```
