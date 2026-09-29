---
title: "WinterJS2.shell"
slug: WinterJS2/shell
---

The **`WinterJS2.shell`** property provides shell-word utilities: `expand` handles `~` and `$VAR` (`${VAR}`); sandboxed runs reject unallowed variables.

## Syntax

```js
WinterJS2.shell.expand("~/x")
```

### Parameters

- `text`
  - : The word to expand.
