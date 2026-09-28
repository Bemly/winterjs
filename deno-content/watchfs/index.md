---
title: "Deno.watchFs"
slug: Deno/watchFs
---

Watch for file system events against one or more paths, which can be files or directories. These paths must exist already. One user action (e.g. touch test.file) can generate multiple file system events. Likewise, one user action can result in multiple file paths in one event (e.g. mv old_name.txt new_name.txt).
The recursive option is true by default and, for directories, will watch the specified directory and all sub directories.
Note that the exact ordering of the events can vary between operating systems.
The ignore option can be used to filter out events for one or more paths. A path matches when it is, or is contained within, an ignored path, so ignoring a directory ignores everything beneath it. Relative paths are resolved against the current working directory. Ignored paths still require allow-read permission, the same as the watched paths.
Call watcher.close() to stop watching.
Requires allow-read permission.

## Syntax

```ts
export function watchFs( paths: string | string[], options?: { recursive?: boolean; ignore?: string | string[] }, ): FsWatcher;
```
