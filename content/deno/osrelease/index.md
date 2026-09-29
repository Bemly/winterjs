---
title: "Deno.osRelease"
slug: Deno/osRelease
---

Returns the release version of the Operating System.
Requires allow-sys permission. Under consideration to possibly move to Deno.build or Deno.versions and if it should depend sys-info, which may not be desirable.

## Syntax

```ts
export function osRelease(): string;
```
