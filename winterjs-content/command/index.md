---
title: "WinterJS.command"
slug: WinterJS/command
---

The **`WinterJS.command`** property provides child processes over `__wjs_spawn_*`: `run` captures output, `spawn` streams it.

## Syntax

```js
await WinterJS.command.run("/bin/echo", ["hi"])
```
### Parameters

- `file`
  - : The executable path.
