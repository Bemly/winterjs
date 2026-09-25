
// ── 服务端流对象（node ServerHttp2Stream：真 Duplex）────────────────────────
class Http2ServerStream extends Duplex {
  constructor(session, connId, id, options) {
    super(options ?? {});
    this.__session = session;
    this.__server = session.__server;
    this.__conn = connId;
    this.id = id;
    this.readable = true;
    this.writable = true;
    this.__destroyed = false;
    this.__closed = false;
    this.__aborted = false;
    this.__headersSent = false;
    this.__waitForTrailers = false;
    this.__trailersSent = false;
    this.__wantTrailersFired = false;
    this.__sentHeaders = null;
    this.__sentPseudoHeaders = null;
    this.__sentInfoHeaders = [];
    this.__trailers = null;
    this.sendDate = true;
    this.endAfterHeaders = false;
    this.__reqEnded = false;
    this.__rstSent = false;
    // 实例数据属性遮蔽 Duplex 原型只读 getter（socket.set 套件直写直读）
    Object.defineProperty(this, "readable", { value: true, writable: true, configurable: true });
    Object.defineProperty(this, "writable", { value: true, writable: true, configurable: true });
    session.__registerStream(this);
  }
  get session() { return this.__session; }
  get aborted() { return this.__aborted; }
  get closed() { return this.__closed; }
  get destroyed() { return this.__destroyed; }
  get headersSent() { return this.__headersSent; }
  get _header() { return this.__headersSent; }
  get headersSentRaw() { return this.__headersSent; }
  get sentHeaders() { return this.__sentHeaders; }
  get sentPseudoHeaders() { return this.__sentPseudoHeaders; }
  get sentInfoHeaders() { return this.__sentInfoHeaders; }
  get sentTrailers() { return this.__trailers; }
  get pushAllowed() { return false; }
  get bufferSize() { return this.writableLength; }
  get state() {
    const localClose = this.__closed || this.__trailersSent ? 1 : 0;
    const remoteClose = this.__reqEnded ? 1 : 0;
    return {
      state: this.__destroyed ? 7 : (localClose && remoteClose ? 7 : 2),
      weight: 16,
      sumDependencyWeight: 0,
      localClose,
      remoteClose,
      localWindowSize: 65535,
    };
  }
  // 伪头合成由 req 承担；stream 可读侧 = 请求体
  _read() {}
  __feedBody(b64chunk) {
    const u8 = __b64dec(b64chunk ?? "");
    if (u8.length > 0) this.push(Buffer.from(u8));
    // 流式体（P1）：compat req 从 stream 拉取——新块到达即唤它再拉（整收时代块在
    // req._read 前已齐，无需此步）。
    this.__pokeReq();
  }
  __pokeReq() {
    const r = this.__req;
    if (r && !r.__reqDone && !r.destroyed) queueMicrotask(() => r._read());
  }
  __endReq(trailersJson) {
    if (this.__reqEnded) return;
    this.__reqEnded = true;
    const t = JSON.parse(trailersJson ?? "[]");
    // 流式体：compat req 的 trailers/rawTrailers 在体尾才知道（构造时为空）。
    if (t.length > 0 && this.__req) {
      this.__req.trailers = __pairsToObj(t);
      this.__req.rawTrailers = t.flat();
    }
    // node 序：trailers 帧到即发，与读取无关。流动态下体块的 data 事件尚待派发，
    // 为保 data… → trailers 序挂在 end 前（prepend）；非流动（无人读）即刻发——
    // 否则等不到 end，`on('trailers', () => stream.end())` 形永挂（trailers 套件）。
    if (t.length > 0) {
      const fire = () => this.emit("trailers", __pairsToObj(t));
      if (this.readableFlowing === true) this.prependOnceListener("end", fire);
      else queueMicrotask(fire);
    }
    this.push(null);
    this.__pokeReq();
    this.__maybeAutoClose();
  }
  __onAborted() {
    if (this.__aborted) return;
    this.__aborted = true;
    this.emit("aborted");
    this.destroy();
  }
  // node onStreamClose：close 事件由 destroy 机制单发（§事件单发）
  __abort() {
    if (this.__closed || this.__destroyed) return;
    this.__abortDownstream();
    this.destroy();
  }
  __abortDownstream() {
    if (this.__aborted) return;
    this.__aborted = true;
    if (this.__req) this.__req.__onAborted();
    if (this.__res && !this.__res.destroyed) this.__res.__destroySilent();
  }
  __implicitRespond() {
    if (this.__headersSent || this.__destroyed) return;
    this.respond({});
  }
  respond(headers = {}, options = {}) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_STREAM");
    if (this.__headersSent) throw __code("ERR_HTTP2_HEADERS_SENT");
    if (headers === null || typeof headers !== "object") throw __code("ERR_HTTP2_HEADERS_OBJECT");
    if (options === null || typeof options !== "object") {
      throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
    }
    // :status 校验（数字合法才入线；非数字按 node 线上默认 200 处理）
    let status = 200;
    if (headers[":status"] !== undefined) {
      const s = +headers[":status"];
      if (typeof headers[":status"] === "number" &&
          (!Number.isInteger(s) || s < 100 || s > 599)) {
        throw __code("ERR_HTTP2_STATUS_INVALID", headers[":status"]);
      }
      if (Number.isInteger(s) && s >= 100 && s <= 599) status = s;
    }
    __validateH2Headers({ ...headers, ":status": status }, [":status"]);
    this.__waitForTrailers = !!options.waitForTrailers;
    // node：HEAD 请求与 204/205/304 应答自动 END_STREAM（head-request/endafter
    // 套件口径；END_STREAM 随头出线，后续 write 报 ERR_STREAM_WRITE_AFTER_END）
    const endStreamAuto = this.__headRequest === true ||
      status === 204 || status === 205 || status === 304;
    if (endStreamAuto) this.endAfterHeaders = true;
    const entries = [];
    const sent = { __proto__: null };
    const sentPseudo = { __proto__: null };
    sentPseudo[":status"] = String(status);
    if (this.sendDate && headers["date"] === undefined && headers["Date"] === undefined) {
      entries.push(["date", new Date().toUTCString()]);
      sent["date"] = new Date().toUTCString();
    }
    for (const [k, v] of Object.entries(headers)) {
      if (k.startsWith(":")) continue;
      __validateHeaderName(k);
      const w = __headerToWire(v);
      if (Array.isArray(w)) {
        for (const one of w) { __validateHeaderValue(k, one); entries.push([k.toLowerCase(), one]); }
        sent[k.toLowerCase()] = w;
      } else {
        __validateHeaderValue(k, w);
        entries.push([k.toLowerCase(), w]);
        sent[k.toLowerCase()] = w;
      }
    }
    this.__headersSent = true;
    this.__sentHeaders = sent;
    this.__sentPseudoHeaders = sentPseudo;
    __wjs_h2_respond(this.__conn, this.id, status, JSON.stringify(entries));
    if (options.endStream || endStreamAuto) {
      this.endAfterHeaders = true;
      this.__finishWritable();
    }
    return undefined;
  }
  // node：fd 可为 number 或 FileHandle（对象须带数值 fd）；错误消息
  // "The \"fd\" argument must be of type number or an instance of FileHandle."
  respondWithFD(fd, headers, options) {
    if (typeof fd !== "number" &&
        (fd === null || typeof fd !== "object" || typeof fd.fd !== "number")) {
      throw __code("ERR_INVALID_ARG_TYPE", "fd", "number or an instance of FileHandle", fd);
    }
    return this.respondWithFile(typeof fd === "object" ? fd.fd : fd, headers, options);
  }
  respondWithFile(filename, headers = {}, options = {}) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_STREAM");
    if (this.__headersSent) throw __code("ERR_HTTP2_HEADERS_SENT");
    if (typeof options !== "object" || options === null) {
      throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
    }
    if (options.statCheck !== undefined && typeof options.statCheck !== "function") {
      throw __code("ERR_INVALID_ARG_VALUE", "options.statCheck", options.statCheck);
    }
    if (options.onError !== undefined && typeof options.onError !== "function") {
      throw __code("ERR_INVALID_ARG_VALUE", "options.onError", options.onError);
    }
    if (options.offset !== undefined && typeof options.offset !== "number") {
      throw __code("ERR_INVALID_ARG_VALUE", "options.offset", options.offset);
    }
    if (options.length !== undefined && typeof options.length !== "number") {
      throw __code("ERR_INVALID_ARG_VALUE", "options.length", options.length);
    }
    // 204/205/304 禁 payload
    if (headers[":status"] !== undefined) {
      const s = +headers[":status"];
      if (s === 204 || s === 205 || s === 304) {
        throw __code("ERR_HTTP2_PAYLOAD_FORBIDDEN", s);
      }
    }
    const isFd = typeof filename === "number";
    let fd = null;
    let stat;
    try {
      fd = isFd ? filename : fs.openSync(filename, "r");
      stat = fs.fstatSync(fd);
    } catch (err) {
      if (!isFd && fd !== null) { try { fs.close(fd, () => {}); } catch {} }
      if (typeof options.onError === "function") {
        options.onError(err);
        return;
      }
      // node：stat 失败且无 onError → 流 RST(INTERNAL_ERROR) + 'error'
      //（fd-invalid 套件：stream/req 双侧 ERR_HTTP2_STREAM_ERROR）
      this.__rstInternalError();
      return;
    }
    try {
      const h = { ...headers };
      // node statCheck 第三参为 {offset, length} 原样（fd-range 套件断言）
      const statCheckOpts = { offset: options.offset, length: options.length };
      if (typeof options.statCheck === "function") {
        if (options.statCheck.call(this, stat, h, statCheckOpts) === false) {
          if (!isFd) { try { fs.close(fd, () => {}); } catch {} }
          return;
        }
      }
      if (h[":status"] !== undefined && [204, 205, 304].includes(+h[":status"])) {
        throw __code("ERR_HTTP2_PAYLOAD_FORBIDDEN", +h[":status"]);
      }
      let offset = options.offset ?? 0;
      let length = options.length ?? (stat.size - offset);
      if (length < 0) length = stat.size - offset;
      if (length < 0) length = 0;
      const shouldSendBody = h[":status"] === undefined || !![200, 201, 202, 203, 206].includes(+h[":status"]) ||
        (+h[":status"] >= 300 && ![204, 205, 304].includes(+h[":status"]));
      this.respond(h, {});
      if (shouldSendBody && length > 0) {
        const CH = 0x8000;
        let pos = offset;
        while (pos < offset + length) {
          const n = Math.min(CH, offset + length - pos);
          const buf = Buffer.alloc(n);
          const r = fs.readSync(fd, buf, 0, n, pos);
          if (r <= 0) break;
          if (r < n) {
            __wjs_h2_data(this.__conn, this.id, __b64enc(buf.subarray(0, r)));
            break;
          }
          __wjs_h2_data(this.__conn, this.id, __b64enc(buf));
          pos += r;
        }
      }
      if (!isFd) { try { fs.close(fd, () => {}); } catch {} }
      if (!this.writableEnded) this.__finishWritable();
    } catch (err) {
      if (!isFd) { try { fs.close(fd, () => {}); } catch {} }
      if (typeof options.onError === "function") {
        options.onError(err);
        return;
      }
      // node：校验/读失败无 onError → 错误落流 'error'（fd-leak 套件），
      // 并 RST 对端（_destroy 统一发送，否则对端流永等响应）
      this.destroy(err);
    }
  }
  pushStream(headers, options, cb) {
    if (typeof options === "function") { cb = options; options = {}; }
    if (typeof cb !== "function") {
      throw __code("ERR_INVALID_ARG_TYPE", "callback", "Function", cb);
    }
    if (this.__session.type !== 0 || this.__isPush) {
      process.nextTick(() => cb(__code("ERR_HTTP2_NESTED_PUSH")));
      return;
    }
    // 底座无 PUSH_PROMISE（hyper；偏差记档）——回调路径收到 PUSH_DISABLED
    process.nextTick(() => cb(__code("ERR_HTTP2_PUSH_DISABLED")));
    return;
  }
  additionalHeaders(headers) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_STREAM");
    if (!this.__headersSent) throw __code("ERR_HTTP2_HEADERS_SENT");
    __validateH2Headers(headers);
    const status = headers[":status"];
    if (status !== undefined) {
      const s = +status;
      if (!Number.isInteger(s) || s < 200 || s > 599) {
        throw __code("ERR_HTTP2_STATUS_INVALID", status);
      }
    }
    this.__sentInfoHeaders.push(headers);
    return this;
  }
  priority(options) {
    if (options === null || typeof options !== "object") {
      throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
    }
    if (options.weight !== undefined) {
      const w = options.weight;
      if (typeof w !== "number" || !Number.isInteger(w) || w < 1 || w > 256) {
        throw __code("ERR_OUT_OF_RANGE", "options.weight", w);
      }
    }
    if (options.parent !== undefined && options.parent !== 0) {
      const p = options.parent;
      if (typeof p !== "number" || !Number.isInteger(p) || p < 1 || p > 2147483647) {
        throw __code("ERR_OUT_OF_RANGE", "options.parent", p);
      }
    }
    return this;
  }
  sendTrailers(trailers) {
    if (trailers === null || typeof trailers !== "object") {
      throw __code("ERR_INVALID_ARG_TYPE", "trailers", "object", trailers);
    }
    // node core.js sendTrailers 门序：已毁/已关 → INVALID_STREAM；已发 → ALREADY_SENT；
    // 未到 wantTrailers（含未开 waitForTrailers）→ NOT_READY（真机逐项实测）。
    if (this.destroyed || this.__closed) throw __code("ERR_HTTP2_INVALID_STREAM");
    if (this.__trailersSent) throw __code("ERR_HTTP2_TRAILERS_ALREADY_SENT");
    if (!this.__waitForTrailers || !this.__wantTrailersFired) {
      throw __code("ERR_HTTP2_TRAILERS_NOT_READY");
    }
    __validateH2Headers(trailers, []);
    const t = [];
    for (const [k, v] of Object.entries(trailers)) {
      const w = __headerToWire(v);
      if (Array.isArray(w)) for (const one of w) t.push([k, one]);
      else t.push([k, w]);
    }
    this.__trailersSent = true;
    this.__trailers = { ...trailers };
    __wjs_h2_end(this.__conn, this.id, JSON.stringify(t));
    const cb = this.__finalCb;
    if (typeof cb === "function") queueMicrotask(cb);
    return this;
  }
  close(code = 0, cb) {
    if (typeof code === "function") { cb = code; code = 0; }
    if (typeof cb === "function") this.once("close", cb);
    if (this.__closed || this.__destroyed) return this;
    if (!this.__trailersSent) {
      __wjs_h2_reset(this.__conn, this.id, code);
      this.__trailersSent = true;
    }
    this.destroy();
    return this;
  }
  destroy(err, code, cb) {
    if (typeof err === "number") { cb = code; code = err; err = undefined; }
    if (typeof code === "function") { cb = code; code = 0; }
    if (typeof cb === "function") this.once("close", cb);
    if (this.__destroyed) return this;
    if (err) this.__destroyErr = err;
    else if (!this.__destroyErr && (code ?? 0) !== 0) this.__destroyErr = undefined;
    super.destroy(err);
    return this;
  }
  __finishWritable() {
    // respond({endStream:true}) / respondWithFile 收尾路径；
    // __trailersSent 由 _final 在 __wjs_h2_end 发出后置位（预置会跳过
    // END_STREAM → 客户端 'end' 永不到，与 res.end 预置同款挂死）
    super.end();
  }
  _write(chunk, encoding, cb) {
    if (this.__destroyed) {
      cb(__code("ERR_HTTP2_STREAM_CLOSED"));
      return;
    }
    this.__implicitRespond();
    const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk), "write");
    __wjs_h2_data(this.__conn, this.id, __b64enc(u8));
    queueMicrotask(cb);
  }
  _final(cb) {
    if (this.__destroyed) { cb(); return; }
    this.__implicitRespond();
    if (this.__waitForTrailers && !this.__trailersSent && !this.endAfterHeaders) {
      // wantTrailers 窗口：用户 sendTrailers 后回填收尾
      this.__finalCb = cb;
      queueMicrotask(() => {
        if (!this.__destroyed && !this.__trailersSent) {
          this.__wantTrailersFired = true;
          this.emit("wantTrailers");
        }
      });
      return;
    }
    if (!this.__trailersSent) {
      const t = this.__pendingTrailers ?? [];
      this.__trailersSent = true;
      __wjs_h2_end(this.__conn, this.id, JSON.stringify(t));
    }
    this.__maybeAutoClose();
    cb();
  }
  // 双向尽（应答 End 已发 + 请求体已尽）→ 本地收尾（node 流全关后 socket 解绑）
  __maybeAutoClose() {
    if (this.__destroyed || this.__closed) return;
    if (!(this.__reqEnded && this.__trailersSent)) return;
    this.__closed = true;
    this.__destroyed = true;
    this.__session?.__unregisterStream(this);
    queueMicrotask(() => this.emit("close"));
  }
  _destroy(err, cb) {
    // 应答未完成即销毁 → RST 对端（node：无错 RST 干净收尾，客户端仅 'close'；
    // 有错 RST INTERNAL_ERROR，客户端 'error' ERR_HTTP2_STREAM_ERROR——真机实测）
    if (!this.__trailersSent && !this.__rstSent) {
      this.__rstSent = true;
      __wjs_h2_reset(this.__conn, this.id, err ?? this.__destroyErr ? 2 : 0);
    }
    this.__closed = true;
    this.__destroyed = true;
    this.__session?.__unregisterStream(this);
    this.__abortDownstream();
    cb(err ?? this.__destroyErr);
  }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.__closed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
  __attach(req, res) { this.__req = req; this.__res = res; }
  // stat 失败等 native 层错误：RST(INTERNAL_ERROR) + 流 'error'（node 同码同文；
  // RST 由 _destroy 统一发送）
  __rstInternalError() {
    this.destroy(__code("ERR_HTTP2_STREAM_ERROR", "Stream closed with error code NGHTTP2_INTERNAL_ERROR"));
  }
}

