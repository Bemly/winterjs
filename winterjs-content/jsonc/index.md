---
title: "WinterJS.jsonc"
slug: WinterJS/jsonc
---

The **`WinterJS.jsonc`** property provides JSON-with-comments utilities (jsonc-parser backend): `parse` tolerates comments and trailing commas.

## Syntax

```js
WinterJS.jsonc.parse('{ "a": 1, // c\n }')
```

### Parameters

- `text`
  - : The JSONC source text.
