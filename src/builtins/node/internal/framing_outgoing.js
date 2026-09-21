
export class OutgoingMessage extends Writable {
  constructor(options) {
    // autoDestroy 关（同 ServerResponse：销毁一律显式）。
    // emitClose 关：req 'close' 在响应收齐/连接收尾时手动发出（9d 口径：
    // 上传 finish 不等于请求结束），见 __finishResponse/__onSockCloseEv。
    super({ autoDestroy: false, emitClose: false });
    this.headersSent = false;
    this.socket = null;
    // 独立构造（`new OutgoingMessage()`，outgoing-properties 系套件）：无 socket
    // 时 _write 缓冲不落盘——cb 不调（writableLength 保持，Node outputData 口径），
    // 有子类 socket 面时由子类 _write 覆写。
    this.__outputData = [];
  }
  _write(chunk, encoding, cb) {
    this.__outputData.push([chunk, encoding, cb]);
  }
  _implicitHeader() {
    throw new Error("_implicitHeader() method is not implemented");
  }
  // node 口径：destroy(err) 不外发 'error'（仅记 errored），异步发一次 'close'
  //（outgoing-destroyed 套件：destroyed/closed/errored 三面 + close 事件）。
  destroy(err) {
    if (this.destroyed) return this;
    this.__omErrored = err ?? null;
    const __swallow = () => {};
    this.on("error", __swallow);
    const __ret = super.destroy(err);
    queueMicrotask(() => {
      this.removeListener("error", __swallow);
      if (!this.__closeEmitted) {
        this.__closeEmitted = true;
        this.emit("close");
      }
    });
    return __ret;
  }
  get errored() {
    return this.__omErrored ?? (this._writableState ? this._writableState.errored : null);
  }
}

// 头段内裸 CR（后随非 LF）检测——client/server 两侧严格门共用
//（client-reject-cr-no-lf 套件；服务端同形走 400 通道）。
function __hasBareCR(headText) {
  for (let i = 0; i < headText.length; i++) {
    if (headText[i] === "\r" && headText[i + 1] !== "\n") return true;
  }
  return false;
}

// 客户端解析错（node llhttp 口径）：message 'Parse Error: ...' + HPE_* 码
//（client-reject-* 套件断言 err.code 与 /^Parse Error/）。
function __hpe(code, msg) {
  const e = new Error(`Parse Error: ${msg}`);
  e.code = code;
  return e;
}

// node Writable 口径：end 后写不抛——错误走 cb（有则）或下一拍 'error'
//（一次）；errored 后续写静默 false（server-write-after-end/outgoing-destroyed
// 套件真机对拍）。
// node Writable 口径：end 后写不抛——错误走 cb（有则）或下一拍 'error'
//（一次）；errored 后续写静默 false（server-write-after-end/outgoing-destroyed
// 套件真机对拍）。
function __writeAfterEnd(msg, encoding, cb) {
  if (msg.__waeErrored || msg.__waeQueued) return false;
  msg.__waeQueued = true;
  const f = typeof encoding === "function" ? encoding : cb;
  queueMicrotask(() => {
    msg.__waeQueued = false;
    msg.__waeErrored = true;
    const err = new Error("ERR_STREAM_WRITE_AFTER_END: write after end");
    err.code = "ERR_STREAM_WRITE_AFTER_END";
    if (typeof f === "function") f(err);
    else msg.emit("error", err);
  });
  return false;
}

