---
title: "WinterJS.semver"
slug: WinterJS/semver
---

The **`WinterJS.semver`** property provides npm-semantics version utilities: `valid`, `parse`, `satisfies`, `compare`. Prerelease ties compare lexicographically (approximation).

## Syntax

```js
WinterJS.semver.satisfies("1.2.3", "^1.0.0")
```

### Parameters

- `range`
  - : An npm range (tags like `latest` are rejected).
