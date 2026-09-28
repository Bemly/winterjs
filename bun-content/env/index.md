---
title: "Bun.env"
slug: Bun/env
---

The environment variables of the process
Defaults to process.env as it was when the current Bun process launched.
Changes to process.env at runtime won't automatically be reflected in the default value. For that, you can pass process.env explicitly.

## Syntax

```ts
const env: Env & NodeJS.ProcessEnv & ImportMetaEnv;
```
