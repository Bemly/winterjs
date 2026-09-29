---
title: "Deno.upgradeWebSocket"
slug: Deno/upgradeWebSocket
---

Upgrade an incoming HTTP request to a WebSocket.
Given a Request, returns a pair of WebSocket and Response instances. The original request must be responded to with the returned response for the websocket upgrade to be successful.
If the request body is disturbed (read from) before the upgrade is completed, upgrading fails.
This operation does not yet consume the request or open the websocket. This only happens once the returned response has been passed to respondWith().

## Syntax

```ts
export function upgradeWebSocket( request: Request, options?: UpgradeWebSocketOptions, ): WebSocketUpgrade;
```