// ── Http2ServerRequest（Readable；从底层 stream 拉取）────────────────────────
class Http2ServerRequest extends Readable {
  constructor(stream, info, scheme, socketProxy) {
    super();
    this.__stream = stream;
    this.__socket = socketProxy;
    this.complete = false;
    this.aborted = false;
    this.httpVersion = "2.0";
    this.httpVersionMajor = 2;
    this.httpVersionMinor = 0;
    const method = String(info.method);
    const authority = String(info.authority ?? "");
    let phoHint = null;
    const reg = [];
    for (const [k, v] of JSON.parse(info.headers)) {
      if (k === "x-wjs-pho") { phoHint = String(v).split(","); continue; }
      reg.push([k, v]);
    }
    const pseudo = [];
    pseudo.push([":method", method]);
    if (method !== "CONNECT") pseudo.push([":path", String(info.path)]);
    if (authority) pseudo.push([":authority", authority]);
    pseudo.push([":scheme", scheme]);
    let all;
    if (phoHint) {
      const ordered = [];
      for (const name of phoHint) {
        const i = pseudo.findIndex(([n]) => n === name);
        if (i !== -1) ordered.push(pseudo.splice(i, 1)[0]);
      }
      all = [...ordered, ...pseudo, ...reg];
    } else {
      all = [...pseudo, ...reg];
    }
    this.headers = __pairsToObj(all);
    this.rawHeaders = all.flat();
    const trailers = JSON.parse(info.trailers ?? "[]");
    this.trailers = __pairsToObj(trailers);
    this.rawTrailers = trailers.flat();
    this.__url = String(info.path);
    stream.__attach(this, null);
    // stream 末尾 → req 收尾
    stream.on("end", () => {
      if (this.__reqDone) return;
      this.__reqDone = true;
      this.complete = true;
      this.push(null);
    });
    stream.on("aborted", () => this.__onAborted());
  }
  get stream() { return this.__stream; }
  get socket() { return this.__socket; }
  get connection() { return this.__socket; }
  get session() { return this.__stream.__session; }
  get method() { return this.headers[":method"]; }
  set method(v) {
    if (typeof v !== "string") throw __code("ERR_INVALID_ARG_TYPE", "method", "string", v);
    if (!__TOKEN_RE.test(v)) throw __code("ERR_INVALID_ARG_VALUE", "method", v);
    this.headers[":method"] = v;
  }
  get scheme() { return this.headers[":scheme"]; }
  set scheme(v) {
    if (typeof v !== "string") throw __code("ERR_INVALID_ARG_TYPE", "scheme", "string", v);
    this.headers[":scheme"] = v;
  }
  get authority() { return this.headers[":authority"] ?? this.headers.host; }
  set authority(v) {
    if (typeof v !== "string") throw __code("ERR_INVALID_ARG_TYPE", "authority", "string", v);
    this.headers[":authority"] = v;
  }
  get url() { return this.__url; }
  set url(v) { this.__url = v; }
  pause() { this.__userPaused = true; return super.pause(); }
  resume() { this.__userPaused = false; return super.resume(); }
  _read() {
    if (this.__reqDone) return;
    const s = this.__stream;
    let chunk;
    while ((chunk = s.read()) !== null) {
      if (!this.push(chunk)) return;
    }
    if (s.readableEnded) {
      this.__reqDone = true;
      this.complete = true;
      this.push(null);
    }
  }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.complete && !this.__stream.__closed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
  __onAborted() {
    if (this.aborted) return;
    this.aborted = true;
    this.complete = true;
    this.emit("aborted");
    this.destroy();
  }
}

