const kPendingOptions = Symbol("options");

// ── 客户端 ──────────────────────────────────────────────────────────────
const __DEFERRED_METHODS = new Set(["POST", "PUT", "PATCH"]);
class ClientHttp2Stream extends Duplex {
  constructor(session, id, headers, options) {
    super();
    this.__session = session;
    this.id = id;
    this.sentHeaders = headers;
    this.__ended = false;
    this.aborted = false;
    this.__opened = false;
    this.__deferred = __DEFERRED_METHODS.has(String(headers[":method"]));
    this.__waitTrailers = !!(options && options.waitForTrailers);
    this.__pendingBody = [];
    this.endAfterHeaders = false;
    this.__responseReceived = false;
  }
  get session() { return this.__session; }
  get bufferSize() { return this.writableLength; }
  get pushAllowed() { return false; }
  get sentPseudoHeaders() {
    const out = { __proto__: null };
    for (const [k, v] of Object.entries(this.sentHeaders)) {
      if (k.startsWith(":")) out[k] = Array.isArray(v) ? String(v[0]) : String(v);
    }
    return out;
  }
  get sentInfoHeaders() { return []; }
  get sentTrailers() { return null; }
  get state() {
    return {
      state: this.destroyed ? 7 : 2,
      weight: 16,
      sumDependencyWeight: 0,
      localClose: this.writableEnded ? 1 : 0,
      remoteClose: this.readableEnded ? 1 : 0,
      localWindowSize: 65535,
    };
  }
  _read() {}
  __openNow(extraBody) {
    if (this.__opened) return;
    this.__opened = true;
    const parts = extraBody ? [...this.__pendingBody, extraBody] : this.__pendingBody;
    this.__pendingBody = [];
    let total = 0;
    for (const b of parts) total += b.length;
    const flat = new Uint8Array(total);
    let off = 0;
    for (const b of parts) { flat.set(b, off); off += b.length; }
    try {
      __wjs_h2_open(this.__session.__id, this.id,
        JSON.stringify({
          method: this.sentHeaders[":method"], path: this.sentHeaders[":path"],
          scheme: this.sentHeaders[":scheme"], authority: this.sentHeaders[":authority"],
          waitTrailers: this.__waitTrailers,
          headers: Object.entries(this.sentHeaders)
            .filter(([k]) => !k.startsWith(":"))
            .map(([k, v]) => [
              k,
              // node prepareRequestHeaders 口径：cookie 数组 "; " 并串为单条线
              (k === "cookie" && Array.isArray(v)) ? v.join("; ") : v,
            ]),
        }),
        __b64enc(flat));
    } catch {
      // 会话已亡（销毁竞态）：流随会话终止，不再上抛
      this.__detachFromSession();
    }
    if (this.__waitTrailers) {
      queueMicrotask(() => {
        if (this.destroyed) return;
        this.__wantTrailersFired = true;
        this.emit("wantTrailers");
      });
    }
  }
  _write(chunk, encoding, cb) {
    if (this.__opened || this.__ended) {
      cb(__code("ERR_STREAM_WRITE_AFTER_END"));
      return;
    }
    const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk), "write");
    this.__pendingBody.push(u8);
    queueMicrotask(cb);
  }
  _final(cb) {
    this.__openNow(null);
    cb();
  }
  sendTrailers(trailers) {
    // node core.js 门序（同服务端 sendTrailers 注）。
    if (this.destroyed || this.closed) throw __code("ERR_HTTP2_INVALID_STREAM");
    if (this.__trailersSent) throw __code("ERR_HTTP2_TRAILERS_ALREADY_SENT");
    if (!this.__opened || !this.__waitTrailers || !this.__wantTrailersFired) {
      throw __code("ERR_HTTP2_TRAILERS_NOT_READY");
    }
    this.__trailersSent = true;
    const t = [];
    for (const [k, v] of Object.entries(trailers ?? {})) t.push([k, String(v)]);
    __wjs_h2_open_trailers(this.__session.__id, this.id, JSON.stringify(t));
    return this;
  }
  __onResponse(headers, flags) {
    this.__responseReceived = true;
    if (flags & 1) this.endAfterHeaders = true;
    this.emit("response", headers, flags);
  }
  __onData(u8) { this.push(Buffer.from(u8)); }
  __onTrailers(t) { this.emit("trailers", t); }
  __onEnd() {
    this.push(null);
    // node：END_STREAM 收到 + 请求侧已尽 → 流 close（不必等 readable 消费；
    // respond-file-errors 套件 req 无 data 监听仅等 'close'）
    if (this.writableEnded && !this.destroyed) {
      queueMicrotask(() => { if (!this.destroyed) this.destroy(); });
    }
  }
  __onAborted() {
    if (this.aborted) return;
    // node：session destroy 路径的流 'aborted' 事件发出但 aborted 属性
    // 保持 false（aborted 属性 = 对端 RST 中断语义；client-destroy 套件）
    this.emit("aborted");
    this.push(null);
    this.destroy(__code("ERR_HTTP2_STREAM_CANCEL"));
  }
  priority(options) {
    __h2PriorityDeprecate();
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
    if (options.silent !== true) { /* 无 PRIORITY 帧底座（偏差记档） */ }
    return this;
  }
  close(code, cb) {
    if (typeof code === "function") { cb = code; code = 0; }
    if (!this.destroyed) {
      if (typeof cb === "function") this.once("close", cb);
      this.__ended = true;
      if (this.__opened && this.__session.__id) {
        __wjs_h2_reset(this.__session.__id, this.id, code ?? 0);
      }
      this.destroy();
    } else if (typeof cb === "function") {
      queueMicrotask(cb);
    }
    return this;
  }
  destroy(err, code, cb) {
    if (typeof err === "number") { cb = code; code = err; err = undefined; }
    if (typeof code === "function") { cb = code; code = 0; }
    if (typeof cb === "function") this.once("close", cb);
    if (this.__destroyed) return this;
    // node：流销毁 → RST(CANCEL) 通知对端（未收到应答且已出线的流）
    if (this.__opened && !this.__responseReceived && this.__session?.__id) {
      __wjs_h2_reset(this.__session.__id, this.id, 8);
    }
    this.__detachFromSession();
    super.destroy(err);
    return this;
  }
  __detachFromSession() {
    const s = this.__session;
    if (s && s.__streams) {
      s.__streams.delete(this.id);
      const pi = s.__pendingOpens ? s.__pendingOpens.indexOf(this) : -1;
      if (pi !== -1) s.__pendingOpens.splice(pi, 1);
      if (typeof s.__gracefulWait === "function") s.__gracefulWait();
    }
  }
  destroy(err, code, cb) {
    if (typeof err === "number") { cb = code; code = err; err = undefined; }
    if (typeof code === "function") { cb = code; code = 0; }
    if (typeof cb === "function") this.once("close", cb);
    if (this.__destroyed) return this;
    this.__detachFromSession();
    super.destroy(err);
    return this;
  }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.destroyed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
}

