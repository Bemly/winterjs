---
title: "Deno.test"
slug: Deno/test
---

Register a test which will be run when deno test is used on the command line and the containing module looks like a test module.
fn can be async if required.
Tests are discovered before they are executed, so registrations must happen at module load time. Nested Deno.test() calls are not supported. Use t.step() for nested tests.

## Syntax

```ts
export const test: DenoTest;
```