// ── Http2ServerResponse（Writable；写路径委派给底层 stream）──────────────────
class Http2ServerResponse extends Writable {
  constructor(req, stream) {
    super({ autoDestroy: true });
    this.req = req;
    this.__stream = stream;
    this.__conn = stream.__conn;
    this.__id = stream.id;
    this.__statusCode = 200;
    this.__headers = Object.create(null);
    this.__trailers = Object.create(null);
    this.headersSent = false;
    this.sendDate = true;
    this.__finishEmitted = false;
    this.__ended = false;
    stream.on("drain", () => this.emit("drain"));
    stream.on("finish", () => {
      if (!this.__finishEmitted) {
        this.__finishEmitted = true;
        this.emit("finish");
      }
    });
    stream.on("close", () => {
      if (!this.__ended) this.__ended = true;
      queueMicrotask(() => this.emit("close"));
    });
    stream.__attach(req, this);
  }
  get stream() { return this.__stream; }
  get socket() { return this.__stream.__destroyed ? undefined : this.req.socket; }
  get connection() { return this.socket; }
  get session() { return this.__stream.__session; }
  // node 口径：finished/writableEnded 即时位（end() 同步置位）；长度/水位读底层流
  get finished() { return this.__ended === true; }
  get writableEnded() { return this.__ended === true; }
  get writableFinished() { return this.__finishEmitted === true; }
  get writableLength() { return this.__stream.writableLength; }
  get writableHighWaterMark() { return this.__stream.writableHighWaterMark; }
  get writableCorked() { return this.__stream.writableCorked; }
  get _header() { return this.headersSent; }
  setHeader(name, value) {
    __validateHeaderName(name);
    __validateHeaderValue(name, value);
    this.__headers[String(name).toLowerCase()] = value;
    return this;
  }
  getHeader(name) {
    const v = this.__headers[String(name).toLowerCase()];
    return v === undefined ? undefined : __headerToWire(v);
  }
  getHeaders() {
    const out = { __proto__: null };
    for (const [k, v] of Object.entries(this.__headers)) out[k] = __headerToWire(v);
    return out;
  }
  getHeaderNames() { return Object.keys(this.__headers); }
  hasHeader(name) { return this.__headers[String(name).toLowerCase()] !== undefined; }
  removeHeader(name) { delete this.__headers[String(name).toLowerCase()]; return this; }
  appendHeader(name, value) {
    __validateHeaderName(name);
    __validateHeaderValue(name, value);
    const k = String(name).toLowerCase();
    const cur = this.__headers[k];
    if (cur === undefined) this.__headers[k] = value;
    else if (Array.isArray(cur)) cur.push(value);
    else this.__headers[k] = [cur, value];
    return this;
  }
  get statusMessage() { return ""; }
  set statusMessage(v) {
    process.emitWarning("Status message is not supported by HTTP/2 (RFC7540 8.1.2.4)");
  }
  set statusCode(status) {
    if (typeof status !== "number" || !Number.isInteger(status) || status < 100 || status > 599) {
      throw __code("ERR_HTTP2_STATUS_INVALID", status);
    }
    this.__statusCode = status;
  }
  get statusCode() { return this.__statusCode ?? 200; }
  setTrailer(name, value) {
    __validateHeaderName(name);
    __validateHeaderValue(name, value);
    this.__trailers[String(name).toLowerCase()] = value;
    return this;
  }
  addTrailers(obj) {
    if (obj === null || typeof obj !== "object") {
      throw __code("ERR_INVALID_ARG_TYPE", "headers", "object", obj);
    }
    for (const [k, v] of Object.entries(obj)) this.setTrailer(k, v);
    return this;
  }
  writeHead(status, ...rest) {
    if (typeof status !== "number" || !Number.isInteger(status) || status < 100 || status > 599) {
      throw __code("ERR_HTTP2_STATUS_INVALID", status);
    }
    if (this.headersSent) throw __code("ERR_HTTP2_HEADERS_SENT");
    this.__statusCode = status;
    for (const r of rest) {
      if (typeof r === "string") {
        // reason phrase：h2 不支持（警告由 statusMessage setter 承担）
      } else if (Array.isArray(r)) {
        for (let i = 0; i + 1 < r.length; i += 2) this.setHeader(r[i], r[i + 1]);
      } else if (r !== null && typeof r === "object") {
        for (const [k, v] of Object.entries(r)) this.setHeader(k, v);
      }
    }
    return this;
  }
  __sendHead() {
    if (this.headersSent || this.__stream.__destroyed) return;
    const entries = [];
    for (const [k, v] of Object.entries(this.__headers)) {
      if (k.startsWith(":")) continue;
      const w = __headerToWire(v);
      if (Array.isArray(w)) for (const one of w) entries.push([k, one]);
      else entries.push([k, w]);
    }
    if (this.sendDate && !this.hasHeader("date")) {
      entries.push(["date", new Date().toUTCString()]);
    }
    this.headersSent = true;
    this.__stream.__headersSent = true;
    __wjs_h2_respond(this.__conn, this.__id, this.statusCode, JSON.stringify(entries));
  }
  write(chunk, encoding, cb) {
    if (this.__stream.__destroyed) {
      const err = __code("ERR_HTTP2_INVALID_STREAM");
      if (typeof encoding === "function") encoding(err);
      else if (typeof cb === "function") cb(err);
      return false;
    }
    if (this.__ended) {
      const err = __code("ERR_STREAM_WRITE_AFTER_END");
      if (typeof encoding === "function") encoding(err);
      else if (typeof cb === "function") cb(err);
      return false;
    }
    this.__sendHead();
    return this.__stream.write(chunk, encoding, cb);
  }
  cork() { this.__stream.cork?.(); }
  uncork() { this.__stream.uncork?.(); }
  end(chunk, encoding, cb) {
    if (typeof chunk === "function") { cb = chunk; chunk = null; encoding = null; }
    else if (typeof encoding === "function") { cb = encoding; encoding = null; }
    if (this.__ended) {
      // node h2 口径：end 可重复调用不抛错；cb 至少一次（finish 前 → finish 时，后 → nextTick）
      if (typeof cb === "function") {
        if (this.__finishEmitted) process.nextTick(cb);
        else this.once("finish", cb);
      }
      return this;
    }
    this.__ended = true;
    if (typeof cb === "function") this.once("finish", cb);
    this.__sendHead();
    if (chunk !== null && chunk !== undefined) this.__stream.write(chunk, encoding);
    const t = [];
    for (const [k, v] of Object.entries(this.__trailers)) {
      const w = __headerToWire(v);
      if (Array.isArray(w)) for (const one of w) t.push([k, one]);
      else t.push([k, w]);
    }
    this.__stream.__pendingTrailers = t;
    // __trailersSent 由 stream._final 在 __wjs_h2_end 发出后置位；
    // 此处预置会让 _final 跳过 END_STREAM → 客户端 'end' 永不到（挂死根因）。
    this.__stream.end();
    return this;
  }
  // node 口径：destroy 恒发 finish（clean/err 均先 finish 后 close），
  // 错误不落 res（错误走 stream 'error'，res.on('error') 不触发）。
  destroy(err) {
    if (this.destroyed) return this;
    if (!this.__finishEmitted) {
      this.__finishEmitted = true;
      this.emit("finish");
    }
    this.__stream.destroy(err ?? null);
    return super.destroy();
  }
  _destroy(err, cb) {
    if (!this.__finishEmitted) {
      this.__finishEmitted = true;
      this.emit("finish");
    }
    cb(err);
  }
  __destroySilent() {
    if (this.destroyed) return;
    super.destroy();
  }
  createPushResponse(cb) {
    if (typeof cb !== "function") throw __code("ERR_INVALID_ARG_TYPE", "callback", "Function", cb);
    queueMicrotask(() => cb(__code("ERR_HTTP2_PUSH_DISABLED")));
    return undefined;
  }
  writeContinue(cb) { if (typeof cb === "function") queueMicrotask(cb); return this; }
  writeInformation(type, info, cb) {
    if (typeof type !== "number" || !Number.isInteger(type) || type < 200 || type > 599) {
      throw __code("ERR_HTTP2_STATUS_INVALID", type);
    }
    if (type === 204 || type === 304) {
      throw __code("ERR_HTTP2_INVALID_INFO_STATUS", type);
    }
    if (typeof info === "object" && info !== null && !Array.isArray(info)) {
      for (const [k, v] of Object.entries(info)) {
        __validateHeaderName(k);
        __validateHeaderValue(k, v);
      }
    }
    const f = typeof info === "function" ? info : cb;
    if (typeof f === "function") queueMicrotask(f);
    return this;
  }
  writeEarlyHints(hints, cb) {
    const f = typeof hints === "function" ? hints : cb;
    if (hints !== null && typeof hints === "object") {
      for (const [k, v] of Object.entries(hints)) {
        __validateHeaderName(k);
        __validateHeaderValue(k, v);
      }
    }
    if (typeof f === "function") queueMicrotask(f);
    return this;
  }
  flushHeaders() {
    if (!this.headersSent) this.__sendHead();
  }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.__stream.__closed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
}

