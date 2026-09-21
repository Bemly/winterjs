    setHeader(name, value) {
      if (!__TOKEN_RE.test(String(name))) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
      __checkOutboundHeaderValue(this.__validation, value);
      const lk = String(name).toLowerCase();
      this.__headers[lk] = String(value);
      if (lk === "connection") this.__autoConn = false;
      return this;
    }
    // node OutgoingMessage.appendHeader（header-value-relaxed 套件点名）。
    appendHeader(name, value) {
      if (!__TOKEN_RE.test(String(name))) throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", String(name));
      __checkOutboundHeaderValue(this.__validation, value);
      const lk = String(name).toLowerCase();
      const cur = this.__headers[lk];
      this.__headers[lk] = cur !== undefined ? `${cur}, ${value}` : String(value);
      if (lk === "connection") this.__autoConn = false;
      return this;
    }
    getHeader(name) { return this.__headers[String(name).toLowerCase()]; }
    removeHeader(name) { delete this.__headers[String(name).toLowerCase()]; return this; }
    getHeaderNames() { return Object.keys(this.__headers); }
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
    // finish 后调即 noop（set-timeout-after-end 套件：res 'end' 后 setTimeout(0)
    // 不增监听，node `if (this._ended) return this` 口径；_ended 置于 finish，
    // 故 get() 后同步 setTimeout 仍生效）。
    setTimeout(msecs, callback) {
      if (this.__reqFinished) return this;
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
      if (this.__userEnded) {
        const f = typeof chunk === "function" ? chunk : (typeof encoding === "function" ? encoding : cb);
        if (typeof f === "function") f();
        return this;
      }
      if (this.destroyed) {
        // 已销毁（abort 后 end）：node 口径不抛（abort-before-end 套件
        // req.on('error') mustNotCall）；回调按空转处理。
        const f = typeof chunk === "function" ? chunk : (typeof encoding === "function" ? encoding : cb);
        if (typeof f === "function") f();
        return this;
      }
      // CL 快路径判据：end 是首个头触发点（此前无 write）。
      this.__endFast = !this.__sawWrite;
      this.__userEnded = true;
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
      return super.end(chunk, encoding, cb);
    }
    // node 口径（弃用面仍测）：abort = destroy + 'abort' 事件 + aborted 旗。
    abort() {
      if (this.destroyed) return;
      this.__aborted = true;
      this.destroy();
      this.emit("abort");
    }
    get aborted() { return this.__aborted === true; }
    __sendHead() {
      if (this.__headSent) return;
      this.__headSent = true;
      this.headersSent = true;
      const head = [`${this.method} ${this.path} HTTP/1.1`];
      if (this.__headers["content-length"] !== undefined) {
        this.__rawCL = true;
      } else if (this.__chunked) {
        this.__headers["transfer-encoding"] = "chunked";
      }
      // 自动头规范大写（真机口径）；自设头按用户拼写（本仓小写存取）。
      const canon = { "transfer-encoding": "Transfer-Encoding", "content-length": "Content-Length" };
      for (const [k, v] of Object.entries(this.__headers)) {
        const name = k === "connection" && this.__autoConn ? "Connection" : (canon[k] ?? k);
        head.push(`${name}: ${v}`);
      }
      this.__sock.write(new TextEncoder().encode(head.join("\r\n") + "\r\n\r\n"));
    }
    __frame(u8) {
      if (u8.length === 0) return;
      if (this.__chunked && !this.__rawCL) {
        const hex = new TextEncoder().encode(u8.length.toString(16) + "\r\n");
        this.__sock.write(__concat(hex, __concat(u8, new TextEncoder().encode("\r\n"))));
      } else {
        this.__sock.write(u8);
      }
    }
    // 连接就绪或刷盘时机到：头恒发（node _flush 口径）；有体位时按 UCED 定
    // chunked。队列逐帧发出（不合并）：保 TCP 分包（blk09 计数型套件依赖）。
    __tryFlush() {
      if (!this.__connected || this.__sock === null || this.__headSent) return;
      if (this.destroyed) return;
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
      this.__sendHead();
      const q = this.__buf1;
      this.__buf1 = null;
      if (q !== null) for (const b of q) this.__frame(b);
    }
    _write(chunk, encoding, cb) {
      const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk));
      this.__sawWrite = true;
      if (this.__buf1 === null && !this.__headSent) {
        this.__buf1 = [u8];
        this.__holdTimer = setTimeout(() => {
          this.__holdTimer = null;
          if (this.__buf1 !== null && !this.__headSent && !this.destroyed) this.__tryFlush();
        }, 0);
        cb();
        return;
      }
      if (!this.__headSent) {
        // 同拍内第二次写：必为流式，同步刷头。
        if (this.__connected && this.__sock !== null && !this.destroyed) {
          if (this.__chunkDefault) this.__chunked = true;
          this.__sendHead();
          if (this.__buf1 !== null) {
            const q = this.__buf1;
            this.__buf1 = null;
            for (const b of q) this.__frame(b);
          }
        } else {
          // 未连通：逐块排队（连通后逐帧刷出保分包，见 __tryFlush）。
          this.__buf1.push(u8);
          cb();
          return;
        }
      }
      if (this.__connected && this.__sock !== null && !this.destroyed) {
        if (!this.__chunked && !this.__rawCL && this.__chunkDefault) this.__chunked = true;
        this.__frame(u8);
      } else {
        (this.__buf1 ??= []).push(u8);
      }
      cb();
    }
    _final(cb) {
      if (this.__holdTimer !== null) {
        clearTimeout(this.__holdTimer);
        this.__holdTimer = null;
      }
      if (this.destroyed) {
        cb();
        return;
      }
      if (!this.__headSent) {
        if (!this.__connected) {
          // 连接未就绪：连通后一次性发出（CL 快捷；见 connect 回调）。
          this.__pendingFinal = true;
          cb();
          return;
        }
        this.__flushFinal();
      } else if (this.__chunked && !this.__rawCL) {
        if (this.__connected && this.__sock !== null) {
          this.__sock.write(new TextEncoder().encode("0\r\n\r\n"));
        }
      }
      cb();
    }
    // 收尾刷新（调用方保证已连通）：end(data) 为首个头触发点且方法允许体时
    // 走 CL 快捷（合并发出，真机单 write 口径）；write/flushHeaders 在前 →
    // chunked 逐帧（真机 per-write 帧口径）；GET/HEAD 族 → 无 CL/TE 裸体。
    __flushFinal() {
      if (this.__sock === null || this.destroyed) return;
      if (!this.__headSent) {
        // CL 决策已在 end() 落定（node _contentLength 口径）；无 CL 的 UCED
        // 请求 chunked；GET 族（UCED false）无 CL/TE 裸体。
        if (this.__contentLength !== undefined) {
          this.__headers["content-length"] = String(this.__contentLength);
          this.__rawCL = true;
        } else if (this.__chunkDefault && this.__headers["content-length"] === undefined &&
                   this.__headers["transfer-encoding"] === undefined) {
          this.__chunked = true;
          this.__headers["transfer-encoding"] = "chunked";
        }
        this.__sendHead();
        if (this.__buf1 !== null) {
          const q = this.__buf1;
          this.__buf1 = null;
          if (this.__chunked && !this.__rawCL) {
            for (const b of q) this.__frame(b);
          } else {
            this.__frame(__join(q));
          }
        }
        // chunked 收尾终结块（服务端 _final 同款口径）。此前缺失——connect 前
        // write+end 的 POST 走 chunked，服务端 req 'end' 永不触发（loopback
        // 黑盒挂死实录；head 已发路径本就有此写入，两路对齐）。
        if (this.__chunked && !this.__rawCL) {
          this.__sock.write(new TextEncoder().encode("0\r\n\r\n"));
        }
      } else if (this.__chunked && !this.__rawCL) {
        this.__sock.write(new TextEncoder().encode("0\r\n\r\n"));
      }
    }
    _destroy(err, cb) {
      if (this.__holdTimer !== null) {
        clearTimeout(this.__holdTimer);
        this.__holdTimer = null;
      }
      if (this.agent !== null) this.agent.__cancel(this);
      // 响应同销（Node：destroy 中止整个事务；否则 res 永不完结，
      // 挂在它上面的收尾——如 server.close()——永不到）。
      if (this.__res !== null && !this.__res.complete) {
        try { this.__res.destroy(); } catch { /* already gone */ }
      }
      if (this.__sock !== null) {
        if (this.__onSockClose !== null) {
          try { this.__sock.removeListener("close", this.__onSockClose); } catch { /* closed meanwhile */ }
        }
        try { this.__sock.destroy(); } catch { /* closed meanwhile */ }
        this.__sock = null;
      }
      cb(err);
      if (!this.__closeEmitted) {
        this.__closeEmitted = true;
        queueMicrotask(() => this.emit("close"));
      }
    }
    __onSockData(chunk) {
      // 请求已完成（回池/半关）后 socket 的 data 监听仍在——后续响应归下一个
      // 请求，已完成者不得再泵（keep-alive + chunked 响应复用件实测必需）。
      if (this.__respDone || this.__sock === null) return;
      this.__resBuf = __concat(this.__resBuf, chunk);
      while (true) {
        if (this.__res === null) {
          const headEnd = __findHeadEnd(this.__resBuf);
          if (headEnd === -1) return;
          const headText = __latin1(this.__resBuf.slice(0, headEnd));
          if (this.insecureHTTPParser !== true && __hasBareCR(headText)) {
            this.destroy(__hpe("HPE_LF_EXPECTED", "Expected LF after CR"));
            return;
          }
          const { first, headers, rawHeaders } = __parseHead(headText, this.__inboundMode ?? (this.insecureHTTPParser === true ? "lenient" : "strict"));
          if (!first[0].startsWith("HTTP/") || !/^\d{3}$/.test(first[1] ?? "")) {
            this.destroy(__hpe("HPE_INVALID_CONSTANT", "invalid HTTP response line"));
            return;
          }
          // node llhttp strict：TE 与 CL 并存即拒（client-reject-chunked-with-
          // content-length 套件）。
          if (this.insecureHTTPParser !== true && headers["transfer-encoding"] !== undefined &&
              headers["content-length"] !== undefined) {
            this.destroy(__hpe("HPE_INVALID_TRANSFER_ENCODING", "Transfer-Encoding can't be present with Content-Length"));
            return;
          }
          const __statusCode = Number(first[1]);
          // CONNECT 响应：隧道建立——'connect' 事件（res, socket, head 原始态），
          // 不发 'response'，socket 停止 HTTP 解析、不回池（node _http_client 口径）。
          if (this.method === "CONNECT") {
            const res = new IncomingMessage();
            res.statusCode = __statusCode;
            res.statusMessage = first.length >= 3 ? first.slice(2).join(" ") : "";
            res.httpVersion = first[0].slice(5);
            {
              const __vv = res.httpVersion.split(".");
              res.httpVersionMajor = Number(__vv[0] ?? 1) || 0;
              res.httpVersionMinor = Number(__vv[1] ?? 1) || 0;
            }
            res.headers = headers;
            res.rawHeaders = rawHeaders;
            res.socket = this.__sock;
            res.connection = this.__sock;
            res.req = this;
            const __leftover = this.__resBuf.slice(headEnd + 4);
            this.__resBuf = new Uint8Array(0);
            this.__respDone = true;
            this.__upgraded = true;
            this.__res = res;
            this.emit("connect", res, this.__sock, globalThis.Buffer.from(__leftover));
            return;
          }
          // 1xx 信息响应（node parserOnIncomingClient 口径，真机 26.8.2 对拍）：
          // 100 → 'continue' 事件（无实参）；1xx（含 100）→ 'information'（res 形态）；
          // 都不发 'response'，继续等真响应。101 → 'upgrade'（有监听）或销毁。
          if (__statusCode >= 100 && __statusCode < 200) {
            const __leftover = this.__resBuf.slice(headEnd + 4);
            if (__statusCode === 101) {
              if (this.listenerCount("upgrade") > 0) {
                const res = new IncomingMessage();
                res.statusCode = __statusCode;
                res.statusMessage = first.length >= 3 ? first.slice(2).join(" ") : "";
                res.httpVersion = first[0].slice(5);
                {
                  const __vv = res.httpVersion.split(".");
                  res.httpVersionMajor = Number(__vv[0] ?? 1) || 0;
                  res.httpVersionMinor = Number(__vv[1] ?? 1) || 0;
                }
                res.headers = headers;
                res.rawHeaders = rawHeaders;
                res.socket = this.__sock;
                res.connection = this.__sock;
                res.req = this;
                res.__complete();
                this.__resBuf = new Uint8Array(0);
                this.__respDone = true;
                this.__upgraded = true;
                this.__res = res;
                // node 口径：升级前释放解析器（parser-freed-before-upgrade 套件）。
                try { this.__sock.parser = null; } catch { /* gone */ }
                // node 口径（upgrade-agent 套件）：升级即摘池（totalSocketCount
                // 归零、不再复用；先摘后发，用户 upgrade 处理器里可见）+ req
                // 'close' 异步随后（用户在 upgrade 里挂的 close 监听可达）。
                try {
                  if (this.agent !== null && this.agent !== undefined && this.__sock !== null) {
                    const __ag = this.agent;
                    if (this.__sock.__poolCleaner !== undefined) {
                      try { this.__sock.removeListener("close", this.__sock.__poolCleaner); } catch { /* gone */ }
                      this.__sock.__poolCleaner = undefined;
                    }
                    const __k = this.__poolKey;
                    if (__k !== undefined) {
                      const __arr = __ag.sockets[__k];
                      if (__arr !== undefined) {
                        const __i = __arr.indexOf(this.__sock);
                        if (__i !== -1) __arr.splice(__i, 1);
                        if (__arr.length === 0) delete __ag.sockets[__k];
                      }
                    }
                    if (__ag.totalSocketCount > 0) __ag.totalSocketCount--;
                  }
                } catch { /* 摘池失败不阻升级 */ }
                this.emit("upgrade", res, this.__sock, globalThis.Buffer.from(__leftover));
                queueMicrotask(() => {
                  if (!this.__closeEmitted) {
                    this.__closeEmitted = true;
                    try { this.emit("close"); } catch { /* gone */ }
                  }
                });
              } else {
                this.destroy();
              }
              return;
            }
            if (__statusCode === 100) this.emit("continue");
            const info = new IncomingMessage();
            info.statusCode = __statusCode;
            info.statusMessage = first.length >= 3 ? first.slice(2).join(" ") : "";
            info.httpVersion = first[0].slice(5);
            {
              const __vv = info.httpVersion.split(".");
              info.httpVersionMajor = Number(__vv[0] ?? 1) || 0;
              info.httpVersionMinor = Number(__vv[1] ?? 1) || 0;
            }
            info.headers = headers;
            info.rawHeaders = rawHeaders;
            info.socket = this.__sock;
            info.connection = this.__sock;
            info.req = this;
            this.__resBuf = this.__resBuf.slice(headEnd + 4);
            this.emit("information", info);
            continue;
          }
          const res = new IncomingMessage();
          res.statusCode = __statusCode;
          // 状态行无短语合法（"HTTP/1.1 200\r\n"）：短语空串（status-message 套件）。
          res.statusMessage = first.length >= 3 ? first.slice(2).join(" ") : "";
          res.httpVersion = first[0].slice(5);
          {
            const __vv = res.httpVersion.split(".");
            res.httpVersionMajor = Number(__vv[0] ?? 1) || 0;
            res.httpVersionMinor = Number(__vv[1] ?? 1) || 0;
          }
          res.headers = headers;
          res.rawHeaders = rawHeaders;
          // node parserOnIncomingClient 口径：req.shouldKeepAlive 由响应决定
          //（1.1 缺省 keep、'close' 关；1.0 须显式 'keep-alive'；
          // should-keep-alive 套件逐项对拍）。
          {
            const __rc = (headers.connection || "").toLowerCase();
            this.shouldKeepAlive = res.httpVersion === "1.0" ? __rc === "keep-alive" : __rc !== "close";
          }
          res.socket = this.__sock;
          res.connection = this.__sock;
          res.req = this;
          this.__framing = __framingFor(headers, true, res.statusCode, this.method);
          this.__res = res;
          this.__resBuf = this.__resBuf.slice(headEnd + 4);
          // node _http_client.js：响应到达即挂 responseOnTimeout（一次性/socket；
          // 转发 socket 'timeout' → req 'timeout'，响应完结后不再转发）。
          // 计数契约：listeners('timeout') = [onTimeout, emitRequestTimeout,
          // responseOnTimeout]，跨 keep-alive 复用不累加（listeners 套件）。
          if (this.__sock !== null && this.__sock.timeout > 0 && !this.__sock.__respOnTimeout) {
            this.__sock.__respOnTimeout = true;
            const __req = this;
            this.__sock.on("timeout", function responseOnTimeout() {
              if (__req.__res !== null && __req.__res.complete) return;
              __req.emit("timeout");
            });
          }
          // §4.35：先 emit("response")（监听器登记 data/end），再喂体。
          this.emit("response", res);
          // 释放闸门在 'response' 之后挂载（node responseOnEnd 于 emit('response')
          // 内注册、晚于用户监听——真机实测：用户 res 'end' 回调时 socket 仍在
          // agent.sockets（length 1）、freeSockets 键不存在，nextTick 才入池；
          // agent-keepalive 套件逐相断言即此序）。
          this.__armReleaseGates(this.__sock);
          continue;
        }
        const fr = this.__framing;
        if (fr.type === "none") {
          this.__res.__complete();
          this.__finishResponse(false);
          return;
        }
        if (fr.type === "close") {
          if (this.__resBuf.length > 0) {
            this.__res.push(globalThis.Buffer.from(this.__resBuf));
            this.__resBuf = new Uint8Array(0);
          }
          return;
        }
        let r;
        if (fr.type === "cl") {
          r = __pumpCL(fr, this.__res, this.__resBuf);
        } else {
          r = __pumpChunked(fr, this.__res, this.__resBuf);
          if (r.error) {
            this.destroy(__hpe("HPE_INVALID_CONSTANT", "invalid chunked body"));
            return;
          }
        }
        this.__resBuf = r.rest;
        if (!r.done) return;
        if (r.trailersRaw !== undefined) __applyTrailers(this.__res, r.trailersRaw);
        this.__res.__complete();
        this.__finishResponse(false);
        return;
      }
    }
    // 响应收齐：释放闸门（__armReleaseGates）在 res 'end'/'close' 触发回池/关连
    //（真机 p-free/p-ka：未消费前 freeSockets 恒空、排队请求不续行）。
    // socket 先断（close-delimited/半途）时直接收尾。
    __armReleaseGates(sock) {
      if (this.__gatesArmed || sock === null || this.__res === null) return;
      this.__gatesArmed = true;
      let __fired = false;
      const __once = () => {
        if (__fired) return;
        __fired = true;
        this.__finishSock(sock);
        if (!this.__closeEmitted) {
          this.__closeEmitted = true;
          this.emit("close");
        }
      };
      this.__res.once("end", __once);
      this.__res.once("close", __once);
      this.__gateOnce = __once;
    }
    __finishSock(sock) {
      if (sock === null || sock.destroyed) return;
      const conn = this.__res !== null ? (this.__res.headers.connection || "").toLowerCase() : "close";
      const poolable = this.agent !== null && this.agent.keepAlive && conn !== "close";
      if (this.agent !== null) {
        // node responseOnEnd 口径：res 收齐（回池/关连前）即摘请求侧三监听
        //（socketOnEnd/socketErrorListener 对应本仓 end/error + close）——
        // 监听数契约：池态 end=1/error=1（agent-keepalive 套件 checkListeners）。
        if (sock.__reqSockOnEnd !== undefined) {
          try { sock.removeListener("end", sock.__reqSockOnEnd); } catch { /* gone */ }
          sock.__reqSockOnEnd = undefined;
        }
        if (sock.__reqSockOnError !== undefined) {
          try { sock.removeListener("error", sock.__reqSockOnError); } catch { /* gone */ }
          sock.__reqSockOnError = undefined;
        }
        if (sock.__reqSockOnClose !== undefined) {
          try { sock.removeListener("close", sock.__reqSockOnClose); } catch { /* gone */ }
          sock.__reqSockOnClose = undefined;
        }
        // 'data' 同步摘（node 池态断言 data===0）：残留会把复用后响应字节
        // 双喂进已完结的旧请求，第二请求永不完成（黑盒 ipc/loopback 实录）。
        if (sock.__reqSockOnData !== undefined) {
          try { sock.removeListener("data", sock.__reqSockOnData); } catch { /* gone */ }
          sock.__reqSockOnData = undefined;
        }
        // node 口径：freeSocketErrorListener 在 'free' 派发前置上（agent 的
        // free 处理器先于用户 once('free') 注册——'free' 回调断言 error=1，
        // agent-keepalive 套件 checkListeners）；回池失败路径由 __release 销毁，
        // 多挂的监听随 socket 死亡无效。
        if (poolable) {
          if (sock.__freeSockErr !== undefined) {
            try { sock.removeListener("error", sock.__freeSockErr); } catch { /* gone */ }
          }
          sock.__freeSockErr = function freeSocketErrorListener(err) {
            this.destroy();
            this.emit("agentRemove");
          };
          sock.on("error", sock.__freeSockErr);
        }
        // node 口径：先回池（__release：keepSocketAlive + 续行排队请求），再发
        // socket 'free'（agent onFree 早于用户监听注册，故用户 free 处理器里
        // 取到的已是池态——agent-timeout 复用块；emit 前清请求级超时并落 timeoutCb，
        // responseKeepAlive 口径）。
        if (this.timeoutCb !== undefined) {
          try { sock.setTimeout(0); } catch { /* gone */ }
          this.timeoutCb = null;
        }
        this.agent.__release(sock, this.__key, this);
        try { sock.emit("free"); } catch { /* gone */ }
      } else {
        try { sock.end(); } catch { /* closed meanwhile */ }
      }
    }
    __finishResponse(fromClose) {
      if (this.__respDone) return;
      this.__respDone = true;
      const sock = this.__sock;
      this.__sock = null;
      const emitClose = () => {
        if (!this.__closeEmitted) {
          this.__closeEmitted = true;
          this.emit("close");
        }
      };
      if (fromClose || this.__res === null || this.__res.readableEnded || this.__res.destroyed) {
        this.__finishSock(sock);
        emitClose();
      }
      // 否则：闸门已挂（response 派发前），res 终结时统一收尾。
    }
    __onSockCloseEv() {
      if (this.__closeEmitted) return;
      // CONNECT/upgrade 后的裸 socket 关闭：直接发 req 'close'（不回池不触 res）。
      if (this.__upgraded) {
        this.__closeEmitted = true;
        this.emit("close");
        return;
      }
      // 响应体已齐但 res 未被消费时连接先断：毁 res 引发 'close' → finish 链。
      if (this.__respDone && this.__res !== null && !this.__res.readableEnded && !this.__res.destroyed) {
        try { this.__res.destroy(); } catch { /* gone */ }
        return;
      }
      if (this.__res !== null && !this.__res.complete) {
        if (this.__framing !== null && this.__framing.type === "close") {
          this.__res.__complete();
          this.__finishResponse(true);
          return;
        }
        // 意外截断：沿 9d 宽容口径直接结束（不抛）。
        this.__res.complete = true;
        this.__res.push(null);
        this.__finishResponse(true);
        return;
      }
      if (this.__res === null && !this.__respDone) {
        this.__closeEmitted = true;
        this.emit("close");
      }
    }
    destroy(err) {
      if (this.destroyed) return this;
      // super.destroy 会走 _destroy（清 socket + 补 'close'）。
      return super.destroy(err);
    }
  };
}

