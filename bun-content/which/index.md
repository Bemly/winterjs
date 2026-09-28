---
title: "Bun.which"
slug: Bun/which
---

Find the path to an executable, like the which command in your terminal. Reads the PATH environment variable unless overridden with options.PATH.

## Syntax

```ts
function which(command: string, options?: WhichOptions): string | null;
```

### Parameters

- `command`
  - : The name of the executable or script to find
- `options`
  - : Options for the search
