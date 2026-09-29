---
title: "Bun.spawn"
slug: Bun/spawn
---

Spawn a new process
Internally, this uses [posix_spawn(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/posix_spawn.2.html)

## Syntax

```ts
function spawn< const In extends SpawnOptions.Writable = "ignore", const Out extends SpawnOptions.Readable = "pipe", const Err extends SpawnOptions.Readable = "inherit", >( options: SpawnOptions.SpawnOptions<In, Out, Err> &;
```
