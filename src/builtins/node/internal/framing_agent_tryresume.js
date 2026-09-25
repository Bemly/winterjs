// 续行排队请求（同键优先；全局 maxTotalSockets 帽下跨键唤醒，同键队列空
// 时补扫其余键，防他键请求饿死；取空即删键）。
Agent.prototype.__resumeQueued = function (key) {
  const tryResume = (k) => {
    const q = this.__peek(this.requests, k);
    while (q.length > 0) {
      const next = q.shift();
      if (next.req.destroyed) continue;
      next.req.__queued = false;
      this.__acquire(next.req, next.host, next.port, next.extra, next.onSocket);
      this.__dropIfEmpty(this.requests, k);
      return true;
    }
    this.__dropIfEmpty(this.requests, k);
    return false;
  };
  if (!tryResume(key)) {
    for (const k of Object.keys(this.requests)) {
      if (k === key) continue;
      if (tryResume(k)) break;
    }
  }
};
Agent.prototype.__release = function (sock, key, req, poolable = true) {
  if (req !== null && req !== undefined) req.__queued = false;
  // node responseKeepAlive 口径：响应 Connection: close 即销毁不回池
  //（get-pipeline-problem 套件：close 响应入池后复用撞对端 FIN，次请求
  // ECONNRESET；此前 poolable 只门 freeSockErr，入池本身无门）。
  if (sock.destroyed || !this.keepAlive || poolable === false) {
    if (!sock.destroyed) {
      try { sock.destroy(); } catch { /* gone */ }
    }
    this.__noteClosed(sock);
    } else {
      const free = this.__list(this.freeSockets, key);
      if (free.length >= this.maxFreeSockets) {
        try { sock.destroy(); } catch { /* gone */ }
        this.__noteClosed(sock);
      } else {
        // node 口径：入池前调可覆写的 keepSocketAlive（false 即销毁不池化；
        // 默认实现做 TCP keepalive + unref + 空闲计时重置；CustomAgent 覆写
        // 经 this. 调度生效，agent-timeout 套件）。freeSocketErrorListener
        // 已在 __finishSock 的 'free' 派发前挂上（此处不再重复）。
        let __keep = true;
        try {
          __keep = this.keepSocketAlive(sock);
        } catch { __keep = false; }
        if (__keep === false) {
          try { sock.destroy(); } catch { /* gone */ }
          this.__noteClosed(sock);
        } else {
        sock.__inPool = true;
        // node 口径：回池即 detach 解析器 + 武装空闲投毒 guard（free-socket-
        // data-guard 套件：parser null、零 data/readable 监听；投毒到达即销毁。
        // guard 走 __ingestData（监听之外），listenerCount 恒 0）。
        try { sock.parser = null; } catch { /* gone */ }
        sock.__freeGuardArmed = true;
        // 回池 socket 补挂 keylog 转发（监听先行时）。
        try { this.__armKeylog(sock); } catch { /* gone */ }
        free.push(sock);
      // node 口径：入池即移出在用表（agent.sockets 只计在用——
      // agent-maxtotalsockets 的 getTotalSocketsCount 口径）；空键即删
      //（agent.sockets[name] === undefined 断言，agent-keepalive 套件）。
      const __inUse = this.__list(this.sockets, key);
      const __i = __inUse.indexOf(sock);
      if (__i !== -1) __inUse.splice(__i, 1);
      if (__inUse.length === 0) delete this.sockets[key];
      if (sock.__poolCleaner === undefined) {
        const cleaner = () => this.__noteClosed(sock);
        sock.__poolCleaner = cleaner;
        sock.on("close", cleaner);
      }
        }
    }
  }
  // 续行排队请求（__resumeQueued 统一入口；回池与关闭双路径覆盖）。
  this.__resumeQueued(key);
};
Agent.prototype.__cancel = function (req) {
  const key = req.__poolKey;
  if (key === undefined || !req.__queued) return;
  req.__queued = false;
  // node 口径：abort 后同步仍可见排队项（abort-queued 套件 L80 断言 requests
  // 为 1；真机不同步摘），摘除递延；+100ms 后 L86/87 归零不受影响。
  queueMicrotask(() => {
    const q = this.__peek(this.requests, key);
    const i = q.findIndex((e) => e.req === req);
    if (i !== -1) q.splice(i, 1);
    this.__dropIfEmpty(this.requests, key);
  });
};
// node lib/_http_agent.js addRequest 口径（freeSockets 直投/建连/排队三路）：
// 外部直塞 freeSockets 再 addRequest 即复用（agent-uninitialized 套件）。
Agent.prototype.addRequest = function (req, options, port, localAddress) {
  if (typeof options === "string") options = { host: options, port, localAddress };
  options = { ...(options ?? {}), ...(this.options ?? {}) };
  if (options.socketPath) options.path = options.socketPath;
  const name = this.getName(options);
  // 只读探针不用 __list（建空数组污染 keys 计数；真入池/建连由后继 push 落定）。
  const free = this.freeSockets[name];
  let sock;
  if (free) {
    while (free.length > 0 && free[0].destroyed) free.shift();
    sock = this.scheduling === "fifo" ? free.shift() : free.pop();
    if (free.length === 0) delete this.freeSockets[name];
  }
  if (sock) {
    this.__unpool(sock);
    this.__list(this.sockets, name).push(sock);
    req.__poolKey = name;
    req.__queued = false;
    if (!sock.connecting && sock.pending) {
      // node onSocket 口径：手动塞入的裸 socket（无句柄，new net.Socket()）
      // 按请求选项补连，'connect' 事件后续行（agent-uninitialized 套件）。
      const sp = req.socketPath ?? options.socketPath;
      const copts = sp !== undefined && sp !== null
        ? { path: sp }
        : { host: req.host ?? "localhost", port: typeof req.getPort === "function" ? req.getPort() : undefined };
      sock.connect(copts);
      if (typeof req.__attach === "function") req.__attach(sock, false);
      return;
    }
    if (typeof req.__attach === "function") req.__attach(sock, true);
    return;
  }
  if (this.__liveCount(name) >= this.maxSockets) {
    this.__list(this.requests, name).push({ req, host: options.host, port: options.port, extra: options, onSocket: (s, reused) => req.__attach(s, reused) });
    req.__poolKey = name;
    req.__queued = true;
    return;
  }
  req.__poolKey = name;
  req.__queued = false;
  const opts = { ...options, host: options.hostname ?? options.host ?? "localhost", port: options.port ?? this.__defaultPort ?? 80 };
  let done = false;
  const oncreate = (err, s) => {
    if (done) return;
    done = true;
    if (err || !s) {
      if (s) { try { s.destroy(); } catch { /* gone */ } }
      const e = err ?? (() => { const x = new Error("socket hang up"); x.code = "ECONNREFUSED"; return x; })();
      if (!req.destroyed) req.destroy(e);
      return;
    }
    this.__trackSocket(s, name);
    if (typeof req.__attach === "function") req.__attach(s, false);
  };
  this.createSocket(req, opts, oncreate);
};
Agent.prototype.destroy = function () {
  for (const key of Object.keys(this.requests)) {
    const q = this.requests[key];
    this.requests[key] = [];
    for (const { req } of q) {
      if (!req.destroyed) {
        const err = new Error("socket hang up");
        err.code = "ECONNRESET";
        req.destroy(err);
      }
    }
  }
  for (const key of Object.keys(this.sockets)) {
    const arr = this.sockets[key];
    this.sockets[key] = [];
    for (const sock of arr) {
      try { sock.destroy(); } catch { /* gone */ }
    }
  }
  this.freeSockets = {};
};
export { Agent };