// node lib/_http_agent.js writeAfterFIN 逐字口径：对端 FIN 后写 → EPIPE
//（'This socket has been ended by the other party'）+ 销毁。由 agent 的
// onReadableStreamEnd 在非 halfOpen 时置换 socket.write。
function __writeAfterFIN(chunk, encoding, cb) {
  if (typeof encoding === "function") { cb = encoding; encoding = null; }
  const er = new Error("This socket has been ended by the other party");
  er.code = "EPIPE";
  try { this.destroy(er); } catch { /* gone */ }
  if (typeof cb === "function") cb(er);
  return false;
}

// Agent：node lib/_http_agent.js 口径的函数式构造器——`http.Agent({...})` 无 new
// 亦合法（keepalive-client/free/override 系套件点名）。键位统一走 getName 形
// （'host:port:localAddress(:family)'，缺省位仍带分隔冒号——agent-getname 套件）。
function Agent(options = {}) {
  if (!(this instanceof Agent)) return new Agent(options);
  Agent.prototype.__init.call(this, options);
}
Object.setPrototypeOf(Agent.prototype, EventEmitter.prototype);
Object.setPrototypeOf(Agent, EventEmitter);
Agent.prototype.__init = function (options = {}) {
  EventEmitter.call(this);
  this.options = options ?? {};
  this.keepAlive = options.keepAlive ?? false;
  this.keepAliveMsecs = options.keepAliveMsecs ?? 1000;
  this.maxSockets = options.maxSockets ?? Infinity;
  this.maxFreeSockets = options.maxFreeSockets ?? 256;
  this.maxTotalSockets = options.maxTotalSockets ?? Infinity;
  // maxTotalSockets 门（agent-maxtotalsockets 套件真机逐项：非串 → ARG_TYPE；
  // -1/0/NaN → OUT_OF_RANGE；Infinity 合法）。
  if (options.maxTotalSockets !== undefined) {
    if (typeof options.maxTotalSockets !== "number") {
      throw new codes.ERR_INVALID_ARG_TYPE("maxTotalSockets", "number", options.maxTotalSockets);
    }
    // node 口径：NaN/-1/0 拒、Infinity 过（agent-maxtotalsockets 套件点名）。
    if (!(options.maxTotalSockets > 0)) {
      throw new codes.ERR_OUT_OF_RANGE("maxTotalSockets", "> 0", options.maxTotalSockets);
    }
  }
  this.totalSocketCount = 0;
  this.scheduling = options.scheduling ?? "lifo";
  this.sockets = {};
  this.freeSockets = {};
  this.requests = {};
  // __openSocket/__defaultPort 由 flavor 子类经 prototype 提供（本类不设 own
  // 属性，否则遮蔽子类覆盖；裸 BaseAgent 直接用即 TypeError）。
};
// node 口径：createConnection 是 agent 的建连钩（默认 = net.createConnection，
// 同步回值、不调 cb——cb 由 createSocket 的 oncreate 统一收口）；测试以假
// Duplex 覆盖做黑洞/依此注入 socket。同步回值与 cb 双形态由 createSocket 兜。
Agent.prototype.createConnection = function (options, cb) {
  return this.__openSocket(options.host, options.port, options);
};
// node lib/_http_agent.js：createConnection 的记账壳（req, options, cb 三参；
// 同步回值与 cb 双形态，settled 旗防双取）；用户可整体覆写（agent-close 套件
// `createSocket = (req, options, cb) => cb(err)` → req 'error' + 销毁）。
Agent.prototype.createSocket = function (req, options, cb) {
  let settled = false;
  const oncreate = (err, s) => {
    settled = true;
    if (typeof cb === "function") cb(err, s);
  };
  const maybe = this.createConnection(options, oncreate);
  if (!settled && maybe) oncreate(null, maybe);
  return maybe;
};
Agent.prototype.getName = function (options = {}) {
  let name = options.host ?? options.hostname ?? "localhost";
  name += ":";
  if (options.port) name += options.port;
  name += ":";
  if (options.localAddress) name += options.localAddress;
  // node lib/_http_agent.js：socketPath 占独立槽（'localhost:::/path'，
  // agent-getname 套件点名；unix socket 与 TCP localhost 池键由此区分）。
  if (options.socketPath) name += ":" + options.socketPath;
  if (options.family === 4 || options.family === 6) name += ":" + options.family;
  return name;
};
Agent.prototype.__list = function (map, key) {
  if (map[key] === undefined) map[key] = [];
  return map[key];
};
Agent.prototype.__liveCount = function (key) {
  const all = this.__list(this.sockets, key).filter((s) => !s.destroyed);
  return all.length;
};
// 全局活 socket 数（maxTotalSockets 帽的判定口径；agent-maxtotalsockets 套件
// getTotalSocketsCount 同款）。
Agent.prototype.__totalLive = function () {
  let n = 0;
  for (const key of Object.keys(this.sockets)) n += this.__liveCount(key);
  return n;
};
Agent.prototype.__trackSocket = function (sock, key) {
  this.__list(this.sockets, key).push(sock);
  this.totalSocketCount++;
  // 池键随 socket 走（__noteClosed 按 sock.__poolKey 摘表；此前只记 req 侧，
  // 关闭后 sockets/freeSockets 残留——agent-keepalive 套件断言键消失）。
  sock.__poolKey = key;
  // node 口径：agent 托管 socket 创建即挂 onReadableStreamEnd（入池后保留——
  // 监听数契约：'end' active=2/池态=1，agent-keepalive 套件 checkListeners）。
  // FIN 后写 → EPIPE（writeAfterFIN 置换，真机 toString 逐字）。
  sock.on("end", function onReadableStreamEnd() {
    if (!this.allowHalfOpen) {
      this.write = __writeAfterFIN;
    }
  });
  // node agent（installListeners 口径）：onTimeout 单例无条件挂（set-timeout-
  // after-end 套件：无 timeout 的 agent，其 socket 'timeout' 监听数亦为 1）；
  // 超时且 socket 在池即销毁（agent-timeout 块 2：池 socket 不得复用）。
  // options.timeout > 0 才在建连时置 socket 空闲计时（agent-timeout-option
  // 套件：'socket' 事件时 socket.timeout 已 === 50）。
  if (!sock.__onTimeoutSingleton) {
    sock.__onTimeoutSingleton = true;
    const __ag = this;
    sock.on("timeout", function onTimeout() {
      try {
        const __free = __ag.freeSockets;
        for (const __k of Object.keys(__free)) {
          if (__free[__k].includes(sock)) { try { sock.destroy(); } catch { /* gone */ } break; }
        }
      } catch { /* 池表不可读即跳过 */ }
    });
  }
  if (this.options && typeof this.options.timeout === "number" && this.options.timeout > 0) {
    if (typeof sock.setTimeout === "function") {
      sock.setTimeout(this.options.timeout);
      sock.timeout = this.options.timeout;
    }
  }
  const cleaner = () => this.__noteClosed(sock);
  sock.__poolCleaner = cleaner;
  sock.on("close", cleaner);
};
Agent.prototype.keepSocketAlive = function (sock) {
  if (typeof sock.setKeepAlive === "function") {
    try { sock.setKeepAlive(true, this.keepAliveMsecs); } catch { /* gone */ }
  }
  if (typeof sock.unref === "function") {
    try { sock.unref(); } catch { /* gone */ }
  }
  // node lib/_http_agent.js keepSocketAlive 口径：入池即按 agentTimeout 重置
  // 空闲计时（agent-timeout 套件：CustomAgent 覆写调 super 后再 setTimeout(60)；
  // 经 this. 调度使子类覆写生效）。服务端 keep-alive hint 缩减（无 hint 跳过）。
  let agentTimeout = (this.options && typeof this.options.timeout === "number") ? this.options.timeout : 0;
  try {
    const msg = sock._httpMessage;
    const res = msg && msg.res;
    const hint = res && res.headers ? res.headers["keep-alive"] : undefined;
    const m = typeof hint === "string" ? /^timeout=(\d+)/.exec(hint) : null;
    if (m !== null) {
      const buf = (this.options && typeof this.options.agentKeepAliveTimeoutBuffer === "number")
        ? this.options.agentKeepAliveTimeoutBuffer : 1000;
      const t = parseInt(m[1], 10) * 1000 - buf;
      if (t <= 0) return false;
      if (t < agentTimeout) agentTimeout = t;
    }
  } catch { /* hint 解析失败即无 hint */ }
  if (sock.timeout !== agentTimeout) {
    try { sock.setTimeout(agentTimeout); } catch { /* gone */ }
  }
  return true;
};
Agent.prototype.__unpool = function (sock) {
  sock.__inPool = false;
  if (sock.__poolCleaner !== undefined) {
    try { sock.removeListener("close", sock.__poolCleaner); } catch { /* gone */ }
    sock.__poolCleaner = undefined;
  }
};
// 取空闲或新建；满额则排队（release 时续行）。onSocket(sock, reused)。
// 建连统一走 createSocket 钩（req, options, cb 三参——用户覆写点）。
Agent.prototype.__acquire = function (req, host, port, extra, onSocket) {
  const key = this.getName({ host, port, ...(extra ?? {}) });
  // node 口径：freeSockets 键只在真入池后存在——取用不得侧效应建空数组
  //（agent.freeSockets[name] === undefined 断言，agent-keepalive 套件）。
  const free = this.freeSockets[key];
  if (free !== undefined) {
    while (free.length > 0) {
      const sock = this.scheduling === "fifo" ? free.shift() : free.pop();
      if (!sock.destroyed) {
        this.__unpool(sock);
        this.__list(this.sockets, key).push(sock);
        req.__poolKey = key;
        onSocket(sock, true);
        break;
      }
    }
    // 取空即删键（node addRequest 同款），避免残留 [] 破坏 undefined 断言。
    if (free.length === 0) delete this.freeSockets[key];
    if (req.__poolKey === key) return;
  }
  if (this.__liveCount(key) >= this.maxSockets ||
      (this.maxTotalSockets !== Infinity && this.__totalLive() >= this.maxTotalSockets)) {
    this.__list(this.requests, key).push({ req, host, port, extra, onSocket });
    req.__poolKey = key;
    req.__queued = true;
    return;
  }
  req.__poolKey = key;
  req.__queued = false;
  // host/port 后置归一（extra 的 null/undefined host 不得覆盖归一值——
  // hostname-typechecking 的 {host: null} 值形会漏进 net.connect 炸类型门）。
  const opts = { ...(extra ?? {}), host, port };
  let done = false;
  const oncreate = (err, sock) => {
    if (done) return;
    done = true;
    if (err || !sock) {
      if (sock) { try { sock.destroy(); } catch { /* gone */ } }
      const e = err ?? (() => { const x = new Error("socket hang up"); x.code = "ECONNREFUSED"; return x; })();
      if (!req.destroyed) req.destroy(e);
      return;
    }
    this.__trackSocket(sock, key);
    onSocket(sock, false);
  };
  this.createSocket(req, opts, oncreate);
};
Agent.prototype.__noteClosed = function (sock) {
  // node onClose 口径：totalSocketCount 只在 socket 关闭时减（keepalive 套件
  // 远端关闭后断言归零）。
  if (this.totalSocketCount > 0) this.totalSocketCount--;
  const key = sock.__poolKey;
  if (key === undefined) return;
  const drop = (map) => {
    const arr = map[key];
    if (arr !== undefined) {
      const i = arr.indexOf(sock);
      if (i !== -1) arr.splice(i, 1);
      // node 口径：清空即删键（keep-alive 套件 process.on('exit') 断言
      // `!(name in agent.sockets/requests)`——残留空数组会判真）。
      if (arr.length === 0) delete map[key];
    }
  };
  drop(this.sockets);
  drop(this.freeSockets);
};
Agent.prototype.__release = function (sock, key, req) {
  if (req !== null && req !== undefined) req.__queued = false;
  if (sock.destroyed || !this.keepAlive) {
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
  // 续行排队请求（同键优先；全局 maxTotalSockets 帽下跨键唤醒，同键队列空
  // 时补扫其余键，防他键请求饿死）。
  const tryResume = (k) => {
    const q = this.__list(this.requests, k);
    while (q.length > 0) {
      const next = q.shift();
      if (next.req.destroyed) continue;
      next.req.__queued = false;
      this.__acquire(next.req, next.host, next.port, next.extra, next.onSocket);
      return true;
    }
    return false;
  };
  if (!tryResume(key)) {
    for (const k of Object.keys(this.requests)) {
      if (k === key) continue;
      if (tryResume(k)) break;
    }
  }
};
Agent.prototype.__cancel = function (req) {
  const key = req.__poolKey;
  if (key === undefined || !req.__queued) return;
  req.__queued = false;
  const q = this.__list(this.requests, key);
  const i = q.findIndex((e) => e.req === req);
  if (i !== -1) q.splice(i, 1);
};
// node lib/_http_agent.js addRequest 口径（freeSockets 直投/建连/排队三路）：
// 外部直塞 freeSockets 再 addRequest 即复用（agent-uninitialized 套件）。
Agent.prototype.addRequest = function (req, options, port, localAddress) {
  if (typeof options === "string") options = { host: options, port, localAddress };
  options = { ...(options ?? {}), ...(this.options ?? {}) };
  if (options.socketPath) options.path = options.socketPath;
  const name = this.getName(options);
  this.__list(this.sockets, name);
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
  const opts = { ...options, host: options.host ?? options.hostname ?? "localhost", port: options.port ?? this.__defaultPort ?? 80 };
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
