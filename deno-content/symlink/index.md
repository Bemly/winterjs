---
title: "Deno.symlink"
slug: Deno/symlink
---

Creates newpath as a symbolic link to oldpath.
The options.type parameter can be set to "file", "dir" or "junction". This argument is only available on Windows and ignored on other platforms.
Requires allow-read and allow-write permissions granted *without* a path scope (i.e. --allow-read --allow-write, not --allow-read=./dir --allow-write=./dir). A symlink's target may be a relative, absolute, or not-yet-existing path that is only resolved when the link is later traversed, so it cannot be checked against a path-scoped allow-list at creation time. Path-scoped grants are therefore rejected.

## Syntax

```ts
export function symlink( oldpath: string | URL, newpath: string | URL, options?: SymlinkOptions, ): Promise<void>;
```
