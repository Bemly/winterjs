---
title: "Deno.utime"
slug: Deno/utime
---

Changes the access (atime) and modification (mtime) times of the file stream resource. Given times are either in seconds (UNIX epoch time) or as Date objects.

## Syntax

```ts
utime(atime: number | Date, mtime: number | Date): Promise<void>;
```
