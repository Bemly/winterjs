---
title: "WinterJS2.command"
slug: WinterJS2/command
---

The **`WinterJS2.command`** property provides child processes over `__wjs2_spawn_*`: `run` captures output, `spawn` streams it.

## Syntax

```js
await WinterJS2.command.run("/bin/echo", ["hi"])
```
### Parameters

- `file`
  - : The executable path.
