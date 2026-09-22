
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
  // node lib/_http_outgoing.js cork/uncork 原文（OutgoingMessage 基类；
  // ClientRequest 可用）：消息级计数 + socket 镜像。node corked 写滞留
  // kChunkedBuffer 由 uncork 尾flush 合并一帧——本仓客户端写机构无滞留层，
  // cork 仅计数不持字节（无套件点名客户端 cork 缓冲；服务端 ServerResponse
  // 的真缓冲在 framing_head.js，骑流机构 cork）。
  cork() {
    this.__kCorked = (this.__kCorked ?? 0) + 1;
    if (this.socket && typeof this.socket.cork === "function") this.socket.cork();
  }
  uncork() {
    if ((this.__kCorked ?? 0) > 0) this.__kCorked--;
    if (this.socket && typeof this.socket.uncork === "function") this.socket.uncork();
  }
  get writableCorked() { return this.__kCorked ?? 0; }
  _implicitHeader() {
    throw new Error("_implicitHeader() method is not implemented");
  }
  // node 口径（lib/_http_outgoing.js + outgoing-settimeout 套件真机实测）：
  // 基类 setTimeout——cb 挂 once('timeout')；有 socket 直转，无 socket 等
  // 'socket' 事件再转（ClientRequest/ServerResponse 各有覆写，本实现只服务
  // 基类直构与无覆写子类）。
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    // node 口径：无 socket 时等 'socket' 事件，用事件实参（非 this.socket——
    // 手工 emit('socket', fake) 形下 this.socket 仍为 null）。
    const __apply = (sock) => {
      try {
        const __s = sock ?? this.socket;
        if (__s !== undefined && __s !== null &&
            typeof __s.setTimeout === "function") __s.setTimeout(msecs);
      } catch { /* gone */ }
    };
    if (this.socket !== undefined && this.socket !== null) __apply();
    else this.once("socket", __apply);
    return this;
  }
  // node 口径：destroy(err) 不外发 'error'（仅记 errored），异步发一次 'close'
  //（outgoing-destroyed 套件：destroyed/closed/errored 三面 + close 事件）。
  destroy(err) {
    if (this.destroyed) return this;
    this.__omErrored = err ?? null;
    // Node 自实现 destroy 从不外发 msg 'error'（错误只进 errored/socket）；
    // 基类 destroy 经 trampoline 发 error，摘除时序无法排在它之后，
    // 故吞错监听常驻（用户自发 error 仍可达其自有监听，仅永不无监听抛错）。
    this.on("error", () => {});
    const __ret = super.destroy(err);
    queueMicrotask(() => {
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
  // node setHeaders：收 Headers 实例或 Map（ServerResponse 同款双形；
  // ClientRequest 侧同样可用）。
  setHeaders(headers) {
    if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("set");
    let __isHeaders = false;
    if (headers !== null && typeof headers === "object" && typeof headers.entries === "function") {
      const __tag = headers[Symbol.toStringTag];
      const __ctor = headers.constructor !== undefined && headers.constructor !== null
        ? headers.constructor.name : "";
      __isHeaders = __tag === "Headers" || __tag === "Map" || __ctor === "Headers" || __ctor === "Map";
    }
    if (!__isHeaders) {
      throw new codes.ERR_INVALID_ARG_TYPE("headers", ["Headers instance"], headers);
    }
    for (const [k, v] of headers.entries()) this.appendHeader(k, v);
    return this;
  }
  // node 口径：OutgoingMessage.addTrailers（multiple-headers 套件：
  // ClientRequest 亦有；分块终结块尾随头。值数组按元素展开多行——真机
  // addTrailers({k:[a,b]}) 即两行 k: a / k: b；拼写取用户原文。
  // req.trailers/trailersDistinctrawTrailers 即时落账（wire 回环前可读，
  // multiple-headers 套件 req 'end' 内断言）。
  addTrailers(trailers) {
    if (trailers === null || trailers === undefined) return this;
    for (const k of Object.keys(trailers)) {
      if (!__TOKEN_RE.test(String(k))) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(k));
      const __v = trailers[k];
      const __vals = Array.isArray(__v) ? __v : [__v];
      // node 口径 uniqueHeaders：名单内 trailer 行 wire 合并单行 '; '
      //（multiple-headers 套件 rawTrailers 收单对）。
      const __uniq = this.__uniqueHeaders;
      if (Array.isArray(__uniq) && __uniq.includes(String(k).toLowerCase())) {
        for (const __e of __vals) __validateHeaderValue(__e);
        this.__trailer = (this.__trailer ?? "") + `${k}: ${__vals.join("; ")}\r\n`;
        if (this.trailers !== undefined) {
          if (this.rawTrailers === undefined) this.rawTrailers = [];
          this.rawTrailers.push(k, __vals.join("; "));
          const __lk = String(k).toLowerCase();
          this.trailers[__lk] = __vals.join("; ");
          if (this.trailersDistinct === undefined) this.trailersDistinct = Object.create(null);
          if (this.trailersDistinct[__lk] === undefined) this.trailersDistinct[__lk] = [];
          this.trailersDistinct[__lk].push(__vals.join("; "));
        }
        continue;
      }
      for (const __e of __vals) {
        __validateHeaderValue(__e);
        this.__trailer = (this.__trailer ?? "") + `${k}: ${__e}\r\n`;
        // 即时落账（wire 解析回填前可读）。
        if (this.trailers !== undefined) {
          if (this.rawTrailers === undefined) this.rawTrailers = [];
          this.rawTrailers.push(k, String(__e));
          const __lk = String(k).toLowerCase();
          this.trailers[__lk] = `${this.trailers[__lk] !== undefined ? this.trailers[__lk] + ", " : ""}${__e}`;
          if (this.trailersDistinct === undefined) this.trailersDistinct = Object.create(null);
          if (this.trailersDistinct[__lk] === undefined) this.trailersDistinct[__lk] = [];
          this.trailersDistinct[__lk].push(String(__e));
        }
      }
    }
    return this;
  }
}
  // node _http_outgoing.js:1322 口径：capture rejections → destroy
//（capture-rejection 套件；无此接线时 drain/监听抛错变 fatal）。
// destroy 本体不外发 'error'（outgoing-destroyed 套件吞错口径），错误经
// socket 透传（双侧 _destroy 有 error 监听才带 err）。
try {
  OutgoingMessage.prototype[EventEmitter.captureRejectionSymbol] = function (err) {
    this.destroy(err);
  };
} catch { /* 符号缺席即跳过（事件域未备） */ }

// spill 冲刷（升级接管流）：暂存非空且有 data 监听即按序发出（重入守卫
// 防自发 data 回灌）。spill 点直调；迟挂监听经 newListener 钩递延一轮
//（入表后，§4.47）——unread 套件 10ms 后挂 data 仍收齐。
function __spillFlush(sock) {
  const __stashed = sock.__spillBuf;
  if (!__stashed || __stashed.length === 0) return;
  if (typeof sock.listenerCount !== "function" || sock.listenerCount("data") === 0) return;
  sock.__spillBuf = new Uint8Array(0);
  const __out = globalThis.Buffer.from(__stashed);
  sock.__spillGuard = true;
  try { sock.emit("data", __out); } catch { /* 监听抛错不阻收尾 */ }
  sock.__spillGuard = false;
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
      // node 口径：maxHeadersCount 缺省 null（不限），可动态改写
      // （max-headers-count 套件逐轮改写；接收侧截断，0/null 不限）。
      self.maxHeadersCount = null;
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
      // node 口径（server-options-incoming-message/server-options-server-
      // response 套件，真机无校验——任意值照收，请求期当构造器用）：
      // IncomingMessage/ServerResponse 自定义类（须为可构造，子类无显式
      // 构造器即透传）。
      if (o.IncomingMessage !== undefined) self.IncomingMessage = o.IncomingMessage;
      if (o.ServerResponse !== undefined) self.ServerResponse = o.ServerResponse;
      if (o.uniqueHeaders !== undefined) {
        if (!Array.isArray(o.uniqueHeaders)) throw new codes.ERR_INVALID_ARG_TYPE("uniqueHeaders", "Array", o.uniqueHeaders);
        self.uniqueHeaders = o.uniqueHeaders.map((h) => String(h).toLowerCase());
      }
      // node 口径：shouldUpgradeCallback(req) 逐请求门控升级（upgrade-server-
      // callback 套件：true 走 upgrade、false 走 request、抛错走 uncaught）。
      if (o.shouldUpgradeCallback !== undefined) self.shouldUpgradeCallback = o.shouldUpgradeCallback;
      // node 口径（head-throw 套件）：rejectNonStandardBodyWrites 缺省 false。
      self.rejectNonStandardBodyWrites = o.rejectNonStandardBodyWrites === true;
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
            if (!st || (st.req === null && st.res === null)) { try { s.destroy(); } catch { /* gone */ } }
          }
        }, 1000);
        if (typeof __iv.unref === "function") __iv.unref();
        self[kConnectionsCheckingInterval] = __iv;
      });
      self.on("connection", (sock) => {
        self.__sockets.add(sock);
        const st = { buf: new Uint8Array(0), req: null, framing: null, res: null, __hdT: null, __rqT: null, __kaT: null };
        sock.__httpState = st;
        // node 口径 socketOnError：连接 socket 的 error 恒有兜底监听。分流：
        // ① 用户自有 error 监听（除本兜底外）→ 纯多播（clientError 有监听才
        // 转，不吞不毁——u6b）；
        // ② 有 clientError 监听 → 交 handler（无后续动作；handler  idle 即挂，
        // 真机 probe11 同款）；
        // ③ 无监听 + 有在途 res → 刷盘（write-cb 先于落盘，真机 cb 恒在落盘后；
        // 复用 holdback timer 同款步骤）再摧毁 req/res（res 'close' 无 'finish'）
        // + 毁 socket（badrequest 套件，客户端 FIN 后 lenient-end）；
        // ④ 无监听 + 无在途 res（升级/空闲）→ 重抛走 uncaught（u6c 真机实证）。
        // 存根供 CONNECT 隧道 detach 摘除（connect 套件 listenerCount 矩阵）。
        sock.__httpSockOnError = function __httpSockOnError(e) {
          const __nErr = typeof sock.listenerCount === "function" ? sock.listenerCount("error") : 0;
          const __hasClientError = self.listenerCount("clientError") > 0;
          if (__hasClientError) {
            try { self.emit("clientError", e, sock); } catch { /* 监听抛错不阻收尾 */ }
          }
          if (__nErr > 1) return;
          if (__hasClientError) return;
          const __r = st.res;
          if (__r === null || __r === undefined || __r.destroyed) throw e;
          try {
            if (!__r.__headSent) {
              if (__r.__holdTimer !== null && __r.__holdTimer !== undefined) {
                clearTimeout(__r.__holdTimer);
                __r.__holdTimer = null;
              }
              if (__r.__buf1 !== null && __r.__buf1 !== undefined &&
                  __r.__uced && __r.__headers["content-length"] === undefined &&
                  __r.__headers["transfer-encoding"] === undefined &&
                  (typeof __r.__frameSuppressed !== "function" || !__r.__frameSuppressed())) {
                __r.__chunked = true;
              }
              if (typeof __r.__sendHead === "function") __r.__sendHead();
              if (__r.__buf1 !== null && __r.__buf1 !== undefined) {
                const __b = __r.__buf1;
                __r.__buf1 = null;
                if (typeof __r.__frame === "function") __r.__frame(__b);
              }
            }
          } catch { /* gone */ }
          try { if (st.req !== null && st.req !== undefined && !st.req.destroyed) st.req.destroy(); } catch { /* gone */ }
          try { if (!__r.destroyed) __r.destroy(); } catch { /* gone */ }
          try { sock.destroy(); } catch { /* gone */ }
        };
        sock.on("error", sock.__httpSockOnError);
        // 存根供 CONNECT 隧道 detach 摘除（connect 套件 listenerCount 矩阵）。
        sock.__httpSockOnClose = () => {
          self.__clearReqTimers(st);
          self.__sockets.delete(sock);
          // 连接断时未完的req/res一起收尾：req 先走 aborted 级联（aborted
          // 恒发、error 门控），再 destroy 推 close（客户端中断上传的
          // PREMATURE_CLOSE 与 aborted 套件双口径）。未完含两态：
          // 体在途（st.req）与响应在途（st.res.req——st.req 体完即清，
          // 只看它会漏掉已收完头、响应未完的请求）。判定走真机 _destroy
          // 口径（!readableEnded || !complete）。
          const __abortReq = (r) => {
            if (r === null || r === undefined || r.destroyed) return;
            if (!(!r.readableEnded || !r.complete)) return;
            if (typeof r.__abortWithError === "function") {
              try { r.__abortWithError(); } catch { /* 监听抛错不阻收尾 */ }
            }
            r.destroy();
          };
          __abortReq(st.req);
          if (st.res !== null && st.res !== undefined) __abortReq(st.res.req);
          if (st.res !== null && !st.res.writableEnded && !st.res.destroyed) {
            st.res.destroy();
          }
        };
        sock.on("close", sock.__httpSockOnClose);
        // server.timeout：per-socket 空闲计时（10f；单发 timer，data 到达即重臂，
        // 见 data 处理器）。到期 server 发 'timeout'(socket)，不杀连接（net 口径）。
        // 监听只在计时武装时挂（无条件挂会使 listenerCount 恒多 1——connect 套件
        // 矩阵；缺省 timeout=0 即不挂）。存根供 CONNECT 隧道 detach 摘除。
        if (self.timeout > 0) {
          sock.setTimeout(self.timeout);
          sock.__httpSockOnTimeout = () => {
            if (!sock.destroyed) self.emit("timeout", sock);
          };
          sock.on("timeout", sock.__httpSockOnTimeout);
        }
        // 具名存根供升级摘除（升级后 native 直调 __srvFeed，此监听再留着
        // 只会占 data 监听数、提前吞掉 spill 冲刷）。
        const __srvDataListener = (chunk) => {
          // node 口径：close() 只停监听，既有连接（含在途上传与后续管线请求）
          // 照常服务——__closing 不得门控 data（outgoing-finish 套件 handler 内
          // close 后 80KB 上传被吞根因）；升级后转体路由（体喂 req + 体完转
          // socket data），不丢字节（large-body 系套件）。
          if (sock.__upgraded) {
            // spill 自发 data 的重入守卫（__feedUpgraded 尾部经同一 emitter
            // 发 data，接管监听与本监听同表——无 guard 即无限递归）。
            if (sock.__spillGuard) return;
            try {
              self.__feedUpgraded(sock, st, chunk);
            } catch (e) {
              self.__feedError(sock, e);
            }
            return;
          }
          if (self.timeout > 0) sock.setTimeout(self.timeout);
          try {
            self.__feed(sock, st, chunk);
          } catch (e) {
            self.__feedError(sock, e);
          }
        };
        sock.on("data", __srvDataListener);
        // 升级/CONNECT 接管即摘除（native 经 __srvFeed 直调；残留会占
        // listenerCount 提前吞 spill——unread 套件 'upgrade head' 丢失根因）。
        sock.__detachSrvData = () => {
          try { sock.off("data", __srvDataListener); } catch { /* gone */ }
        };
        // node socketOnEnd 口径：客户端 FIN——非 half-open 直接销毁（res 'close'
        // 经 close 处理器）；half-open 留给响应自身收口（server.js 套件的半关
        // 后续响应仍须可写），被截断的请求体提前夭折（'aborted' 语义）。
        // CONNECT/升级劫持后跳过（connect 套件：隧道内 FIN 不得销毁用户
        // socket；监听计数保留 end:1）。仅 CONNECT 形（升级 FIN 语义另行，
        // 不动）。
        sock.on("end", () => {
          if (sock.__connectHijacked) return;
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
        // node 口径：requestTimeout 从连接起算（delayed-headers 套件：零字节
        // 空闲连接到期亦 408；首字节分支只在无计时时起算，故此处先臂后
        // __feed 不再重臂，时序与真机一致；响应后空闲走 kaT，不归它管）。
        if (self.requestTimeout > 0 && st.__rqT == null) {
          st.__rqT = setTimeout(() => { st.__rqT = null; self.__reqTimeout(sock); }, self.requestTimeout);
          if (typeof st.__rqT.unref === "function") st.__rqT.unref();
        }
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
        // node 口径：clientError 事件恒发；无监听才落默认响应 + 销毁——
        // HPE_HEADER_OVERFLOW 默认 431（overflow 套件真机实证），其余 400。
        this.emit("clientError", e, sock);
        if (this.listenerCount("clientError") === 0) {
          if (e.code === "HPE_HEADER_OVERFLOW") {
            try { sock.write(new TextEncoder().encode("HTTP/1.1 431 Request Header Fields Too Large\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
            try { sock.destroy(); } catch { /* gone */ }
          } else {
            this.__badRequest(sock);
          }
        }
        return;
      }
      try { sock.destroy(); } catch { /* gone */ }
    }
    __clearReqTimers(st) {
      if (st.__hdT !== null) { clearTimeout(st.__hdT); st.__hdT = null; }
      if (st.__rqT !== null) { clearTimeout(st.__rqT); st.__rqT = null; }
      if (st.__kaT !== null) { clearTimeout(st.__kaT); st.__kaT = null; }
    }
    // 升级后体路由（upgrade-body 系套件）：头后字节继续喂 req 体（CL/chunked
    // 增量泵，与 __feed 同口径），体完结后续字节转 socket data（接管流）。
    // 体解析错即杀连接（与 __feedError 400 通道不同：升级已接管，无响应可回）。
    __feedUpgraded(sock, st, chunk) {
      st.buf = __concat(st.buf, chunk);
      const fr = st.framing;
      if (st.req !== null && fr !== null && fr !== undefined && fr.type !== "none") {
        let r;
        if (fr.type === "cl") {
          r = __pumpCL(fr, st.req, st.buf);
        } else {
          r = __pumpChunked(fr, st.req, st.buf);
          if (r.error) { try { sock.destroy(); } catch { /* gone */ } return; }
        }
        st.buf = r.rest ?? new Uint8Array(0);
        if (!r.done) return;
        if (r.trailersRaw !== undefined && st.req !== null) __applyTrailers(st.req, r.trailersRaw);
        if (st.req !== null) st.req.__complete();
        st.req = null;
        st.framing = null;
      }
      if (st.buf.length > 0) {
        const __spill = globalThis.Buffer.from(st.buf);
        st.buf = new Uint8Array(0);
        // 迟挂监听不丢字节（unread 套件 10ms 后才挂 data）：无人监听即暂存，
        // newListener 递延冲刷（入表后，§4.47）；有人即直发，保序
        //（暂存 + 直发同走 __spillFlush，先到先发）。
        sock.__spillBuf = __concat(sock.__spillBuf ?? new Uint8Array(0), __spill);
        __spillFlush(sock);
      }
    }
    // 空闲期（等下一请求头）：headersTimeout → 408；keepAliveTimeout → 静默销毁
    // （ka 只在响应完成后臂，withKa——体齐响应未完时挂 ka 会误杀在途响应）。
    // 消息期（头已到、体未齐）：requestTimeout → 408。Node 口径：消息期计时器
    // 不因部分数据重置（interrupted/delayed 系套件依赖）。
    __armIdleTimers(st, sock, withKa = false) {
      this.__clearReqTimers(st);
      if (this.headersTimeout > 0) {
        st.__hdT = setTimeout(() => { st.__hdT = null; this.__reqTimeout(sock); }, this.headersTimeout);
        // 看门狗计时不续命（kaT 同款；dont-set-default 套件 60s 空转根因，
        // 连接本身 ref 续命，计时只负责到期销毁）。
        if (typeof st.__hdT.unref === "function") st.__hdT.unref();
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
        // 空闲 = 无在途请求且无在途响应（体完但响应未落时 st.req 已空、
        // st.res 仍在——outgoing-finish 套件 server.close 后响应被杀根因）。
        if (!st || (st.req === null && st.res === null)) {
          try { sock.destroy(); } catch { /* closed meanwhile */ }
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
          // llhttp 口径：消息边界先吞前导空行（管线残段；insecure-parser
          // 套件尾部现形）再找头终结——此前只在头残缺分支吞，前导空行+
          // 完整头即误解析/400（incoming-pipelined 套件多请求连发只到首个）。
          while (st.buf.length >= 2 && st.buf[0] === 13 && st.buf[1] === 10) {
            st.buf = st.buf.slice(2);
          }
          const headEnd = __findHeadEnd(st.buf);
          if (headEnd === -1) {
            // 头段超出 maxHeaderSize：431 Request Header Fields Too Large
            //（llhttp HPE_HEADER_OVERFLOW；header-overflow 套件精确字节）。
            if (st.buf.length > maxHeaderSize) { this.__headerFieldsTooLarge(sock); return; }
            // 消息期 requestTimeout 从消息首字节起算（request-timeout-
            // pipelining 套件：管线第二请求的残缺头也必须在 requestTimeout
            // 内 408；已有计时（headersTimeout 空闲计时）在跑则不叠加，
            // 部分数据不重置——interrupted/delayed 系既有口径）。
            if (st.buf.length > 0 && st.__rqT == null && st.__hdT == null && this.requestTimeout > 0) {
              st.__rqT = setTimeout(() => { st.__rqT = null; this.__reqTimeout(sock); }, this.requestTimeout);
              st.__rqT.unref();
            }
            // 头未齐也可先校验请求行（llhttp 增量语义；管线残渣 "hello world"
            // 在 URL 段首字节即 400，等不到行终结——blank-header 套件；
            // 前导空行已在上方统一吞，此处不再重复）。
            __checkRequestLinePrefix(st.buf);
            return;
          }
          const headText = __latin1(st.buf.slice(0, headEnd));
          if (this.insecureHTTPParser !== true && __hasBareCR(headText)) {
            throw __mkParseError("LF expected after CR");
          }
          const { first, headers, rawHeaders, headersDistinct } = __parseHead(headText, this.__inboundMode ?? "strict", this.maxHeadersCount);
          __validateRequestHead(first, headers);
          // node 口径（server-options-incoming-message 套件）：IncomingMessage
          // 选项类造 req（无显式构造器即透传同参）。
          const __IM = this.IncomingMessage ?? IncomingMessage;
          const req = new __IM(this.__highWaterMark !== undefined
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
            req.headersDistinct = headersDistinct;
          req.socket = sock;
          req.connection = sock;
          // Node 口径：CONNECT 方法请求不进 request 管线——派发 'connect'
          //（req, socket, head；无监听则销毁连接），socket 停止 HTTP 解析。
          if (req.method === "CONNECT") {
            sock.__upgraded = true;
            // CONNECT 劫持标记（end 处理器跳过 FIN 销毁；升级形不动）。
            sock.__connectHijacked = true;
            try { sock.__detachSrvData && sock.__detachSrvData(); } catch { /* gone */ }
            // node 口径（connect 套件 listenerCount 矩阵真机实测：close 0/
            // drain 0/data 0/end 1/error 0/timeout 0）：隧道接管即拆 http 侧
            // 全部接线，只留 end 监听；记账（__sockets/conns/计时器）即刻结算
            //（close 监听既摘，后续 close 无需 http 收尾；hijacked 连接不再续
            // server.close() 的等待）。
            try { if (sock.__httpSockOnError !== undefined) sock.removeListener("error", sock.__httpSockOnError); } catch { /* gone */ }
            try { if (sock.__httpSockOnClose !== undefined) sock.removeListener("close", sock.__httpSockOnClose); } catch { /* gone */ }
            try { if (sock.__httpSockOnTimeout !== undefined) sock.removeListener("timeout", sock.__httpSockOnTimeout); } catch { /* gone */ }
            try {
              if (sock.__netConnsCleaner !== undefined) {
                sock.removeListener("close", sock.__netConnsCleaner);
                sock.__netConnsCleaner();
                sock.__netConnsCleaner = undefined;
              }
            } catch { /* gone */ }
            try { this.__clearReqTimers(st); } catch { /* gone */ }
            try { this.__sockets.delete(sock); } catch { /* gone */ }
            try { sock.parser = null; } catch { /* gone */ }
            const __leftover = st.buf.slice(headEnd + 4);
            st.buf = new Uint8Array(0);
            if (this.listenerCount("connect") > 0) {
              this.emit("connect", req, sock, globalThis.Buffer.from(__leftover));
            } else {
              sock.destroy();
            }
            return;
          }
          // Node 口径：升级需 connection token 'upgrade' + Upgrade 头双全
          //（advertise 套件：任缺其一即走 request 管线；llhttp 同款）。
          // shouldUpgradeCallback(req) 为 false 即回落 request（server-callback
          // 套件）；true/无回调时无 upgrade 监听才销毁（TrueWithoutHandler
          // 形 ECONNRESET），有监听派发 'upgrade'（req, socket, head）。
          // 无监听回落 request 时（advertise 末段）解析照常继续：不置
          // __upgraded、不切 buf（下方 request 管线自理）。
          const __connTokens = String(headers.connection ?? "").toLowerCase().split(",");
          const __wantsUpgrade = headers.upgrade !== undefined
            && __connTokens.some((t) => t.trim() === "upgrade");
          if (__wantsUpgrade) {
            const __hasCb = typeof this.shouldUpgradeCallback === "function";
            // 决策：回调否决（false）即 request；回调放行/无回调时有监听即
            // upgrade；回调放行但无监听即销毁（TrueWithoutHandler 形）；
            // 无回调又无监听即回落 request（advertise 末段/upgrade-server
            // no-listener 形 200，非销毁——旧"无监听即销毁"系伪语义）。
            let __goUpgrade = true;
            if (__hasCb) {
              // 抛错经 nextTick 重抛交付 uncaughtException（server-callback
              // 末段；IO 回调内裸抛到不了 uncaught 路由——dgram/nextTick 同款），
              // 连接同步先销毁（客户端 ECONNRESET）。
              let __cbOut;
              try {
                __cbOut = this.shouldUpgradeCallback(req);
              } catch (__cbErr) {
                try { sock.destroy(); } catch { /* gone */ }
                // nextTick（非 microtask——后者落 rejection 表走 fatal，
                // 只有 tick/定时回调带 uncaught 路由，见 process_.rs）。
                process.nextTick(() => { throw __cbErr; });
                return;
              }
              if (__cbOut === false) __goUpgrade = false;
            }
            if (__goUpgrade && this.listenerCount("upgrade") > 0) {
              sock.__upgraded = true;
              try { sock.__detachSrvData && sock.__detachSrvData(); } catch { /* gone */ }
              // node 口径：升级前释放解析器（parser-freed-before-upgrade 套件
              // 断言 socket.parser === null，双侧）。
              try { sock.parser = null; } catch { /* gone */ }
              // 升级后体路由：头后字节继续喂 req 体（CL/chunked 增量），体完
              // 后续字节转 socket data（large-body/unread 系套件）。native
              // __ev data 直调喂体（__srvFeed），不经 emitter（同表双发会使
              // 用户收到原始体 + spill 双份）；迟挂监听经暂存 + newListener
              // 冲刷，不丢字节。
              st.req = req;
              st.framing = __framingFor(headers, false, null, req.method);
              sock.__srvUpgraded = true;
              sock.__spillBuf = new Uint8Array(0);
              sock.__srvFeed = (c) => this.__feedUpgraded(sock, st, c);
              if (!sock.__spillFlushHook) {
                sock.__spillFlushHook = true;
                sock.on("newListener", (__evName) => {
                  if (__evName === "data") queueMicrotask(() => __spillFlush(sock));
                });
              }
              // Node 口径三参 (req, socket, head)：head 恒 Buffer（零长=无残留，
              // ws 库 setSocket 读 head.length——undefined 即 TypeError；
              // server-callback 套件点名 instanceof Buffer）。
              const leftover = globalThis.Buffer.from(st.buf.slice(headEnd + 4));
              st.buf = new Uint8Array(0);
              this.emit("upgrade", req, sock, leftover);
              if (st.framing.type === "none") {
                // 无体升级（GET 形）：当即完结，后续字节走 socket。
                if (st.req !== null) st.req.__complete();
                st.req = null;
                st.framing = null;
              }
              return;
            }
            if (__goUpgrade && __hasCb) {
              // 回调放行但无监听：销毁（TrueWithoutHandler 形 ECONNRESET）。
              sock.destroy();
              return;
            }
            // 回调否决 / 无回调无监听：落到下方 request 管线（__upgraded 不置，
            // buf 不动）。
          }
          const framing = __framingFor(headers, false, null, req.method);
          const conn = (headers.connection || "").toLowerCase();
          const keepAlive = req.httpVersion === "1.1" ? conn !== "close" : conn === "keep-alive";
          // node 口径（server-options-server-response 套件）：ServerResponse
          // 选项类造 res（传 socket；子类无显式构造器即透传，见 ServerResponse
          // ctor 双形兼容）。
          const __SR = this.ServerResponse ?? ServerResponse;
          const res = new __SR(sock);
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
          // node 口径（head-throw 套件）：服务端拒写旗逐响应透传。
          res.__rejectBody = this.rejectNonStandardBodyWrites === true;
          // 响应头决策所需服务端上下文（Keep-Alive: timeout / maxRequestsPerSocket
          // / uniqueHeaders 名单）。
          res.__kaTimeout = this.keepAliveTimeout;
          res.__maxReq = this.maxRequestsPerSocket;
          if (this.uniqueHeaders !== undefined) res.__uniqueHeaders = this.uniqueHeaders;
          st.reqCount = (st.reqCount ?? 0) + 1;
          // node 口径：超 maxRequestsPerSocket 的管线请求回 503 +
          // 关连接（keep-alive-pipeline-max-requests 套件第 4 路），并派发
          // 'dropRequest'（request, socket；drop-requests 套件点名类型）。
          if (this.maxRequestsPerSocket > 0 && st.reqCount > this.maxRequestsPerSocket) {
            try { this.emit("dropRequest", req, sock); } catch { /* 监听抛错不阻 503 */ }
            try { sock.write(new TextEncoder().encode("HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
            try { sock.destroy(); } catch { /* gone */ }
            return;
          }
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
      // node close 口径：空闲连接销毁；仍有在途请求/响应的连接 unref（进程不
      // 等待，响应落地后自然退出——pipeline-assertionerror-finish 套件；
      // 空闲判定同 closeIdleConnections，体完响应在途不算空闲）。
      for (const sock of this.__sockets) {
        const __st = sock.__httpState;
        if (!__st || (__st.req === null && __st.res === null)) {
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
      // node 口径 _removedHeader：删掉的头不再自动补（remove-header 套件）。
      this._removedHeader = {};
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
        // node 口径（lib/_http_client.js 真机源码 + url.parse 套件）：
        // hostname 优先于 host（url.parse 对象同时带 `host: "h:port"` 与
        // `hostname: "h"`，取 host 会把端口当主机名连过去即 ECONNRESET）。
        host = options.hostname ?? options.host ?? "localhost";
        // node 口径：defaultPort 逐级——显式 port > agent.defaultPort > flavor 缺省
        //（default-port 套件：globalAgent.defaultPort 动态改写生效，host 头
        // 按“port === 生效缺省”省略端口；agent 缺省取隐式 globalAgent）。
        const __ag = options.agent !== undefined ? options.agent : flavor.defaultAgent;
        const __agentDp = __ag && __ag.defaultPort !== undefined ? __ag.defaultPort : flavor.defaultPort;
        port = Number(options.port ?? __agentDp);
        path = options.path ?? "/";
        // node 口径（lib/_http_client.js 293-295 行 + connect 套件真机实测）：
        // CONNECT（authority-form）与 OPTIONS * 不补前导斜杠、不校验。
        if (method !== "CONNECT" && !(method === "OPTIONS" && path === "*")) {
          if (!path.startsWith("/")) path = "/" + path;
        }
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
      // node 口径（outgoing-properties 套件真机实测）：req.protocol 恒 flavor
      // 协议（'http:'/'https:'），非自有计算。
      this.protocol = flavor.protocol;
      // node 口径 uniqueHeaders：请求侧名单（multiple-headers 套件；名单内头
      // wire 用 '; ' 合并单行，distinct 收单元素）。
      if (options.uniqueHeaders !== undefined) {
        if (!Array.isArray(options.uniqueHeaders)) throw new codes.ERR_INVALID_ARG_TYPE("uniqueHeaders", "Array", options.uniqueHeaders);
        this.__uniqueHeaders = options.uniqueHeaders.map((h) => String(h).toLowerCase());
      }
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
      // node 口径：maxHeadersCount 缺省 null（不限），响应解析前可改写
      // （max-headers-count 套件：构造后赋值截断接收头数）。
      this.maxHeadersCount = null;
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
      // node 口径（lib/_http_client.js 真机源码 + host-header-ipv6-fail 套件）：
      // Host 拼接比较的是**显式配置**的 defaultPort（options.defaultPort ??
      // agent.defaultPort，未配置即 undefined），不是 flavor 缺省 80——
      // `+port !== defaultPort` 在 defaultPort 缺席时恒成立，故缺省恒拼 `:80`
      //（'example.com'→'example.com:80'）；仅显式缺省与 port 相等才省略。
      // IPv6 加框：双冒号以上且首字符非 '[' 才加框（'::1'→'[::1]'，
      // 'foo:1234' 单冒号不加框，直接拼端口）。
      const __cfgDp = options.defaultPort ?? (this.agent !== null ? this.agent.defaultPort : undefined);
      const __bracketHost = (() => {
        const __pos = host.indexOf(":");
        if (__pos !== -1 && host.includes(":", __pos + 1) && host.charCodeAt(0) !== 91) return `[${host}]`;
        return host;
      })();
      const __hostHeader = port !== __cfgDp ? `${__bracketHost}:${port}` : __bracketHost;
      this.__hostHeader = __hostHeader;
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
      // node 口径：headers 数组形（[k,v,...]，dupes 有序保留）与
      // setDefaultHeaders=false（禁自动 Host/Connection；dont-set-default
      // 套件）。数组形另存有序对供发头（setHeader 后调即并入有序对——真机
      // 构造期数组头不进查取表但占 wire 位，multiple-headers 套件）。
      this.__headerList = null;
      this.__headerNames = Object.create(null);
      if (Array.isArray(userHeaders)) {
        // node 口径：数组形收两种——扁平 `[k,v,...]` 与对形 `[[k,v],...]`
        //（upgrade-client 套件对形；真机逐项实测，双形同发头）。
        let __list;
        if (userHeaders.length > 0 && Array.isArray(userHeaders[0])) {
          __list = [];
          for (let __i = 0; __i < userHeaders.length; __i++) {
            const __p = userHeaders[__i];
            if (!Array.isArray(__p) || __p.length !== 2) {
              throw new codes.ERR_INVALID_ARG_TYPE(`headers[${__i}]`, "Array", __p);
            }
            const __k = String(__p[0]);
            if (!__TOKEN_RE.test(__k)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", __k);
            __list.push([__k, String(__p[1])]);
            if (this.__headerNames[__k.toLowerCase()] === undefined) this.__headerNames[__k.toLowerCase()] = __k;
          }
        } else {
          if (userHeaders.length % 2 !== 0) {
            throw new codes.ERR_INVALID_ARG_TYPE("headers", "object", userHeaders);
          }
          __list = [];
          for (let __i = 0; __i < userHeaders.length; __i += 2) {
            const __k = String(userHeaders[__i]);
            if (!__TOKEN_RE.test(__k)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", __k);
            __list.push([__k, String(userHeaders[__i + 1])]);
            if (this.__headerNames[__k.toLowerCase()] === undefined) this.__headerNames[__k.toLowerCase()] = __k;
          }
        }
        this.__headerList = __list;
        // node 口径：数组形 headers 不进 __headers 查取表（multiple-headers
        // 套件：构造期数组头只走 wire 有序对；setHeader 后调才入表）。
        // 旧 Object.fromEntries 版把首对洗进查取表系伪语义。
        this.__headers = {};
      } else {
        this.__headers = __lowerHeaders(userHeaders);
        for (const __k of Object.keys(userHeaders ?? {})) {
          this.__headerNames[String(__k).toLowerCase()] = String(__k);
        }
      }
      const __noDefaults = options.setDefaultHeaders === false;
      // 自动 CL/TE 同禁（dont-set-default 套件：POST 空体不补 CL:0；显式
      // CL/TE 照发）。_final/刷盘路径经 __noDefaults 查之。
      this.__noDefaults = __noDefaults;
      // node 口径（lib/_http_client.js 551 行 `if (options.auth && ...)` 真机原文 +
      // url.parse-auth/decoded-auth 套件实测）：options.auth 真值在场且用户未显式
      // 给 Authorization 即补 Basic（base64 全串；对象/数组双形都查，显式值恒赢）。
      if (options.auth) {
        let __hasAuth = this.__headers.authorization !== undefined;
        if (!__hasAuth && Array.isArray(this.__headerList)) {
          for (const [__k] of this.__headerList) {
            if (String(__k).toLowerCase() === "authorization") { __hasAuth = true; break; }
          }
        }
        if (!__hasAuth) {
          const __cred = Buffer.from(String(options.auth)).toString("base64");
          if (Array.isArray(this.__headerList)) {
            this.__headerList.push(["Authorization", `Basic ${__cred}`]);
          } else {
            this.__headers.authorization = `Basic ${__cred}`;
          }
          this.__headerNames.authorization = "Authorization";
        }
      }
      // setHost:true 即补 Host（dont-set 下亦补；拼写取规范 'Host'）。
      if (options.setHost === true && this.__headers.host === undefined) {
        this.__headers.host = this.__hostHeader;
        this.__headerNames.host = "Host";
      }
      if (!__noDefaults && this.__headers.host === undefined) {
        // node 口径（lib/_http_client.js 546 行 + connect-default-host-header
        // 套件真机实测）：CONNECT 且 options.path 在场时 Host 取 path 本体
        //（authority），不取连接主机。
        this.__headers.host = (method === "CONNECT" && options.path !== undefined)
          ? String(path)
          : this.__hostHeader;
        this.__headerNames.host = "Host";
      }
      // node ctor 口径（_http_client.js）：有 agent 即默认 keep-alive，
      // 仅非 keepAlive agent + maxSockets 无限时回落 close；无 agent 即 close。
      this.shouldKeepAlive = this.agent !== null &&
        (this.agent.keepAlive === true || Number.isFinite(this.agent.maxSockets));
      // node 口径：headers.host 数组即 ERR_INVALID_ARG_TYPE（host-array 套件
      // 逐字 'The "options.headers.host" property must be of type string'）。
      if (Array.isArray(userHeaders) === false && userHeaders !== undefined && userHeaders !== null &&
          Array.isArray(userHeaders.host)) {
        throw new codes.ERR_INVALID_ARG_TYPE("options.headers.host", "string", userHeaders.host);
      }
      // node 口径：自动 connection 对 header 面不可见（mutable-headers 套件：
      // getHeaderNames/getHeader/hasHeader 均不见它；真机实测 get-conn 为
      // undefined）。存旁路供发头，__headers 内不留痕。
      this.__autoConn = false;
      this.__autoConnVal = undefined;
      if (!__noDefaults && this.__headers.connection === undefined &&
          !this._removedHeader.connection) {
        this.__autoConnVal = this.shouldKeepAlive ? "keep-alive" : "close";
        this.__autoConn = true;
      }
      this.__headSent = false;
      // 真机口径（10f G3，node 26.8.2 实测）：CL 快路径仅当 end(data) 是首个
      // 头触发点；write/flushHeaders 在前 → chunked；GET/HEAD/DELETE/OPTIONS/
      // TRACE/CONNECT（useChunkedEncodingByDefault=false 族）→ 无 CL/TE 裸体。
      // setDefaultHeaders=false 一律裸体（dont-set-default 套件：无自动 CL/TE）。
      this.__chunkDefault = !this.__noDefaults &&
        !["GET", "HEAD", "DELETE", "OPTIONS", "TRACE", "CONNECT"].includes(method);
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

      if (this.__pendingNoDelay !== undefined) {
        try { sock.setNoDelay(this.__pendingNoDelay); } catch { /* gone */ }
      }
      if (this.__pendingKeepAlive !== undefined) {
        try { sock.setKeepAlive(this.__pendingKeepAlive[0], this.__pendingKeepAlive[1]); } catch { /* gone */ }
      }
      // node 口径（agent setRequestSocket + client setSocketTimeout）：
      // 构造期 timeout（__timeoutMs：请求级优先于 agent 级）在 attach 时立即
      // 武装（'socket' 事件时可见）；构造后 setTimeout 的覆写值（__reqTimeoutMs
      // 与构造期不同）defer 到 'connect'（client-set-timeout 套件时序）。
      // 假 socket（createConnection 注入的 Duplex）无 setTimeout 面则跳过。
      if (this.timeoutCb !== undefined) {
        const __ctorMs = (typeof this.__timeoutMs === "number" && this.__timeoutMs > 0) ? this.__timeoutMs : undefined;
        const __overMs = (this.__reqTimeoutMs !== undefined && this.__reqTimeoutMs !== __ctorMs) ? this.__reqTimeoutMs : undefined;
        // node setRequestSocket 口径：请求级 timeout 覆盖 agent 级（timeout-
        // option-with-agent 套件：agent 50 + 请求 100 → socket.timeout 为 100）；
        // 相等时不重臂（agent 级建连已置位）。
        if (__ctorMs !== undefined && sock.timeout !== __ctorMs) {
          this.__applySockTimeout(sock, __ctorMs);
        }
        if (__overMs !== undefined) {
          this.__deferSockTimeout(sock, __overMs);
        } else if (__ctorMs === undefined && !sock.timeout) {
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
      // node onSocket 口径：'socket' 事件异步发出——构造后同步挂载的监听
      // 必须能收到（microtask 递延仍先于 'connect'/首包到达，socket-before-
      // connect 序不变；同步直发会使构造后挂载全部 miss）。
      // 监听器抛错不得阻断 attach 后续接线（probe85d：抛错会吞掉 connect/
      // data 等全部后继注册；真机 emit 抛错同样向外抛但接线是 C++ 侧已就绪）。
      if (!this.destroyed) {
        queueMicrotask(() => {
          if (this.destroyed) return;
          try {
            this.emit("socket", sock);
          } catch (__sockEvErr) {
            queueMicrotask(() => { throw __sockEvErr; });
          }
        });
      }
      sock.on("connect", (sock.__reqSockOnConnect = () => {
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
      }));
      sock.on("secureConnect", (sock.__reqSockOnSecureConnect = () => {
        this.__connected = true;
        if (this.__pendingFinal) {
          this.__pendingFinal = false;
          this.__flushFinal();
          return;
        }
        this.__tryFlush();
      }));
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
      // 复用连接已连通：递延一拍再刷（loopback 套件：构造后同步 setHeader
      // 必须赶在发头前；同步直刷会使复用形构造即发头，后续 setHeader 全炸）。
      // 'socket' 事件 microtask 先排，发头随后——序与真机一致。
      if (reused === true) {
        const __self = this;
        queueMicrotask(() => {
          if (__self.destroyed) return;
          __self.__connected = true;
          __self.__tryFlush();
          if (__self.__pendingFinal) {
            __self.__pendingFinal = false;
            __self.__flushFinal();
          }
        });
      }
    }