class ClientHttp2Session extends EventEmitter {
  constructor(authority, options) {
    super();
    this.__id = 0;
    this.__seq = 1;
    this.__streams = new Map();
    this.__authority = authority;
    this.__options = options ?? {};
    this.destroyed = false;
    this.closed = false;
    this.type = 1; // NGHTTP2_SESSION_CLIENT
    this.encrypted = false;
    this.connecting = true;
    this.__settings = { ...__DEFAULT_SETTINGS, ...(options?.settings ? __validateSettings(options.settings) : {}) };
    this.__remoteSettings = { ...__DEFAULT_SETTINGS };
    this.__pendingSettingsAck = false;
    this.__outstandingSettings = 0;
    this.__maxOutstandingSettings = options?.maxOutstandingSettings ?? Infinity;
    // node 缺省 maxOutstandingPings = 2（ping.js 超额即 CANCEL）
    this.__maxOutstandingPings = options?.maxOutstandingPings ?? 2;
    this.__pendingOpens = [];
    this.state = {
      effectiveLocalWindowSize: 65535,
      effectiveRemoteWindowSize: 65535,
      localWindowSize: 65535,
      remoteWindowSize: 65535,
      outboundQueueSize: 0,
      deflateDynamicTableSize: 4096,
      inflateDynamicTableSize: 4096,
    };
    this.__ev = this.__ev.bind(this);
  }
  get socket() {
    if (this[kSocket] === undefined) {
      const s = this;
      const sock = new EventEmitter();
      sock.connecting = true;
      sock.destroyed = false;
      sock.readable = true;
      sock.writable = true;
      sock.remoteAddress = s.__host;
      sock.remotePort = s.__port;
      sock.localAddress = undefined;
      sock.localPort = undefined;
      sock.on("newListener", (ev) => {
        if (ev === "close" && s.closed && !sock.destroyed) {
          queueMicrotask(() => sock.emit("close"));
        }
      });
      sock.destroy = (err) => {
        if (sock.destroyed) return;
        sock.destroyed = true;
        if (err) sock.emit("error", err);
        s.destroy();
      };
      sock.end = () => { s.close(); return sock; };
      sock.write = () => true;
      sock.setTimeout = (m, cb) => { s.setTimeout(m, cb); return sock; };
      sock.address = () => ({ address: s.__host, port: s.__port, family: s.__host?.includes(":") ? "IPv6" : "IPv4" });
      this[kSocket] = sock;
    }
    return this[kSocket];
  }
  get alpnProtocol() { return this.__secure ? "h2" : false; }
  get localSettings() { return this.__settings; }
  get remoteSettings() { return this.__remoteSettings; }
  get pendingSettingsAck() { return this.__pendingSettingsAck; }
  get originSet() { return this.__secure ? [] : undefined; }
  get unrefed() { return false; }
  setTimeout(msecs, callback) {
    if (typeof callback === "function") this.once("timeout", callback);
    const ms = Number(msecs) || 0;
    if (this.__timeoutTimer !== undefined) clearTimeout(this.__timeoutTimer);
    if (ms > 0 && !this.closed && !this.destroyed) {
      this.__timeoutTimer = setTimeout(() => {
        this.__timeoutTimer = undefined;
        this.emit("timeout");
      }, ms);
    }
    return this;
  }
  ref() { if (this.__id) __wjs_net_ref(this.__id); return this; }
  unref() { if (this.__id) __wjs_net_unref(this.__id); return this; }
  settings(settings, cb) {
    const validated = __validateSettings(settings);
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    this.__pendingSettingsAck = true;
    this.__outstandingSettings++;
    if (Number.isFinite(this.__maxOutstandingSettings) &&
        this.__outstandingSettings >= this.__maxOutstandingSettings) {
      this.__outstandingSettings = 0;
      this.__pendingSettingsAck = false;
      process.nextTick(() => {
        this.emit("error", __code("ERR_HTTP2_MAX_PENDING_SETTINGS_ACK"));
        this.destroy();
      });
      return this;
    }
    setTimeout(() => {
      this.__outstandingSettings = Math.max(0, this.__outstandingSettings - 1);
      if (this.__outstandingSettings === 0) this.__pendingSettingsAck = false;
      this.__settings = __applySettings(this.__settings, validated);
      if (!this.destroyed) {
        this.emit("localSettings", this.__settings);
        if (typeof cb === "function") cb();
      }
    }, 1);
    return this;
  }
  updateSettings(settings) {
    const validated = __validateSettings(settings);
    this.__settings = __applySettings(this.__settings, validated);
    return this;
  }
  // node：windowSize 校验（number、0..2^31-1）后仅本地状态面（flow control
  // 由底座管理；setLocalWindowSize-errors/套件只断言校验与 state 反映）
  setLocalWindowSize(windowSize) {
    // node：destroyed 校验先于实参校验（client-destroy 套件）
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    if (typeof windowSize !== "number") {
      throw __code("ERR_INVALID_ARG_TYPE", "windowSize", "number", windowSize);
    }
    if (windowSize < 0 || windowSize > 2147483647) {
      throw __code("ERR_OUT_OF_RANGE", "windowSize", ">= 0 && <= 2147483647", windowSize);
    }
    this.state.effectiveLocalWindowSize = windowSize;
    this.state.localWindowSize = windowSize;
    return this;
  }
  // node 口径：ping([payload, ]callback)；payload 非 ArrayBufferView →
  // TypeError、长度≠8 → RangeError ERR_HTTP2_PING_LENGTH、超 maxOutstandingPings
  // → 返 false 且回调 ERR_HTTP2_PING_CANCEL（真机 ping.js/onping 实测）
  ping(payload, callback) {
    if (typeof payload === "function") { callback = payload; payload = undefined; }
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    let buf = null;
    if (payload !== undefined && payload !== null) {
      if (typeof payload !== "object" || !ArrayBuffer.isView(payload)) {
        throw __code("ERR_INVALID_ARG_TYPE", "payload", ["Buffer", "TypedArray", "DataView"], payload);
      }
      const u8 = payload instanceof Uint8Array ? payload : new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength);
      if (u8.length !== 8) throw __code("ERR_HTTP2_PING_LENGTH");
      buf = u8;
    }
    if (typeof callback !== "function") {
      throw __code("ERR_INVALID_ARG_TYPE", "callback", "function", callback);
    }
    const cap = Number.isInteger(this.__maxOutstandingPings) ? this.__maxOutstandingPings : 2;
    if ((this.__outstandingPings ?? 0) >= cap) {
      const cancel = __code("ERR_HTTP2_PING_CANCEL");
      queueMicrotask(() => callback(cancel));
      return false;
    }
    this.__outstandingPings = (this.__outstandingPings ?? 0) + 1;
    const ret = Buffer.alloc(8);
    if (buf) Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength).copy(ret);
    setTimeout(() => {
      this.__outstandingPings = Math.max(0, (this.__outstandingPings ?? 1) - 1);
      if (this.destroyed) { callback(__code("ERR_HTTP2_PING_CANCEL")); return; }
      callback(null, 1, ret);
    }, 1);
    return true;
  }
  goaway(code = 0, lastStreamID = 0, opaqueData) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    if (typeof code === "object" && code !== null) {
      const o = code;
      code = o.errorCode ?? 0;
      lastStreamID = o.lastStreamID ?? 0;
      opaqueData = o.opaqueData;
    }
    this.__goaway = { code, lastStreamID, opaqueData };
    setImmediate(() => this.close());
    return this;
  }
  setNextStreamID(id) {
    if (this.destroyed) throw __code("ERR_HTTP2_INVALID_SESSION");
    if (typeof id !== "number" || !Number.isInteger(id) || id < 0 || id > 2147483647) {
      throw __code("ERR_OUT_OF_RANGE", "id", id);
    }
    this.__seq = id;
    return this;
  }
  altsvc(alt, origin) {
    if (typeof alt === "string" || alt === undefined) return this;
    throw __code("ERR_INVALID_ARG_TYPE", "alt", "string", alt);
  }
  origin(...origins) { return this; }
  __start(port, host) {
    this.__host = host;
    this.__port = port;
    // kSocket 即刻物化（node：connect() 即有 socket；套件 connect() 返回后
    // 直探 client[kSocket]，懒 getter 拿到 undefined）
    void this.socket;
    const wire = {};
    if (this.__options.tls !== undefined || this.__secure) {
      const t = this.__options.tls ?? this.__options;
      wire.tls = {};
      if (t.ca !== undefined) wire.tls.ca = String(t.ca);
      if (t.rejectUnauthorized !== undefined) wire.tls.rejectUnauthorized = !!t.rejectUnauthorized;
      if (t.servername !== undefined) wire.tls.servername = String(t.servername);
    }
    this.__id = Number(__wjs_h2_connect(host, port, JSON.stringify(wire), this));
    return this;
  }
  __ev(kind, payload) {
    switch (kind) {
      case "connect":
        this.connecting = false;
        if (this[kSocket]) {
          this[kSocket].connecting = false;
          queueMicrotask(() => this[kSocket].emit("connect"));
        }
        for (const pst of this.__pendingOpens.splice(0)) pst.__openNow(null);
        this.emit("connect", this, null);
        break;
      case "response": {
        const o = JSON.parse(payload);
        const st = this.__streams.get(Number(o.streamId));
        if (!st) break;
        const head = JSON.parse(o.payload);
        const headers = __pairsToObj(JSON.parse(head.headers));
        headers[":status"] = head.status;
        st.__onResponse(headers, head.flags ?? 4);
        break;
      }
      case "data": {
        const o = JSON.parse(payload);
        const st = this.__streams.get(Number(o.streamId));
        if (!st) break;
        st.__onData(__b64dec(o.payload));
        break;
      }
      case "trailers": {
        const o = JSON.parse(payload);
        const st = this.__streams.get(Number(o.streamId));
        if (!st) break;
        st.__onTrailers(__pairsToObj(JSON.parse(o.payload)));
        break;
      }
      case "end": {
        const o = JSON.parse(payload);
        const sid = Number(o.streamId);
        const st = this.__streams.get(sid);
        if (!st) break;
        st.__onEnd();
        this.__streams.delete(sid);
        break;
      }
      case "aborted": {
        const o = JSON.parse(payload);
        const sid = Number(o.streamId);
        const st = this.__streams.get(sid);
        if (!st) break;
        st.__onAborted();
        this.__streams.delete(sid);
        break;
      }
      case "error": {
        // 已销毁会话的迟到错误（如握手失败竞态）吞掉（node：destroy 后不事件）
        if (this.destroyed) break;
        const o = JSON.parse(payload);
        const inner = o.payload === undefined ? o : JSON.parse(o.payload);
        const sid = o.streamId === undefined ? undefined : Number(o.streamId);
        const err = __h2Err(inner.code ?? "ERR_HTTP2_STREAM_ERROR", inner.msg ?? "");
        const st = sid !== undefined ? this.__streams.get(sid) : undefined;
        if (st) {
          // 流级错误：派发后即销毁（node：RST 后流走 destroy → 'close'；
          // 否则 close 永不到，fd-invalid 类套件挂死）。无监听器回落会话
          //（旧口径保留，error-order 系套件依赖）。
          if (inner.rst !== undefined && inner.rst !== null) st.rstCode = Number(inner.rst);
          if (st.listenerCount("error") > 0) st.emit("error", err);
          else this.emit("error", err);
          st.destroy();
        } else {
          this.emit("error", err);
        }
        break;
      }
      case "close":
        this.closed = true;
        this.connecting = false;
        if (this[kSocket] && !this[kSocket].destroyed) {
          this[kSocket].destroyed = true;
          queueMicrotask(() => this[kSocket].emit("close"));
        }
        this.emit("close");
        break;
    }
  }
  request(headers, options) {
    // node：请求选项带优先级字段即弃用告警（DEP0194，RFC 9113 废止优先级信令；进程级一次）。
    if (options && (options.weight !== undefined || options.parent !== undefined ||
        options.exclusive !== undefined || options.silent !== undefined)) {
      __h2RequestPriorityDeprecate();
    }
    // node：closed（GOAWAY 后）新建流同步抛 ERR_HTTP2_GOAWAY_SESSION；
    // destroyed 会话上 request() 不同步抛——返回一个异步 error
    // ERR_HTTP2_INVALID_SESSION + 'close' 的流（client-destroy 套件）
    if (this.closed && !this.destroyed) throw __code("ERR_HTTP2_GOAWAY_SESSION");
    if (this.destroyed) {
      const sid = this.__seq;
      this.__seq += 2;
      const h = { ":method": "GET", ":path": "/", ":scheme": this.__secure ? "https" : "http", ":authority": this.__authority ?? "" };
      const st = new ClientHttp2Stream(this, sid, h, options);
      process.nextTick(() => {
        if (st.destroyed) return;
        st.emit("error", __code("ERR_HTTP2_INVALID_SESSION"));
        st.destroy();
      });
      return st;
    }
    const h = { ...headers };
    if (h[":method"] === undefined) h[":method"] = "GET";
    if (h[":path"] === undefined && h[":method"] !== "CONNECT") h[":path"] = "/";
    if (h[":authority"] === undefined && h.host === undefined) h[":authority"] = this.__authority;
    if (h[":scheme"] === undefined) h[":scheme"] = this.__secure ? "https" : "http";
    const pseudoKeys = Object.keys(h).filter((k) => k.startsWith(":"));
    const canonical = [":method", ":scheme", ":authority", ":path"];
    const differs = pseudoKeys.length !== canonical.length ||
      pseudoKeys.some((k, i) => k !== canonical[i]);
    if (differs) h["x-wjs-pho"] = pseudoKeys.join(",");
    const sid = this.__seq;
    this.__seq += 2; // 客户端单数流（RFC 7540 口径）
    const st = new ClientHttp2Stream(this, sid, h, options);
    this.__streams.set(sid, st);
    // options.signal：abort → ABORT_ERR 销毁（AbortSignal 套件）
    const signal = options?.signal;
    if (signal) {
      const onAbort = () => {
        if (!st.destroyed) {
          st.emit("error", __code("ABORT_ERR"));
          st.destroy();
        }
      };
      if (signal.aborted) {
        // node：预中止信号 → 同 tick destroyed（destroy 套件），error 下一跳
        st.destroy();
        process.nextTick(() => st.emit("error", __code("ABORT_ERR")));
      } else {
        // 经 addAbortListener（listenerCount 可见）
        const disposable = addAbortListener(signal, onAbort);
        st.once("close", () => disposable[Symbol.dispose]());
      }
    }
    if (!st.__deferred) {
      if (this.connecting && !this.closed) this.__pendingOpens.push(st);
      else st.__openNow(null);
      // node isPayloadMeaningless：GET/HEAD/DELETE 缺省 endStream=true → 可写侧即尽，
      // 收完应答流即 'close'（修前可写侧悬着，req 'close' 永不到）。
      const m = String(st.sentHeaders[":method"]);
      const endStream = options?.endStream ?? (m === "GET" || m === "HEAD" || m === "DELETE");
      if (endStream) st.end();
    }
    return st;
  }
  // node：client.close() 优雅关（GOAWAY）：新流被拒、已收到应答的流继续、
  // 未收到应答的流报 ERR_HTTP2_GOAWAY_SESSION。本实现以 100ms 宽限扫描近似
  // （响应在途的流大多在窗口内完成；真机无此延迟——偏差记档）
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    if (this.closed) return this;
    this.closed = true;
    this.connecting = false;
    // 未出线的 open 直接取消（node：close before connect，对端不可见流）
    for (const pst of this.__pendingOpens.splice(0)) {
      pst.emit("error", __code("ERR_HTTP2_GOAWAY_SESSION"));
      pst.destroy();
    }
    this.__gracefulWait();
    if (!this.destroyed) {
      setTimeout(() => {
        if (this.destroyed) return;
        for (const [, st] of this.__streams) {
          if (!st.__responseReceived) {
            st.emit("error", __code("ERR_HTTP2_GOAWAY_SESSION"));
            st.destroy();
          }
        }
        this.__gracefulWait();
      }, 100);
    }
    return this;
  }
  __gracefulWait() {
    // 仅在 close()/destroy() 发起后、流已排空时硬关；单纯流 detach
    // （响应完成）不得关会话——否则后续请求报 INVALID_SESSION
    if ((this.closed || this.destroyed) && !this.destroyed && this.__streams.size === 0 && this.__id) {
      this.destroyed = true;
      __wjs_net_destroy(this.__id);
      this.__id = 0;
    }
  }
  destroy(code, cb) {
    if (typeof code === "function") { cb = code; code = 0; }
    if (typeof cb === "function") this.once("close", cb);
    if (!this.destroyed) {
      this.destroyed = true;
      this.closed = true;
      if (code && typeof code === "object") this.emit("error", code);
      // 在途流：RST 对端（防 server 侧 service 永挂）+ 本地 CANCEL 错
      for (const [, st] of this.__streams) {
        if (!st.__responseReceived && this.__id) {
          __wjs_h2_reset(this.__id, st.id, 8);
        }
        st.__onAborted();
      }
      for (const pst of this.__pendingOpens.splice(0)) {
        pst.emit("error", __code("ERR_HTTP2_STREAM_CANCEL"));
        pst.destroy();
      }
      if (this.__id) {
        __wjs_net_destroy(this.__id);
        this.__id = 0;
      }
    }
    return this;
  }
}

