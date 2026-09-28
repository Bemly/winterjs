---
title: "Deno.chown"
slug: Deno/chown
---

Change owner of a regular file or directory.
This functionality is not available on Windows.
Requires allow-write permission.
Throws Error (not implemented) if executed on Windows.

## Syntax

```ts
export function chown( path: string | URL, uid: number | null, gid: number | null, ): Promise<void>;
```

### Parameters

- `path`
  - : path to the file
- `uid`
  - : user id (UID) of the new owner, or `null` for no change
- `gid`
  - : group id (GID) of the new owner, or `null` for no change
