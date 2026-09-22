
class __ServerClass extends EventEmitter {
  constructor(options, cb) {
    super();
    // node Server（server-options 套件逐字）：function 即 connectionListener；
    // null/undefined 走 {}；其余非对象（0/'path'/true）→ ARG_TYPE validateObject 形。
    if (options !== undefined && options !== null && typeof options !== "object" && typeof options !== "function") {
      const e = new TypeError(`The "options" argument must be of type object. Received ${__netGot(options)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    this.__id = 0;
    this.__listening = null;
    this.allowHalfOpen = !!(options && typeof options === "object" && options.allowHalfOpen);
    // node 口径（server-keepalive 套件点名）：keepAlive/keepAliveInitialDelay 存自身
    // （默认 false/0，真机 26 实测）；_handle 在 listen 成功路径建桩（含 onconnection）。
    this.keepAlive = !!(options && typeof options === "object" && options.keepAlive);
    this.keepAliveInitialDelay = (options && typeof options === "object" && options.keepAliveInitialDelay !== undefined) ? options.keepAliveInitialDelay : 0;
    // server 选项面（blocklist/drop-connections/pause-on-connect 套件）
    this.pauseOnConnect = !!(options && typeof options === "object" && options.pauseOnConnect);
    this.__blockList = (options && typeof options === "object" && options.blockList) || null;
    this.__connsSet = new Set();
    this._handle = null;
    this.__pendingConnPayload = null;
    // transfer-guards：Server 同 Socket 不可 transfer（成功转移面另案，见 Socket 注）。
    try { (globalThis.__wjs_netXfer ??= new Map()).set(this, "net.Server"); } catch {}
    if (typeof options === "function") { cb = options; options = undefined; }
    if (typeof cb === "function") this.on("connection", cb);
    // 派发钩子预绑定（同 Socket 注）
    this.__ev = this.__ev.bind(this);
  }
  get listening() { return this.__listening !== null; }
  // _handle 建桩：onconnection 为箭头函数（构造/方法 this 捕获 server 本体，
  // 用户 `.call(handle, …)` 改绑不影响）；连接创建逻辑收敛 __acceptConn，
  // __ev connection 经此进入（套件包装 onconnection 即被调用）。
  __setupHandle() {
    this._handle = { onconnection: (err, clientHandle) => {
      const payload = this.__pendingConnPayload; this.__pendingConnPayload = null;
      if (err) { this.emit("error", err); return; }
      if (this.keepAlive && clientHandle && typeof clientHandle.setKeepAlive === "function") {
        try { clientHandle.setKeepAlive(true, this.keepAliveInitialDelay); } catch {}
      }
      this.__acceptConn(payload);
    } };
  }
  __acceptConn(payload) {
    const o = JSON.parse(payload);
    const s = new Socket();
    if (o.uds) s.__attachUds(o);
    else s.__attachConn(o);
    // 真机：server 侧 socket.server 全等 server 本体；本端地址族取监听地址。
    s.server = this;
    s.allowHalfOpen = !!this.allowHalfOpen;
    // server keepAlive 落已接受 socket（去重缓存预置，首个同值显式调用被吞，
    // server-keepalive 套件三调只进二）。
    if (this.keepAlive) { try { s.setKeepAlive(true, this.keepAliveInitialDelay); } catch {} }
    if (this.__listening && typeof this.__listening === "object") {
      s.localAddress = this.__listening.address;
      s.localPort = this.__listening.port;
      s.localFamily = this.__listening.family;
    }
    // blockList 拒绝：静默销毁，不进 connection（node 口径，server-blocklist 套件）。
    if (this.__blockList && s.remoteAddress && typeof this.__blockList.check === "function" && this.__blockList.check(s.remoteAddress)) {
      s.destroy();
      return;
    }
    // 超限：'drop' 事件（五元组）+ 销毁，不进 connection。node 语义：
    // maxConnections=0 即全拒（dormantServer 用例）；undefined = 不限。
    if (this.maxConnections !== undefined && this.maxConnections !== null &&
        (this.__conns ?? 0) >= this.maxConnections) {
      this.emit("drop", {
        localAddress: s.localAddress, localPort: s.localPort,
        remoteAddress: s.remoteAddress, remotePort: s.remotePort, remoteFamily: s.remoteFamily,
      });
      s.destroy();
      return;
    }
    if (this.pauseOnConnect) s.__paused = true;
    this.__conns = (this.__conns ?? 0) + 1;
    this.__connsSet.add(s);
    // 存根供 http CONNECT 隧道 detach 摘除（connect 套件 close:0 矩阵；摘除时
    // 手工同步记账，见 framing_outgoing CONNECT 分支）。
    s.__netConnsCleaner = () => { this.__conns = Math.max(0, (this.__conns ?? 1) - 1); this.__connsSet.delete(s); };
    s.once("close", s.__netConnsCleaner);
    this.emit("connection", s);
  }
  // node：销毁全部已接受连接（server-drop-connections 套件）。
  dropConnections() {
    for (const s of [...this.__connsSet]) {
      try { s.destroy(); } catch {}
    }
  }
  __doListen(port, host, cb, reusePort) {
    // relisten 必须清 close-during-listen 窗口旗（close() 置位后柄已清，
    // 重听的 listening 派发不再属"bind 窗口内 close"——残留 true 会吞掉
    // 新一轮 listening 派发，listening 回调永不触发）。
    this.__closing = false;
    if (port && typeof port === "object" && typeof port.address === "function" && port.__boundPort !== undefined) {
      // listen(bound)：adopt（旧柄失效；server 地址 = bound 地址）。
      if (port.__adopted) {
        const e = new Error("The bound socket has already been adopted by a server or socket");
        e.code = "ERR_SOCKET_HANDLE_ADOPTED"; throw e;
      }
      port.__adopted = true;
      if (port.__udsPath !== undefined) __boundPaths.delete(port.__udsPath);
      if (port.__holdToken) { try { __wjs_net_unhold(port.__holdToken); } catch {} }
      if (cb) this.once("listening", cb);
      this.__setupHandle();
      if (port.__isPipe) {
        this.__port = 0; this.__udsPath = port.__udsPath;
        this.__id = Number(__wjs_net_listen(0, "UDS:" + port.__udsPath, this));
      } else {
        this.__port = port.__boundPort;
        // reusePort 占位柄释放后重绑仍须带 SO_REUSEPORT（boundsocket reusePort
        // 双 listen 块；native 第 4 参 "1" 即开）。
        this.__id = Number(__wjs_net_listen(port.__boundPort, port.__boundHost, this, port.__reusePort === true ? "1" : ""));
      }
      return this;
    }
    if (typeof port === "string" || (port !== null && typeof port === "object")) {
      // node normalizeArgs：可解析为有限数的非空字符串 = TCP 端口（listen("0")
      // → 通配，真机 26 实证 address 回 port；旧实现一律当 UDS 路径，listen("0")
      // 建出名为 "0" 的套接字文件，listen-options 套件二次绑定即 EADDRINUSE）。
      // 其余字符串才是 IPC 路径（"abc" → path，真机同）。
      if (typeof port === "string" && port.trim() !== "" && Number.isFinite(Number(port))) {
        port = Number(port);
      } else {
        const p = typeof port === "string" ? port : port.path;
        let modeBits = "";
        if (port !== null && typeof port === "object") {
          if (port.readableAll) modeBits += "r";
          if (port.writableAll) modeBits += "w";
        }
        if (cb) this.once("listening", cb);
        this.__port = 0; this.__udsPath = String(p);
        this.__setupHandle();
        this.__id = Number(__wjs_net_listen(0, "UDS:" + String(p) + "\n" + modeBits, this));
        return this;
      }
    }
    if (port === undefined || port === null) port = 0;
    __vPort(port, "options.port");
    if (cb) this.once("listening", cb);
    this.__port = Number(port);
    this.__setupHandle();
    // reusePort 直通 native 第 4 参（child reuseport 套件：fork 共享端口；
    // BoundSocket-adopt 路径早有同款，此处 direct 路径补齐）。
    this.__id = Number(__wjs_net_listen(Number(port), host === null ? "0.0.0.0" : host, this, reusePort === true ? "1" : ""));
    return this;
  }
  // node 口径：listen(cb)/listen()/listen(null) 即 listen(0)；listen(port[, host][, cb])
  // 全形态（port 缺省 0；cb 可在任意位置）。
  listen(...args) {
    // node：listening 期间再 listen 即同步抛（真机 26：`Error ERR_SERVER_ALREADY_LISTEN
    // "Listen method has been called more than once without closing."`；close 同步
    // 清柄故 close 后可再听——call-listen-multiple 套件三段全覆盖）。
    if (this._handle) {
      const e = new Error("Listen method has been called more than once without closing.");
      e.code = "ERR_SERVER_ALREADY_LISTEN";
      throw e;
    }
    let port, host = null, cb = null;
    if (typeof args[0] === "function") return this.__doListen(0, null, args[0]);
    if (args[0] === undefined || args[0] === null) {
      for (let i = 1; i < args.length; i++) {
        if (typeof args[i] === "string" && host === null) host = args[i];
        else if (typeof args[i] === "function") cb = args[i];
      }
      return this.__doListen(0, host, cb);
    }
    if (args[0] && typeof args[0] === "object" && typeof args[0].address === "function" && args[0].__boundPort !== undefined)
      return this.__doListen(args[0], null, typeof args[1] === "function" ? args[1] : null);
    if (typeof args[0] === "object" && args[0] !== null) {
      const o = args[0];
      const cb0 = typeof args[1] === "function" ? args[1] : null;
      // node addServerAbortSignalOption：port/path 分派前校验（ARG_TYPE 逐字）+
      // abort 即 close（pre-aborted 走 nextTick 同位微任务）。
      if (o.signal !== undefined) {
        if (o.signal === null || typeof o.signal !== "object" || !("aborted" in o.signal)) {
          const e = new TypeError(`The "options.signal" property must be an instance of AbortSignal. Received ${__netGot(o.signal)}`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        if (o.signal.aborted) queueMicrotask(() => { try { this.close(); } catch {} });
        else {
          // abort-controller 套件：直调 addEventListener 的监听须入侧表
          // （events.listenerCount 对 EventTarget 读侧表；原生忽略 once，自摘）。
          const __sigHandler = () => { __etRemove(o.signal, "abort", __sigHandler); try { this.close(); } catch {} };
          o.signal.addEventListener("abort", __sigHandler, { once: true });
          __etAdd(o.signal, "abort", __sigHandler);
        }
      }
      if (typeof o.fd === "number" && o.fd >= 0) {
        // node 口径：listen({fd}) 非法 fd 即异步 EINVAL（error 事件；真机实证）；
        // 负数/非数值 fd 落 node 尾 throw（{fd:-1} → 'must have the property'）。
        if (cb0) this.once("listening", cb0);
        queueMicrotask(() => {
          const e = new Error(`listen EINVAL: invalid argument`);
          e.code = "EINVAL"; e.syscall = "listen"; e.errno = -4071;
          this.emit("error", e);
        });
        return this;
      }
      if (("port" in o) && (o.port === undefined || o.port === null)) {
        // node：port 显式 undefined/null 即 0（listen({port}) 通配）。
        return this.__doListen(0, o.host ?? null, cb0, o.reusePort);
      }
      if (typeof o.port === "number" || typeof o.port === "string") {
        // node：port 分支先于 path（{port:-1, path} 点名 BAD_PORT 先抛）。
        __vPort(o.port, "options.port");
        return this.__doListen(o.port, o.host ?? null, cb0, o.reusePort);
      }
      if (o.path && typeof o.path === "string") {
        const o0 = { path: String(o.path) };
        if (o.readableAll !== undefined) o0.readableAll = !!o.readableAll;
        if (o.writableAll !== undefined) o0.writableAll = !!o.writableAll;
        return this.__doListen(o0, null, cb0);
      }
      // node 尾两 throw：无 port/path 键（{}/fd 负数）→ 'must have the property'；
      // 有键但不合格（{port:false}/{path:-1}）→ 'is invalid'（均 ARG_VALUE inspect 形）。
      if (!("port" in o) && !("path" in o)) {
        const e = new TypeError(`The argument 'options' must have the property "port" or "path". Received ${__netInspect(o)}`);
        e.code = "ERR_INVALID_ARG_VALUE"; throw e;
      }
      const e = new TypeError(`The argument 'options' is invalid. Received ${__netInspect(o)}`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    } else {
      port = args[0];
      // node normalizeArgs：非对象/非 pipe 首参（含 boolean）进 options.port，
      // listen(true/false) 落尾 throw ARG_VALUE('options')（inspect { port: true } 形）。
      if (typeof port === "boolean") {
        const e = new TypeError(`The argument 'options' is invalid. Received ${__netInspect({ port })}`);
        e.code = "ERR_INVALID_ARG_VALUE"; throw e;
      }
      for (let i = 1; i < args.length; i++) {
        // host 位：首参为数字（含数字字符串，node normalizeArgs 同判）才轮到 host。
        const portNum = typeof port === "number" || (typeof port === "string" && port.trim() !== "" && Number.isFinite(Number(port)));
        if (typeof args[i] === "string" && host === null && portNum) host = args[i];
        else if (typeof args[i] === "function") cb = args[i];
      }
    }
    return this.__doListen(port, host, cb);
  }
  __ev(kind, payload) {
    switch (kind) {
      case "listening": {
        // bind 窗口内 close()（listen-close-server 套件）：listening 派发吞掉，
        // listening 回调永不触发；'close' 由 destroy 路径照常发。
        if (this.__closing) break;
        const o = JSON.parse(payload);
        if (o.uds) {
          this.__listening = o.path;
          this._connectionKey = `unix:${o.path}`;
          this.emit("listening");
          break;
        }
        this.__listening = { address: o.addr, port: o.port, family: String(o.addr).includes(":") ? "IPv6" : "IPv4" };
        // node Server._connectionKey：'<family>:<host>:<请求端口>'（listen(0) 键含 '0'）
        this._connectionKey = `${String(o.addr).includes(":") ? 6 : 4}:${o.addr}:${this.__port}`;
        this.emit("listening");
        break;
      }
      case "connection": {
        // 经 _handle.onconnection 进入（套件可包装观测；缺桩回落直建）。
        this.__pendingConnPayload = payload;
        const h = this._handle;
        if (h && typeof h.onconnection === "function") h.onconnection(null, { setKeepAlive: (en, ms) => {} });
        else this.__acceptConn(payload);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        const e = __netErr(o.code, o.msg);
        e.port = this.__listening ? this.__listening.port : this.__port;
        // node bind 系标配：syscall + errno（EADDRINUSE=-4091/-48；EACCES=-4092/-13）。
        e.syscall = "listen";
        e.errno = o.code === "EADDRINUSE" ? -4091 : (o.code === "EACCES" ? -4092 : -4094);
        // listen 失败柄即清：error 后可立即重听（node 口径，call-listen-multiple
        // 第一段；ALREADY_LISTEN 守卫读 `_handle`，不清即卡死重听）。
        this._handle = null; this.__id = 0;
        this.emit("error", e);
        break;
      }
      case "close": this.__listening = null; this.emit("close"); break;
    }
  }
  address() { return this.__listening; }
  // node 口径：getConnections(cb) 异步回现存连接数；无 cb 直回数（真机同）。
  // 计数位由 connection/+socket-close 维护（server 侧 socket close 即减）。
  getConnections(cb) {
    const n = this.__conns ?? 0;
    if (typeof cb === "function") { queueMicrotask(() => { try { cb(null, n); } catch {} }); return this; }
    return n;
  }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.__id) {
      __wjs_net_destroy(this.__id);
      // 柄同步即清（node 口径：close 后 listen 立即可用，call-listen-multiple 第三段）。
      // __closing 旗拦 bind 窗口内已就绪的 Listening 派发（listen-close-server
      // 套件：close() 后 listening 回调必须永不触发）。
      this.__closing = true;
      this.__id = 0;
      this._handle = null;
      this.__listening = undefined;
    }
    return this;
  }
  // 10a：ref 真计数（同 Socket）。
  ref() { if (this.__id) __wjs_net_ref(this.__id); return this; }
  unref() { if (this.__id) __wjs_net_unref(this.__id); return this; }
}

Socket.prototype.__attachConn = function (info) {
  this.__id = Number(info.connId);
  this.__connected = true;
  this.__handleClosed = false;
  this._handle = this.__makeHandle();
  this.remoteAddress = info.remoteAddress;
  this.remotePort = info.remotePort;
  this.remoteFamily = String(info.remoteAddress).includes(":") ? "IPv6" : "IPv4";
  if (info.serverId !== undefined) this.server = info.serverId;
  this.localAddress = info.localAddress;
  this.localPort = info.localPort;
  this.readable = true; this.writable = true;
  __wjs_net_attach(this.__id, this);
};
// Node Socket.unshift：字节塞回读流头部（ws setSocket 对升级残留用）；
// 本仓读流无 JS 侧缓冲，以 data 事件回灌近似（先于后续 pump chunk——
// microtask 时序，残余错序仅限升级瞬间的罕见重叠帧）。
Socket.prototype.__attachUds = function (info) {
  this.__id = Number(info.connId);
  this.__connected = true;
  this.__handleClosed = false;
  this._handle = this.__makeHandle();
  this.remoteAddress = undefined; this.remotePort = undefined;
  this.localAddress = undefined; this.localPort = undefined;
  this.remoteFamily = undefined;
  this.readable = true; this.writable = true;
  __wjs_net_attach(this.__id, this);
};
Socket.prototype.unshift = function (chunk) {
  if (chunk && chunk.length > 0) queueMicrotask(() => this.emit("data", chunk));
  return this;
};

export function createServer(options, cb) {
  return new Server(options, cb);
}
// Node 口径：Server/Socket 裸调用返回新实例（lib/net.js 原文
// `if (!(this instanceof Server)) return new Server(...)`）。
function Server(...args) {
  if (!(this instanceof __ServerClass)) return new __ServerClass(...args);
  return Reflect.construct(__ServerClass, args, new.target ?? __ServerClass);
}
Object.setPrototypeOf(Server, __ServerClass);
Server.prototype = __ServerClass.prototype;
export function createConnection(...args) {
  // node 口径（incoming-message-options 套件）：options 进 Socket 构造器
  //（readableHighWaterMark 等流选项生效；旧无参构造即丢）。
  const __o = args.length > 0 && args[0] !== null && typeof args[0] === "object" && !Array.isArray(args[0]) ? args[0] : undefined;
  return new Socket(__o).connect(...args);
}
export const connect = createConnection;
// node BoundSocket（同步 bind 句柄；adopt 即迁入 server/socket，旧柄失效）。
// 底座：TCP 占位 bind（地址/冲突语义真，fd 桩 -1 记档）；UDS 真 bind（path 串）。
// 校验族对 lib/net.js：非对象 → ERR_INVALID_ARG_TYPE；host 非法串（localhost 等
// 不可 bind 名）→ ERR_INVALID_ARG_VALUE；bind 失败 → code+syscall=bind。
class BoundSocket {
  constructor(options) {
    // node 口径：无参即 {}（0.0.0.0:0 通配）；null/数组/非对象才 ARG_TYPE。
    if (options === undefined) options = {};
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      const e = new TypeError(`The "options" argument must be of type object. Received ${options === null ? "null" : Array.isArray(options) ? "an instance of Array" : typeof options}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    const { host, port, path, ipv6Only, reusePort } = options;
    // path 与 TCP 族互斥（真机 ERR_INVALID_ARG_VALUE）；path 非串 → ARG_TYPE。
    if (path !== undefined) {
      if (typeof path !== "string") {
        const e = new TypeError(`The "options.path" property must be of type string. Received type ${typeof path}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      for (const k of ["host", "port", "ipv6Only", "reusePort"]) {
        if (options[k] !== undefined) {
          const e = new TypeError(`The "options.path" property cannot be used with "options.${k}"`);
          e.code = "ERR_INVALID_ARG_VALUE"; throw e;
        }
      }
    }
    if (host !== undefined && typeof host !== "string") {
      const e = new TypeError(`The "options.host" property must be of type string. Received type ${typeof host}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (host !== undefined && (host === "localhost" || host === "")) {
      const e = new TypeError(`The "options.host" property must be a valid IP address or hostname. Received '${host}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    this.__adopted = false;
    this.__isPipe = path !== undefined;
    if (path !== undefined) {
      // 抽象地址（首字节 \0）仅 Linux 支持；其余平台同步 ERR_INVALID_ARG_VALUE（真机口径）。
      if (String(path).charCodeAt(0) === 0) {
        let isLinux = false;
        try { isLinux = process.platform === "linux"; } catch {}
        if (!isLinux) {
          const e = new TypeError(`The "options.path" property must be a valid path. Received '${path}'`);
          e.code = "ERR_INVALID_ARG_VALUE"; throw e;
        }
      }
      // 路径形态预检（同步抛，不进 native）：超长 → EINVAL；缺父目录 → EACCES。
      // （真机：uv_pipe_bind + UV_PIPE_NO_TRUNCATE；libuv ENOENT→EACCES 跨平台对齐。）
      // 同 path 重复 bind → EADDRINUSE（真机 pipe bind 同步语义；抽象地址走内存集合）。
      if (__boundPaths.has(String(path))) {
        const e = new Error(`bind EADDRINUSE ${String(path)}`);
        e.code = "EADDRINUSE"; e.syscall = "bind"; e.errno = -4091; throw e;
      }
      if (String(path).charCodeAt(0) !== 0) {
        // 缺父目录 → 同步 EACCES（真机实证：ENOENT→EACCES 跨平台对齐；__bs_cut 误判曾疑此，
        // 实为探针路径非法——node 同路径同 EACCES，检查无辜）。
        // 相对路径先绝对化再取父（与 kernel bind 同解析；cwd 即进程 cwd）。
        if (String(path).length > 100) {
          const e = new Error(`bind EINVAL ${String(path).slice(0, 40)}...`);
          e.code = "EINVAL"; e.syscall = "bind"; e.errno = -4071; throw e;
        }
        {
          const abs = String(path).startsWith("/") ? String(path) : (process.cwd() + "/" + String(path));
          const norm = abs.split("/").filter((x) => x !== "" && x !== ".").join("/");
          const parts = norm.split("/");
          parts.pop();
          const parent = "/" + parts.join("/");
          let parentExists = false;
          try { __wjs_fs_stat(parent, true); parentExists = true; } catch { parentExists = false; }
          if (!parentExists) {
            const e = new Error(`bind EACCES ${String(path)}`);
            e.code = "EACCES"; e.syscall = "bind"; e.errno = -4092; throw e;
          }
        }
      }
      let bound;
      try { bound = __wjs_net_bind("", 0, String(path)); }
      catch (e) { throw __bindErr(String((e && e.message) || e)); }
      this.__udsPath = String(path);
      this.__boundPort = 0;
      __boundPaths.add(String(path));
    } else {
      // node：ipv6Only 即绑定 IPv6 通配 '::'（boundsocket IPv6 块：address '::'/IPv6）。
      const h = host !== undefined ? host : (ipv6Only === true ? "::" : "0.0.0.0");
      const p = port !== undefined ? Number(port) : 0;
      let bound;
      // reusePort → native 走 SO_REUSEPORT bind（真机 macOS/Linux 同支持；
      // 平台不支持由 native setsockopt 失败即 Err，套件 probe 落 first=null 跳过）。
      try { bound = __wjs_net_bind(h, p, "", reusePort === true ? "1" : ""); }
      catch (e) { throw __bindErr(String((e && e.message) || e)); }
      // native 回 "port:token"（占位保活；close/adopt 时 __wjs_net_unhold(token) 释放）。
      const parts = String(bound).split(":");
      this.__boundHost = h;
      this.__boundPort = Number(parts[0]);
      this.__holdToken = Number(parts[1] || 0);
    }
    this.__ipv6Only = !!ipv6Only; this.__reusePort = !!reusePort;
  }
  get isPipe() { return this.__isPipe; }
  address() {
    if (this.__adopted) {
      const e = new Error("The bound socket has already been adopted by a server or socket");
      e.code = "ERR_SOCKET_HANDLE_ADOPTED"; throw e;
    }
    if (this.__isPipe) return this.__udsPath;
    const h = this.__boundHost;
    return { address: h, port: this.__boundPort, family: h.includes(":") ? "IPv6" : "IPv4" };
  }
  fd() {
    if (this.__adopted) {
      const e = new Error("The bound socket has already been adopted by a server or socket");
      e.code = "ERR_SOCKET_HANDLE_ADOPTED"; throw e;
    }
    // 真 fd：占位 listener dup（unix；win 回 -1，套件 win 侧只断类型）。
    if (this.__holdToken) return Number(__wjs_net_fd(this.__holdToken));
    return -1;
  }
  close() {
    if (this.__adopted) {
      const e = new Error("The bound socket has already been adopted by a server or socket");
      e.code = "ERR_SOCKET_HANDLE_ADOPTED"; throw e;
    }
    if (this.__udsPath !== undefined) __boundPaths.delete(this.__udsPath);
    if (this.__holdToken) { try { __wjs_net_unhold(this.__holdToken); } catch {} }
    this.__adopted = true; // close 即失效（真机二次 close 同 ADOPTED 口径）
  }
}
// native BINDFAIL 文本 → code + syscall=bind 整形（node "bind CODE addr" 形）。
function __bindErr(msg) {
  const m = /^BINDFAIL (\S+): bind (\S+) (.*)$/.exec(msg);
  const code = m ? m[1] : "EADDRINUSE";
  const e = new Error(m ? `bind ${m[2]} ${m[3]}` : msg);
  e.code = code; e.syscall = "bind"; e.errno = -4078;
  return e;
}
export { Socket, Server, BoundSocket };
// node legacy：net.Stream(...) 无 new 可调（lib/net.js `Stream.Stream = Stream` 族）
export const Stream = new Proxy(Socket, {
  apply(_t, _this, args) { return new Socket(...args); },
});
// Node `net.isIP/isIPv4/isIPv6`（vite 请求路径 host 校验用；std 解析对齐语义）
// ── 10f net 对拍：校验器 + IP 解析纯 JS（std 拒前导零，node 收）─────────
// node `invalidArgTypeHelper` 口径（ARG_TYPE 助记形：`Received type string ('x')`/
// `Received null`/`Received an instance of Array`；ARG_VALUE 走 inspect 形，§4.114）。
function __netGot(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  if (typeof v === "string") return `type string ('${v}')`;
  if (typeof v === "object") return `an instance of ${v.constructor?.name ?? "Object"}`;
  return `type ${typeof v} (${String(v)})`;
}
// node inspect 简形（ARG_VALUE 收尾 `Received { port: false }` 形；listen-options 套件
// 断言到正则 `Received .+`，浅对象逐键 node 形即可，深结构记档）。
function __netInspect(v) {
  if (v === null || v === undefined) return String(v);
  if (typeof v === "string") return `'${v}'`;
  if (typeof v === "number" || typeof v === "boolean" || typeof v === "bigint") return String(v);
  if (typeof v !== "object") return `[${typeof v}]`;
  if (Array.isArray(v)) return "[Array]";
  try {
    const ks = Object.keys(v);
    if (ks.length === 0) return "{}";
    return `{ ${ks.map((k) => `${k}: ${__netInspect(v[k])}`).join(", ")} }`;
  } catch { return "{}"; }
}
// node validatePort（lib/internal/validators.js 逐字语义）：number/string、串 trim 非空、
// `+p === (+p >>> 0)`、`p <= 0xFFFF`（'0x10' 数值线名同收——connect-options-port
// canConnect('0x..') 点名；123.456/-1/65536/NaN/±Infinity 拒）。
// name：connect 系缺省 'Port'；listen 系传 'options.port'（真机两口径）。
function __vPort(p, name = "Port") {
  if ((typeof p !== "number" && typeof p !== "string") ||
      (typeof p === "string" && p.trim().length === 0) ||
      +p !== (+p >>> 0) ||
      p > 0xFFFF) {
    const e = new RangeError(`${name} should be >= 0 and < 65536. Received ${__netGot(p)}.`);
    e.code = "ERR_SOCKET_BAD_PORT"; throw e;
  }
  return p | 0;
}
// connect(options.port) ARG_TYPE 门（lookupAndConnect：类型不合先于 BAD_PORT；
// 真机文案 `must be one of type number or string`，name 恒 'options.port'）。
function __vPortType(p) {
  if (typeof p !== "number" && typeof p !== "string") {
    const e = new TypeError(`The "options.port" property must be one of type number or string. Received ${__netGot(p)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
}
function __isIPv4(s) {
  if (typeof s !== "string") return false;
  const parts = s.split(".");
  if (parts.length !== 4) return false;
  for (const p of parts) {
    if (!/^[0-9]{1,3}$/.test(p)) return false;
    if (p.length > 1 && p.charCodeAt(0) === 48) return false;
    if (Number(p) > 255) return false;
  }
  return true;
}
function __v6Groups(head, tail) {
  const group = (g) => /^[0-9A-Fa-f]{1,4}$/.test(g);
  if (tail === null) return head.length === 8 && head.every(group);
  return head.length + tail.length <= 7 && head.every(group) && tail.every(group);
}
function __isIPv6(s) {
  if (typeof s !== "string" || s === "") return false;
  // node 口径：尾部 %zone 可选（'fe80::2008%eth0' → 6；'@' 等非法字符拒）
  const zi = s.indexOf("%");
  if (zi !== -1) {
    const zone = s.slice(zi + 1);
    if (zone === "" || !/^[A-Za-z0-9._~-]+$/.test(zone)) return false;
    s = s.slice(0, zi);
  }
  let head = s, tail = null;
  const i = s.indexOf("::");
  if (i !== -1) {
    if (s.indexOf("::", i + 1) !== -1) return false;
    head = s.slice(0, i);
    tail = s.slice(i + 2);
  }
  let hp = head === "" ? [] : head.split(":");
  let tp = tail === null ? null : (tail === "" ? [] : tail.split(":"));
  if (tp !== null && tp.length > 0 && tp[tp.length - 1].includes(".")) {
    const v4 = tp.pop();
    if (!__isIPv4(v4)) return false;
    tp.push("0", "0");
  } else if (tp === null && hp.length > 0 && hp[hp.length - 1].includes(".")) {
    const v4 = hp.pop();
    if (!__isIPv4(v4)) return false;
    hp.push("0", "0");
  }
  return __v6Groups(hp, tp);
}
export function isIP(input) {
  // node 口径：对象经 String() 转换（{ toString: () => '127.0.0.1' } → 4）
  const s = (input && typeof input === "object") ? String(input) : input;
  if (typeof s !== "string") return 0;
  if (__isIPv4(s)) return 4;
  if (__isIPv6(s)) return 6;
  return 0;
}
// 已占 pipe 路径集合（BoundSocket 同步 EADDRINUSE 判重；close 即释放）。
// 文件形 bind 失败（残留 vs 真占用）由 native 探活区分；此处集合只管抽象地址 + 文件形已占标记。
const __boundPaths = new Set();
// net.BlockList（10f，node 口径：v4/v6 BigInt 二元比较；规则 = address/subnet/range）
class BlockList {
  #rules = [];
  addAddress(ip, type) {
    const t = type ?? (isIP(ip) === 4 ? "ipv4" : isIP(ip) === 6 ? "ipv6" : null);
    const one = __blOne(ip, t);
    one.type = "address";
    this.#rules.push(one);
    return this;
  }
  addSubnet(net, prefix, type) {
    const t = type ?? (isIP(net) === 4 ? "ipv4" : isIP(net) === 6 ? "ipv6" : null);
    const bits = t === "ipv4" ? 32 : 128;
    const p = Number(prefix);
    if (!(Number.isInteger(p) && p >= 0 && p <= bits)) {
      const e = new RangeError(`The "prefix" argument must be >= 0 and <= ${bits}. Received ${prefix}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    const n = __blOne(net, t);
    this.#rules.push({ t, net: n.ip & __blMask(n.ip, p, bits), mask: __blMask(n.ip, p, bits), type: "subnet", prefix: p });
    return this;
  }
  addRange(start, end, type) {
    let t = type ?? (isIP(start) === 4 ? "ipv4" : isIP(start) === 6 ? "ipv6" : null);
    const a = __blOne(start, t), b = __blOne(end, t);
    if (a.t !== b.t) { const e = new TypeError("IP addresses must be of the same family"); e.code = "ERR_INVALID_ARG_TYPE"; throw e; }
    this.#rules.push({ t, min: a.ip < b.ip ? a.ip : b.ip, max: a.ip < b.ip ? b.ip : a.ip, type: "range" });
    return this;
  }
  check(ip, type) {
    const t = type ?? (isIP(ip) === 4 ? "ipv4" : isIP(ip) === 6 ? "ipv6" : null);
    let one;
    try { one = __blOne(ip, t); } catch { return false; }
    return this.#rules.some((r) => r.t === t && (
      r.type === "address" ? r.ip === one.ip
      : r.type === "subnet" ? (one.ip & r.mask) === r.net
      : one.ip >= r.min && one.ip <= r.max));
  }
  get size() { return this.#rules.length; }
}
function __blMask(ip, prefix, bits) {
  return prefix === 0 ? 0n : ((1n << BigInt(prefix)) - 1n) << BigInt(bits - prefix);
}
function __blOne(ip, t) {
  if (t === "ipv4") {
    if (!__isIPv4(ip)) { const e = new TypeError(`The "ip" argument must be a valid IPv4 address. Received '${ip}'`); e.code = "ERR_INVALID_ARG_VALUE"; throw e; }
    const p = ip.split(".");
    let n = 0n;
    for (const x of p) n = (n << 8n) | BigInt(Number(x));
    return { ip: n, t };
  }
  if (t === "ipv6") {
    if (!__isIPv6(ip)) { const e = new TypeError(`The "ip" argument must be a valid IPv6 address. Received '${ip}'`); e.code = "ERR_INVALID_ARG_VALUE"; throw e; }
    let head = ip, tail = null;
    const i = ip.indexOf("::");
    if (i !== -1) { head = ip.slice(0, i); tail = ip.slice(i + 2); }
    let hp = head === "" ? [] : head.split(":");
    let tp = tail === null ? null : (tail === "" ? [] : tail.split(":"));
    if (tp !== null && tp.length > 0 && tp[tp.length - 1].includes(".")) {
      const v4 = tp.pop();
      const p4 = v4.split(".");
      const hi = (Number(p4[0]) << 8) | Number(p4[1]);
      const lo = (Number(p4[2]) << 8) | Number(p4[3]);
      tp.push(hi.toString(16), lo.toString(16));
    } else if (tp === null && hp.length > 0 && hp[hp.length - 1].includes(".")) {
      const v4 = hp.pop();
      const p4 = v4.split(".");
      const hi = (Number(p4[0]) << 8) | Number(p4[1]);
      const lo = (Number(p4[2]) << 8) | Number(p4[3]);
      hp.push(hi.toString(16), lo.toString(16));
    }
    const fill = tp === null ? 0 : 8 - hp.length - tp.length;
    const all = tp === null ? hp : [...hp, ...Array(fill).fill("0"), ...tp];
    let n = 0n;
    for (const g of all) n = (n << 16n) | BigInt(parseInt(g, 16));
    return { ip: n, t };
  }
  const e = new TypeError(`The "type" argument must be 'ipv4' or 'ipv6'. Received '${t}'`);
  e.code = "ERR_INVALID_ARG_TYPE"; throw e;
}
Object.defineProperty(BlockList.prototype, Symbol.toStringTag, { value: "BlockList" });
BlockList.ValidateString = (ip, type) => { __blOne(ip, type); return true; };
export { BlockList };
export function isIPv4(input) { return isIP(input) === 4; }
export function isIPv6(input) { return isIP(input) === 6; }
// Happy-eyeballs 超时存值（10f：test/common 前置；连接侧暂不实现自动族选择，记档）。
let __autoSelectTimeout = 500;
// 默认自动族选择开关（真机 26 默认 true，实测）；连接侧 Happy Eyeballs 未实现，记档。
let __autoSelectFamily = true;
export function getDefaultAutoSelectFamily() { return __autoSelectFamily; }
export function setDefaultAutoSelectFamily(value) { __autoSelectFamily = !!value; }
export function getDefaultAutoSelectFamilyAttemptTimeout() { return __autoSelectTimeout; }
export function setDefaultAutoSelectFamilyAttemptTimeout(value) {
  // 真机口径（HE 校验族套件）：int [1,60000] 之外 OUT_OF_RANGE；
  // 存取钳 [10,60000]（1/9 → getDefault 10，套件逐项）。
  if (typeof value !== "number" || Number.isNaN(value)) {
    const err = new TypeError("timeout must be a number");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!Number.isFinite(value) || !Number.isInteger(value) || value < 1 || value > 60000) {
    const err = new RangeError(`The value of "autoSelectFamilyAttemptTimeout" is out of range. It must be an integer >= 1 && <= 60000. Received ${String(value)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  __autoSelectTimeout = Math.min(Math.max(value, 10), 60000);
}
const __api = { Socket, Server, BlockList, BoundSocket, createServer, createConnection, connect, Stream, isIP, isIPv4, isIPv6, getDefaultAutoSelectFamily, setDefaultAutoSelectFamily, getDefaultAutoSelectFamilyAttemptTimeout, setDefaultAutoSelectFamilyAttemptTimeout };
export default __api;
