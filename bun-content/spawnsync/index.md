---
title: "Bun.spawnSync"
slug: Bun/spawnSync
---

Synchronously spawn a new process
Internally, this uses [posix_spawn(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/posix_spawn.2.html)

## Syntax

```ts
function spawnSync< const In extends SpawnOptions.Writable = "ignore", const Out extends SpawnOptions.Readable = "pipe", const Err extends SpawnOptions.Readable = "pipe", >( options: SpawnOptions.SpawnSyncOptions<In, Out, Err> &;
```
