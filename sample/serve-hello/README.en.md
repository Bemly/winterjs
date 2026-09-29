# serve-hello Sample

> Minimal standalone-serve (WinterCG handler) sample: static + dynamic `fetch` +
> WebSocket echo on one port (route order: WS > static > handler).
> 中文版见 [README.md](./README.md).

## Run

```bash
# from the repo root (change the port if needed)
./target/debug/winterjs2 --serve sample/serve-hello/public --port 8080 \
  --handler sample/serve-hello/handler.mjs
```

Wait for `serving … on http://127.0.0.1:8080`.

## Routes

| Request | Goes to | Expect |
|---|---|---|
| `GET /` | static (`public/index.html`, never touches JS) | 200 `hi static` |
| `GET /api/hello` | handler | 200 `hello from winterjs2 serve [http:]` |
| `POST /api/echo` | handler (streaming upload) | 201, verbatim echo |
| `GET /nope` | handler | 404 |
| `GET /ws` + `Upgrade: websocket` | WS takeover (WS wins over static) | 101 |

Try:

```bash
curl http://127.0.0.1:8080/                    # static
curl http://127.0.0.1:8080/api/hello           # dynamic
curl -X POST --data 'abc' http://127.0.0.1:8080/api/echo
```

## WebSocket echo

Any GET carrying `Upgrade: websocket` enters the WS flow first. A bad handshake
(missing `Sec-WebSocket-Key`, version other than 13, non-GET) gets **400**
without touching JS.

```bash
# 101 + a correct Sec-WebSocket-Accept means success; send a text frame, get an echo
python3 - <<'EOF'
import socket
s = socket.create_connection(('127.0.0.1', 8080), timeout=10)
s.sendall(b'GET /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\n'
          b'Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n'
          b'Sec-WebSocket-Version: 13\r\n\r\n')
d = b''
while b'\r\n\r\n' not in d:
    d += s.recv(4096)
print(d.split(b'\r\n')[0].decode())
mask = bytes([1, 2, 3, 4])
payload = b'hello-ws'
fr = bytes([0x81, 0x80 | len(payload)]) + mask + bytes(
    b ^ mask[i % 4] for i, b in enumerate(payload))
s.sendall(fr)
h = s.recv(2)
ln = h[1] & 0x7f
body = b''
while len(body) < ln:
    body += s.recv(ln - len(body))
print('echo:', body.decode())
s.close()
EOF
```

JS side (`handler.mjs`):

```js
const ws = __wjs2_serve_socket(req); // only valid for serve requests
ws.onmessage = (e) => ws.send(e.data); // echo text and binary
ws.onclose = (e) => console.log("bye", e.code);
return ws; // returning the socket accepts the upgrade; returning a Response declines (plain HTTP)
```

## TLS / H2 / H3

```bash
# self-signed cert for the demo only (needs CA:FALSE + serverAuth or rustls refuses it, see AGENTS §4.38)
openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 2 -nodes \
  -subj "/CN=127.0.0.1" -extensions v3_req \
  -config <(printf "[req]\ndistinguished_name=dn\n[v3_req]\nsubjectAltName=IP:127.0.0.1\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n[dn]")

./target/debug/winterjs2 --serve sample/serve-hello/public --port 8443 \
  --cert cert.pem --key key.pem --handler sample/serve-hello/handler.mjs
# https://127.0.0.1:8443/api/hello  → scheme shows https:
# H1/H2 auto-negotiated (curl negotiates H2 by default); H3 on the same UDP port
# (needs an http3-capable curl; the macOS SecureTransport build lacks it — harness-tested)
```

Without `--cert/--key`, H3 is skipped with a warning and H1 keeps serving.

## Handler contract (plan4 §0)

- Both export shapes accepted: `export default { fetch }` first, named
  `export function fetch` as fallback; missing both fails fast at startup (no silent 503).
- Sees only WinterCG `Request` / `Response`, never sockets; large request/response
  bodies stream (response pushes are chunked at 64KB internally).
