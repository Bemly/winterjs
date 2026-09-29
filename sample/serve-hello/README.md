# serve-hello 示例

> 独立 serve（WinterCG handler）最小示例：静态 + 动态 `fetch` + WebSocket 回声，
> 同端口（路由序：WS > 静态 > handler）。英文版见 [README.en.md](./README.en.md)。

## 跑起来

```bash
# 仓库根目录执行（端口按需改）
./target/debug/winterjs2 --serve sample/serve-hello/public --port 8080 \
  --handler sample/serve-hello/handler.mjs
```

看到 `serving … on http://127.0.0.1:8080` 即就绪。

## 路由

| 请求 | 走向 | 期望 |
|---|---|---|
| `GET /` | 静态（`public/index.html`，不进 JS） | 200 `hi static` |
| `GET /api/hello` | handler | 200 `hello from winterjs2 serve [http:]` |
| `POST /api/echo` | handler（请求体流式上行） | 201，原样回声 |
| `GET /nope` | handler | 404 |
| `GET /ws` + `Upgrade: websocket` | WS 接管（WS 优先于静态） | 101 |

试一试：

```bash
curl http://127.0.0.1:8080/                    # 静态
curl http://127.0.0.1:8080/api/hello           # 动态
curl -X POST --data 'abc' http://127.0.0.1:8080/api/echo
```

## WebSocket 回声

任何带 `Upgrade: websocket` 的 GET 都优先进 WS 流程（哪怕静态文件存在，
这就是"WS 路径优先"）。握手头缺 `Sec-WebSocket-Key` / 版本不是 13 /
非 GET，一律 **400**，不进 JS。

```bash
# 101 + 正确的 Sec-WebSocket-Accept 即成功；发文本帧即回声
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

JS 侧写法（`handler.mjs` 已演示）：

```js
const ws = __wjs2_serve_socket(req); // 只能对 serve 请求调用
ws.onmessage = (e) => ws.send(e.data); // 文本/二进制都回声
ws.onclose = (e) => console.log("bye", e.code);
return ws; // 直接返回 socket 即接受升级；返回 Response 即拒绝（走普通 HTTP）
```

## TLS / H2 / H3

```bash
# 自签证书（仅示例；CA:FALSE + serverAuth，rustls 才认，见 AGENTS §4.38）
openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 2 -nodes \
  -subj "/CN=127.0.0.1" -extensions v3_req \
  -config <(printf "[req]\ndistinguished_name=dn\n[v3_req]\nsubjectAltName=IP:127.0.0.1\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n[dn]")

./target/debug/winterjs2 --serve sample/serve-hello/public --port 8443 \
  --cert cert.pem --key key.pem --handler sample/serve-hello/handler.mjs
# https://127.0.0.1:8443/api/hello  → scheme 显示 https:
# H1/H2 自动协商（curl 默认谈 H2）；H3 同端口 UDP（curl 需 http3 版，本机 SecureTransport 版 curl 不支持，用 harness 测）
```

无 `--cert/--key` 时 H3 自动跳过（warn），H1 照常服务。

## handler 契约（plan4 §0）

- 两种导出都认：`export default { fetch }` 优先，回落具名 `export function fetch`；
  双缺即启动期报错退出（不静默）。
- 只见 WinterCG `Request` / `Response`，见不到 socket；大请求体/响应体走流式
  （响应单次推送封顶 64KB，内部自动分片）。
