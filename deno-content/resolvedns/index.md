---
title: "Deno.resolveDns"
slug: Deno/resolveDns
---

Performs DNS resolution against the given query, returning resolved records.
Fails in the cases such as:
- the query is in invalid format. - the options have an invalid parameter. For example nameServer.port is beyond the range of 16-bit unsigned integer. - the request timed out.
The "A", "AAAA", "ANAME", "CNAME", "NS" and "PTR" record types resolve to an array of strings.
Requires allow-net permission.

## Syntax

```ts
export function resolveDns( query: string, recordType: "A" | "AAAA" | "ANAME" | "CNAME" | "NS" | "PTR", options?: ResolveDnsOptions, ): Promise<string[]>;
```