// ── 服务端 ──────────────────────────────────────────────────────────────
class Http2Server extends EventEmitter {
  constructor(options, secure) {
    super();
    if (typeof options !== "object" || options === null) {
      throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
    }
    this.__id = 0;
    this.__listening = null;
    this.__secure = !!secure;
    this.__tlsOpts = null;
    this.__streams = new Map();
    this.__sessions = new Map();
    this.__sockBag = {};
    this.__opts = options;
    this[kPendingOptions] = this.__opts;
    if (options.settings !== undefined) __validateSettings(options.settings);
    if (options.maxOutstandingSettings !== undefined) {
      if (typeof options.maxOutstandingSettings !== "number" ||
          !Number.isInteger(options.maxOutstandingSettings) ||
          options.maxOutstandingSettings < 0) {
        throw __code("ERR_OUT_OF_RANGE", "options.maxOutstandingSettings", options.maxOutstandingSettings);
      }
    }
    if (secure) {
      for (const k of ["maxSessionInvalidStreams", "maxSessionRejectedStreams", "maxSessionInvalidFrames"]) {
        if (options[k] !== undefined &&
            (typeof options[k] !== "number" || !Number.isInteger(options[k]) || options[k] < 0)) {
          throw __code("ERR_OUT_OF_RANGE", `options.${k}`, options[k]);
        }
      }
      if (options.ALPNCallback !== undefined && options.ALPNProtocols !== undefined) {
        throw __code("ERR_TLS_ALPN_CALLBACK_WITH_PROTOCOLS");
      }
      if (options.key !== undefined && options.cert !== undefined) {
        this.__tlsOpts = { tls: { key: String(Array.isArray(options.key) ? options.key[0] : options.key),
                                  cert: String(Array.isArray(options.cert) ? options.cert[0] : options.cert) } };
      } else {
        this.__tlsOpts = null;
      }
    }
    // request 监听器只由 createServer/createSecureServer 接线（§4.39）
    this.__ev = this.__ev.bind(this);
  }
  get socket() { return this.__sockBag; }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        for (const [, s] of this.__sessions) s.emit("timeout");
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
  updateSettings(settings) {
    const validated = __validateSettings(settings);
    this.__opts.settings = __applySettings(this.__opts.settings ?? {}, validated);
    return this;
  }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.__closing) return this;
    this.__closing = true;
    if (this.__id) {
      __wjs_net_destroy(this.__id);
      this.__id = 0;
    } else {
      // 未监听或已关：直接收尾（node server.close 无监听也回调）
      queueMicrotask(() => {
        if (this.__sessions.size === 0) this.__emitClose();
      });
    }
    return this;
  }
  __emitClose() {
    if (this.__closeEmitted) return;
    this.__closeEmitted = true;
    this.emit("close");
  }
  __sessionFor(connId, peerObj) {
    let s = this.__sessions.get(connId);
    if (s === undefined) {
      s = new Http2Session(this, connId, peerObj);
      this.__sessions.set(connId, s);
      this.emit("session", s);
    }
    return s;
  }
  __abortEntry(entry) {
    if (!entry) return;
    entry.stream.__abort();
    this.__streams.delete(entry.stream.id);
  }
  __ev(kind, payload) {
    switch (kind) {
      case "listening": {
        const o = JSON.parse(payload);
        this.__listening = { address: o.addr, port: o.port, family: String(o.addr).includes(":") ? "IPv6" : "IPv4" };
        this.emit("listening");
        break;
      }
      case "request": {
        const o = JSON.parse(payload);
        const id = Number(o.streamId);
        const peer = String(o.peer ?? "");
        const peerObj = {
          addr: peer.includes(":") ? peer.slice(0, peer.lastIndexOf(":")) : peer,
          port: peer.includes(":") ? Number(peer.slice(peer.lastIndexOf(":") + 1)) : 0,
        };
        const session = this.__sessionFor(Number(o.connId), peerObj);
        const stream = new Http2ServerStream(session, Number(o.connId), id);
        const streaming = o.streaming === true;
        const bodyEmpty = !streaming && (o.body ?? "") === "";
        const trailersEmpty = !streaming && (o.trailers ?? "[]") === "[]";
        const flags = bodyEmpty && trailersEmpty ? 5 : 4;
        stream.endAfterHeaders = bodyEmpty && trailersEmpty;
        const sock = __mkSocketProxy(stream, this, peerObj);
        const scheme = this.__secure ? "https" : "http";
        const req = new Http2ServerRequest(stream, o, scheme, sock);
        const res = new Http2ServerResponse(req, stream);
        this.__streams.set(id, { stream, req, res });
        stream.once("close", () => this.__streams.delete(id));
        const method = req.headers[":method"];
        if (method === "HEAD") {
          // HEAD 应答自动 END_STREAM（respond 内处理）
          stream.__headRequest = true;
        }
        if (method === "CONNECT") {
          if (this.listenerCount("connect") > 0) this.emit("connect", req, res);
          else { res.statusCode = 501; res.end(); }
        } else {
          // node 顺序：'stream'（raw）先于 'request'（compat）
          this.emit("stream", stream, req.headers, flags, req.rawHeaders);
          const hasCompat = this.listenerCount("request") > 0;
          if (hasCompat) this.emit("request", req, res);
          // 流式体：`body`/`reqEnd` 流事件逐块跟进（见下）；否则头即终。
          if (!streaming) {
            stream.__feedBody(o.body ?? "");
            stream.__endReq(o.trailers ?? "[]");
          }
          if (hasCompat) queueMicrotask(() => { if (!req.destroyed && !req.__userPaused) req.resume(); });
        }
        break;
      }
      case "body": {
        const o = JSON.parse(payload);
        const e = this.__streams.get(Number(o.streamId));
        if (e) e.stream.__feedBody(o.payload ?? "");
        break;
      }
      case "reqEnd": {
        const o = JSON.parse(payload);
        const e = this.__streams.get(Number(o.streamId));
        if (e) e.stream.__endReq(o.payload ?? "[]");
        break;
      }
      case "aborted": {
        const o = JSON.parse(payload);
        const entry = this.__streams.get(Number(o.streamId));
        if (entry && o.payload !== undefined && o.payload !== "") {
          entry.stream.rstCode = Number(o.payload);
        }
        this.__abortEntry(entry);
        break;
      }
      case "connClose": {
        // Rust 侧 payload = conn_id 裸串；包装层 streamId 恒 0，须读 .payload
        const connId = Number(JSON.parse(payload).payload);
        const s = this.__sessions.get(connId);
        if (s) s.__ev("connClose", payload);
        // 全会话已收且监听器已关 → server 'close'
        queueMicrotask(() => {
          if (this.__closing && !this.__id && this.__sessions.size === 0) this.__emitClose();
        });
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        this.emit("error", __h2Err(o.code, o.msg));
        break;
      }
      case "close": {
        // 监听器已关（node：不杀活连接）。'close' 等全部会话排空
        // （connClose 走 conn 自身 target，不受本次 purge 影响）。
        queueMicrotask(() => {
          if (this.__closing && !this.__id && this.__sessions.size === 0) this.__emitClose();
        });
        break;
      }
    }
  }
  address() { return this.__listening; }
  listen(...args) {
    let port, host = null, cb = null;
    if (typeof args[0] === "object" && args[0] !== null) {
      port = args[0].port;
      host = args[0].host ?? null;
      cb = typeof args[1] === "function" ? args[1] : null;
    } else {
      port = args[0];
      for (let i = 1; i < args.length; i++) {
        if (typeof args[i] === "string" && host === null) host = args[i];
        else if (typeof args[i] === "function") cb = args[i];
      }
    }
    if (cb) this.once("listening", cb);
    this.__port = Number(port);
    this.__closing = false;
    this.__id = Number(__wjs_h2_listen(Number(port), host === null ? "0.0.0.0" : host,
      JSON.stringify(this.__tlsOpts ?? {}), this));
    return this;
  }
  [Symbol.asyncDispose]() {
    return new Promise((resolve) => {
      this.close(() => resolve(undefined));
    });
  }
  ref() { if (this.__id) __wjs_net_ref(this.__id); return this; }
  unref() { if (this.__id) __wjs_net_unref(this.__id); return this; }
}
