---
title: "WinterJS"
slug: WinterJS/index
---

The **`WinterJS`** global object is the winterjs runtime namespace for
non-standard, runtime-specific APIs. Web standards stay on their own globals
(`fetch`, `URL`, `storage`); Node compatibility lives under `node:` modules.

## Syntax

```js
WinterJS.version
WinterJS.storage
```

### Parameters

- `version`
  - : The current winterjs version string.
- `storage`
  - : The WinterCG async KV store for the current project.
