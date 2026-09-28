---
title: "Deno.systemMemoryInfo"
slug: Deno/systemMemoryInfo
---

Displays the total amount of free and used physical and swap memory in the system, as well as the buffers and caches used by the kernel.
This is similar to the free command in Linux
Requires allow-sys permission.

## Syntax

```ts
export function systemMemoryInfo(): SystemMemoryInfo;
```
