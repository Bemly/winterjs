---
title: "WinterJS.shell"
slug: WinterJS/shell
---

The **`WinterJS.shell`** property provides shell-word utilities (shellexpand backend): `expand` handles `~` and `$VAR` (`${VAR}`); sandboxed runs reject unallowed variables.

## Syntax

```js
WinterJS.shell.expand("~/x")
```

### Parameters

- `text`
  - : The word to expand.
