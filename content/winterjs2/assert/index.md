---
title: "WinterJS2.assert"
slug: WinterJS2/assert
---

The **`WinterJS2.assert`** property provides structural assertions
(`ok/equal/strictEqual/deepEqual/throws/rejects/match` and negations).
`deepEqual` compares structure, not prototypes (node:assert does both).

## Syntax

```js
WinterJS2.assert.deepEqual({ a: 1 }, { a: 1 })
```

### Parameters

- `actual`
  - : The value under test.
