---
title: "Deno.consoleSize"
slug: Deno/consoleSize
---

Gets the size of the console as columns/rows.
This returns the size of the console window as reported by the operating system. It's not a reflection of how many characters will fit within the console window, but can be used as part of that calculation.
Throws if none of stdin, stdout, or stderr is connected to a terminal (e.g. all are piped or redirected). Use Deno.stdout.isTerminal to check before calling.

## Syntax

```ts
export function consoleSize():;
```