export function createServer(options, listener) {
  if (typeof options === "function") { listener = options; options = undefined; }
  const s = new Http2Server(options ?? {}, false);
  if (typeof listener === "function") s.on("request", listener);
  return s;
}
export function createSecureServer(options, listener) {
  if (options !== undefined && (typeof options !== "object" || options === null)) {
    throw __code("ERR_INVALID_ARG_TYPE", "options", "object", options);
  }
  const s = new Http2Server(options ?? {}, true);
  if (typeof listener === "function") s.on("request", listener);
  return s;
}
export function connect(authority, options, listener) {
  let url = null;
  let host, port, secure;
  if (typeof authority === "string") {
    url = new URL(authority);
  } else if (authority !== null && typeof authority === "object") {
    const o = authority;
    if (typeof o.href === "string" && typeof o.protocol === "string" && typeof o.hostname === "string") {
      url = new URL(o.href); // URL 实例
    } else {
      const proto = String(o.protocol ?? "http:").toLowerCase();
      if (!proto.endsWith(":")) throw __code("ERR_HTTP2_INVALID_PROTOCOL", proto, ":");
      secure = proto === "https:";
      host = String(o.authority ?? o.hostname ?? o.host ?? "localhost");
      port = o.port !== undefined ? Number(o.port) : (secure ? 443 : 80);
      if (typeof options === "function") { listener = options; options = {}; }
      const session = new ClientHttp2Session(host, { ...(options ?? {}), tls: secure ? (options ?? {}) : undefined });
      session.__secure = secure;
      wireSessionSignal(session, options);
      if (typeof listener === "function") session.once("connect", listener);
      session.__start(port, host);
      return session;
    }
  } else {
    options = authority ?? {};
  }
  if (typeof options === "function") { listener = options; options = {}; }
  secure = url.protocol === "https:";
  const session = new ClientHttp2Session(url.host, { ...(options ?? {}), tls: secure ? (options ?? {}) : undefined });
  session.__secure = secure;
  wireSessionSignal(session, options);
  if (typeof listener === "function") session.once("connect", listener);
  port = url.port ? Number(url.port) : (secure ? 443 : 80);
  session.__start(port, url.hostname);
  return session;
}

