---
title: "Deno.chmod"
slug: Deno/chmod
---

Changes the permission of a specific file/directory of specified path. Ignores the process's umask.
The mode is a sequence of 3 octal numbers. The first/left-most number specifies the permissions for the owner. The second number specifies the permissions for the group. The last/right-most number specifies the permissions for others. For example, with a mode of 0o764, the owner (7) can read/write/execute, the group (6) can read/write and everyone else (4) can read only.
| Number | Description | | ------ | ----------- | | 7      | read, write, and execute | | 6      | read and write | | 5      | read and execute | | 4      | read only | | 3      | write and execute | | 2      | write only | | 1      | execute only | | 0      | no permission |
Note: On Windows, only the read and write permissions can be modified. Distinctions between owner, group, and others are not supported.
Requires allow-write permission.

## Syntax

```ts
export function chmod(path: string | URL, mode: number): Promise<void>;
```
