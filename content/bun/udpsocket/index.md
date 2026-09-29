---
title: "Bun.udpSocket"
slug: Bun/udpSocket
---

Create a UDP socket

## Syntax

```ts
export function udpSocket<DataBinaryType extends BinaryType = "buffer">( options: udp.SocketOptions<DataBinaryType>, ): Promise<udp.Socket<DataBinaryType>>;
```

### Parameters

- `options`
  - : The options to use when creating the socket
- `options.socket`
  - : The socket handler to use
- `options.hostname`
  - : The hostname to listen on
- `options.port`
  - : The port to listen on
- `options.binaryType`
  - : The binary type to use for the socket
- `options.connect`
  - : The hostname and port to connect to