// 服务端混入：Base = net.Server / tls.Server（构造实参原样透传基类）。
// http 面选项（10f 对拍，node lib/_http_server.js 口径）：requestTimeout 默认
// 300000、headersTimeout 默认 min(60000, requestTimeout)、keepAliveTimeout 5000、
// keepAliveTimeoutBuffer 1000；headersTimeout > requestTimeout 即 ERR_OUT_OF_RANGE。
export function withHttpServer(Base) {
  // 初始化逻辑独立成函数：`new Server()` 走构造器，`http.Server.call(this)`
  //（upgrade-server 套件 testServer 老式继承）直接在 this 上跑同一段。
  function __initServer(self, args) {
      const o = (args[0] && typeof args[0] === "object" && !Array.isArray(args[0])) ? args[0] : {};
      self.timeout = 0;
      self.requestTimeout = 300_000;
      self.headersTimeout = 60_000;
      self.keepAliveTimeout = 5_000;
      self.keepAliveTimeoutBuffer = 1_000;
      self.maxRequestsPerSocket = 0;
      // 每服务器宽松解析旗（insecure-parser-per-stream 套件）。
      // httpValidation 门（node storeHTTPOptions 口径：validateOneOf + 与
      // insecureHTTPParser 互斥，ERR_INVALID_ARG_VALUE）。
      self.__inboundMode = __parseModeOf(__resolveHttpValidation(o.httpValidation, o.insecureHTTPParser));
      self.insecureHTTPParser = o.insecureHTTPParser ?? false;
      // node highWaterMark 选项（server-options-highwatermark 套件：req 流
      // HWM 与 res[kHighWaterMark] 同源；缺省 getDefaultHighWaterMark()）。
      self.__highWaterMark = o.highWaterMark;
      const rt = o.requestTimeout !== undefined ? __validateInteger(o.requestTimeout, "requestTimeout") : undefined;
      if (rt !== undefined) self.requestTimeout = rt;
      const ht = o.headersTimeout !== undefined ? __validateInteger(o.headersTimeout, "headersTimeout") : undefined;
      self.headersTimeout = ht !== undefined ? ht : Math.min(60_000, self.requestTimeout);
      if (self.requestTimeout > 0 && self.headersTimeout > 0 && self.headersTimeout > self.requestTimeout) {
        throw new codes.ERR_OUT_OF_RANGE("headersTimeout", "<= requestTimeout", o.headersTimeout);
      }
      const kt = o.keepAliveTimeout !== undefined ? __validateInteger(o.keepAliveTimeout, "keepAliveTimeout") : undefined;
      if (kt !== undefined) self.keepAliveTimeout = kt;
      const kb = o.keepAliveTimeoutBuffer !== undefined ? __validateInteger(o.keepAliveTimeoutBuffer, "keepAliveTimeoutBuffer") : undefined;
      if (kb !== undefined) self.keepAliveTimeoutBuffer = kb;
      if (o.maxRequestsPerSocket !== undefined) self.maxRequestsPerSocket = o.maxRequestsPerSocket;
      self.__closing = false;
      self.__sockets = new Set();
      // node setupConnectionsTracking 口径（真机 toString 逐字对拍）：listening
      // 即起连接检查 timer（unref），每次 listening 先清旧再建新——clear-timer
      // 套件连手工 emit('listening') 形态都点名（旧 timer 须 _destroyed）。
      // running 态 tick 不杀 keep-alive 空闲连接（真机 600ms×4 tick 实测不杀）。
      self.on("listening", () => {
        const __cur = self[kConnectionsCheckingInterval];
        if (__cur !== undefined) clearInterval(__cur);
        const __iv = setInterval(() => {
          // closing 后清空闲连接（node checkConnections 看门狗口径；常规路径
          // close() 已同步 clearInterval，此 tick 仅守非同步收尾形态）。
          if (!self.__closing) return;
          for (const s of [...self.__sockets]) {
            const st = s.__httpState;
            if (!st || st.req === null) { try { s.destroy(); } catch { /* gone */ } }
          }
        }, 1000);
        if (typeof __iv.unref === "function") __iv.unref();
        self[kConnectionsCheckingInterval] = __iv;
      });
      self.on("connection", (sock) => {
        self.__sockets.add(sock);
        const st = { buf: new Uint8Array(0), req: null, framing: null, res: null, __hdT: null, __rqT: null, __kaT: null };
        sock.__httpState = st;
        sock.on("close", () => {
          self.__clearReqTimers(st);
          self.__sockets.delete(sock);
          // 连接断时未完的req/res一起收尾：req destroy触发pipeline的
          // PREMATURE_CLOSE（客户端中断上传用例），res destroy防写半开。
          if (st.req !== null && !st.req.complete && !st.req.destroyed) {
            st.req.destroy();
          }
          if (st.res !== null && !st.res.writableEnded && !st.res.destroyed) {
            st.res.destroy();
          }
        });
        // server.timeout：per-socket 空闲计时（10f；单发 timer，data 到达即重臂，
        // 见 data 处理器）。到期 server 发 'timeout'(socket)，不杀连接（net 口径）。
        if (self.timeout > 0) sock.setTimeout(self.timeout);
        sock.on("timeout", () => {
          if (!sock.destroyed) self.emit("timeout", sock);
        });
        sock.on("data", (chunk) => {
          if (self.__closing || sock.__upgraded) return;
          if (self.timeout > 0) sock.setTimeout(self.timeout);
          try {
            self.__feed(sock, st, chunk);
          } catch (e) {
            self.__feedError(sock, e);
          }
        });
        // node socketOnEnd 口径：客户端 FIN——非 half-open 直接销毁（res 'close'
        // 经 close 处理器）；half-open 留给响应自身收口（server.js 套件的半关
        // 后续响应仍须可写），被截断的请求体提前夭折（'aborted' 语义）。
        sock.on("end", () => {
          sock.__finReceived = true;
          if (!self.httpAllowHalfOpen) {
            try { sock.destroy(); } catch { /* gone */ }
          } else {
            if (st.req !== null && !st.req.complete && !st.req.destroyed) {
              st.req.destroy();
            }
            if (st.res !== null) {
              st.res.once("close", () => {
                try { sock.destroy(); } catch { /* gone */ }
              });
            }
          }
        });
        // 连接即开 headers 计时（headersTimeout 内须收到完整头，否则 408）。
        self.__armIdleTimers(st, sock);
      });
  }
  class __HttpServer extends Base {
    constructor(...args) {
      super(...args);
      __initServer(this, args);
    }
    // 400 Bad Request（Node clientError 默认响应）+ 销毁。
    __badRequest(sock) {
      try { sock.write(new TextEncoder().encode("HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
      try { sock.destroy(); } catch { /* gone */ }
    }
    // 408 Request Timeout（requestTimeout/headersTimeout 到期；精确字节见
    // test-http-server-request-timeout-delayed-headers 套件）+ 销毁。
    __reqTimeout(sock) {
      try { sock.write(new TextEncoder().encode("HTTP/1.1 408 Request Timeout\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
      try { sock.destroy(); } catch { /* gone */ }
    }
    // 413 Payload Too Large：chunk 扩展总量超限（chunk-extensions-limit 套件，
    // 精确字节真机实测）+ 销毁。
    __payloadTooLarge(sock) {
      try { sock.write(new TextEncoder().encode("HTTP/1.1 413 Payload Too Large\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
      try { sock.destroy(); } catch { /* gone */ }
    }
    // 431 Request Header Fields Too Large：trailer 名+值累计超 maxHeaderSize
    // （真机实测精确字节）+ 销毁。
    __headerFieldsTooLarge(sock) {
      try { sock.write(new TextEncoder().encode("HTTP/1.1 431 Request Header Fields Too Large\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
      try { sock.destroy(); } catch { /* gone */ }
    }
    // __feed 解析错的统一出口（400 通道 / 兜底销毁）。数据事件与 res 收尾的
    // microtask re-feed（__onDone）共用；后者不在 data 处理器的 try/catch 内，
    // 裸抛会变成 unhandled rejection（chunked-smuggling 套件）。
    __feedError(sock, e) {
      if (e && e.__httpParse) {
        // node 口径：clientError 事件恒发；无监听才落默认 400 + 销毁。
        this.emit("clientError", e, sock);
        if (this.listenerCount("clientError") === 0) this.__badRequest(sock);
        return;
      }
      try { sock.destroy(); } catch { /* gone */ }
    }
    __clearReqTimers(st) {
      if (st.__hdT !== null) { clearTimeout(st.__hdT); st.__hdT = null; }
      if (st.__rqT !== null) { clearTimeout(st.__rqT); st.__rqT = null; }
      if (st.__kaT !== null) { clearTimeout(st.__kaT); st.__kaT = null; }
    }
    // 空闲期（等下一请求头）：headersTimeout → 408；keepAliveTimeout → 静默销毁
    // （ka 只在响应完成后臂，withKa——体齐响应未完时挂 ka 会误杀在途响应）。
    // 消息期（头已到、体未齐）：requestTimeout → 408。Node 口径：消息期计时器
    // 不因部分数据重置（interrupted/delayed 系套件依赖）。
    __armIdleTimers(st, sock, withKa = false) {
      this.__clearReqTimers(st);
      if (this.headersTimeout > 0) {
        st.__hdT = setTimeout(() => { st.__hdT = null; this.__reqTimeout(sock); }, this.headersTimeout);
        st.__hdT.unref();
      }
      if (withKa && st.sawRequest && this.keepAliveTimeout > 0) {
        st.__kaT = setTimeout(() => { st.__kaT = null; try { sock.destroy(); } catch { /* gone */ } }, this.keepAliveTimeout + this.keepAliveTimeoutBuffer);
        st.__kaT.unref();
      }
    }
    __armMsgTimer(st, sock) {
      this.__clearReqTimers(st);
      if (this.requestTimeout > 0) {
        st.__rqT = setTimeout(() => { st.__rqT = null; this.__reqTimeout(sock); }, this.requestTimeout);
        st.__rqT.unref();
      }
    }
    setTimeout(msecs, callback) {
      this.timeout = msecs;
      if (typeof callback === "function") this.on("timeout", callback);
      return this;
    }
    closeIdleConnections() {
      for (const sock of this.__sockets) {
        const st = sock.__httpState;
        if (!st || st.req === null) {
          try { sock.destroy(); } catch { /* gone */ }
        }
      }
    }
    closeAllConnections() {
      for (const sock of this.__sockets) {
        try { sock.destroy(); } catch { /* gone */ }
      }
    }
    __feed(sock, st, chunk) {
      st.buf = __concat(st.buf, chunk);
      while (true) {
        if (st.req === null) {
          const headEnd = __findHeadEnd(st.buf);
          if (headEnd === -1) {
            // 头段超出 maxHeaderSize：431 Request Header Fields Too Large
            //（llhttp HPE_HEADER_OVERFLOW；header-overflow 套件精确字节）。
            if (st.buf.length > maxHeaderSize) { this.__headerFieldsTooLarge(sock); return; }
            // 头未齐也可先校验请求行（llhttp 增量语义；管线残渣 "hello world"
            // 在 URL 段首字节即 400，等不到行终结——blank-header 套件）。
            // llhttp 口径：请求行前的 CRLF 空行容忍（管线残段；insecure-parser
            // 套件尾部 '\r\n\r\n' 现场记录），先吞再校验。
            while (st.buf.length >= 2 && st.buf[0] === 13 && st.buf[1] === 10) {
              st.buf = st.buf.slice(2);
            }
            __checkRequestLinePrefix(st.buf);
            return;
          }
          const headText = __latin1(st.buf.slice(0, headEnd));
          if (this.insecureHTTPParser !== true && __hasBareCR(headText)) {
            throw __mkParseError("LF expected after CR");
          }
          const { first, headers, rawHeaders } = __parseHead(headText, this.__inboundMode ?? "strict");
          __validateRequestHead(first, headers);
          const req = new IncomingMessage(this.__highWaterMark !== undefined
            ? { highWaterMark: this.__highWaterMark } : undefined);
          req.method = first[0];
          req.url = first[1];
          req.httpVersion = first[2].replace("HTTP/", "");
          {
            const __vv = req.httpVersion.split(".");
            req.httpVersionMajor = Number(__vv[0] ?? 1) || 0;
            req.httpVersionMinor = Number(__vv[1] ?? 1) || 0;
          }
          req.headers = headers;
          req.rawHeaders = rawHeaders;
          req.socket = sock;
          req.connection = sock;
          // Node 口径：CONNECT 方法请求不进 request 管线——派发 'connect'
          //（req, socket, head；无监听则销毁连接），socket 停止 HTTP 解析。
          if (req.method === "CONNECT") {
            sock.__upgraded = true;
            const __leftover = st.buf.slice(headEnd + 4);
            st.buf = new Uint8Array(0);
            if (this.listenerCount("connect") > 0) {
              this.emit("connect", req, sock, globalThis.Buffer.from(__leftover));
            } else {
              sock.destroy();
            }
            return;
          }
          // Node 口径：带 Upgrade 头的请求不进 request 管线——派发 'upgrade'
          //（req, 原始 socket；vite 的 ws 库经它完成 101 握手与帧收发），无监听
          // 则销毁连接。升级后本连接停止 HTTP 解析（__upgraded 旗）；头后残留
          // 字节（罕见）经 microtask 以裸 data 事件回灌（监听方已同步登记）。
          if (headers.upgrade !== undefined) {
            sock.__upgraded = true;
            if (this.listenerCount("upgrade") > 0) {
              // Node 口径三参 (req, socket, head)：head 恒 Buffer（零长=无残留，
              // ws 库 setSocket 读 head.length——undefined 即 TypeError）。
              const leftover = st.buf.slice(headEnd + 4);
              st.buf = new Uint8Array(0);
              this.emit("upgrade", req, sock, leftover);
            } else {
              sock.destroy();
            }
            return;
          }
          const framing = __framingFor(headers, false, null, req.method);
          const conn = (headers.connection || "").toLowerCase();
          const keepAlive = req.httpVersion === "1.1" ? conn !== "close" : conn === "keep-alive";
          const res = new ServerResponse(sock);
          // 出站校验档随服务端 httpValidation（node 同一选项双向往返）。
          res.__validation = this.__inboundMode;
          // node _http_server.js 口径：res[kHighWaterMark] 记服务端 HWM
          //（缺省 getDefaultHighWaterMark()）。
          res[kHighWaterMark] = this.__highWaterMark ?? getDefaultHighWaterMark(false);
          // node ServerResponse ctor 口径：UCED 1.1 恒 true；1.0 = 请求 TE 头
          // 含 chunked（真机 1.0-keep-alive 套件 TE: chunked 形）。
          res.__uced = req.httpVersion === "1.1" ? true : /(?:^|\W)chunked/i.test(headers.te ?? "");
          res.req = req;
          req.res = res;
          res.__keepAlive = keepAlive;
          res.__headOnly = req.method === "HEAD";
          // 响应头决策所需服务端上下文（Keep-Alive: timeout / maxRequestsPerSocket）。
          res.__kaTimeout = this.keepAliveTimeout;
          res.__maxReq = this.maxRequestsPerSocket;
          st.reqCount = (st.reqCount ?? 0) + 1;
          if (this.maxRequestsPerSocket > 0 && st.reqCount >= this.maxRequestsPerSocket) {
            // node 口径：达额请求的响应带 Connection: close（响应完关连接）。
            res.__maxReqReached = true;
            res.__keepAlive = false;
          }
          // Expect 头三路（node parserOnIncoming 口径，真机 26.8.2 对拍）：
          // 100-continue → checkContinue 监听在场发它、否则默认 writeContinue+request；
          // 其他期望值 → checkExpectation 监听在场发它、否则默认 417 应急停
          //（响应后原请求体进丢弃泵，st.req 不接线）。
          let __wired = true;
          let __ev = "request";
          const __expect = headers.expect;
          if (__expect !== undefined) {
            if (__EXPECT_CONTINUE_RE.test(__expect)) {
              if (this.listenerCount("checkContinue") > 0) __ev = "checkContinue";
              else res.writeContinue();
            } else if (this.listenerCount("checkExpectation") > 0) {
              __ev = "checkExpectation";
            } else {
              __wired = false;
              __ev = null;
              res.statusCode = 417;
            }
          }
          st.req = __wired ? req : null;
          st.framing = framing;
          st.res = res;
          // 头已齐、体在途：消息期 requestTimeout 计时。
          this.__armMsgTimer(st, sock);
          res.__onDone = () => {
            st.req = null;
            st.framing = null;
            st.res = null;
            st.sawRequest = true;
            // 半关连接（客户端已 FIN）且这是最后一个在途响应：收口不续
            // keep-alive；还有管线中响应（st.res 已是后继请求的 res）则继续。
            if (sock.__finReceived && st.res === res) {
              try { sock.destroy(); } catch { /* gone */ }
              return;
            }
            // 请求+响应完整落地：回空闲期（headersTimeout/keepAliveTimeout 双计时）。
            this.__armIdleTimers(st, sock, true);
            // 连接已销毁（如 mid-body 400/413）则不再 re-feed 剩余缓冲；
            // 活着时解析错也要走统一 400 通道（microtask 内裸抛 = unhandled rejection）。
            if (sock.destroyed) return;
            try {
              this.__feed(sock, st, new Uint8Array(0));
            } catch (e) {
              this.__feedError(sock, e);
            }
          };
          // node 口径：server.close() 只停监听，既有连接上的后续管线请求照常
          // 服务（pipeline-assertionerror-finish 套件 mustCall(10) 点名；
          // 原自创 503 路径会使后续响应写进已 end 的 socket）。
          st.buf = st.buf.slice(headEnd + 4);
          if (__wired) {
            // §4.35：先 emit("request")（监听器登记 data/end），再喂体。
            this.emit(__ev, req, res);
          } else {
            // 417 默认路径：响应立即收尾；后续体字节走丢弃泵（st.req 为 null）。
            res.end();
          }
          continue;
        }
        // 体泵：CL / chunked 增量；none 直接完结（st.req 为 null = 417 丢弃泵）。
        const fr = st.framing;
        if (fr.type === "none") {
          if (st.req !== null) st.req.__complete();
          st.req = null;
          st.framing = null;
          this.__armIdleTimers(st, sock);
          continue;
        }
        let r;
        if (fr.type === "cl") {
          r = __pumpCL(fr, st.req, st.buf);
        } else {
          r = __pumpChunked(fr, st.req, st.buf);
          // 413/431 是精确字节响应 + 销毁（不是 400 通道）；其余解析错走 400。
          if (r.error === 413) { this.__payloadTooLarge(sock); return; }
          if (r.error === 431) { this.__headerFieldsTooLarge(sock); return; }
          if (r.error) throw __mkParseError("bad chunked body");
        }
        st.buf = r.rest;
        if (!r.done) return;
        if (r.trailersRaw !== undefined && st.req !== null) __applyTrailers(st.req, r.trailersRaw);
        if (st.req !== null) st.req.__complete();
        st.req = null;
        st.framing = null;
        this.__armIdleTimers(st, sock);
      }
    }
    close(cb) {
      this.__closing = true;
      if (typeof cb === "function") this.once("close", cb);
      // node close 口径：空闲连接销毁；仍有在途响应的连接 unref（进程不等待，
      // 响应落地后自然退出——pipeline-assertionerror-finish 套件）。
      for (const sock of this.__sockets) {
        const __st = sock.__httpState;
        if (!__st || __st.req === null) {
          try { sock.destroy(); } catch { /* closed meanwhile */ }
        } else if (typeof sock.unref === "function") {
          try { sock.unref(); } catch { /* gone */ }
        }
      }
      // node httpServerPreClose 口径（真机 toString 逐字）：closeIdleConnections +
      // clearInterval——timer 对象保留在符号键下（_destroyed 置位，close 回调里
      // 可断言；close-destroy-timeout/async-dispose 套件），不置 undefined。
      const __cur = this[kConnectionsCheckingInterval];
      if (__cur !== undefined) clearInterval(__cur);
      super.close();
      return this;
    }
    // node Server asyncDispose（node 26：close 承诺化）。
    [Symbol.asyncDispose]() {
      return new Promise((resolve) => {
        this.close(() => resolve());
      });
    }
  }
  // Node 口径：Server 裸调用返回新实例（lib/_http_server.js 原文）；
  // `Server.call(this)` 形（upgrade-server 套件 testServer 老式继承）直接在
  // this 上跑初始化——Reflect.construct 会造新对象弃 this，老式子类全挂。
  function HttpServer(...args) {
    if (!(this instanceof __HttpServer)) return new __HttpServer(...args);
    if (new.target !== undefined) return Reflect.construct(__HttpServer, args, new.target);
    __initServer(this, args);
  }
  Object.setPrototypeOf(HttpServer, __HttpServer);
  HttpServer.prototype = __HttpServer.prototype;
  // http.rs Server 壳的 .call 形入口（this 已是派生实例时在其上初始化）。
  HttpServer.__initOn = (obj, args) => __initServer(obj, args);
  return HttpServer;
}

// 客户端工厂：openSocket(host, port, extra) 开传输 socket；
// flavor = { protocol: "http:", defaultPort: 80, other: "node:https" }。
export function withClientRequest(openSocket, flavor) {
  return class ClientRequest extends OutgoingMessage {
    constructor(options, cb) {
      super();
      let host, port, path, method, userHeaders, extra;
      if (typeof options === "string" || options instanceof URL) {
        const u = __parseUrlArg(String(options));
        if (u.protocol !== flavor.protocol) {
          throw new codes.ERR_INVALID_PROTOCOL(u.protocol, flavor.protocol);
        }
        method = "GET";
        host = u.hostname;
        port = u.port ? Number(u.port) : flavor.defaultPort;
        path = u.pathname + u.search;
        userHeaders = {};
        extra = {};
      } else {
        // node _http_client.js：协议门（url.parse 形对象带 protocol 字段；
        // url.parse-only 套件——file:/mailto:/ftp: 等一律 ERR_INVALID_PROTOCOL）。
        if (options.protocol !== undefined && options.protocol !== flavor.protocol) {
          throw new codes.ERR_INVALID_PROTOCOL(options.protocol, flavor.protocol);
        }
        // host/hostname 类型门（真机逐字：'of type string or one of undefined
        // or null'；hostname-typechecking 套件）。
        if (options.hostname !== undefined && options.hostname !== null && typeof options.hostname !== "string") {
          throw new codes.ERR_INVALID_ARG_TYPE("options.hostname", ["string", "undefined", "null"], options.hostname);
        }
        if (options.host !== undefined && options.host !== null && typeof options.host !== "string") {
          throw new codes.ERR_INVALID_ARG_TYPE("options.host", ["string", "undefined", "null"], options.host);
        }
        // agent 门（Agent-like Object/undefined/null/false；
        // reject-unexpected-agent 套件真机逐字）。
        if (options.agent !== undefined && options.agent !== null && options.agent !== false) {
          if (typeof options.agent !== "object" || typeof options.agent.addRequest !== "function") {
            throw new codes.ERR_INVALID_ARG_TYPE("options.agent", ["Agent-like Object", "undefined", "false"], options.agent);
          }
        }
        // insecureHTTPParser 类型门（insecure-parser-per-stream 套件 test5）。
        if (options.insecureHTTPParser !== undefined && typeof options.insecureHTTPParser !== "boolean") {
          throw new codes.ERR_INVALID_ARG_TYPE("options.insecureHTTPParser", "boolean", options.insecureHTTPParser);
        }
        // method 门：非串 → ARG_TYPE（check-http-token 套件）；非法 token →
        // ERR_INVALID_HTTP_TOKEN（request-invalid-method-error 套件 '\0'）。
        if (options.method !== undefined && options.method !== null) {
          if (typeof options.method !== "string") {
            throw new codes.ERR_INVALID_ARG_TYPE("options.method", "string", options.method);
          }
          // 空串等 falsy method → 缺省 GET（client-defaults 套件）；非法
          // token → ERR_INVALID_HTTP_TOKEN（request-invalid-method-error 套件）。
          if (options.method !== "" && !__TOKEN_RE.test(options.method)) {
            throw new codes.ERR_INVALID_HTTP_TOKEN("Method", options.method);
          }
        }
        method = (options.method ?? "GET").toUpperCase() || "GET";
        host = options.host ?? options.hostname ?? "localhost";
        // node 口径：defaultPort 逐级——显式 port > agent.defaultPort > flavor 缺省
        //（default-port 套件：globalAgent.defaultPort 动态改写生效，host 头
        // 按“port === 生效缺省”省略端口；agent 缺省取隐式 globalAgent）。
        const __ag = options.agent !== undefined ? options.agent : flavor.defaultAgent;
        const __agentDp = __ag && __ag.defaultPort !== undefined ? __ag.defaultPort : flavor.defaultPort;
        port = Number(options.port ?? __agentDp);
        path = options.path ?? "/";
        if (!path.startsWith("/")) path = "/" + path;
        userHeaders = options.headers ?? {};
        extra = options;
        // node addRequest 口径：socketPath 在场即以之改写 connect 用的 path
        //（防 HTTP path 泄进 net.connect 误连错目标——ENOTSOCK 现场记录）。
        if (options.socketPath !== undefined) options.path = options.socketPath;
      }
      // node lib/_http_client.js：path 控制字符/空格即 ERR_UNESCAPED_CHARACTERS。
      if (INVALID_PATH_REGEX.test(path)) {
        throw new codes.ERR_UNESCAPED_CHARACTERS("Request path");
      }
      this.method = method;
      this.host = host;
      // node 口径：.port 不是自有属性（req.port === undefined；取值走 getPort()）。
      this.__port = port;
      // IPC 形（node：req.socketPath 自有属性；openSocket 钩按它走 UDS）。
      this.socketPath = options.socketPath;
      // path 访问器（node setPath 口径）：赋值即校验，控制字符/空格一律
      // ERR_UNESCAPED_CHARACTERS（path-toctou 套件：`req.path = '/evil\r\n...'`）。
      Object.defineProperty(this, "path", {
        get() { return this.__pathVal; },
        set(v) {
          const s = typeof v === "string" ? v : String(v);
          if (INVALID_PATH_REGEX.test(s)) {
            throw new codes.ERR_UNESCAPED_CHARACTERS("Request path");
          }
          this.__pathVal = v;
        },
        enumerable: true,
        configurable: true,
      });
      this.path = path;
      // 自设请求头名字门（invalidheaderfield 套件：'testing 123' → TypeError）。
      for (const __k of Object.keys(userHeaders ?? {})) {
        if (!__TOKEN_RE.test(__k)) {
          throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", __k);
        }
      }
      // 每请求宽松解析旗（insecure-parser-per-stream 套件：头值控制字符严格门）。
      // httpValidation 门（client 与 server 同口径：validateOneOf + 互斥，
      // 真机 ERR_INVALID_ARG_VALUE 逐项对拍）。
      this.__inboundMode = __parseModeOf(__resolveHttpValidation(options.httpValidation, options.insecureHTTPParser));
      this.__validation = options.httpValidation ?? (options.insecureHTTPParser === true ? "insecure" : undefined);
      this.insecureHTTPParser = options.insecureHTTPParser ?? false;
      this.socket = null;
      this.agent = options.agent === undefined ? (flavor.defaultAgent ?? null) : (options.agent || null);
      this.__agentFalse = options.agent === false;
      this.__defaultPort = this.agent !== null && this.agent.defaultPort !== undefined
        ? this.agent.defaultPort : flavor.defaultPort;
      // timeout 双检（node validateNumber 口径，真机 26.8.2 逐项：null/'x' →
      // ARG_TYPE，NaN/负 → OUT_OF_RANGE）。
      if (options.timeout !== undefined) {
        if (typeof options.timeout !== "number") {
          throw new codes.ERR_INVALID_ARG_TYPE("timeout", "number", options.timeout);
        }
        if (!Number.isFinite(options.timeout) || options.timeout < 0) {
          throw new codes.ERR_OUT_OF_RANGE("timeout", "a non-negative finite number", options.timeout);
        }
      }
      this.timeout = options.timeout !== undefined ? Number(options.timeout) : undefined;
      // 请求级 timeout（__attach 时覆盖 agent 级的 socket 计时）。
      this.__reqTimeoutMs = this.timeout !== undefined && this.timeout > 0 ? this.timeout : undefined;
      // node onSocket 口径：请求级 timeout 优先于 agent 级；任一在场即挂
      // timeoutCb（emitRequestTimeout——转发 socket 'timeout' → req 'timeout'）。
      const __agentTimeout = this.agent !== null && this.agent.options ? this.agent.options.timeout : undefined;
      this.__timeoutMs = this.timeout ?? __agentTimeout;
      if (this.timeout !== undefined || (typeof __agentTimeout === "number" && __agentTimeout > 0)) {
        this.timeoutCb = () => this.emit("timeout");
      }
      this.__headers = __lowerHeaders(userHeaders);
      if (this.__headers.host === undefined) {
        this.__headers.host = port === this.__defaultPort ? host : `${host}:${port}`;
      }
      // node ctor 口径：shouldKeepAlive = agent 在场且 keepAlive（真机
      // _http_client.js ctor：无 agent / 非 keepAlive agent → Connection: close）。
      this.shouldKeepAlive = this.agent !== null && this.agent.keepAlive === true;
      if (this.__headers.connection === undefined) {
        this.__headers.connection = this.shouldKeepAlive ? "keep-alive" : "close";
        this.__autoConn = true;
      }
      this.__headSent = false;
      // 真机口径（10f G3，node 26.8.2 实测）：CL 快路径仅当 end(data) 是首个
      // 头触发点；write/flushHeaders 在前 → chunked；GET/HEAD/DELETE/OPTIONS/
      // TRACE/CONNECT（useChunkedEncodingByDefault=false 族）→ 无 CL/TE 裸体。
      this.__chunkDefault = !["GET", "HEAD", "DELETE", "OPTIONS", "TRACE", "CONNECT"].includes(method);
      this.__sawWrite = false;
      this.__endFast = false;
      this.__chunked = false;
      this.__rawCL = false;
      this.__contentLength = undefined;
      this.__headerStored = false;
      this.__buf1 = null;
      this.__holdTimer = null;
      this.__userEnded = false;
      this.__connected = false;
      this.__sock = null;
      this.__onSockClose = null;
      this.__res = null;
      this.__framing = null;
      this.__resBuf = new Uint8Array(0);
      this.__respDone = false;
      this.__closeEmitted = false;
      // 池键与 agent 键位统一（getName 形；keep-alive 测试以 agent.getName 命中；
      // 全量 extra 进键——https 的 TLS 选项字段参与去重）。
      this.__key = this.agent !== null
        ? this.agent.getName({ host, port, ...(extra ?? {}) })
        : `${host}:${port}`;
      this.reusedSocket = false;
      if (typeof cb === "function") this.on("response", cb);
      if (typeof options.createConnection === "function") {
        // request 级 createConnection（node _http_client 口径）：绕 agent 直建。
        // 实参是请求 options 的浅拷贝且 path 值被摘除（TCP；真机逐键实测
        // ["createConnection","headers","host","path(undefined)","port"]）、
        // IPC 时改写为 socketPath——防 HTTP path 泄进 net.connect 误走管道。
        this.__createConn = options.createConnection;
        const connOpts = { ...(extra ?? {}) };
        connOpts.path = options.socketPath !== undefined ? options.socketPath : undefined;
        let out;
        let settled = false;
        const oncreate = (err, s) => {
          settled = true;
          if (s) this.__attach(s, false);
        };
        const maybe = this.__createConn(connOpts, oncreate);
        if (!settled && maybe) this.__attach(maybe, false);
      } else if (this.agent !== null) {
        this.agent.__acquire(this, host, port, extra, (sock, reused) => this.__attach(sock, reused));
      } else {
        this.__attach(openSocket(host, port, extra), false);
      }
    }
    __attach(sock, reused) {
      if (this.destroyed) {
        try { sock.destroy(); } catch { /* closed meanwhile */ }
        return;
      }
      this.__sock = sock;
      this.socket = sock;
      // node setRequestProps 口径：socket._httpMessage 指回当前请求（connect
      // 套件在 'socket' 事件断言全等）。
      sock._httpMessage = this;
      this.reusedSocket = reused === true;
      // 防御：上轮请求侧监听残留即先摘（正常路径 __finishSock 已摘）——
      // 必须先于本函数的一切注册，否则会把刚挂的监听当残留摘掉。
      // node 口径 attach 换装四件——socketOnEnd/socketErrorListener/
      // socketCloseListener/socketOnData + 池态 freeSocketErrorListener。
      if (sock.__reqSockOnEnd !== undefined) {
        try { sock.removeListener("end", sock.__reqSockOnEnd); } catch { /* gone */ }
      }
      if (sock.__reqSockOnError !== undefined) {
        try { sock.removeListener("error", sock.__reqSockOnError); } catch { /* gone */ }
      }
      if (sock.__reqSockOnClose !== undefined) {
        try { sock.removeListener("close", sock.__reqSockOnClose); } catch { /* gone */ }
      }
      if (sock.__reqSockOnData !== undefined) {
        try { sock.removeListener("data", sock.__reqSockOnData); } catch { /* gone */ }
      }
      if (sock.__freeSockErr !== undefined) {
        try { sock.removeListener("error", sock.__freeSockErr); } catch { /* gone */ }
      }
      // node onSocket 口径：'socket' 事件异步（nextTick）发出——get()/request()
      // 返回后同步注册的监听器必须能收到（agent-timeout-option 套件形态）。
      queueMicrotask(() => {
        if (!this.destroyed) this.emit("socket", sock);
      });
      if (this.__pendingNoDelay !== undefined) {
        try { sock.setNoDelay(this.__pendingNoDelay); } catch { /* gone */ }
      }
      if (this.__pendingKeepAlive !== undefined) {
        try { sock.setKeepAlive(this.__pendingKeepAlive[0], this.__pendingKeepAlive[1]); } catch { /* gone */ }
      }
      // node onSocket 口径：timeoutCb 在场即挂 once；请求级 timeout 覆盖 agent 级
      //（socket.timeout 反映最后一次 setTimeout）；agent 级已在建连时置位则不重臂。
      // 假 socket（createConnection 注入的 Duplex）无 setTimeout 面则跳过。
      if (this.timeoutCb !== undefined) {
        if (this.__reqTimeoutMs !== undefined) {
          this.__applySockTimeout(sock, this.__reqTimeoutMs);
        } else if (!sock.timeout) {
          const __ms = this.__timeoutMs;
          if (typeof __ms === "number" && __ms > 0) this.__applySockTimeout(sock, __ms);
        }
        // 复用连接换请求：摘上一请求的 emitRequestTimeout（listeners 计数契约：
        // [onTimeout, emitRequestTimeout, responseOnTimeout] 不随复用累加）。
        if (sock.__lastTimeoutCb !== undefined && sock.__lastTimeoutCb !== this.timeoutCb) {
          try { sock.removeListener("timeout", sock.__lastTimeoutCb); } catch { /* gone */ }
        }
        sock.__lastTimeoutCb = this.timeoutCb;
        sock.once("timeout", this.timeoutCb);
      }
      this.__onSockClose = () => this.__onSockCloseEv();
      sock.on("connect", () => {
        this.__connected = true;
        if (this.__pendingFinal) {
          // end() 已调：整事务一次刷出（CL 决策在 end 时已定）。
          this.__pendingFinal = false;
          this.__flushFinal();
          return;
        }
        // node _flush 口径：连通即发头（无体请求——如 Expect: 100-continue
        // 等 continue 的形态——头也必须立即出网）。
        this.__tryFlush();
      });
      sock.on("secureConnect", () => {
        this.__connected = true;
        if (this.__pendingFinal) {
          this.__pendingFinal = false;
          this.__flushFinal();
          return;
        }
        this.__tryFlush();
      });
      const __sockOnData = (chunk) => {
        try {
          this.__onSockData(chunk);
        } catch (e) {
          // node 口径：响应头解析错（严格门）→ req 'error'（经 destroy(err)）。
          this.destroy(e);
        }
      };
      sock.on("data", __sockOnData);
      sock.__reqSockOnData = __sockOnData;
      // 复用入池连接：node keepSocketAlive 的逆操作——ref 回事件循环
      //（池态 socket unref 不阻退出，复活后必须计数）。
      if (reused === true && typeof sock.ref === "function") {
        try { sock.ref(); } catch { /* gone */ }
      }
      const __sockOnError = (e) => {
        if (this.listenerCount("error") === 0) throw e;
        this.emit("error", e);
        // node 口径：连接错无响应即销毁请求（'close' 时 req.destroyed === true，
        // agent-close/timeout-option 系套件断言）。
        this.destroy();
      };
      sock.on("error", __sockOnError);
      sock.__reqSockOnError = __sockOnError;
      // node socketOnEnd 口径（真机 toString 逐字）：无响应收到 FIN（'end'）
      // → req 'socket hang up'（ECONNRESET）+ 销毁；本仓宽容口径——无监听不
      // 加崩（emitErrorEvent 差异记档）。有 res 时只销毁，截断宽容路径不变
      //（__onSockCloseEv 收口）。监听存 sock 供 __finishSock 入池前摘除。
      const __sockOnEnd = function socketOnEnd() {
        const req = this._httpMessage;
        if (req !== undefined && req !== null && !req.destroyed &&
            (req.__res === null || req.__res === undefined) && !req.__hadError) {
          req.__hadError = true;
          if (req.listenerCount("error") > 0) {
            const e = new Error("socket hang up");
            e.code = "ECONNRESET";
            req.emit("error", e);
          }
        }
        try { this.destroy(); } catch { /* gone */ }
      };
      sock.on("end", __sockOnEnd);
      sock.__reqSockOnEnd = __sockOnEnd;
      sock.on("close", this.__onSockClose);
      sock.__reqSockOnClose = this.__onSockClose;
      // 复用连接已连通：直接刷。
      if (reused === true) {
        this.__connected = true;
        this.__tryFlush();
        if (this.__pendingFinal) {
          this.__pendingFinal = false;
          this.__flushFinal();
        }
      }
    }
