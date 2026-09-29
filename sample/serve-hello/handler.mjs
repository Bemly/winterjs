export default {
  async fetch(req) {
    const url = new URL(req.url);

    // WebSocket 回声：配对服务端 socket，收啥回啥。
    if ((req.headers.get("upgrade") || "").toLowerCase() === "websocket") {
      const ws = __wjs2_serve_socket(req);
      ws.onmessage = (e) => {
        ws.send(e.data);
      };
      return ws;
    }

    // POST 回声：读完请求体原样返回。
    if (url.pathname === "/api/echo" && req.method === "POST") {
      const body = await req.text();
      return new Response(body, {
        status: 201,
        headers: { "content-type": "text/plain; charset=utf-8" },
      });
    }

    // GET 问候：顺手回显 handler 看到的 scheme（http/https）。
    if (url.pathname === "/api/hello") {
      return new Response(`hello from winterjs2 serve [${url.protocol}]`, {
        headers: { "content-type": "text/plain; charset=utf-8" },
      });
    }

    return new Response("not found", { status: 404 });
  },
};