// connect(options.signal)：abort → 会话 ABORT_ERR 销毁（AbortSignal 套件）
function wireSessionSignal(session, options) {
  const signal = options?.signal;
  if (!signal) return;
  const onAbort = () => session.destroy(__code("ABORT_ERR"));
  if (signal.aborted) {
    process.nextTick(onAbort);
    return;
  }
  const disposable = addAbortListener(signal, onAbort);
  session.once("close", () => disposable[Symbol.dispose]());
}

export function getDefaultSettings() {
  return { ...__DEFAULT_SETTINGS, customSettings: {} };
}
export function getPackedSettings(settings) {
  const s = __validateSettings(settings ?? {});
  const out = [];
  const push = (id, val) => {
    out.push((id >> 8) & 0xff, id & 0xff, (val >>> 24) & 0xff, (val >>> 16) & 0xff, (val >>> 8) & 0xff, val & 0xff);
  };
  if (s.headerTableSize !== undefined) push(0x1, s.headerTableSize);
  if (s.enablePush !== undefined) push(0x2, s.enablePush ? 1 : 0);
  if (s.maxConcurrentStreams !== undefined) push(0x3, s.maxConcurrentStreams);
  if (s.initialWindowSize !== undefined) push(0x4, s.initialWindowSize);
  if (s.maxFrameSize !== undefined) push(0x5, s.maxFrameSize);
  // id 0x6：maxHeaderListSize 优先，缺省回落 maxHeaderSize（真机对拍）
  if (s.maxHeaderListSize !== undefined) push(0x6, s.maxHeaderListSize);
  else if (s.maxHeaderSize !== undefined) push(0x6, s.maxHeaderSize);
  if (s.enableConnectProtocol !== undefined) push(0x8, s.enableConnectProtocol ? 1 : 0);
  if (s.customSettings) {
    for (const kRaw of Object.keys(s.customSettings)) {
      const id = Number(kRaw);
      const v = Number(s.customSettings[kRaw]);
      if (!Number.isInteger(id) || id < 0 || id > 0xffff) throw __settingErr("customSettings", kRaw, false);
      if (!Number.isInteger(v) || v < 0 || v > 0xffffffff) throw __settingErr("customSettings", v, false);
    }
    if (Object.keys(s.customSettings).length > 10) throw __code("ERR_HTTP2_TOO_MANY_CUSTOM_SETTINGS");
    for (const k of Object.keys(s.customSettings).map(Number).sort((a, b) => a - b)) {
      push(k, Number(s.customSettings[String(k)]));
    }
  }
  return Buffer.from(out);
}
export function getUnpackedSettings(buf, options) {
  if (!Buffer.isBuffer(buf) && !(buf instanceof ArrayBuffer) && (!ArrayBuffer.isView(buf) || buf instanceof DataView)) {
    throw __code("ERR_INVALID_ARG_TYPE", "buf", ["Buffer", "TypedArray"], buf);
  }
  const validate = options?.validate === true;
  let u8;
  if (Buffer.isBuffer(buf)) u8 = buf;
  else if (buf instanceof ArrayBuffer) u8 = new Uint8Array(buf);
  else if (buf instanceof Uint8Array) u8 = buf;
  else u8 = Buffer.from(buf); // Uint16Array 等：Buffer.from 元素截断语义（node 同）
  if (u8.length % 6 !== 0) throw __code("ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH");
  const out = {};
  for (let i = 0; i < u8.length; i += 6) {
    const id = (u8[i] << 8) | u8[i + 1];
    const val = ((u8[i + 2] << 24) | (u8[i + 3] << 16) | (u8[i + 4] << 8) | u8[i + 5]) >>> 0;
    switch (id) {
      case 0x1: if (validate) __validateSetting("headerTableSize", val); out.headerTableSize = val; break;
      case 0x2: out.enablePush = val !== 0; break;
      case 0x3: if (validate) __validateSetting("maxConcurrentStreams", val); out.maxConcurrentStreams = val; break;
      case 0x4: if (validate) __validateSetting("initialWindowSize", val); out.initialWindowSize = val; break;
      case 0x5: __validateSetting("maxFrameSize", val); out.maxFrameSize = val; break;
      case 0x6:
        if (validate) __validateSetting("maxHeaderListSize", val);
        out.maxHeaderListSize = val;
        out.maxHeaderSize = val;
        break;
      case 0x8: out.enableConnectProtocol = val !== 0; break;
      default: if (val > 0) out.customSettings = { ...(out.customSettings ?? {}), [String(id)]: val };
    }
  }
  return out;
}
function __validateSetting(name, val) {
  const spec = __SETTING_RANGES[name];
  if (spec !== undefined && spec !== "boolean" && (val < spec[0] || val > spec[1])) {
    throw __settingErr(name, val, false);
  }
}
export const sensitiveHeaders = Symbol("sensitiveHeaders");

