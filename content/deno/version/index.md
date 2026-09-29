---
title: "Deno.version"
slug: Deno/version
---

Version information related to the current Deno CLI runtime environment.
Users are discouraged from code branching based on this information, as assumptions about what is available in what build environment might change over time. Developers should specifically sniff out the features they intend to use.
The intended use for the information is for logging and debugging purposes.

## Syntax

```ts
export const version:;
```
