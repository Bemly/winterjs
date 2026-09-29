---
title: "Bun.randomUUIDv7"
slug: Bun/randomUUIDv7
---

Generate a UUIDv7, a sequential ID based on the current timestamp with a random component.
When the same timestamp is used multiple times, a monotonically increasing counter is appended to allow sorting. The final 8 bytes are cryptographically random. When the timestamp changes, the counter resets to a pseudo-random integer.

## Syntax

```ts
function randomUUIDv7( /** * @default "hex" */ encoding?: "hex" | "base64" | "base64url", /**;
```

### Parameters

- `encoding`
  - : Output encoding for the UUID
- `timestamp`
  - : Unix timestamp in milliseconds, defaults to `Date.now()`
