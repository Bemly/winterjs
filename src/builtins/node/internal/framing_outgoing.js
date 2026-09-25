
export class OutgoingMessage extends Writable {
  constructor(options) {
    // autoDestroy 关（同 ServerResponse：销毁一律显式）。
    // emitClose 关：req 'close' 在响应收齐/连接收尾时手动发出（9d 口径：
    // 上传 finish 不等于请求结束），见 __finishResponse/__onSockCloseEv。
    super({ autoDestroy: false, emitClose: false });
    this.headersSent = false;
    this.socket = null;
    // node 口径 OM 标记（_closed/_defaultKeepAlive/_removedConnection/
    // _removedContLen；isOutgoingMessage/willEmitClose 判定 + finished 语义）。
    this._closed = false;
    this._defaultKeepAlive = true;
    this._removedConnection = false;
    this._removedContLen = false;
    // node 口径 finished（finish-writable 套件）：自有属性，end() 同步置 true
    //（与 writableFinished 无关——后者按 finish 事件；真机 end 后 finished 真、
    // writableFinished 假）。
    this.finished = false;
    // node 口径 kOutHeaders（对表，供 _renderHeaders/internal/http 直读）。
    this[kOutHeaders] = {};
    // 独立构造（`new OutgoingMessage()`，outgoing-properties 系套件）：无 socket
    // 时 _write 缓冲不落盘——cb 不调（writableLength 保持，Node outputData 口径），
    // 有子类 socket 面时由子类 _write 覆写。
    this.__outputData = [];
  }
  // node 口径 setHeader（基类本体；proto 套件 `new OutgoingMessage()` 直调）：
  // headersSent 门 → 名 TOKEN 门（数字名亦错）→ 值 undefined 门；数组值原样存。
  setHeader(name, value) {
    // node 口径：发头标记走 `this._header`（真机按此字段判；外来 this 形
    // proto 套件 `{_header:'test'}` 即此门）。本仓实现从不自置 `_header`，
    // 故只拦外来已发头形，不误伤正常实例。
    if (this.headersSent || this._header !== undefined) throw new codes.ERR_HTTP_HEADERS_SENT("set");
    if (typeof name !== "string") throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
    if (!__TOKEN_RE.test(name)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", name);
    if (value === undefined) throw new codes.ERR_HTTP_INVALID_HEADER_VALUE("undefined", String(name));
    const lk = String(name).toLowerCase();
    if (this._removedHeader !== undefined) delete this._removedHeader[lk];
    if (this.__headers === null || this.__headers === undefined) this.__headers = {};
    const __vals = Array.isArray(value) ? [...value] : [value];
    for (const __e of __vals) __checkOutboundHeaderValue(this.__validation, __e, String(name));
    this.__headers[lk] = Array.isArray(value) ? [...value] : value;
  }
  // node lib/_http_outgoing.js _renderHeaders（renderHeaders 套件）：
  // 对表 [原名, 值] → {原名: 值}；null/非对象即 {}；_header 在场（已发头标记，
  // 真机按此字段判）即抛 ERR_HTTP_HEADERS_SENT。
  // node 口径 outputData（destroyed-socket-write2 套件直读 length；
  // outgoing-buffer 套件 outputSize 累计）：排队输出明细 {data, encoding}；
  // 本仓 socket 面同步落盘恒空（_write 缓冲语义见 __outputData）。
  get outputData() { return this.__outputData ?? []; }
  // node 口径 writableLength：精确基类直构无 socket 时走 outputSize 累计
  //（_write 缓冲语义；round1 p11 套件）；子类/有 socket 一律走流机构
  //（零行为差——未连通客户端排队字节仍按 state.length）。
  get writableLength() {
    if (this.constructor.name === "OutgoingMessage" &&
        (this.socket === null || this.socket === undefined) &&
        (this.__sock === null || this.__sock === undefined)) {
      return this.__outputSize ?? 0;
    }
    return super.writableLength;
  }
  // node 口径 write 分流（outgoing-buffer 套件）：纯 standalone（无 socket、
  // 未停靠、基类直构）缓冲进 outputData，返回值走 outputSize/HWM（while 形可终）；
  // 其余一律走流机构（子类 _write 面零行为差）。
  write(chunk, encoding, cb) {
    if (typeof encoding === "function") { cb = encoding; encoding = null; }
    const __hasSock = (this.socket !== null && this.socket !== undefined) ||
      (this.__sock !== null && this.__sock !== undefined);
    // standalone = 精确基类直构无 socket（子类实例一律走流机构，保持旧语义；
    // 外来 this（fake-this 形）按 standalone 校验块形态，proto 套件钉住）。
    const __isExactOM = (this instanceof OutgoingMessage) && this.constructor.name === "OutgoingMessage";
    if (!__isExactOM && (this instanceof OutgoingMessage)) {
      return super.write(chunk, encoding, cb);
    }
    if (__isExactOM && (__hasSock || this.__queued === true)) {
      return super.write(chunk, encoding, cb);
    }
    // node validChunk 口径（proto 套件 fake-this 形）：先校验块形态
    // （string/Uint8Array 系直收；null → NULL_VALUES；其余 → ARG_TYPE），
    // 再调 _implicitHeader（NOT_IMPLEMENTED 门在后）。已销毁即
    // ERR_STREAM_DESTROYED 进回调（outgoing-destroy 套件；不同步抛）。
    if (this.destroyed) {
      const __cb = typeof cb === "function" ? cb : null;
      queueMicrotask(() => { if (__cb) { try { __cb(new codes.ERR_STREAM_DESTROYED("write")); } catch {} } });
      return false;
    }
    if (chunk === null || chunk === undefined) {
      if (chunk === null) throw new codes.ERR_STREAM_NULL_VALUES("chunk");
      throw new codes.ERR_INVALID_ARG_TYPE("chunk", ["string", "Buffer", "Uint8Array"], chunk);
    }
    if (typeof chunk !== "string" && !(chunk instanceof Uint8Array)) {
      throw new codes.ERR_INVALID_ARG_TYPE("chunk", ["string", "Buffer", "Uint8Array"], chunk);
    }
    this._implicitHeader();
    this.__outputData.push({ data: chunk, encoding, callback: cb });
    if (typeof chunk === "string") this.__outputSize = (this.__outputSize ?? 0) + Buffer.byteLength(chunk);
    else if (chunk instanceof Uint8Array) this.__outputSize = (this.__outputSize ?? 0) + chunk.length;
    else if (chunk instanceof ArrayBuffer) this.__outputSize = (this.__outputSize ?? 0) + chunk.byteLength;
    const __hwm = (this._writableState && this._writableState.highWaterMark) || 16384;
    return this.outputSize < __hwm;
  }
  // node 口径 end 即 finished（finish-writable 套件同步断言；writableFinished
  // 仍按 finish 事件）。
  end(chunk, encoding, cb) {
    if (typeof chunk === "function") { cb = chunk; chunk = null; encoding = null; }
    else if (typeof encoding === "function") { cb = encoding; encoding = null; }
    this.finished = true;
    return super.end(chunk, encoding, cb);
  }
  // node 口径 outputSize：排队字节累计计数（逐写 O(1)；全量求和即 O(n²)，
  // buffer 套件 21845 轮必超时）。
  get outputSize() { return this.__outputSize ?? 0; }
  // node 口径 writable 恒 true（finish-writable 套件：end/close 后仍 true，
  // LEGACY；背压走 write() 返回值与 needDrain，不走此旗）。
  get writable() { return true; }
  _renderHeaders() {
    if (this._header) throw new codes.ERR_HTTP_HEADERS_SENT("render");
    const src = this[kOutHeaders];
    const out = {};
    if (src !== null && typeof src === "object") {
      for (const k of Object.keys(src)) {
        const e = src[k];
        if (Array.isArray(e) && e.length >= 2) out[e[0]] = e[1];
      }
    }
    return out;
  }
  _write(chunk, encoding, cb) {
    // node 口径：基类写即调 _implicitHeader（未覆写即
    // ERR_METHOD_NOT_IMPLEMENTED；outgoing-buffer 套件先覆写再用）。
    // 明细原样存（data/encoding/callback 三键）。
    this._implicitHeader();
    this.__outputData.push({ data: chunk, encoding, callback: cb });
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
    throw new codes.ERR_METHOD_NOT_IMPLEMENTED("_implicitHeader()");
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
    // node 口径：OutgoingMessage 无错时 errored 为 undefined（IncomingMessage
    // 侧为 null；outgoing-destroyed:89 钉住），有错回 err 本体。
    const __v = this.__omErrored ?? (this._writableState ? this._writableState.errored : null);
    return __v === null || __v === undefined ? undefined : __v;
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
    // node 口径：无参即 TypeError（proto 套件只认类型不认文案；引擎
    // Object.entries(undefined) 同款）。
    if (trailers === null || trailers === undefined) {
      throw new TypeError("Cannot convert undefined or null to object");
    }
    // node 口径：对形 `[[k,v],...]` 与对象形双收（raw-headers 套件；writeHead
    // 对形同源）。对形逐对取 [0]/[1] 归一。
    if (Array.isArray(trailers)) {
      const __flat = {};
      for (const __p of trailers) {
        const __k = String(__p[0]);
        if (__flat[__k] === undefined) __flat[__k] = [];
        __flat[__k].push(__p[1]);
      }
      trailers = __flat;
    }
    for (const k of Object.keys(trailers)) {
      // node 口径：trailer 名门标签为 "Trailer name"（proto 套件逐字），
      // 非 header 门。
      if (!__TOKEN_RE.test(String(k))) throw new codes.ERR_INVALID_HTTP_TOKEN("Trailer name", String(k));
      const __v = trailers[k];
      const __vals = Array.isArray(__v) ? __v : [__v];
      // node 口径 uniqueHeaders：名单内 trailer 行 wire 合并单行 '; '
      //（multiple-headers 套件 rawTrailers 收单对）。
      const __uniq = this.__uniqueHeaders;
      if (Array.isArray(__uniq) && __uniq.includes(String(k).toLowerCase())) {
        for (const __e of __vals) {
          // node 口径：trailer 值门标签为 "trailer content"（proto 套件逐字）。
          if (!__validHeaderValue(String(__e))) throw new codes.ERR_INVALID_CHAR(String(k), "trailer content");
        }
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
        // node 口径：trailer 值门标签为 "trailer content"（proto 套件逐字）。
        if (!__validHeaderValue(String(__e))) throw new codes.ERR_INVALID_CHAR(String(k), "trailer content");
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
//（client-reject-* 套件断言 err.code 与 /^Parse Error/）。__parseErr 旗供
// __sockOnData 区分解析错（→ req destroy+error）与用户回调 throw（→ 重抛，
// uncaught-from-request-callback 套件：response 监听 throw 须到
// uncaughtException，node 解析错走返回值通道、用户 throw 原样上抛）。
function __hpe(code, msg) {
  const e = new Error(`Parse Error: ${msg}`);
  e.code = code;
  // node 口径 reason（rawbytes 套件：裸消息，与 message 的前缀形并存）。
  e.reason = msg;
  e.__parseErr = true;
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
// node 真机继承链 ServerResponse extends OutgoingMessage（_http_server 1294）。
// 本仓 ServerResponse 帧机构独立成类（framing_head），原型桥会改道其
// super.write/end/cork/destroy 全链（cork 面实锤）——身份语义改走品牌判定：
// `x instanceof OutgoingMessage` = 真链命中（ClientRequest/OM 本体）或
// ServerResponse 品牌位（set-timeout-server 套件 res 形断言）。
Object.defineProperty(OutgoingMessage, Symbol.hasInstance, {
  value: function (i) {
  if (i === null || (typeof i !== "object" && typeof i !== "function")) return false;
    return Object.prototype.isPrototypeOf.call(this.prototype, i) || i.__omBrand === true;
  },
  writable: true,
  configurable: true,
});

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
      // node 口径 maxHeaderSize（缺省 16384；max-header-size-per-stream 套件
      // 逐流覆写——服务端选项/客户端请求选项双侧）。
      if (o.maxHeaderSize !== undefined) {
        const __mhs = Number(o.maxHeaderSize);
        if (Number.isFinite(__mhs) && __mhs >= 0) self.maxHeaderSize = __mhs;
      }
      if (self.maxHeaderSize === undefined) self.maxHeaderSize = __defaultMaxHeaderSize();
      // 每服务器宽松解析旗（insecure-parser-per-stream 套件）。
      // httpValidation 门（node storeHTTPOptions 口径：validateOneOf + 与
      // insecureHTTPParser 互斥，ERR_INVALID_ARG_VALUE）。
      // node 口径：--insecure-http-parser 进程旗（兼容旗透传）未显式给选项时
      // 即默认宽松（真机：旗开即全局 lenient；显式选项恒赢）。
      const __insecDefault = o.httpValidation === undefined && o.insecureHTTPParser === undefined &&
        typeof globalThis.__wjs_nodeCompat !== "undefined" &&
        Array.isArray(globalThis.__wjs_nodeCompat) &&
        globalThis.__wjs_nodeCompat.includes("--insecure-http-parser");
      self.__inboundMode = __parseModeOf(__resolveHttpValidation(o.httpValidation, __insecDefault ? true : o.insecureHTTPParser));
      self.insecureHTTPParser = o.insecureHTTPParser ?? __insecDefault;
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
      // node 口径 joinDuplicateHeaders（缺省 false：重复头首个赢；true 即
      // ', ' 合并；cookie '; '/set-cookie 数组不受门控，真机 26.8.2 实测）+
      // requireHostHeader（缺省 true：1.1 缺 Host 即静默 400）+
      // noDelay（缺省 true，真机实测；建连透传）。
      self.joinDuplicateHeaders = o.joinDuplicateHeaders === true;
      self.requireHostHeader = o.requireHostHeader !== false;
      self.noDelay = o.noDelay !== false;
      if (o.shouldUpgradeCallback !== undefined) self.shouldUpgradeCallback = o.shouldUpgradeCallback;
      // node 口径（head-throw 套件）：rejectNonStandardBodyWrites 缺省 false。
      self.rejectNonStandardBodyWrites = o.rejectNonStandardBodyWrites === true;
      // node kOptimizeEmptyRequests 口径（optimize-empty-requests 套件）：缺省 false。
      self.optimizeEmptyRequests = o.optimizeEmptyRequests === true;
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
        // HTTP 服务端 socket 标记（socket-encoding-error 套件：禁 setEncoding）。
        try { sock.__httpServerSocket = true; } catch { /* gone */ }
        const st = { buf: new Uint8Array(0), req: null, framing: null, res: null, __hdT: null, __rqT: null, __kaT: null };
        sock.__httpState = st;
        // node 口径 socket.parser（connection-list-when-close 套件）：每连接
        // 解析器对象（free/close/remove 可覆写；升级/CONNECT 即置 null）。
        // free 单发语义（freeParser 口径）：res finish 与 socket close 各调一次
        // 机会，已释放即跳过；新请求解析即复位（keep-alive 复用逐轮释放）。
        if (sock.parser === undefined || sock.parser === null) {
          sock.parser = {
            free() { /* 默认：归池，无可观测 */ },
            close() { /* 默认：关闭，无可观测 */ },
            remove() { /* 默认：摘除，无可观测 */ },
          };
          // node 口径 kOnTimeout（memory-retention 套件：request 期为函数，
          // socket close 即 null；键值见 _http_common 骨架 kOnTimeout=6）。
          sock.parser[6] = function () { /* 默认：超时，无可观测 */ };
        }
        sock.__parserFreed = false;
        self.__freeSocketParser = self.__freeSocketParser ?? ((s) => {
          try {
            if (s.__parserFreed || s.parser === undefined || s.parser === null) return;
            s.__parserFreed = true;
            s.parser.free();
          } catch { /* 用户覆写抛错不阻收尾 */ }
        });
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
          // 同源迟到错（res 已因本次失败销毁且记错）：错误已走 res 通道递送，
          // 不再重抛（writable-finished 套件 mock 形；否则同一失败报两次，
          // 第二次变 uncaught）。
          if (__r !== null && __r !== undefined && __r.destroyed) {
            let __re = null;
            try { __re = __r.errored; } catch {}
            if (__re !== null && __re !== undefined) return;
          }
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
          // 连接关闭即释放解析器（单发守卫见 connection 段）+ kOnTimeout 清理
          // （memory-retention 套件：close 后为 null）。
          try { self.__freeSocketParser(sock); } catch { /* gone */ }
          try { if (sock.parser !== undefined && sock.parser !== null) sock.parser[6] = null; } catch { /* gone */ }
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
          // node 口径 req.signal 早夭 abort（request-signal 套件）：socket 关闭
          // 时响应未完（res 缺席即请求未完）→ abort req signal；正常收齐
          // （res 已 end）永不 abort（真机探针钉住）。幂等，多次关闭不重发。
          try {
            const __rq = st.req;
            if (__rq !== null && __rq !== undefined) {
              const __rs = st.res;
              const __premature = (__rs === null || __rs === undefined)
                ? (!__rq.complete || !__rq.readableEnded)
                : (__rs.writableEnded !== true);
              if (__premature) {
                try {
                  if (__rq.__abortController !== null && __rq.__abortController !== undefined) {
                    try { __rq.__abortController.abort(); } catch { /* 幂等 */ }
                  } else {
                    __rq.__signalAborted = true;
                  }
                } catch { /* signal 永不阻收尾 */ }
              }
            }
          } catch { /* signal 永不阻收尾 */ }
          if (st.res !== null && !st.res.writableEnded && !st.res.destroyed) {
            st.res.destroy();
          }
          // 管线排空销毁（轮转未及的入列响应同收尾，不悬挂 parked 回调）。
          if (st.outgoing !== undefined && st.outgoing !== null) {
            for (const __q of st.outgoing) {
              try { if (__q !== undefined && !__q.destroyed) __q.destroy(); } catch { /* gone */ }
            }
            st.outgoing = [];
          }
          // 手动 close 兼容（req-close-robust 套件直调 close 监听）：传输尚活
          // 即真杀（正常路径已 destroy 即 no-op，不碰自然收尾）。
          try { if (!sock.destroyed) sock.destroy(); } catch { /* gone */ }
        };
        sock.on("close", sock.__httpSockOnClose);
        // server.timeout：per-socket 空闲计时（10f；单发 timer，data 到达即重臂，
        // 见 data 处理器）。到期 server 发 'timeout'(socket)，不杀连接（net 口径）。
        // 监听只在计时武装时挂（无条件挂会使 listenerCount 恒多 1——connect 套件
        // 矩阵；缺省 timeout=0 即不挂）。存根供 CONNECT 隧道 detach 摘除。
        // node socketOnTimeout（_http_server 785/900-906 行逐字）：每连接
        // 无条件挂——socket 超时（server.timeout 空闲计时或 req/res.setTimeout
        // 自设）即 req（未完结才发）/ res / server 三路转发，全带 socket 实参
        //（set-timeout-server 套件 requestNotTimeoutAfterEnd/res 形/idle 形）。
        sock.__httpSockOnTimeout = () => {
          if (sock.destroyed) return;
          try {
            if (st.req !== null && st.req !== undefined && !st.req.complete) {
              st.req.emit("timeout", sock);
            }
          } catch { /* gone */ }
          try {
            if (st.res !== null && st.res !== undefined) {
              st.res.emit("timeout", sock);
            }
          } catch { /* gone */ }
          try { self.emit("timeout", sock); } catch { /* gone */ }
        };
        sock.on("timeout", sock.__httpSockOnTimeout);
        if (self.timeout > 0) sock.setTimeout(self.timeout);
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
    // node _http_server.js 716 行逐字：captureRejections 的 request 事件兜底
    //（events.captureRejections = true 时 async handler throw 走此路，不进
    // uncaught）——未发头：清头防泄漏 + 500 'Internal Server Error'；
    // 已发头：destroy（server-capture-rejections 套件三块）。
    [Symbol.for("nodejs.rejection")](err, event, ...args) {
      if (event !== "request") return;
      const res = args[1];
      if (res === null || res === undefined) return;
      if (!res.headersSent && !res.writableEnded) {
        try {
          for (const name of res.getHeaderNames()) res.removeHeader(name);
          res.statusCode = 500;
          // STATUS_CODES[500]（RFC 7231 6.6.1 reason 固定串）。
          res.end("Internal Server Error");
        } catch { /* 已销毁即无事可做 */ }
      } else {
        try { res.destroy(); } catch { /* gone */ }
      }
    }
    // 400 Bad Request（Node clientError 默认响应）+ 销毁（err 在场即经
    // destroy 递送 socket 'error'，真机默认分支口径）。
    __badRequest(sock, err) {
      try { sock.write(new TextEncoder().encode("HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
      try { err !== undefined ? sock.destroy(err) : sock.destroy(); } catch { /* gone */ }
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
        // rawPacket 缺席即按触发当片补齐（llhttp rawPacket=当片，非累计；
        // 有监听无监听一律补——clientError 监听侧照常断言 rawPacket）。
        if (e.rawPacket === undefined || e.rawPacket === null) {
          try {
            const __st = sock.__httpState;
            e.rawPacket = globalThis.Buffer.from((__st && __st.__lastPkt) ?? new Uint8Array(0));
          } catch { /* gone */ }
        }
        // node 口径：clientError 恒发；无监听才落默认响应 + 销毁——
        // HPE_HEADER_OVERFLOW 默认 431（overflow 套件真机实证），其余 400.
        // 默认分支附带 socket 'error'（真机： SockError 先于 431 到达，close
        // hadError=true）——只递送给用户监听（计数含 __httpSockOnError 兜底，
        // 故 >1 才带 err 销毁；仅兜底时递送即 tick 内裸抛走 uncaught fatal）。
        this.emit("clientError", e, sock);
        if (this.listenerCount("clientError") === 0) {
          const __hasSockErr = typeof sock.listenerCount === "function" && sock.listenerCount("error") > 1;
          const __kill = () => { try { __hasSockErr ? sock.destroy(e) : sock.destroy(); } catch { /* gone */ } };
          if (e.code === "HPE_HEADER_OVERFLOW") {
            try { sock.write(new TextEncoder().encode("HTTP/1.1 431 Request Header Fields Too Large\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
            __kill();
          } else {
            this.__badRequest(sock, __hasSockErr ? e : undefined);
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
      // node 口径（双套件钉出）：headersTimeout 计时开于 **连接建立**（首
      // 消息未启，408 可先于首字节——request-timeout 黑盒 block5）与 **新
      // 消息首字节**（残头未齐——headers-timeout-keepalive 第二请求）；
      // 请求完成即撤（空闲 keep-alive 1.5×headersTimeout 无 408——同套件
      // 第一阶段）。buf 非空=残头在途照开；sawRequest=false=首消息未启照开。
      if (this.headersTimeout > 0 &&
          (st.buf.length > 0 || st.sawRequest !== true)) {
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
