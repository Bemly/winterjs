    // node 口径（mutable-headers 套件逐字）：无名 → ERR_INVALID_HTTP_TOKEN
    // 'Header name must be a valid HTTP token ["undefined"]'；无值 →
    // ERR_HTTP_INVALID_HEADER_VALUE 'Invalid value "undefined" for header …'；
    // 值原样存（number/array 不转串，content-length/set-cookie 套件）。
    setHeader(name, value) {
      // node 口径：发头后改头即 ERR_HTTP_HEADERS_SENT（multiple-headers 套件；
      // 无参亦先判此门，真机实测）。
      if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("set");
      if (typeof name !== "string") throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
      if (!__TOKEN_RE.test(name)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", name);
      if (value === undefined) throw new codes.ERR_HTTP_INVALID_HEADER_VALUE("undefined", String(name));
      const lk = String(name).toLowerCase();
      if (this._removedHeader !== undefined) delete this._removedHeader[lk];
      // node 口径：数组值按多行发出（dont-set-default 套件 foo 双行）；
      // 用户拼写首写优先（multiple-headers 套件全 'X-Req-a'）。
      // 构造期数组形（__headerList 在场）下 setHeader 原位替换同键对——
      // 真机 wire 探针：构造期对占位，后调 set 即原位顶替。
      const __vals = Array.isArray(value) ? [...value] : [value];
      for (const __e of __vals) __checkOutboundHeaderValue(this.__validation, __e, String(name));
      if (this.__headerList !== null && this.__headerList !== undefined) {
        const __nl = [];
        let __placed = false;
        for (const [__k, __v] of this.__headerList) {
          if (String(__k).toLowerCase() === lk) {
            if (!__placed) {
              __placed = true;
              for (const __e of __vals) __nl.push([String(name), __e]);
            }
          } else {
            __nl.push([__k, __v]);
          }
        }
        if (!__placed) for (const __e of __vals) __nl.push([String(name), __e]);
        this.__headerList = __nl;
        this.__headers[lk] = Array.isArray(value) ? [...value] : value;
      } else if (Array.isArray(value)) {
        this.__headers[lk] = [...value];
      } else {
        this.__headers[lk] = value;
      }
      if ((this.__headerNames ??= {})[lk] === undefined) this.__headerNames[lk] = String(name);
      if (lk === "connection") this.__autoConn = false;
      // 用户显式改 Host 即按原文发（wire 不补端口；自动 Host 的补端口旗失效）。
      if (lk === "host") this.__hostBarePort = null;
      return this;
    }
    // node OutgoingMessage.appendHeader（header-value-relaxed 套件点名）。
    // node 口径：数组值按元素追加（multiple-headers 套件 [BBB CCC] 形）；
    // 缺省追加为单元素（真机 new+s/a+s 探针）。发头后即 ERR_HTTP_HEADERS_SENT。
    appendHeader(name, value) {
      if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("append");
      if (typeof name !== "string") throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
      if (!__TOKEN_RE.test(name)) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", name);
      const lk = String(name).toLowerCase();
      const __vals = Array.isArray(value) ? [...value] : [value];
      for (const __e of __vals) __checkOutboundHeaderValue(this.__validation, __e, String(name));
      // 构造期数组形下 append 即尾部并入有序对（真机探针：既有行保留原位）。
      if (this.__headerList !== null && this.__headerList !== undefined) {
        for (const __e of __vals) this.__headerList.push([String(name), __e]);
      }
      // 查取表镜像：缺省单元素、数组恒数组（真机 new+s/a+s 探针）。
      const cur = this.__headers[lk];
      if (cur === undefined) {
        this.__headers[lk] = Array.isArray(value) ? [...value] : value;
      } else if (Array.isArray(cur)) {
        for (const __e of __vals) cur.push(__e);
      } else {
        this.__headers[lk] = Array.isArray(value) ? [cur, ...value] : [cur, value];
      }
      if ((this.__headerNames ??= {})[lk] === undefined) this.__headerNames[lk] = String(name);
      if (lk === "connection") this.__autoConn = false;
      if (lk === "host") this.__hostBarePort = null;
      return this;
    }
    // node 口径：getHeader 原样回（数组不 join；multiple-headers 套件）。
    getHeader(name) {
      if (typeof name !== "string") throw new codes.ERR_INVALID_ARG_TYPE("name", "string", name);
      return this.__headers[name.toLowerCase()];
    }
    removeHeader(name) {
      // node 口径：发头后即 ERR_HTTP_HEADERS_SENT（remove-header-after-sent 套件）。
      if (this.headersSent) throw new codes.ERR_HTTP_HEADERS_SENT("remove");
      if (typeof name !== "string") throw new codes.ERR_INVALID_ARG_TYPE("name", "string", name);
      const lk = name.toLowerCase();
      delete this.__headers[lk];
      if (this.__headerNames !== undefined) delete this.__headerNames[lk];
      if (this._removedHeader !== undefined) this._removedHeader[lk] = true;
      return this;
    }
    hasHeader(name) {
      if (typeof name !== "string") throw new codes.ERR_INVALID_ARG_TYPE("name", "string", name);
      return this.__headers[name.toLowerCase()] !== undefined;
    }
    getHeaderNames() { return Object.keys(this.__headers); }
    // node 口径（mutable-headers 套件）：getHeaders 回 null 原型拷贝；
    // getRawHeaderNames 回用户原拼写数组。
    getHeaders() {
      const __out = Object.create(null);
      for (const k of Object.keys(this.__headers)) __out[k] = this.__headers[k];
      return __out;
    }
    getRawHeaderNames() {
      const __names = this.__headerNames ?? {};
      return Object.keys(this.__headers).map((k) => __names[k] ?? k);
    }
    getPort() { return this.__port; }
    getHost() { return this.host; }
    // node 口径：连接后落到 socket；未连接先存 pending（deferToConnect 语义）。
    setNoDelay(noDelay) {
      this.__pendingNoDelay = noDelay ?? true;
      if (this.__sock !== null && this.__sock !== undefined && typeof this.__sock.setNoDelay === "function") {
        try { this.__sock.setNoDelay(this.__pendingNoDelay); } catch { /* gone */ }
      }
      return this;
    }
    setSocketKeepAlive(enable, initialDelay) {
      this.__pendingKeepAlive = [enable ?? true, initialDelay ?? 0];
      if (this.__sock !== null && this.__sock !== undefined && typeof this.__sock.setKeepAlive === "function") {
        try { this.__sock.setKeepAlive(enable ?? true, initialDelay ?? 0); } catch { /* gone */ }
      }
      return this;
    }
    // socket 空闲计时（http 层驱动时补记 .timeout + 'onTimeout' 占位监听——
    // 对齐 node net 内部监听形状：listeners('timeout') = [onTimeout,
    // emitRequestTimeout, ...]；onTimeout 是 socket 单例（node net 构造时挂一
    // 次，setTimeout 重臂不重复挂）；net 域 setTimeout 不发布 .timeout，偏差记档）。
    __applySockTimeout(sock, ms) {
      if (typeof sock.setTimeout !== "function") return;
      sock.setTimeout(ms);
      sock.timeout = ms;
      if (!sock.__onTimeoutSingleton) {
        sock.__onTimeoutSingleton = true;
        sock.on("timeout", function onTimeout() {});
      }
    }
    // node lib/_http_client.js setSocketTimeout 口径：connecting 期 defer 到
    // 'connect'（client-set-timeout 套件：'socket' 事件时仍见构造期 2000，
    // 'connect' 后才见 setTimeout 的 1000）。
    __deferSockTimeout(sock, ms) {
      if (sock.connecting) {
        try { sock.once("connect", () => this.__applySockTimeout(sock, ms)); } catch { /* gone */ }
      } else {
        this.__applySockTimeout(sock, ms);
      }
    }
    // node lib/_http_client.js：once('timeout') + socket 空闲计时（已连即臂，
    // 未连记位，__attach 落地）。setTimeout 必须补建 timeoutCb，否则 __attach
    // 见 timeoutCb 缺席即跳过武装（client-timeout 套件 hang 根因）。
    // 响应结束后调即 noop（set-timeout-after-end 套件：res 'end' 后
    // setTimeout(0) 不增监听，node `if (this._ended) return this` 口径；
    // _ended 置于响应结束（responseOnEnd），请求 finish 不算——响应中
    // setTimeout 必须生效（client-timeout-with-data 套件）。
    setTimeout(msecs, callback) {
      if (this.__res !== null && this.__res !== undefined && this.__res.readableEnded) return this;
      // getTimerDuration（node lib/internal/timers.js）：同步校验 + 溢出告警（告警栈含
      // 调用点，timeout-client-warning 套件点名；落到 socket 再告警即丢调用栈）。
      if (typeof msecs !== "number") throw new codes.ERR_INVALID_ARG_TYPE("msecs", "number", msecs);
      if (msecs < 0 || !Number.isFinite(msecs)) {
        throw new codes.ERR_OUT_OF_RANGE("msecs", "a non-negative finite number", msecs);
      }
      if (msecs > 2147483647) {
        process.emitWarning(`${msecs} does not fit into a 32-bit signed integer.` +
          "\nTimer duration was truncated to 2147483647.", "TimeoutOverflowWarning");
        msecs = 2147483647;
      }
      if (typeof callback === "function") this.once("timeout", callback);
      const ms = Number(msecs) || 0;
      this.__reqTimeoutMs = ms > 0 ? ms : undefined;
      if (ms > 0 && this.timeoutCb === undefined) {
        this.timeoutCb = () => this.emit("timeout");
      }
      if (this.__sock !== null && this.__sock !== undefined) {
        if (ms > 0) {
          this.__deferSockTimeout(this.__sock, ms);
          // 已 attach 后调 setTimeout：补挂转发（__attach 只在 attach 时挂一次，
          // 去重经 __lastTimeoutCb，与 __attach 同口径）。
          if (this.timeoutCb !== undefined) {
            const sock = this.__sock;
            if (sock.__lastTimeoutCb !== undefined && sock.__lastTimeoutCb !== this.timeoutCb) {
              try { sock.removeListener("timeout", sock.__lastTimeoutCb); } catch { /* gone */ }
            }
            if (sock.__lastTimeoutCb !== this.timeoutCb) {
              try { sock.once("timeout", this.timeoutCb); } catch { /* gone */ }
              sock.__lastTimeoutCb = this.timeoutCb;
            }
          }
        } else if (typeof this.__sock.setTimeout === "function") {
          this.__sock.setTimeout(0);
          this.__sock.timeout = 0;
        }
      }
      return this;
    }
    clearTimeout(cb) { return this.setTimeout(0, cb); }
    // 立即发头（node flushHeaders：_implicitHeader + 强制刷）。
    flushHeaders() {
      if (this.__headSent) return;
      this.__headerStored = true;
      if (this.__connected && this.__sock !== null && this.__sock !== undefined && !this.destroyed) {
        if (this.__buf1 !== null && this.__headers["content-length"] === undefined &&
            this.__headers["transfer-encoding"] === undefined) {
          this.__chunked = this.__chunkDefault;
        }
        this.__sendHead();
        if (this.__buf1 !== null) {
          const q = this.__buf1;
          this.__buf1 = null;
          for (const b of q) this.__frame(b);
        }
      } else {
        this.__forceHead = true;
      }
    }
    write(chunk, encoding, cb) {
      if (this.__userEnded) return __writeAfterEnd(this, encoding, cb);
      return super.write(chunk, encoding, cb);
    }
    end(chunk, encoding, cb) {
      // Node OutgoingMessage.end 口径（见服务端同改）：finished/ending 状态的
      // 重复 end 不经基类错误通道（不毒化 errored）。
      if (typeof chunk === "function") { cb = chunk; chunk = null; encoding = null; }
      else if (typeof encoding === "function") { cb = encoding; encoding = null; }
      const __hasChunk = chunk !== undefined && chunk !== null;
      const __cb = typeof cb === "function" ? cb : null;
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
      if (this.destroyed) {
        // 已销毁（abort 后 end）：node 口径不抛（abort-before-end 套件
        // req.on('error') mustNotCall）；回调按空转处理。
        const f = typeof chunk === "function" ? chunk : (typeof encoding === "function" ? encoding : cb);
        if (typeof f === "function") f();
        return this;
      }
      // CL 快路径判据：end 是首个头触发点（此前无 write）。TE 在场（含数组
      // 形有序对——查取表看不见它；multiple-headers 套件 CL:0 伪影）即无 CL 快捷。
      // node 口径：end() 返回后头即算发出（服务端同改）。
      this.headersSent = true;
      this.__endFast = !this.__sawWrite;
      this.__userEnded = true;
      if (this.__headers["transfer-encoding"] !== undefined ||
          (this.__headerList !== null && this.__headerList !== undefined &&
           this.__headerList.some(([k]) => String(k).toLowerCase() === "transfer-encoding"))) {
        this.__endFast = false;
      }
      // node maybePrepareFinalChunk 口径：end(data) 为首个头触发点时
      // _contentLength 即刻落定（UCED 方法族；GET 族无 CL——framing 顺序）。
      if (this.__endFast && !this.__headSent && !this.__headerStored &&
          this.__chunkDefault &&
          this.__headers["content-length"] === undefined &&
          this.__headers["transfer-encoding"] === undefined) {
        let __len = 0;
        if (chunk !== undefined && chunk !== null && typeof chunk !== "function") {
          if (typeof chunk === "string") {
            __len = globalThis.Buffer !== undefined && typeof globalThis.Buffer.byteLength === "function"
              ? globalThis.Buffer.byteLength(chunk, typeof encoding === "string" ? encoding : "utf8")
              : new TextEncoder().encode(chunk).length;
          } else if (chunk.byteLength !== undefined) {
            __len = chunk.byteLength;
          }
        }
        this.__contentLength = __len;
      }
      try {
        return super.end(chunk, encoding, cb);
      } catch (e) {
        // 基类校验抛不得毒化旗位（见服务端同改）。
        this.__userEnded = false;
        this.__endFast = false;
        this.__contentLength = undefined;
        throw e;
      }
    }
    // node 口径（弃用面仍测）：abort = destroy + 'abort' 事件 + aborted 旗；
    // 在途响应同步走 aborted 级联（aborted 套件：res aborted → error → close）。
    // 'abort' 事件经 nextTick（abort-stream-end 套件：调用方 abort() 后同步重置
    // 状态；且真机序 abort 先于 error——destroy 的 error 同为 tick，按序排后）。
    // aborted 旗保持同步。
    abort() {
      if (this.destroyed) return;
      this.__aborted = true;
      if (this.__res !== null && this.__res !== undefined && !this.__res.complete &&
          typeof this.__res.__abortWithError === "function") {
        try { this.__res.__abortWithError(); } catch { /* 监听抛错不阻销毁 */ }
      }
      process.nextTick(() => { try { this.emit("abort"); } catch { /* 监听抛错不阻收尾 */ } });
      this.destroy();
    }
    get aborted() { return this.__aborted === true; }
    // node 口径 req.res（destroyed-socket-write2 套件直读）：响应未到即 null。
    get res() { return this.__res ?? null; }
    __sendHead(box) {
      if (this.__headSent) return;
      this.__headSent = true;
      this.headersSent = true;
      // node _storeHeader TE 扫描口径：用户 TE 值含 chunked（大小写不敏感，
      // 数组形 join 后判）即 chunked 帧——与 UCED 族（__chunkDefault）无关
      //（raw-headers 套件 GET+'CHUNKED' 形：真机体走 chunked 帧，非裸体）。
      const __tev = this.__headers["transfer-encoding"];
      const __teText = Array.isArray(__tev) ? __tev.join(", ") : (__tev ?? "");
      if (__teText !== "" && /(?:^|\W)chunked/i.test(__teText)) this.__chunked = true;
      const head = [`${this.method} ${this.path} HTTP/1.1`];
      if (this.__headers["content-length"] !== undefined) {
        this.__rawCL = true;
      } else if (this.__chunked && this.__headers["transfer-encoding"] === undefined) {
        // 用户显式 TE 原样保留（raw-headers 套件 'CHUNKED' 大小写；仅缺席才补）。
        this.__headers["transfer-encoding"] = "chunked";
      }
      // 自动头规范大写（真机口径）；自设头按用户拼写（__headerNames；
      // dont-set-default 套件 'HOST' 原样），数组值逐行发出。
      const canon = { "transfer-encoding": "Transfer-Encoding", "content-length": "Content-Length" };
      const __names = this.__headerNames ?? {};
      // node 口径 uniqueHeaders：名单内头 wire 用 '; ' 合并单行（multiple-
      // headers 套件；distinct 同收单元素）。
      const __uniq = this.__uniqueHeaders;
      const __emitOne = (k, v) => {
        // node 口径：自动 Host 在 wire 补 `:port`（存储省缺省端口；batch5
        // `foo:1234:80`；用户显式改 Host 即原文，__hostBarePort 已失效）。
        if (k === "host" && this.__hostBarePort !== null && this.__hostBarePort !== undefined) {
          const __n = __names[k] ?? (canon[k] ?? k);
          head.push(`${__n}: ${v}:${this.__hostBarePort}`);
          return;
        }
        if (Array.isArray(v)) {
          const __n = __names[k] ?? (canon[k] ?? k);
          // cookie 数组恒单行 '; ' 合并（真机 26.8.2 实测，双端同）。
          if (k === "cookie") {
            head.push(`${__n}: ${v.join("; ")}`);
            return;
          }
          if (Array.isArray(__uniq) && __uniq.includes(k)) {
            head.push(`${__n}: ${v.join("; ")}`);
            return;
          }
          for (const __e of v) head.push(`${__n}: ${__e}`);
          return;
        }
        let name;
        if (__names[k] !== undefined) name = __names[k];
        else if (k === "connection" && this.__autoConn) name = "Connection";
        else name = canon[k] ?? k;
        head.push(`${name}: ${v}`);
      };
      // 数组形 headers：有序对原样发出（含 dupes；dont-set-default 套件），
      // 对象侧自动头（host/connection）缺位即补。自动 connection 存旁路
      //（__autoConnVal，header 面不可见），用户未覆写即补发。
      if (this.__headerList !== null && this.__headerList !== undefined) {
        const __seen = new Set(this.__headerList.map(([k]) => String(k).toLowerCase()));
        // node 口径 uniqueHeaders：名单内头合并单行 '; '（multiple-headers
        // 套件；首现位置发出，后续同键跳过——与 __emitOne 同口径）。
        const __uniq = this.__uniqueHeaders;
        const __merged = new Set();
        for (const [k, v] of this.__headerList) {
          const __lk = String(k).toLowerCase();
          if (Array.isArray(__uniq) && __uniq.includes(__lk)) {
            if (__merged.has(__lk)) continue;
            __merged.add(__lk);
            const __all = this.__headerList.filter(([k2]) => String(k2).toLowerCase() === __lk).map(([, v2]) => v2);
            head.push(`${k}: ${__all.join("; ")}`);
            continue;
          }
          head.push(`${k}: ${v}`);
        }
        for (const [k, v] of Object.entries(this.__headers)) {
          if (!__seen.has(k)) __emitOne(k, v);
        }
        if (this.__autoConn && this.__headers.connection === undefined && !__seen.has("connection") &&
            !(this._removedHeader !== undefined && this._removedHeader.connection)) {
          head.push(`Connection: ${this.__autoConnVal}`);
        }
      } else {
        for (const [k, v] of Object.entries(this.__headers)) __emitOne(k, v);
        if (this.__autoConn && this.__headers.connection === undefined &&
            !(this._removedHeader !== undefined && this._removedHeader.connection)) {
          head.push(`Connection: ${this.__autoConnVal}`);
        }
      }
      // 头写同样计数确认（box 为空沿旧路）。
      if (box === null || box === undefined) {
        this.__sock.write(__latin1Bytes(head.join("\r\n") + "\r\n\r\n"));
      } else {
        box.pend++;
        try { this.__sock.write(__latin1Bytes(head.join("\r\n") + "\r\n\r\n"), box.cap); } catch (e) { try { box.cap(e); } catch {} }
      }
    }
    // socket 写错收集器（writable-finished 套件）：确认计数（pend）+ 首错
    //（err）+ 迟到确认等待表（waiters）。收尾经顶层 __afterSockFlush
    //（微任务拍，mock 异步确认可达；成功路回调值恒 undefined）。
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
    // 计数写（box 为空即沿旧路无回调，零行为差）。
    __w(data, box) {
      if (box === null || box === undefined) {
        try { this.__sock.write(data); } catch { /* 落盘永不阻收尾 */ }
        return;
      }
      box.pend++;
      try { this.__sock.write(data, box.cap); } catch (e) { try { box.cap(e); } catch {} }
    }
    // 暂存写回调排空（holdback 期写回调随落盘结果递送，node _write 经
    // socket.write 透传口径；成功路恒 undefined，与旧 microtaskcb 同值）。
    // 出错由调用方显式递送同一 err 对象后 destroy() 收尾（destroy 不复发
    // error——流机构 destroy 不碰在途回调，见 destroy.rs；带 err destroy 会
    // 二次 emitError，mustCall(1) 形必红——实锤坑）。
    __fireFlushCbs(err) {
      const q = this.__flushCbs;
      this.__flushCbs = [];
      if (q !== null && q !== undefined) {
        for (const f of q) { try { f(err ?? undefined); } catch { /* 回调抛错不阻排空 */ } }
      }
    }
    // 落盘出错统一收尾（writable-finished 套件）：显式递送同一 err（暂存写
    // 回调 + 终结回调，用户侧 strictEqual 同一性）→ destroy() 收尾。顺序是
    // 语义：先递送（首个 err 触发唯一 error 事件），再 destroy 置位——后到
    // 的 socket 'error' 被 destroyed 门吞掉（否则同一失败报两次；二次 emit
    // 皆因递送后未置位，实锤坑）。destroy() 不带 err（不复发 error，只做
    // close + socket 清理；带 err 会二次 emitError，mustCall(1) 形必红）。
    __failFlush(err, finalCb) {
      this.__fireFlushCbs(err);
      try { this.destroy(); } catch {}
      if (typeof finalCb === "function") { try { finalCb(err); } catch {} }
    }
    // 落盘收尾统一出口：确认计数等待（mock 异步错可达）后，结果递送——
    // 出错显式递送同一 err + destroy() 收尾，成功排空 + 终结回调。
    __finishFlush(box, finalCb) {
      __afterSockFlush(this, box, (err) => {
        if (err !== null && err !== undefined) { this.__failFlush(err, finalCb); return; }
        this.__fireFlushCbs(undefined);
        if (typeof finalCb === "function") {
          try { finalCb(undefined); } catch {}
        }
      });
    }
    __frame(u8, box) {
      if (u8.length === 0) return;
      // node 口径：socket 写经计数确认（writable-finished 套件；box 为空沿旧路）。
      if (this.__chunked && !this.__rawCL) {
        const hex = new TextEncoder().encode(u8.length.toString(16) + "\r\n");
        this.__w(__concat(hex, __concat(u8, new TextEncoder().encode("\r\n"))), box);
      } else {
        this.__w(u8, box);
      }
    }
    // 连接就绪或刷盘时机到：头恒发（node _flush 口径）；有体位时按 UCED 定
    // chunked。队列逐帧发出（不合并）：保 TCP 分包（blk09 计数型套件依赖）。
    __tryFlush() {
      if (!this.__connected || this.__sock === null || this.__headSent) return;
      if (this.destroyed) return;
      // 终结已暂存（_final 先到）：走完整收尾（含 chunked 终结块），不可只刷头
      //（writable-finished 套件 end-again 形；收口经 __finishFlush）。
      if (this.__finalStashed === true) {
        const __fb = this.__flushFinal();
        this.__finalStashed = false;
        const __fcb = this.__pendingFinalCb;
        this.__pendingFinalCb = null;
        this.__finishFlush(__fb, __fcb);
        return;
      }
      if (this.__buf1 !== null) {
        if (this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined) {
          this.__chunked = this.__chunkDefault;
        }
      } else if (this.__chunkDefault && this.__headers["content-length"] === undefined &&
                 this.__headers["transfer-encoding"] === undefined) {
        // 无体 UCED 请求（含 Expect: 100-continue 等 continue 的形态）：chunked 起拍。
        this.__chunked = true;
      }
      this.__forceHead = false;
      this.__headerStored = true;
      // node 口径：end(data) 已落定 CL 即走捷径（content-length 套件 end-with-
      // data/empty 形；_write holdback 暂存使 _final 滞后，timer 先刷时同样
      // 认 CL——否则恒 chunked）。TE 在场/已设 CL 即不动。
      if (this.__contentLength !== undefined &&
          this.__headers["content-length"] === undefined &&
          this.__headers["transfer-encoding"] === undefined) {
        this.__headers["content-length"] = String(this.__contentLength);
        this.__rawCL = true;
      }
      const __box = this.__sockCap();
      this.__sendHead(__box);
      const q = this.__buf1;
      this.__buf1 = null;
      if (q !== null) for (const b of q) this.__frame(b, __box);
      // holdback 暂存写回调随落盘结果递送（确认计数等待，mock 异步错可达）。
      __afterSockFlush(this, __box, (err) => this.__fireFlushCbs(err));
    }
    _write(chunk, encoding, cb) {
      const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk));
      this.__sawWrite = true;
      // node 口径：首个 write 即算发头（服务端同改；holdback 只延迟落盘）。
      this.headersSent = true;
      if (this.__buf1 === null && !this.__headSent) {
        this.__buf1 = [u8];
        this.__holdTimer = setTimeout(() => {
          this.__holdTimer = null;
          if (this.__buf1 !== null && !this.__headSent && !this.destroyed) this.__tryFlush();
        }, 0);
        // node 口径：holdback 期写回调随落盘结果递送（writable-finished 套件
        // mock 失败形；成功路落盘后 microtask 回 null，与旧节奏同值不同拍——
        // 旧 microtask 即回，现有落盘才回；无回调形零可观测差）。
        (this.__flushCbs ??= []).push(cb);
        return;
      }
      if (!this.__headSent) {
        // 同拍内第二次写：必为流式，同步刷头。
        if (this.__connected && this.__sock !== null && !this.destroyed) {
          if (this.__chunkDefault) this.__chunked = true;
          const __box = this.__sockCap();
          this.__sendHead(__box);
          if (this.__buf1 !== null) {
            const q = this.__buf1;
            this.__buf1 = null;
            for (const b of q) this.__frame(b, __box);
          }
          if (!this.__chunked && !this.__rawCL && this.__chunkDefault) this.__chunked = true;
          this.__frame(u8, __box);
          __afterSockFlush(this, __box, (err) => {
            // 出错显式递送同一 err + destroy() 收尾（单次 error 事件，见
            // __failFlush 注）；成功即排空暂存 + 本回调（undefined）。
            if (err !== null && err !== undefined) { this.__failFlush(err, (e) => cb(e)); return; }
            this.__fireFlushCbs(undefined);
            cb(undefined);
          });
          return;
        } else {
          // 未连通：逐块排队（连通后逐帧刷出保分包，见 __tryFlush）。
          this.__buf1.push(u8);
          queueMicrotask(cb);
          return;
        }
      }
      if (this.__connected && this.__sock !== null && !this.destroyed) {
        if (!this.__chunked && !this.__rawCL && this.__chunkDefault) this.__chunked = true;
        const __box = this.__sockCap();
        this.__frame(u8, __box);
        __afterSockFlush(this, __box, (err) => {
          if (err !== null && err !== undefined) { this.__failFlush(err, (e) => cb(e)); return; }
          cb(undefined);
        });
        return;
      } else {
        (this.__buf1 ??= []).push(u8);
      }
      queueMicrotask(cb);
    }
    _final(cb) {
      if (this.__holdTimer !== null) {
        clearTimeout(this.__holdTimer);
        this.__holdTimer = null;
      }
      if (this.destroyed) {
        // node 口径：已销毁（多为先前写错）时 end 回调带已记错误
        //（writable-finished 套件 end-again 形）；无错即沿旧路 null。
        const __de = this._writableState !== null && this._writableState !== undefined ? this._writableState.errored : null;
        cb(__de ?? undefined);
        return;
      }
      // node 口径：用户显式 TE: chunked 即 chunked 帧（空体亦发终结块；
      // mh58：TE 在场无终结即对端永等）。数组形有序对里的 TE 同算在场。
      if (!this.__chunked && !this.__rawCL && this.__hasUserTEChunked()) {
        this.__chunked = true;
      }
      if (!this.__headSent) {
        if (!this.__connected) {
          // 连接未就绪：连通后一次性发出（CL 快捷；见 connect 回调）。
          // holdback 在途（写后即 end）时终结回调随落盘结果递送（writable-
          // finished 套件）；纯裸 end（无写在途）沿旧路即时回（timing 零改）。
          this.__pendingFinal = true;
          if (this.__buf1 !== null) {
            this.__finalStashed = true;
            this.__pendingFinalCb = cb;
            return;
          }
          cb();
          return;
        }
        // holdback 在途即就地收尾（end 已到无需再等一拍；CL 快捷判定即时）。
        // 结果经统一出口（微任务拍读数，mock 异步错可达）。
        this.__finishFlush(this.__flushFinal(), cb);
        this.__finalStashed = false;
        this.__pendingFinalCb = null;
      } else if (this.__chunked && !this.__rawCL) {
        if (this.__connected && this.__sock !== null) {
          const __box = this.__sockCap();
          this.__w(new TextEncoder().encode("0\r\n" + (this.__trailer ?? "") + "\r\n"), __box)
          this.__finishFlush(__box, cb);
        } else {
          cb();
        }
      } else {
        cb();
      }
    }
    // 收尾刷新（调用方保证已连通）：end(data) 为首个头触发点且方法允许体时
    // 走 CL 快捷（合并发出，真机单 write 口径）；write/flushHeaders 在前 →
    // chunked 逐帧（真机 per-write 帧口径）；GET/HEAD 族 → 无 CL/TE 裸体。
    // 用户显式 TE: chunked 判定（含数组形有序对；三处帧决策共用）。
    __hasUserTEChunked() {
      if (/(?:^|\W)chunked/i.test(String(this.__headers["transfer-encoding"] ?? ""))) return true;
      const __hl = this.__headerList;
      if (__hl !== null && __hl !== undefined) {
        for (const [k, v] of __hl) {
          if (String(k).toLowerCase() === "transfer-encoding" && /(?:^|\W)chunked/i.test(String(v ?? ""))) return true;
        }
      }
      return false;
    }
    // 收尾刷新（调用方保证已连通）：回首个同步 socket 写错（无则 null），
    // 暂存写回调随结果排空（writable-finished 套件）。
    __flushFinal() {
      if (this.__sock === null || this.destroyed) return null;
      // 用户显式 TE: chunked 即 chunked 帧（_final 同款归一；pendingFinal
      // 路径直达此处，绕过 _final 入口）。
      if (!this.__chunked && !this.__rawCL && this.__hasUserTEChunked()) {
        this.__chunked = true;
      }
      const __box = this.__sockCap();
      if (!this.__headSent) {
        // CL 决策已在 end() 落定（node _contentLength 口径）；无 CL 的 UCED
        // 请求 chunked；GET 族（UCED false）无 CL/TE 裸体。数组形有序对里的
        // TE 同样算在场（multiple-headers 套件）。
        const __hasTE = this.__headers["transfer-encoding"] !== undefined ||
          (this.__headerList !== null && this.__headerList !== undefined &&
           this.__headerList.some(([k]) => String(k).toLowerCase() === "transfer-encoding"));
        if (this.__contentLength !== undefined) {
          this.__headers["content-length"] = String(this.__contentLength);
          this.__rawCL = true;
        } else if (this.__chunkDefault && this.__headers["content-length"] === undefined &&
                   !__hasTE) {
          this.__chunked = true;
          this.__headers["transfer-encoding"] = "chunked";
        }
        this.__sendHead(__box);
        if (this.__buf1 !== null) {
          const q = this.__buf1;
          this.__buf1 = null;
          if (this.__chunked && !this.__rawCL) {
            for (const b of q) this.__frame(b, __box);
          } else {
            this.__frame(__join(q), __box);
          }
        }
        // chunked 收尾终结块（服务端 _final 同款口径）。此前缺失——connect 前
        // write+end 的 POST 走 chunked，服务端 req 'end' 永不触发（loopback
        // 黑盒挂死实录；head 已发路径本就有此写入，两路对齐）。
        // addTrailers 的 trailer 跟终结块（multiple-headers 套件）。
        if (this.__chunked && !this.__rawCL) {
          this.__w(new TextEncoder().encode("0\r\n" + (this.__trailer ?? "") + "\r\n"), __box)
        }
      } else if (this.__chunked && !this.__rawCL) {
        this.__w(new TextEncoder().encode("0\r\n" + (this.__trailer ?? "") + "\r\n"), __box)
      }
      // 排空与终结回调由调用方经 __finishFlush 统一出口（微任务拍读数）。
      return __box;
    }
    _destroy(err, cb) {
      if (this.__holdTimer !== null) {
        clearTimeout(this.__holdTimer);
        this.__holdTimer = null;
      }
      // signal 监听收尾（abort 触发或正常结束皆摘，不泄漏）。
      if (this.__sigCleanup !== undefined && this.__sigCleanup !== null) {
        try { this.__sigCleanup(); } catch { /* gone */ }
        this.__sigCleanup = null;
      }
      if (this.agent !== null && this.agent !== undefined) this.agent.__cancel(this);
      // node 口径：请求 error 即摘 socket data/end 请求级监听（agent 的
      // onReadableStreamEnd 保留；client-parse-error 套件 data=0/end=1）。
      // 注意走 this.socket（__sock 在 __finishResponse 即抽空，见 949 行）。
      // 正常 destroy（无 err）不动。
      if (err !== undefined && err !== null && this.socket !== null && this.socket !== undefined) {
        try {
          const __s = this.socket;
          if (__s.__reqSockOnData !== undefined) {
            try { __s.removeListener("data", __s.__reqSockOnData); } catch { /* gone */ }
            __s.__reqSockOnData = undefined;
          }
          if (__s.__reqSockOnEnd !== undefined) {
            try { __s.removeListener("end", __s.__reqSockOnEnd); } catch { /* gone */ }
            __s.__reqSockOnEnd = undefined;
          }
        } catch { /* gone */ }
      }
      // 响应同销（Node：destroy 中止整个事务；否则 res 永不完结，
      // 挂在它上面的收尾——如 server.close()——永不到）。
      // 构造期同步 destroy（预 abort signal）下 __res 尚 undefined，守卫。
      if (this.__res !== null && this.__res !== undefined && !this.__res.complete) {
        try { this.__res.destroy(); } catch { /* already gone */ }
      }
      if (this.__sock !== null && this.__sock !== undefined) {
        if (this.__onSockClose !== null) {
          try { this.__sock.removeListener("close", this.__onSockClose); } catch { /* closed meanwhile */ }
        }
        const __s = this.__sock;
        // node 口径：__poolOnDestroy 标记 + socket 存活 + 干净（零收发——已发包
        // 的超时/中断销毁照旧杀连接，t1 超时形）→ 摘请求级监听后回池
        //（listeners-leak 套件；不销毁，防监听泄漏；error 照常经 cb 递送）。
        const __pristine = ((__s.bytesWritten ?? 0) === 0) && ((__s.bytesRead ?? 0) === 0);
        if (this.__poolOnDestroy === true && !__s.destroyed && __pristine &&
            this.agent !== null && this.agent !== undefined) {
          this.__poolOnDestroy = false;
          try {
            if (__s.__reqSockOnEnd !== undefined) { try { __s.removeListener("end", __s.__reqSockOnEnd); } catch {} __s.__reqSockOnEnd = undefined; }
            if (__s.__reqSockOnError !== undefined) { try { __s.removeListener("error", __s.__reqSockOnError); } catch {} __s.__reqSockOnError = undefined; }
            if (__s.__reqSockOnClose !== undefined) { try { __s.removeListener("close", __s.__reqSockOnClose); } catch {} __s.__reqSockOnClose = undefined; }
            if (__s.__reqSockOnData !== undefined) { try { __s.removeListener("data", __s.__reqSockOnData); } catch {} __s.__reqSockOnData = undefined; }
            if (__s.__freeSockErr !== undefined) { try { __s.removeListener("error", __s.__freeSockErr); } catch {} }
            __s.__freeSockErr = function freeSocketErrorListener(err) {
              this.destroy();
              this.emit("agentRemove");
            };
            __s.on("error", __s.__freeSockErr);
          } catch { /* 摘除失败即回落销毁 */ }
          this.__sock = null;
          if (this.__poolKey !== undefined) {
            try { this.agent.__release(__s, this.__poolKey, this, true); } catch { /* gone */ }
          } else {
            try { __s.destroy(); } catch { /* gone */ }
          }
        } else {
          // capture-rejection 套件：同服务端，destroy(err) 有 error 监听才带
          // err 透传（经 __reqSockOnError 回 req 'error'）。
          try {
            if (err !== undefined && err !== null && typeof __s.listenerCount === "function" && __s.listenerCount("error") > 0) __s.destroy(err);
            else __s.destroy();
          } catch { /* closed meanwhile */ }
          this.__sock = null;
        }
      }
      cb(err);
      if (!this.__closeEmitted) {
        this.__closeEmitted = true;
        queueMicrotask(() => this.emit("close"));
      }
    }