// request/get 三形态归一：(url[, options][, cb]) / (options[, cb]) → [options, cb]。
// url 与 options 并存时 options 优先；跨协议即 ERR_INVALID_PROTOCOL（Node 口径）。
// node 口径：URL 形实参解析失败一律 ERR_INVALID_URL（invalid-urls 套件：
// 'www.nodejs.org' 等无协议串 → TypeError/ERR_INVALID_URL，真机对拍）。
function __parseUrlArg(str) {
  try {
    return new URL(str);
  } catch (e) {
    const err = new TypeError("Invalid URL");
    err.code = "ERR_INVALID_URL";
    err.input = str;
    throw err;
  }
}
export function normalizeRequestArgs(a, b, c, flavor) {
  if (typeof a === "string" || a instanceof URL) {
    const u = __parseUrlArg(String(a));
    if (u.protocol !== flavor.protocol) {
      throw new codes.ERR_INVALID_PROTOCOL(u.protocol, flavor.protocol);
    }
    const fromUrl = { hostname: u.hostname };
    if (u.port) fromUrl.port = Number(u.port);
    fromUrl.path = u.pathname + u.search;
    // node 口径（internal/url urlToHttpOptions + decoded-auth 套件真机实测）：
    // userinfo 进 options.auth（decodeURIComponent 双侧），IPv6 主机名去框
    //（'[::1]'→'::1'，加框由发头侧按需做）。
    if (fromUrl.hostname.startsWith("[") && fromUrl.hostname.endsWith("]")) {
      fromUrl.hostname = fromUrl.hostname.slice(1, -1);
    }
    if (u.username || u.password) {
      fromUrl.auth = `${decodeURIComponent(u.username)}:${decodeURIComponent(u.password)}`;
    }
    // node 口径：URL 实例 expando headers 随行（request-options 套件往 URL 上
    // 挂 headers；String() 转换会洗掉，直读原对象）。
    if (a !== null && typeof a === "object" &&
        a.headers !== undefined && a.headers !== null && typeof a.headers === "object") {
      fromUrl.headers = a.headers;
    }
    if (typeof b === "function") return [fromUrl, b];
    return [{ ...fromUrl, ...(b ?? {}) }, c];
  }
  return [a, typeof b === "function" ? b : undefined];
}
export function requestFrom(ClientRequest, options, cb) {
  const req = new ClientRequest(options, cb);
  return req;
}
export function getFrom(ClientRequest, options, cb) {
  const req = requestFrom(ClientRequest, options, cb);
  req.end();
  return req;
}
