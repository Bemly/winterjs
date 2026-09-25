export class ServerResponse extends Writable {
  constructor(sock) {
    // autoDestroy 关（finish 后连接必须活着；销毁由 finish-hook 手动排——
    // 流机构 auto 会在 finish 发射链中重入 destroy，b5 实锤 hang。手动 destroy
    // 在 finish 监听后 microtask 排，时序干净；req-res-close 套件时序同满足）。
    // 写机构 HWM 跟 socket 可写 HWM（node 口径：res 背压由 conn.write 治理，
    // socket 机构 HWM 是判据——response-drain-cork 套件改
    // `socket._writableState.highWaterMark = 1000` 后 res.write(1010) 即 false；
    // 本仓背压机构在 res 层，HWM 构造期取 socket 侧，缺省 socket 双 65536）。
    const __shwm = sock && typeof sock.write === "function" &&
      sock._writableState !== undefined && sock._writableState !== null
      ? sock._writableState.highWaterMark : undefined;
    super({ autoDestroy: false, highWaterMark: __shwm });
    // finish 后手动 destroy（close 随后；req-res-close 套件：finish 时 destroyed
    // 仍 false，close 时 true）。_destroy 智能分流（见下）。
    this.once("finish", () => {
      queueMicrotask(() => {
        if (this.destroyed) return;
        this.__autoTeardown = true;
        try { this.destroy(); } catch { /* gone */ }
      });
    });
    // socket 写错只进写/终结回调、不外发 res 'error'（writable-finished 套件
    // block3/4 无 error 监听；用户自有监听仍可达，仅永不无监听抛错——
    // OM destroy 吞错的构造期版，同口径）。
    this.on("error", () => {});
    // 构造首参：server 流程传 socket；独立构造传 req 形信息对象（node 口径
    // `new ServerResponse(req)`，standalone 套件）——非 socket 一律不入 __sock。
    this.__sock = sock && typeof sock.write === "function" ? sock : null;
    this.__sockAssigned = false;
    // node 口径 writableLength（outgoing-properties 套件）：已排队待上 socket
    // 的字节（渲染头 + 帧化块；standalone 无头即裸块累计）。流机构 length 不
    // 可用（_write 回调 microtask 即清零，与落盘节奏脱节），故独立记账：
    // 写时预测递增、落盘按实际递减、_final 兜底清零。
    this.__wlen = 0;
    this.__headCounted = false;
    // node 口径：res.socket / res.connection 指向响应 socket（agent-keepalive
    // 套件服务端经 res.connection 取 socket 再 end）。
    this.socket = this.__sock;
    this.connection = this.__sock;
    this.statusCode = 200;
    this.statusMessage = undefined;
    this.__headers = Object.create(null);
    // node 口径 _removedHeader：删掉的头不再自动补（remove-header 套件）。
    this._removedHeader = {};
    // 用户拼写记录（node kOutHeaders [name, value] 口径：wire 保留原大小写）。
    this.__headerNames = Object.create(null);
    this.headersSent = false;
    this.__headSent = false;
    // node _storeHeader 决策表旗标（真机 26.8.2 + lib/_http_outgoing.js 对拍）：
    // __uced = useChunkedEncodingByDefault（1.1 恒 true；1.0 = /chunked/i.test
    // 请求 TE 头）；__last = 响应后关连接（node _last）；__keepAlive = llhttp
    // shouldKeepAlive；__contentLength 由 end() 快路径预置（node _contentLength）。
    this.__headStored = false;
    this.__uced = true;
    this.__last = false;
    this.__keepAlive = false;
    this.__chunked = false;
    this.__rawCL = false;
    this.__contentLength = undefined;
    this.__kaTimeout = undefined;
    this.__maxReq = 0;
    this.__maxReqReached = false;
    this.__defaultKA = true;
    this.__buf1 = null;
    this.__holdTimer = null;
    this.__headOnly = false;
    this.__noBody = false;
    // node 口径（head-throw 套件真机实测）：rejectNonStandardBodyWrites 为 true
    // 时，1xx/204/304/HEAD 响应的 write/end(chunk) 同步抛 BODY_NOT_ALLOWED。
    this.__rejectBody = false;
    this.__userEnded = false;
    this.__onDone = null;
    // node 口径 finished（finish-writable 套件 end 后同步真）。
    this.finished = false;
    // 独立构造：从 req 形对象提取版本/方法面（node ServerResponse ctor 口径）。
    if (this.__sock === null && sock && typeof sock === "object") {
      if (sock.method === "HEAD") this.__headOnly = true;
      const hv = String(sock.httpVersion ?? "1.1");
      if (hv === "1.0") {
        this.__uced = /(?:^|\W)chunked/i.test(sock.headers?.te ?? "");
        this.__keepAlive = false;
      }
    }
  }
  // node 口径 _implicitHeader（proto 套件：ServerResponse.prototype 上须为函数；
  // 本仓发头走 __sendHead，此处转调）。
  _implicitHeader() {
    if (!this.__headSent) this.__sendHead();
  }
  // node 口径 writableLength 覆写（基类流机构 getter 只计未调 _write 的字节，
  // 与 socket 落盘脱节——outgoing-properties 套件要渲染头+帧化块精确字节）。
  get writableLength() { return this.__wlen ?? 0; }
  // socket 落盘统一出口（记账递减 + 递送；钳零——100-continue/终结块等非 body
  // 写不参与计数，CL 快捷真值由 _final 兜底清零对齐）。
  // node 口径：socket 写错回传 _write/_final 回调（writable-finished 套件
  // mock Duplex 形；确认计数等待，见顶层 __afterSockFlush）。
  // box 为空即沿旧路无回调（零行为差）。
  __sockCap() {
    const box = { err: null, pend: 0, waiters: [] };
    box.cap = (e) => {
      if (e !== undefined && e !== null && box.err === null) box.err = e;
      box.pend--;
      if (box.pend === 0) {
        const ws = box.waiters;
        box.waiters = [];
        for (const w of ws) { try { w(box.err); } catch {} }
      }
    };
    return box;
  }
  // 落盘出错统一收尾（客户端 __failFlush 同款）：显式递送同一 err 后
  // destroy() 收尾（silent，不外发 res error；close 照发）。
  __failFlush(err, finalCb) {
    if (typeof finalCb === "function") { try { finalCb(err); } catch {} }
    try { this.destroy(); } catch {}
  }
  __sockWrite(b, box) {
    // node 口径：落盘同步（spurious-aborted 套件：字节即时上网），wlen 递减异步
    //（writableLength 按 onwrite 节奏——同步读 write() 后仍见排队字节，
    // outgoing-properties 131/139；微任务降零后释停靠 drain）。
    const __n = b !== undefined && b !== null ? b.length : 0;
    queueMicrotask(() => {
      try {
        if (typeof this.__wlen === "number") {
          this.__wlen = Math.max(0, this.__wlen - __n);
        }
      } catch { /* 计数永不阻递送 */ }
      // 停靠 drain 释放（计数清零即递送，异步一轮——真机 drain 恒异步）。
      if (this.__wlen === 0 && this.__parkedDrain === true) {
        this.__parkedDrain = false;
        queueMicrotask(() => {
          if (!this.destroyed) { try { super.emit("drain"); } catch { /* 监听抛错不阻收尾 */ } }
        });
      }
    });
    // bytesWritten pending 核销（实际落盘长度；CL 快捷等真值偏差由 _final 兜底）。
    // 与 wlen 不同：bytesWritten 是累计值，同步核销无可观测差。
    try {
      if (this.__sock !== null && typeof this.__sock.__bwSub === "function") {
        this.__sock.__bwSub(__n);
      }
    } catch { /* 计数永不阻递送 */ }
    if (box === null || box === undefined) {
      try { return this.__sock.write(b); } catch { return false; }
    }
    box.pend++;
    try { return this.__sock.write(b, box.cap); } catch (e) { try { box.cap(e); } catch {} return false; }
  }
  // 头渲染 dry-run（计数专用）：快照→渲染→取值→还原。调用点保证头已终局
  // （首 _write 后 setHeader/writeHead 即抛，头冻结），故重渲染逐字节恒等
  // （Date 同长），还原后真实渲染不受影响。
  __predictHeadLen() {
    const __snap = {
      headers: { ...this.__headers },
      headSent: this.__headSent,
      headersSent: this.headersSent,
      chunked: this.__chunked,
      rawCL: this.__rawCL,
      last: this.__last,
      keepAlive: this.__keepAlive,
      autoConn: this.__autoConn,
      autoDate: this.__autoDate,
      autoKA: this.__autoKA,
      defaultKA: this.__defaultKA,
      contentLength: this.__contentLength,
    };
    let __n = 0;
    try {
      __n = this.__headBytes().length;
    } catch { __n = 0; }
    this.__headers = __snap.headers;
    this.__headSent = __snap.headSent;
    this.headersSent = __snap.headersSent;
    this.__chunked = __snap.chunked;
    this.__rawCL = __snap.rawCL;
    this.__last = __snap.last;
    this.__keepAlive = __snap.keepAlive;
    this.__autoConn = __snap.autoConn;
    this.__autoDate = __snap.autoDate;
    this.__autoKA = __snap.autoKA;
    this.__defaultKA = __snap.defaultKA;
    this.__contentLength = __snap.contentLength;
    return __n;
  }
  // 块帧化长度预测（与 __frame / _write 落盘判定同谓词，见 _write 1338 行族）。
  __predictFrameLen(u8) {
    if (this.__noBody || this.__headOnly || u8.length === 0) return 0;
    const __ch = this.__chunked || (this.__uced && this.__headers["content-length"] === undefined &&
      this.__headers["transfer-encoding"] === undefined && !this.__frameSuppressed());
    if (__ch && !this.__rawCL) return u8.length.toString(16).length + 2 + u8.length + 2;
    return u8.length;
  }
  // 写时记账（_write/_send 入口）：首渲染头 + 帧化块。socket-null 停靠（Slice B
  // 管线队列）同计数，assignSocket 排空时按实际递减。
  __countOut(u8) {
    if (!this.__headCounted && !this.__headSent) {
      this.__headCounted = true;
      this.__wlen += this.__predictHeadLen();
    }
    this.__wlen += this.__predictFrameLen(u8);
  }
  setHeader(name, value) {
    if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("set");
    // node 口径（write-head 套件真机实测）：数字名亦 HTTP_TOKEN（"3840" 本身是
    // 合法 token 字符，故不能只测 String(name)，须先判 typeof）。
    if (typeof name !== "string") throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
    if (!__TOKEN_RE.test(name)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", name);
    if (value === undefined) throw new codes.ERR_HTTP_INVALID_HEADER_VALUE("undefined", String(name));
    const lk = String(name).toLowerCase();
    if (this._removedHeader !== undefined) delete this._removedHeader[lk];
    // node 口径：数组值原样存（set-cookie/array 套件；wire 逐行发出）。
    if (Array.isArray(value)) {
      const __arr = [];
      for (const __e of value) {
        __checkOutboundHeaderValue(this.__validation, __e, String(name));
        __arr.push(__e);
      }
      this.__headers[lk] = __arr;
    } else {
      __checkOutboundHeaderValue(this.__validation, value, String(name));
      this.__headers[lk] = value;
    }
    // node 口径：wire 保留用户原拼写（kOutHeaders 存 [name, value] 原文名，
    // 首写优先——multiple-headers 套件全 'X-Res-a'）。
    if (this.__headerNames[lk] === undefined) this.__headerNames[lk] = String(name);
    return this;
  }
  // node OutgoingMessage.appendHeader（header-value-relaxed 套件点名）。
  // node 口径（multiple-headers 套件 + 真机探针）：缺省追加为单元素；
  // 任意一侧数组即数组拼接；发头后即 ERR_HTTP_HEADERS_SENT。
  appendHeader(name, value) {
    if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("append");
    if (typeof name !== "string") throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
    if (!__TOKEN_RE.test(name)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", name);
    const lk = String(name).toLowerCase();
    const __vals = Array.isArray(value) ? value : [value];
    for (const __e of __vals) __checkOutboundHeaderValue(this.__validation, __e, String(name));
    const cur = this.__headers[lk];
    if (cur === undefined) {
      this.__headers[lk] = Array.isArray(value) ? [...value] : value;
    } else if (Array.isArray(cur)) {
      for (const __e of __vals) cur.push(__e);
    } else {
      this.__headers[lk] = Array.isArray(value) ? [cur, ...value] : [cur, value];
    }
    if (this.__headerNames[lk] === undefined) this.__headerNames[lk] = String(name);
    return this;
  }
  // node 口径：查询面只见用户头（[kOutHeaders]）——自动头（Date/Connection/
  // Keep-Alive/自动 CL/TE）_storeHeader 时虽入 __headers 供状态机读，但对
  // getHeader/hasHeader/getHeaderNames/getHeaders/getRawHeaderNames 不可见
  //（mutable-headers：发送前后 hasHeader('Connection') 恒 false；
  // multiple-headers：getHeaderNames 不见自动 TE）。
  __isAutoKey(k) {
    if (k === "connection" && this.__autoConn) return true;
    if (k === "date" && this.__autoDate) return true;
    if (k === "keep-alive" && this.__autoKA) return true;
    if ((k === "content-length" || k === "transfer-encoding") &&
        (this.__headerNames?.[k] === undefined)) return true;
    return false;
  }
  __userKeys() {
    return Object.keys(this.__headers).filter((k) => !this.__isAutoKey(k));
  }
  getHeader(name) {
    if (typeof name !== "string") throw new codes.ERR_INVALID_ARG_TYPE("name", "string", name);
    const lk = name.toLowerCase();
    return this.__isAutoKey(lk) ? undefined : this.__headers[lk];
  }
  removeHeader(name) {
    // node 口径：发头后即 ERR_HTTP_HEADERS_SENT（remove-header-after-sent 套件）。
    if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("remove");
    if (typeof name !== "string") throw new codes.ERR_INVALID_ARG_TYPE("name", "string", name);
    const lk = name.toLowerCase();
    delete this.__headers[lk];
    delete this.__headerNames[lk];
    // node _removedHeader 口径：删掉的自动头不再补（remove-header 套件）。
    if (this._removedHeader !== undefined) this._removedHeader[lk] = true;
    return this;
  }
  getHeaderNames() { return this.__userKeys(); }
  hasHeader(name) {
    if (typeof name !== "string") throw new codes.ERR_INVALID_ARG_TYPE("name", "string", name);
    const lk = name.toLowerCase();
    return !this.__isAutoKey(lk) && this.__headers[lk] !== undefined;
  }
  getHeaders() {
    const __out = Object.create(null);
    for (const k of this.__userKeys()) __out[k] = this.__headers[k];
    return __out;
  }
  getRawHeaderNames() {
    const __names = this.__headerNames ?? {};
    return this.__userKeys().map((k) => __names[k] ?? k);
  }
  // node 口径 statusMessage 校验（status-reason-invalid-chars 套件）：
  // \r\n/NUL/DEL/非 latin1 即同步抛 'Invalid character in statusMessage'；
  // null/undefined 照存（构造与回滚路径）。
  get statusMessage() { return this.__statusMessage; }
  set statusMessage(v) {
    if (v !== undefined && v !== null) {
      const __s = String(v);
      for (let __i = 0; __i < __s.length; __i++) {
        const __cc = __s.charCodeAt(__i);
        if (!(__cc === 9 || (__cc >= 32 && __cc <= 126) || (__cc >= 128 && __cc <= 255))) {
          throw new Error("Invalid character in statusMessage");
        }
      }
    }
    this.__statusMessage = v;
  }
  // node 口径 writable 恒 true（finish-writable 套件 LEGACY；背压走 write()
  // 返回值与 needDrain，不走此旗）。
  get writable() { return true; }
  writeHead(status, ...rest) {
    // node 口径（write-head 套件真机实测）：已发头再 write 即 HEADERS_SENT。
    if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("write");
    // node 口径：writeHead 即算发头（setheaders 套件 writeHead 后 setHeaders
    // 即 HEADERS_SENT）——旗在合并完成后立，合并走 setHeader 不得自炸。
    // 状态码门（node lib/_http_server.js writeHead 原文 + response-statuscode
    // 套件 13 形态：`statusCode |= 0` 后判 100..999，错抛原值（`%s` 遇对象走
    // inspect——{}→'{}'；字符串 '1000' 越界仍原样；RangeError）。
    const __origStatus = status;
    status |= 0;
    if (status < 100 || status > 999) {
      throw new codes.ERR_HTTP_INVALID_STATUS_CODE(__origStatus);
    }
    const obj = rest.find((r) => r && typeof r === "object");
    // node 口径：writeHead 的头参数收扁平数组（setheaders 套件块 4
    // ['foo','3'] 即覆盖；与 ClientRequest 构造器数组形同源）。
    const __objArr = Array.isArray(obj) ? obj : null;
    const msg = rest.find((r) => typeof r === "string");
    // node 口径：writeHead 原子提交——合并/校验抛错（TRAILER_INVALID 等）即
    // 全量回滚（timeout 黑盒 t9：失败的 writeHead 不得污染 CL/状态码/旗位，
    // 否则后续 removeHeader + end 全灭）。
    const __snap = {
      headers: this.__headers,
      names: this.__headerNames,
      removed: this._removedHeader,
      code: this.statusCode,
      message: this.statusMessage,
      stored: this.__storedStatus,
      sent: this.headersSent,
    };
    // 浅拷贝表层（值数组另拷，防合并中途污染原数组）。
    const __copyHeaders = () => {
      const __o = Object.create(null);
      for (const __k of Object.keys(this.__headers)) {
        const __v = this.__headers[__k];
        __o[__k] = Array.isArray(__v) ? [...__v] : __v;
      }
      return __o;
    };
    this.__headers = __copyHeaders();
    this.__headerNames = { ...(this.__headerNames ?? {}) };
    this._removedHeader = { ...(this._removedHeader ?? {}) };
    const __rollback = (e) => {
      this.__headers = __snap.headers;
      this.__headerNames = __snap.names;
      this._removedHeader = __snap.removed;
      this.statusCode = __snap.code;
      this.statusMessage = __snap.message;
      this.__storedStatus = __snap.stored;
      this.headersSent = __snap.sent;
      throw e;
    };
    this.statusCode = status;
    // node 口径：wire 状态码以 writeHead 时为准，事后改 statusCode 属性只改
    // 属性值、不改 wire（mutable-headers writeHead 案：属性 201/wire 200）。
    this.__storedStatus = status;
    // node 口径（write-head 套件真机实测）：无显式短语即标准短语，未知码为
    // 'unknown'（220 案；wire 同）。
    if (msg !== undefined) this.statusMessage = msg;
    else this.statusMessage = STATUS_CODES[status] ?? "unknown";
    // node 口径：writeHead 合并头值数组原样存（逐行发出；multiple-headers 套件
    // 'x-res-c': ['HHH','III'] 即两行，旧 Object.assign 经 __lowerHeaders 洗成
    // 逗号串系伪语义）。扁平数组即逐对 setHeader（setheaders 套件块 4）。
    // 内联合并（不得走 setHeader：headersSent 门会自炸；校验与存值同对象分支）。
    // 合并 + 校验整体 try 包裹，抛错即回滚（上见 __rollback）。
    try {
    if (__objArr !== null) {
      // node 口径（set-trailers 套件真机实测）：writeHead 收对形 `[[k,v],...]`
      //（与 ClientRequest 构造器双形同源）——逐对取 [0]/[1]（ ["b"] 即 value
      // undefined 走 INVALID_HEADER_VALUE；超长对多余元忽略），归一扁平后走
      // 下方同套逻辑。
      let __pairs = __objArr;
      if (__objArr.length > 0 && Array.isArray(__objArr[0])) {
        __pairs = [];
        for (const __p of __objArr) __pairs.push(__p[0], __p[1]);
      }
      // node 口径（write-head 套件真机实测）：奇长数组即 ARG_VALUE 'headers'，
      // 非 ARG_TYPE（"The argument 'headers' is invalid"）。
      if (__pairs.length % 2 !== 0) throw new codes.ERR_INVALID_ARG_VALUE("headers", obj);
      // node 口径（write-head-after-set-header 套件真机实测）：扁平数组内同键
      // 对逐行保留（['a','1','a','2'] 即两行）；首对覆写先前 setHeader，同键后对
      // 累积（首触覆写、再触累积）。
      const __touched = new Set();
      for (let __i = 0; __i < __pairs.length; __i += 2) {
        const __k = String(__pairs[__i]);
        const __v = __pairs[__i + 1];
        if (!__TOKEN_RE.test(__k)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", __k);
        if (__v === undefined) throw new codes.ERR_HTTP_INVALID_HEADER_VALUE("undefined", __k);
        const __lk = __k.toLowerCase();
        if (this._removedHeader !== undefined) delete this._removedHeader[__lk];
        const __vals = Array.isArray(__v) ? __v : [__v];
        for (const __e of __vals) __checkOutboundHeaderValue(this.__validation, __e, __k);
        if (!__touched.has(__lk)) {
          __touched.add(__lk);
          this.__headers[__lk] = Array.isArray(__v) ? [...__v] : __v;
        } else {
          const __cur = this.__headers[__lk];
          if (Array.isArray(__cur)) for (const __e of __vals) __cur.push(__e);
          else this.__headers[__lk] = [__cur, ...__vals];
        }
        // node 口径（write-head 套件真机实测）：writeHead 覆写拼写（setHeader
        // 'test' 后 writeHead {Test:'2'}，wire 为 'Test'，非首写优先——首写优先
        // 仅 setHeader 之间）。
        this.__headerNames[__lk] = __k;
      }
    }
    this.headersSent = true;
    if (obj !== undefined && __objArr === null) {
      for (const k of Object.keys(obj)) {
        if (!__TOKEN_RE.test(String(k))) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(k));
        const __v = obj[k];
        const __lk = String(k).toLowerCase();
        if (this._removedHeader !== undefined) delete this._removedHeader[__lk];
        if (Array.isArray(__v)) {
          const __arr = [];
          for (const __e of __v) {
            __checkOutboundHeaderValue(this.__validation, __e, String(k));
            __arr.push(__e);
          }
          this.__headers[__lk] = __arr;
        } else {
          __checkOutboundHeaderValue(this.__validation, __v, String(k));
          this.__headers[__lk] = __v;
        }
        this.__headerNames[__lk] = String(k);
      }
    }
    // node _storeHeader 口径（de-chunked-trailer 套件）：非 chunked 传输带
    // Trailer 头即同步抛 ERR_HTTP_TRAILER_INVALID（Trailer 只能随 chunked 走；
    // 无 CL/TE 时自动 chunked 故合法，不抛）。
    if (this.__headers["trailer"] !== undefined) {
      const __te = this.__headers["transfer-encoding"];
      const __teChunked = __te !== undefined && /(?:^|\W)chunked/i.test(String(__te));
      const __hasCL = this.__headers["content-length"] !== undefined;
      const __autoChunked = !__hasCL && __te === undefined &&
        this.__uced !== false && !this.__noBody && !this.__headOnly;
      if (!__teChunked && !__autoChunked) {
        const e = new Error("Trailers are invalid with this transfer encoding");
        e.code = "ERR_HTTP_TRAILER_INVALID";
        throw e;
      }
    }
    } catch (__whErr) {
      __rollback(__whErr);
    }
    this.headersSent = true;
    // 头已存：随后的 end(data) 不再走 CL 快路径（真机 chunked 口径）。
    this.__headStored = true;
    return this;
  }
  // node writeContinue：headersSent 前一次性发 100 Continue 中间响应；
  // 回调落盘后触发（write-callbacks 套件；旧实现吞回调即挂死）。
  writeContinue(cb) {
    if (this.__continueSent || this.headersSent || this.__headSent) {
      if (typeof cb === "function") queueMicrotask(cb);
      return;
    }
    this.__continueSent = true;
    if (this.__sock !== null) {
      try { this.__sockWrite(new TextEncoder().encode("HTTP/1.1 100 Continue\r\n\r\n")); } catch { /* gone */ }
    }
    if (typeof cb === "function") queueMicrotask(cb);
  }
  // node writeProcessing()：writeInformation(102) 速记。
  writeProcessing() {
    return this.writeInformation(102);
  }
  // node writeEarlyHints(hints)：103 Early Hints（early-hints 套件）。
  // link 缺席/空结果即静默跳过（mustNotCall information 形）；数组以 ", "
  // 连接；link 格式逐字真机正则；其余键原样透传（非法字符/名走既有头校验门）。
  writeEarlyHints(hints) {
    if (hints === null || typeof hints !== "object" || Array.isArray(hints)) {
      throw new codes.ERR_INVALID_ARG_TYPE("hints", "object", hints);
    }
    const link = hints.link;
    if (link === null || link === undefined) return;
    const __linkRe = /^(?:<[^>\r\n]*>)(?:\s*;\s*[^;"\s]+(?:=(")?[^;"\s]*\1)?)*$/;
    const __checkLink = (v) => {
      if (typeof v !== "string" || !__linkRe.test(v)) {
        throw new codes.ERR_INVALID_ARG_VALUE("hints", v);
      }
    };
    let joined;
    if (typeof link === "string") {
      __checkLink(link);
      joined = link;
    } else if (Array.isArray(link)) {
      if (link.length === 0) return;
      for (const e of link) __checkLink(e);
      joined = link.join(", ");
    } else {
      throw new codes.ERR_INVALID_ARG_VALUE("hints", link);
    }
    if (joined.length === 0) return;
    const headers = Object.create(null);
    headers.Link = joined;
    for (const k of Object.keys(hints)) {
      if (k !== "link") headers[k] = hints[k];
    }
    return this.writeInformation(103, headers);
  }
  // node writeInformation(statusCode[, headers])：1xx 中间响应直发（不占终态
  // 头、不置 headersSent；information/early-hints 套件；客户端侧 'information'
  // 事件既有）。非法码抛 ERR_HTTP_INVALID_STATUS_CODE（与 writeHead 同门）。
  // node _http_outgoing OutgoingMessage.setTimeout 口径（set-timeout-server
  // 套件 res 形）：cb 记 'timeout' 监听；socket 在场即武装 + res 侧桥带
  // socket 实参（同 socketOnTimeout）；无 socket 记 timeoutCb/timeout
  //（node _onTimeout 延迟武装形，本仓无 socket 场景未跑计时）。
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.on("timeout", callback);
    if (this.socket === null || this.socket === undefined) {
      this.timeoutCb = this._onTimeout;
      this.timeout = msecs;
      return this;
    }
    if (!this.__outTimeoutFwd) {
      this.__outTimeoutFwd = true;
      const __fwd = () => {
        if (this.writableEnded || this.destroyed) return;
        this.emit("timeout", this.socket);
      };
      try { this.socket.on("timeout", __fwd); } catch { /* gone */ }
      this.once("close", () => {
        try { this.socket.removeListener("timeout", __fwd); } catch { /* gone */ }
      });
    }
    this.socket.setTimeout(msecs);
    return this;
  }
  writeInformation(info, headers) {
    let status;
    let hdrs;
    if (info !== null && typeof info === "object") {
      status = info.statusCode;
      hdrs = info.headers;
    } else {
      status = info;
      hdrs = headers;
    }
    // node _http_server.js 317 行逐字：发头门（ERR_HTTP_HEADERS_SENT）→
    // validateInteger(100,199)（非数 ERR_INVALID_ARG_TYPE / 越界
    // ERR_OUT_OF_RANGE）→ 101 拒收（ERR_HTTP_INVALID_STATUS_CODE——101 协议
    // 切换非信息响应；write-information 套件错误块逐项）。
    if (this.headersSent || this._header) {
      throw new codes.ERR_HTTP_HEADERS_SENT("write");
    }
    if (typeof status !== "number") {
      throw new codes.ERR_INVALID_ARG_TYPE("statusCode", "number", status);
    }
    if (!Number.isInteger(status) || status < 100 || status > 199) {
      throw new codes.ERR_OUT_OF_RANGE("statusCode", ">= 100 && <= 199", status);
    }
    if (status === 101) {
      throw new codes.ERR_HTTP_INVALID_STATUS_CODE(status);
    }
    if (this.__sock === null || this.__sock.destroyed) return false;
    const reason = STATUS_CODES[status] ?? "";
    const lines = [`HTTP/1.1 ${status} ${reason}`.trimEnd()];
    // 原拼写上网（rawHeaders 回显；information 套件断言 'Foo' 非 'foo'）。
    // 三形（node _http_server.js 340-360 行逐字）：对形 [[k,v],...] / 扁平形
    // [k1,v1,...]（奇长 ERR_INVALID_ARG_VALUE）/ 对象形。
    if (Array.isArray(hdrs)) {
      const __pairs = hdrs.length > 0 && Array.isArray(hdrs[0])
        ? hdrs
        : (hdrs.length % 2 !== 0
          ? (() => { throw new codes.ERR_INVALID_ARG_VALUE("headers", hdrs); })()
          : Array.from({ length: hdrs.length / 2 }, (_, i) => [hdrs[i * 2], hdrs[i * 2 + 1]]));
      for (const [k, v] of __pairs) {
        const __names = Object.create(null);
        const __lv = __lowerHeaders({ [String(k)]: v }, this.__validation, __names);
        const __lk = Object.keys(__lv)[0];
        lines.push(`${__names[__lk] ?? __lk}: ${__lv[__lk]}`);
      }
    } else {
      const __names = Object.create(null);
      for (const [k, v] of Object.entries(__lowerHeaders(hdrs ?? {}, this.__validation, __names))) {
        lines.push(`${__names[k] ?? k}: ${v}`);
      }
    }
    try {
      this.__sockWrite(__latin1Bytes(lines.join("\r\n") + "\r\n\r\n"));
    } catch { return false; /* gone */ }
    return true;
  }
  // node setHeaders：收 Headers 实例或 Map（setheaders 套件真机实测双形；
  // 其余（数组/对象/null/undefined/字符串/数字）一律 ERR_INVALID_ARG_TYPE；
  // 品牌按名判定，跨域稳定）。
  // 发头后即 ERR_HTTP_HEADERS_SENT（首块）。
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
  // node addTrailers：分块响应终结块尾随头（原拼写输出；真机 rawTrailers 口径）。
  // 值数组按元素展开多行（multiple-headers 套件；拼写取用户原文）。
  // trailers/trailersDistinct/rawTrailers 即时落账（multiple-headers 套件
  // req 'end' 内断言 wire 回环值）。
  addTrailers(trailers) {
    if (trailers === null || trailers === undefined) return this;
    // node 口径：对形 `[[k,v],...]` 与对象形双收（raw-headers 套件）。
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
      if (!__TOKEN_RE.test(String(k))) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(k));
      const __v = trailers[k];
      const __vals = Array.isArray(__v) ? __v : [__v];
      // node 口径 uniqueHeaders：名单内 trailer 行 wire 合并单行 '; '
      //（multiple-headers 套件；与 ClientRequest 侧同口径）。
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
  // node 口径（write-after-end 套件 + head-throw 套件真机实测）：
  // end 后再写不进基类（基类置 errored 会压住在途 _final 致 chunk 终结块丢失，
  // 同步/异步双探针实证）——自发 error + 回 false；end 优先于拒写旗（204 先
  // end 后写仍走 WRITE_AFTER_END，非同步抛）。
  write(chunk, encoding, cb) {
    if (!this.destroyed && (this.__userEnded || this.writableEnded)) {
      if (typeof encoding === "function") { cb = encoding; encoding = null; }
      const __cb = typeof cb === "function" ? cb : null;
      const er = new codes.ERR_STREAM_WRITE_AFTER_END();
      queueMicrotask(() => { if (__cb) __cb(er); if (!this.destroyed) this.emit("error", er); });
      return false;
    }
    // node 口径（head-throw 套件真机实测）：拒写旗下无体响应的 write 同步抛
    //（含空串；校验在 write 包装层，不进 _write——流内抛会毒化 writing 态，
    // 后续 end 永挂）。
    if (this.__rejectBody && this.__isNoBodyStatus()) {
      throw new codes.ERR_HTTP_BODY_NOT_ALLOWED();
    }
    if (this.__sockGone || this.__sock === null || (this.__sock !== null && this.__sock.destroyed)) {
      // socket-null 有两种：入列停靠（__queued，写继续走流机构→_write park）与
      // 独立构造（未入列，旧口径回 false——incoming-pipelined 套件管线续行写）。
      if (this.__sock === null && this.__queued === true) {
        if (chunk !== undefined && chunk !== null) {
          try { this.__countOut(chunk instanceof Uint8Array ? chunk : __toU8(String(chunk))); } catch { /* 计数永不阻写 */ }
        }
        return super.write(chunk, encoding, cb);
      }
      return false;
    }
    // node 口径 strictContentLength（content-length-mismatch 套件）：显式 CL +
    // 严格旗即超写同步抛 ERR_HTTP_CONTENT_LENGTH_MISMATCH（校验在记账/落盘前，
    // 抛错不污染计数）。
    if (chunk !== undefined && chunk !== null) {
      const __cl = this.__strictCL();
      if (__cl !== null) {
        const __len = chunk instanceof Uint8Array ? chunk.length : __toU8(String(chunk)).length;
        if ((this.__clWritten ?? 0) + __len > __cl) {
          throw new codes.ERR_HTTP_CONTENT_LENGTH_MISMATCH((this.__clWritten ?? 0) + __len, __cl);
        }
      }
    }
    // 写时记账（writableLength 精确字节，见 __countOut）：write 包装层同步计
    // （流机构异步派发 _write，_write 时机计数会漏同步读——outgoing-properties
    // 套件连写两行后同步读）；end 块由 end 包装层计，_write 内不计（防双计）。
    // socket bytesWritten 同步预测（byteswritten 套件）：__wlen 增量同步到
    // socket pending（落盘 __sockWrite 核销、_final 兜底清零）。
    // 严格 CL 体计数同行累加（上已校验不超，此处只落账）。
    if (chunk !== undefined && chunk !== null) {
      try {
        const __u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk));
        if (this.__strictCL() !== null) this.__clWritten = (this.__clWritten ?? 0) + __u8.length;
        const __before = this.__wlen ?? 0;
        this.__countOut(__u8);
        const __d = (this.__wlen ?? 0) - __before;
        if (__d > 0 && this.__sock !== null && typeof this.__sock.__bwAdd === "function") {
          this.__sock.__bwAdd(__d);
        }
      } catch { /* 计数永不阻写 */ }
    }
    return super.write(chunk, encoding, cb);
  }
  // node 口径 strictContentLength 有效 CL（content-length-mismatch 套件）：
  // 严格旗 + 用户显式 CL（自动补的不算，__headerNames 有名才算）→ 数值，
  // 否则 null（不 enforcement）。
  __strictCL() {
    if (this.strictContentLength !== true) return null;
    if (this.__headerNames === undefined || this.__headerNames["content-length"] === undefined) return null;
    const __n = Number(this.__headers["content-length"]);
    return Number.isFinite(__n) && __n >= 0 ? __n : null;
  }
  // node 口径（head-throw 套件）：1xx/204/304/HEAD 为无体响应（writeHead 置
  // __noBody/方法置 __headOnly；writeHead 前按 statusCode 活读）。
  __isNoBodyStatus() {
    const __sc = this.__storedStatus !== undefined ? this.__storedStatus : this.statusCode;
    return this.__headOnly || (__sc >= 100 && __sc <= 199) || __sc === 204 || __sc === 304;
  }
  end(chunk, encoding, cb) {
    // Node OutgoingMessage.end 口径（end-multiple 套件）：finished 后 end(chunk)
    // 走 onError（cb + 'error'，不碰基类错误通道——基类会置 errored 毒化在途
    // 首个 end 的 finish）；finished 后裸 end 回 ALREADY_FINISHED（同步）；
    // ending 中带块同 onError。皆不经 super.end。
    if (typeof chunk === "function") { cb = chunk; chunk = null; encoding = null; }
    else if (typeof encoding === "function") { cb = encoding; encoding = null; }
    const __hasChunk = chunk !== undefined && chunk !== null;
    const __cb = typeof cb === "function" ? cb : null;
    // node 口径（head-throw 套件真机实测）：拒写旗下 end(chunk) 同步抛（write 同）。
    if (__hasChunk && this.__rejectBody && this.__isNoBodyStatus()) {
      throw new codes.ERR_HTTP_BODY_NOT_ALLOWED();
    }
    if (this.writableFinished) {
      if (__hasChunk) {
        if (this.destroyed) return this;
        const er = new codes.ERR_STREAM_WRITE_AFTER_END();
        queueMicrotask(() => { if (__cb) __cb(er); if (!this.destroyed) this.emit("error", er); });
      } else if (__cb) {
        __cb(new codes.ERR_STREAM_ALREADY_FINISHED("end"));
      }
      return this;
    }
    if (this.__userEnded) {
      if (!__hasChunk) return super.end(null, null, cb);
      if (this.destroyed) return this;
      const er = new codes.ERR_STREAM_WRITE_AFTER_END();
      queueMicrotask(() => { if (__cb) __cb(er); if (!this.destroyed) this.emit("error", er); });
      return this;
    }
    // CL 快路径判据（真机）：end 的数据块存在性——write 后裸 end() 不走快路径
    //（真机 chunked + 终结块口径）。
    // node 口径：end() 返回后头即算发出（multiple-headers 套件 end 后
    // appendHeader 即 HEADERS_SENT；_final 异步，旗必须同步立）。
    // node end() 原文：end 即全开——socket corked 置 1 再 uncork（强制归零）
    // + 消息级 corked 置 1 再 uncork（滞留尾随排空；outgoing-end-cork 套件
    // end 后 writableCorked===0；response-cork 套件 end 后双侧 corked 恒等）。
    if (this._writableState && this._writableState.corked > 0) {
      this._writableState.corked = 1;
      super.uncork();
    }
    if (this.__sock !== null && typeof this.__sock.uncork === "function" &&
        (this.__sock.__corkCnt ?? 0) > 0) {
      this.__sock.__corkCnt = 1;
      this.__sock.uncork();
    }
    this.headersSent = true;
    this.__endHadData = chunk !== undefined && chunk !== null && typeof chunk !== "function";
    this.__userEnded = true;
    this.finished = true;
    // node 口径 strictContentLength（content-length-mismatch 套件）：end 块超
    // 即同步抛；收尾不足（累计 < CL）同样同步抛。校验在落盘前。
    if (this.__endHadData || this.__strictCL() !== null) {
      const __cl = this.__strictCL();
      if (__cl !== null) {
        const __add = this.__endHadData
          ? (chunk instanceof Uint8Array ? chunk.length : __toU8(String(chunk)).length) : 0;
        const __total = (this.__clWritten ?? 0) + __add;
        if (__total !== __cl) {
          this.__endHadData = false;
          this.__userEnded = false;
          throw new codes.ERR_HTTP_CONTENT_LENGTH_MISMATCH(__total, __cl);
        }
      }
    }
    try {
      const __r = super.end(chunk, encoding, cb);
      // end 块同步记账（流 end 经内部 _write 直调，不走 write 包装层，此处补计；
      // socket pending 同上；严格 CL 体计数同步累加）。
      if (this.__endHadData) {
        try {
          const __u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk));
          if (this.__strictCL() !== null) this.__clWritten = (this.__clWritten ?? 0) + __u8.length;
          const __before = this.__wlen ?? 0;
          this.__countOut(__u8);
          const __d = (this.__wlen ?? 0) - __before;
          if (__d > 0 && this.__sock !== null && typeof this.__sock.__bwAdd === "function") {
            this.__sock.__bwAdd(__d);
          }
        } catch { /* 计数永不阻收尾 */ }
      }
      return __r;
    } catch (e) {
      // 基类校验抛（如数组 chunk）不得毒化旗位，否则后续合法 end 永不到
      // （end-types 套件 hang 根因）。
      this.__endHadData = false;
      this.__userEnded = false;
      throw e;
    }
  }
  // node lib/_http_outgoing.js cork 口径（response-cork/response-drain-cork/
  // outgoing-end-cork 三套件逐项对拍）：res.cork() = 消息级计数 + socket.cork()
  // 镜像（writableCorked 双侧恒等；node 消息级 kCorked 以流机构 cork 承载——
  // 本仓 res 即 Writable，机构 cork 滞留字节于 res 缓冲，corked 期间 _write
  // 不被调、socket.write 不被调；node 由 socket 机构/kChunkedBuffer 持，
  // 可观测等价：滞留不落盘、write() 返回值走 HWM、drain 随排空发射）。
  // 偏差记档：node uncork 尾flush 把滞留块**合并为一个 chunk**（kChunkedBuffer
  // 总长一帧），本仓机构排空逐块成帧——字节流恒等，chunk 边界不同，套件未点名。
  // node 口径：ServerResponse 品牌位（instanceof OutgoingMessage 身份语义；
  // 真机为真继承——本仓结构偏离记档，见 framing_outgoing hasInstance）。
  __omBrand = true;
  cork() {
    super.cork();
    if (this.__sock !== null && typeof this.__sock.cork === "function") this.__sock.cork();
    return this;
  }
  uncork() {
    super.uncork();
    if (this.__sock !== null && typeof this.__sock.uncork === "function") this.__sock.uncork();
    return this;
  }
  // drain 门控（drain-writable-length 套件）：socket 落盘未完（__wlen>0）时
  // 流机构的 'drain' 递延至清零（早发即 writableLength 非零）；销毁即弃。
  // 无积压即直通（常规路径零行为差）。
  emit(ev, ...args) {
    if (ev === "drain" && (this.__wlen ?? 0) > 0) {
      if (!this.destroyed) this.__parkedDrain = true;
      return false;
    }
    return super.emit(ev, ...args);
  }
  // CL/TE 被删掉时自动帧全停（remove-header 套件；__headBytes 帧决策同口径）。
  __frameSuppressed() {
    return this._removedHeader !== undefined &&
      !!(this._removedHeader["content-length"] || this._removedHeader["transfer-encoding"]);
  }
  // 立即发头（Node flushHeaders：body 可经 chunked 帧，end 后补终结块）。
  flushHeaders() {
    if (this.__headSent || this.__noBody || this.__headOnly) return;
    if (this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined && this.__uced && !this.__frameSuppressed()) {
      this.__chunked = true;
    }
    this.__sendHead();
    if (this.__buf1 !== null) {
      const b = this.__buf1;
      this.__buf1 = null;
      this.__frame(b);
    }
  }
  // 独立构造的 res 后挂 socket（standalone 套件）；双挂即 ERR_HTTP_SOCKET_ASSIGNED。
  // 管线轮转同样经此（排空停靠写 + 递补终结，见 __feed/__onDone）。
  assignSocket(sock) {
    if (this.__sockAssigned) {
      const e = new Error("Socket is already assigned");
      e.code = "ERR_HTTP_SOCKET_ASSIGNED";
      throw e;
    }
    this.__sockAssigned = true;
    this.__sock = sock;
    this.socket = sock;
    this.connection = sock;
    this.__queued = false;
    // node 口径：assign 即发 'socket'（setTimeout 无 sock 等待形与监听侧靠它）。
    try { this.emit("socket", sock); } catch { /* 监听抛错不阻排空 */ }
    const __q = this.__parked ?? [];
    this.__parked = [];
    for (const [__b, __c] of __q) {
      try { this._write(__b, null, __c); } catch { try { __c(); } catch { /* gone */ } }
    }
    if (this.__finalParked !== null && this.__finalParked !== undefined) {
      const __f = this.__finalParked;
      this.__finalParked = null;
      try { this._final(__f); } catch { try { __f(); } catch { /* gone */ } }
    }
  }