export const constants = {
  NGHTTP2_SESSION_SERVER: 0, NGHTTP2_SESSION_CLIENT: 1,
  NGHTTP2_STREAM_STATE_IDLE: 1, NGHTTP2_STREAM_STATE_OPEN: 2,
  NGHTTP2_STREAM_STATE_RESERVED_LOCAL: 3, NGHTTP2_STREAM_STATE_RESERVED_REMOTE: 4,
  NGHTTP2_STREAM_STATE_HALF_CLOSED_LOCAL: 5, NGHTTP2_STREAM_STATE_HALF_CLOSED_REMOTE: 6,
  NGHTTP2_STREAM_STATE_CLOSED: 7,
  NGHTTP2_NO_ERROR: 0x00, NGHTTP2_PROTOCOL_ERROR: 0x01, NGHTTP2_INTERNAL_ERROR: 0x02,
  NGHTTP2_FLOW_CONTROL_ERROR: 0x03, NGHTTP2_SETTINGS_TIMEOUT: 0x04, NGHTTP2_STREAM_CLOSED: 0x05,
  NGHTTP2_FRAME_SIZE_ERROR: 0x06, NGHTTP2_REFUSED_STREAM: 0x07, NGHTTP2_CANCEL: 0x08,
  NGHTTP2_COMPRESSION_ERROR: 0x09, NGHTTP2_CONNECT_ERROR: 0x0a, NGHTTP2_ENHANCE_YOUR_CALM: 0x0b,
  NGHTTP2_INADEQUATE_SECURITY: 0x0c, NGHTTP2_HTTP_1_1_REQUIRED: 0x0d,
  NGHTTP2_ERR_FRAME_SIZE_ERROR: -522, NGHTTP2_ERR_HEADER_COMPRESSION: -502,
  NGHTTP2_ERR_FLOW_CONTROL: -506, NGHTTP2_ERR_START_STREAM_NOT_ALLOWED: -512,
  NGHTTP2_DEFAULT_WEIGHT: 16,
  HTTP2_HEADER_STATUS: ":status", HTTP2_HEADER_METHOD: ":method",
  HTTP2_HEADER_PATH: ":path", HTTP2_HEADER_SCHEME: ":scheme",
  HTTP2_HEADER_AUTHORITY: ":authority",
  HTTP2_HEADER_ACCEPT_CHARSET: "accept-charset", HTTP2_HEADER_ACCEPT_ENCODING: "accept-encoding",
  HTTP2_HEADER_ACCEPT_LANGUAGE: "accept-language", HTTP2_HEADER_ACCEPT_RANGES: "accept-ranges",
  HTTP2_HEADER_ACCEPT: "accept", HTTP2_HEADER_ACCESS_CONTROL_ALLOW_CREDENTIALS: "access-control-allow-credentials",
  HTTP2_HEADER_ACCESS_CONTROL_ALLOW_HEADERS: "access-control-allow-headers",
  HTTP2_HEADER_ACCESS_CONTROL_ALLOW_METHODS: "access-control-allow-methods",
  HTTP2_HEADER_ACCESS_CONTROL_ALLOW_ORIGIN: "access-control-allow-origin",
  HTTP2_HEADER_ACCESS_CONTROL_EXPOSE_HEADERS: "access-control-expose-headers",
  HTTP2_HEADER_ACCESS_CONTROL_MAX_AGE: "access-control-max-age",
  HTTP2_HEADER_ACCESS_CONTROL_REQUEST_HEADERS: "access-control-request-headers",
  HTTP2_HEADER_ACCESS_CONTROL_REQUEST_METHOD: "access-control-request-method",
  HTTP2_HEADER_AGE: "age", HTTP2_HEADER_AUTHORIZATION: "authorization",
  HTTP2_HEADER_CACHE_CONTROL: "cache-control", HTTP2_HEADER_CONTENT_DISPOSITION: "content-disposition",
  HTTP2_HEADER_CONTENT_ENCODING: "content-encoding", HTTP2_HEADER_CONTENT_LANGUAGE: "content-language",
  HTTP2_HEADER_CONTENT_LENGTH: "content-length", HTTP2_HEADER_CONTENT_LOCATION: "content-location",
  HTTP2_HEADER_CONTENT_RANGE: "content-range", HTTP2_HEADER_CONTENT_TYPE: "content-type",
  HTTP2_HEADER_COOKIE: "cookie", HTTP2_HEADER_DATE: "date", HTTP2_HEADER_DNT: "dnt",
  HTTP2_HEADER_ETAG: "etag", HTTP2_HEADER_EXPECT: "expect", HTTP2_HEADER_EXPIRES: "expires",
  HTTP2_HEADER_FORWARDED: "forwarded", HTTP2_HEADER_FROM: "from",
  HTTP2_HEADER_HOST: "host", HTTP2_HEADER_IF_MATCH: "if-match",
  HTTP2_HEADER_IF_MODIFIED_SINCE: "if-modified-since", HTTP2_HEADER_IF_NONE_MATCH: "if-none-match",
  HTTP2_HEADER_IF_RANGE: "if-range", HTTP2_HEADER_IF_UNMODIFIED_SINCE: "if-unmodified-since",
  HTTP2_HEADER_LAST_MODIFIED: "last-modified", HTTP2_HEADER_LINK: "link",
  HTTP2_HEADER_LOCATION: "location", HTTP2_HEADER_MAX_FORWARDS: "max-forwards",
  HTTP2_HEADER_PREFER: "prefer", HTTP2_HEADER_PROXY_AUTHENTICATE: "proxy-authenticate",
  HTTP2_HEADER_PROXY_AUTHORIZATION: "proxy-authorization", HTTP2_HEADER_RANGE: "range",
  HTTP2_HEADER_REFERER: "referer", HTTP2_HEADER_REFRESH: "refresh",
  HTTP2_HEADER_RETRY_AFTER: "retry-after", HTTP2_HEADER_SERVER: "server",
  HTTP2_HEADER_SET_COOKIE: "set-cookie", HTTP2_HEADER_STRICT_TRANSPORT_SECURITY: "strict-transport-security",
  HTTP2_HEADER_TRAILER: "trailer", HTTP2_HEADER_TK: "tk",
  HTTP2_HEADER_UPGRADE_INSECURE_REQUESTS: "upgrade-insecure-requests",
  HTTP2_HEADER_USER_AGENT: "user-agent", HTTP2_HEADER_VARY: "vary",
  HTTP2_HEADER_VIA: "via", HTTP2_HEADER_WARNING: "warning",
  HTTP2_HEADER_WWW_AUTHENTICATE: "www-authenticate", HTTP2_HEADER_X_CONTENT_TYPE_OPTIONS: "x-content-type-options",
  HTTP2_HEADER_X_FRAME_OPTIONS: "x-frame-options",
  HTTP2_HEADER_CONNECTION: "connection", HTTP2_HEADER_UPGRADE: "upgrade",
  HTTP2_HEADER_HTTP2_SETTINGS: "http2-settings", HTTP2_HEADER_TE: "te",
  HTTP2_HEADER_TRANSFER_ENCODING: "transfer-encoding", HTTP2_HEADER_KEEP_ALIVE: "keep-alive",
  HTTP2_HEADER_PROXY_CONNECTION: "proxy-connection",
  HTTP2_METHOD_CONNECT: "CONNECT", HTTP2_METHOD_DELETE: "DELETE", HTTP2_METHOD_GET: "GET",
  HTTP2_METHOD_HEAD: "HEAD", HTTP2_METHOD_MERGE: "MERGE", HTTP2_METHOD_OPTIONS: "OPTIONS",
  HTTP2_METHOD_PATCH: "PATCH", HTTP2_METHOD_POST: "POST", HTTP2_METHOD_PUT: "PUT",
  HTTP2_METHOD_TRACE: "TRACE",
  HTTP_STATUS_CONTINUE: 100, HTTP_STATUS_SWITCHING_PROTOCOLS: 101, HTTP_STATUS_PROCESSING: 102,
  HTTP_STATUS_EARLY_HINTS: 103, HTTP_STATUS_OK: 200, HTTP_STATUS_CREATED: 201,
  HTTP_STATUS_ACCEPTED: 202, HTTP_STATUS_NON_AUTHORITATIVE_INFORMATION: 203,
  HTTP_STATUS_NO_CONTENT: 204, HTTP_STATUS_RESET_CONTENT: 205, HTTP_STATUS_PARTIAL_CONTENT: 206,
  HTTP_STATUS_MULTIPLE_CHOICES: 300, HTTP_STATUS_MOVED_PERMANENTLY: 301, HTTP_STATUS_FOUND: 302,
  HTTP_STATUS_SEE_OTHER: 303, HTTP_STATUS_NOT_MODIFIED: 304, HTTP_STATUS_USE_PROXY: 305,
  HTTP_STATUS_TEMPORARY_REDIRECT: 307, HTTP_STATUS_PERMANENT_REDIRECT: 308,
  HTTP_STATUS_BAD_REQUEST: 400, HTTP_STATUS_UNAUTHORIZED: 401, HTTP_STATUS_PAYMENT_REQUIRED: 402,
  HTTP_STATUS_FORBIDDEN: 403, HTTP_STATUS_NOT_FOUND: 404, HTTP_STATUS_METHOD_NOT_ALLOWED: 405,
  HTTP_STATUS_NOT_ACCEPTABLE: 406, HTTP_STATUS_PROXY_AUTHENTICATION_REQUIRED: 407,
  HTTP_STATUS_REQUEST_TIMEOUT: 408, HTTP_STATUS_CONFLICT: 409, HTTP_STATUS_GONE: 410,
  HTTP_STATUS_LENGTH_REQUIRED: 411, HTTP_STATUS_PRECONDITION_FAILED: 412,
  HTTP_STATUS_REQUEST_ENTITY_TOO_LARGE: 413, HTTP_STATUS_REQUEST_URI_TOO_LONG: 414,
  HTTP_STATUS_UNSUPPORTED_MEDIA_TYPE: 415, HTTP_STATUS_REQUESTED_RANGE_NOT_SATISFIABLE: 416,
  HTTP_STATUS_EXPECTATION_FAILED: 417, HTTP_STATUS_IM_A_TEAPOT: 418,
  HTTP_STATUS_MISDIRECTED_REQUEST: 421, HTTP_STATUS_UNPROCESSABLE_ENTITY: 422,
  HTTP_STATUS_LOCKED: 423, HTTP_STATUS_FAILED_DEPENDENCY: 424, HTTP_STATUS_TOO_EARLY: 425,
  HTTP_STATUS_UPGRADE_REQUIRED: 426, HTTP_STATUS_PRECONDITION_REQUIRED: 428,
  HTTP_STATUS_TOO_MANY_REQUESTS: 429, HTTP_STATUS_REQUEST_HEADERS_FIELDS_TOO_LARGE: 431,
  HTTP_STATUS_UNAVAILABLE_FOR_LEGAL_REASONS: 451, HTTP_STATUS_INTERNAL_SERVER_ERROR: 500,
  HTTP_STATUS_METHOD_NOT_IMPLEMENTED: 501, HTTP_STATUS_BAD_GATEWAY: 502,
  HTTP_STATUS_SERVICE_UNAVAILABLE: 503, HTTP_STATUS_GATEWAY_TIMEOUT: 504,
  HTTP_STATUS_HTTP_VERSION_NOT_SUPPORTED: 505, HTTP_STATUS_VARIANT_ALSO_NEGOTIATES: 506,
  HTTP_STATUS_INSUFFICIENT_STORAGE: 507, HTTP_STATUS_LOOP_DETECTED: 508,
  HTTP_STATUS_BANDWIDTH_LIMIT_EXCEEDED: 509, HTTP_STATUS_NOT_EXTENDED: 510,
  HTTP_STATUS_NETWORK_AUTHENTICATION_REQUIRED: 511,
};
const __api = {
  createServer, createSecureServer, connect, constants,
  getDefaultSettings, getPackedSettings, getUnpackedSettings, sensitiveHeaders,
  Http2ServerRequest, Http2ServerResponse,
};
// 真机 http2 顶层导出（26.8.2 实测）：Http2ServerRequest/Response 为具名导出
export { Http2ServerRequest, Http2ServerResponse };
export default __api;
