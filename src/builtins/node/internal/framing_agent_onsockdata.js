    __onSockData(chunk) {
      // 请求已完成（回池/半关）后 socket 的 data 监听仍在——后续响应归下一个
      // 请求，已完成者不得再泵（keep-alive + chunked 响应复用件实测必需）。
      if (this.__respDone || this.__sock === null) return;
      this.__resBuf = __concat(this.__resBuf, chunk);
      while (true) {
        if (this.__res === null) {
          const headEnd = __findHeadEnd(this.__resBuf);
          if (headEnd === -1) {
            // node llhttp 增量语义：版本首字节即判（'bad http...' 无 CRLF 也
            // 即时 HPE_INVALID_CONSTANT，不等 FIN；client-parse-error 套件）。
            // 合法响应恒 'H' 开头，前导空行跳过后非 H 即错（分包安全：首字节
            // 不可能后变）。
            if (this.insecureHTTPParser !== true) {
              let __i = 0;
              const __b = this.__resBuf;
              while (__i + 1 < __b.length && __b[__i] === 13 && __b[__i + 1] === 10) __i += 2;
              if (__i < __b.length && __b[__i] !== 72) {
                this.destroy(__hpe("HPE_INVALID_CONSTANT", "Expected HTTP/, RTSP/ or ICE/"));
                return;
              }
            }
            // 响应头超限（语义计数，见 __headSemCount；max-http-headers 套件
            // test1：16KB 响应头即 HPE_HEADER_OVERFLOW，不等 FIN；上限取请求选项）。
            if (__headSemCount(this.__resBuf, true) >= (this.maxHeaderSize ?? maxHeaderSize)) {
              this.destroy(__hpe("HPE_HEADER_OVERFLOW", "Header overflow"));
              return;
            }
            return;
          }
          // 整头超限（语义计数；一次凑齐亦拒；上限取请求选项）。
          if (__headSemCount(this.__resBuf.slice(0, headEnd + 4), true) >= (this.maxHeaderSize ?? maxHeaderSize)) {
            this.destroy(__hpe("HPE_HEADER_OVERFLOW", "Header overflow"));
            return;
          }
          const headText = __latin1(this.__resBuf.slice(0, headEnd));
          if (this.insecureHTTPParser !== true && __hasBareCR(headText)) {
            this.destroy(__hpe("HPE_LF_EXPECTED", "Expected LF after CR"));
            return;
          }
          const { first, headers, rawHeaders, headersDistinct } = __parseHead(headText, this.__inboundMode ?? (this.insecureHTTPParser === true ? "lenient" : "strict"), this.maxHeadersCount, true, this.parser !== undefined && this.parser !== null && this.parser.joinDuplicateHeaders === true);
          if (!first[0].startsWith("HTTP/") || !/^\d{3}$/.test(first[1] ?? "")) {
            this.destroy(__hpe("HPE_INVALID_CONSTANT", "Expected HTTP/, RTSP/ or ICE/"));
            return;
          }
          // node llhttp strict：TE 与 CL 并存即拒（client-reject-chunked-with-
          // content-length 套件；rawbytes 套件另要 bytesParsed/rawPacket）。
          if (this.insecureHTTPParser !== true && headers["transfer-encoding"] !== undefined &&
              headers["content-length"] !== undefined) {
            const __e = __hpe("HPE_INVALID_TRANSFER_ENCODING", "Transfer-Encoding can't be present with Content-Length");
            __e.bytesParsed = headEnd + 4;
            try { __e.rawPacket = globalThis.Buffer.from(chunk); } catch { /* gone */ }
            this.destroy(__e);
            return;
          }
          // node llhttp 口径：多 CL 行即拒（response-multi-content-length 套件；
          // 真机 26.8.2 实测文案 'Duplicate Content-Length'）。
          if (this.insecureHTTPParser !== true) {
            let __clCount = 0;
            for (let __i = 0; __i < rawHeaders.length; __i += 2) {
              if (String(rawHeaders[__i]).toLowerCase() === "content-length") __clCount++;
            }
            if (__clCount > 1) {
              const __e = __hpe("HPE_UNEXPECTED_CONTENT_LENGTH", "Duplicate Content-Length");
              __e.bytesParsed = headEnd + 4;
              try { __e.rawPacket = globalThis.Buffer.from(chunk); } catch { /* gone */ }
              this.destroy(__e);
              return;
            }
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
            res.headersDistinct = headersDistinct;
            res.socket = this.__sock;
            res.connection = this.__sock;
            res.req = this;
            const __leftover = this.__resBuf.slice(headEnd + 4);
            this.__resBuf = new Uint8Array(0);
            this.__respDone = true;
            this.__upgraded = true;
            this.__res = res;
            // node 口径（connect 套件 listenerCount 矩阵真机实测：connect 0/
            // data 0/drain 0/end 1/free 0/close 0/error 0/agentRemove 0/
            // timeout 0；_httpMessage null）：隧道建立即拆请求侧接线，只留
            // agent onReadableStreamEnd 恰一个 end；req 置 destroyed + 异步
            // 'close'（socket 不动，归用户；__sock 置空使后续 destroy 不及隧道）。
            const __tunSock = this.__sock;
            try {
              const __s = __tunSock;
              if (__s.__reqSockOnConnect !== undefined) __s.removeListener("connect", __s.__reqSockOnConnect);
              if (__s.__reqSockOnSecureConnect !== undefined) __s.removeListener("secureConnect", __s.__reqSockOnSecureConnect);
              if (__s.__reqSockOnData !== undefined) __s.removeListener("data", __s.__reqSockOnData);
              if (__s.__reqSockOnError !== undefined) __s.removeListener("error", __s.__reqSockOnError);
              if (__s.__reqSockOnEnd !== undefined) __s.removeListener("end", __s.__reqSockOnEnd);
              if (__s.__reqSockOnClose !== undefined) __s.removeListener("close", __s.__reqSockOnClose);
              if (__s.__lastTimeoutCb !== undefined) { try { __s.removeListener("timeout", __s.__lastTimeoutCb); } catch {} __s.__lastTimeoutCb = undefined; }
              if (__s.__agTimeoutSingleton !== undefined) { try { __s.removeListener("timeout", __s.__agTimeoutSingleton); } catch {} }
              if (__s.__freeSockErr !== undefined) { try { __s.removeListener("error", __s.__freeSockErr); } catch {} }
              __s._httpMessage = null;
            } catch { /* 摘除失败不阻隧道 */ }
            // 摘池（101 升级同款：先摘后发；CONNECT 从不入 freeSockets，
            // sockets 残留与 totalSocketCount 同步清）。
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
            } catch { /* 摘池失败不阻隧道 */ }
            try { this.destroyed = true; } catch { /* gone */ }
            this.__sock = null;
            if (!this.__closeEmitted) {
              this.__closeEmitted = true;
              queueMicrotask(() => { try { this.emit("close"); } catch { /* gone */ } });
            }
            this.emit("connect", res, __tunSock, globalThis.Buffer.from(__leftover));
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
            res.headersDistinct = headersDistinct;
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
            info.headersDistinct = headersDistinct;
            info.socket = this.__sock;
            info.connection = this.__sock;
            info.req = this;
            this.__resBuf = this.__resBuf.slice(headEnd + 4);
            this.emit("information", info);
            continue;
          }
          const res = new IncomingMessage(
            this.__sock !== null && this.__sock !== undefined &&
            typeof this.__sock.readableHighWaterMark === "number"
              ? { highWaterMark: this.__sock.readableHighWaterMark } : undefined);
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
          res.headersDistinct = headersDistinct;
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
          // node 口径 parser 清理（memory-retention 套件）：res 'end' 先于用户
          // 监听置空 onIncoming/joinDuplicateHeaders（本监听挂载最早——res 创建
          // 即挂，emit('response') 之前，用户监听恒后到）。
          res.once("end", () => {
            try {
              if (this.parser !== undefined && this.parser !== null) {
                this.parser.onIncoming = null;
                this.parser.joinDuplicateHeaders = null;
                // node freeParser 口径：字段置空即回全局池（复用同一对象）。
                if (this.agent !== null && this.agent !== undefined &&
                    typeof this.agent.__freeParser === "function") {
                  this.agent.__freeParser(this.parser);
                }
              }
            } catch { /* gone */ }
          });
          this.__framing = __framingFor(headers, true, res.statusCode, this.method);
          this.__res = res;
          this.__resBuf = this.__resBuf.slice(headEnd + 4);
          // node _http_client.js：响应到达即挂 responseOnTimeout（一次性/socket；
          // 转发 socket 'timeout' → req 'timeout'，响应完结后不再转发）。
          // 计数契约：listeners('timeout') = [onTimeout, emitRequestTimeout,
          // responseOnTimeout]，跨 keep-alive 复用不累加（listeners 套件）。
          // node _http_client.js 1055 行口径：responseOnTimeout 超时打 **res**
          //（req 侧走 req.setTimeout 的 timeoutCb 独立通路）；complete 门 ≈
          // 真机 responseOnEnd 的 removeListener。挂载条件保留 timeout>0——
          // 恒挂会多占 EE 监听数（set-timeout-after-end 套件 count===1 钉住）；
          // res.setTimeout 后置形由 IM.setTimeout 桥自武装（framing_head）。
          if (this.__sock !== null && this.__sock.timeout > 0 && !this.__sock.__respOnTimeout) {
            this.__sock.__respOnTimeout = true;
            const __req = this;
            this.__sock.on("timeout", function responseOnTimeout() {
              const __res = __req.__res;
              if (__res === null || __res === undefined || __res.complete) return;
              __res.emit("timeout");
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
      // node 口径：响应 end 发射前请求已终结（end-close-event 套件 res-end
      // 回调内 destroyed 已 true；data 时点仍可用 abort——destroy 不得早于
      // end）。prepend 抢在用户 end 监听前；静默销毁。
      if (typeof this.__res.prependListener === "function") {
        this.__res.prependListener("end", () => {
          if (!this.destroyed) { try { this.destroy(); } catch { /* gone */ } }
        });
      }
    }
    __finishSock(sock) {
      if (sock === null || sock.destroyed) return;
      // node 口径：响应被销毁（非正常收齐）即销毁 socket，不回池——destroyed
      // 响应不可复用（outgoing-destroyed pipe 形：客户端 res.destroy() 后
      // 服务端必须见到连接死亡；正常 end+close 才可池化，keepalive 复用）。
      // 收齐后（readableEnded）才 destroy 的不杀——node responseOnEnd 即回池
      //（keepSocketAlive），completed 消息的 destroy 不碰 socket（agent
      // keep-alive 套件 res.destroy 后复用形，修前杀池 socket 即新建）。
      if (this.__res !== null && this.__res !== undefined && this.__res.destroyed &&
          this.__res.readableEnded !== true) {
        try { sock.destroy(); } catch { /* gone */ }
        try { if (this.agent !== null) this.agent.__noteClosed(sock); } catch { /* 记账永不阻收尾 */ }
        return;
      }
      // node responseOnEnd 口径：回池门是 **req.shouldKeepAlive**（响应版本×
      // Connection 缺省已折算——1.0 缺省 false、1.0 'keep-alive' true、1.1
      // 缺省 true、任意版本 'close' false）。只看 conn !== 'close' 会把 1.0
      // 缺省响应的 socket 入池，复用撞服务端单发语义（永不回包）即挂死
      // （should-keep-alive 套件 index 2 实录）。
      const poolable = this.agent !== null && this.agent.keepAlive &&
        this.shouldKeepAlive === true;
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
        this.agent.__release(sock, this.__key, this, poolable);
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
      // node socketCloseListener 首行口径：socket 关闭即 req.destroyed = true
      //（close 蕴含 destroyed；override-global-agent 套件 'close' 读 destroyed）。
      this.destroyed = true;
      // CONNECT/upgrade 后的裸 socket 关闭：直接发 req 'close'（不回池不触 res）。
      if (this.__upgraded) {
        this.__closeEmitted = true;
        this.emit("close");
        return;
      }
      // 响应体已齐但 res 未被消费时连接先断：毁 res 引发 'close' → finish 链。
      // 但有残留字节时 node 按管线下条消息解析，失败即 req error（client-parse-
      // error 套件 302 + 'hi world' 残留形）——先查残留。
      if (this.__respDone && this.__res !== null && !this.__res.readableEnded && !this.__res.destroyed) {
        if (!this.__upgraded && this.__res.complete &&
            this.__resBuf.length > 0 && !this.destroyed) {
          this.destroy(__hpe("HPE_INVALID_CONSTANT", "Expected HTTP/, RTSP/ or ICE/"));
          return;
        }
        try { this.__res.destroy(); } catch { /* gone */ }
        return;
      }
      if (this.__res !== null && !this.__res.complete) {
        if (this.__framing !== null && this.__framing.type === "close") {
          this.__res.__complete();
          this.__finishResponse(true);
          return;
        }
        // 体未齐连接先断：node 口径一律 abort（aborted + ECONNRESET，无 end；
        // 真机实测：干净 FIN 中断同样 abort。旧 9d 宽容 end 系偏差。
        // close-delimited（framing 'close'）的 close 即终结，走正常完成。
        // error 同步守门递送：真机序 aborted → error → close（p4 真机差分
        // 2026-09-25）——error 须先于 destroy 的 close 上车；destroy(__e)
        // 递不出去（IM._destroy 吞错口径）；无监听守门不发（无守门发即抛）。
        const __e = new Error("aborted");
        __e.code = "ECONNRESET";
        try { this.__res.__aborted = true; } catch { /* gone */ }
        try { this.__res.emit("aborted"); } catch { /* 监听抛错不阻收尾 */ }
        try {
          if (typeof this.__res.listenerCount === "function" &&
              this.__res.listenerCount("error") > 0) {
            this.__res.emit("error", __e);
          }
        } catch { /* 监听抛错不阻收尾 */ }
        try { this.__res.destroy(__e); } catch { /* gone */ }
        this.__finishResponse(true);
        return;
      }
      if (this.__res === null && !this.__respDone) {
        this.__closeEmitted = true;
        this.emit("close");
        return;
      }
      // 完整响应后的残留字节：node 按管线下条消息解析，失败即 req error
      //（client-parse-error 套件：302 后 'hi world' 残留 + close 即
      // HPE_INVALID_CONSTANT；正常复用残留恒空，此分支不可达）。
      if (!this.__upgraded && this.__res !== null && this.__res.complete &&
          this.__resBuf.length > 0 && !this.destroyed) {
        this.destroy(__hpe("HPE_INVALID_CONSTANT", "Expected HTTP/, RTSP/ or ICE/"));
        return;
      }
    }
    destroy(err) {
      if (this.destroyed) return this;
      // 用户显式无错销毁（abort-destroy 套件语义分流用；内部错误销毁带 err）。
      const __clean = err === undefined || err === null;
      // node 口径：无响应即销毁 → ECONNRESET 'socket hang up'（abort-destroy
      // 套件；有响应在途即静默；已发过错（__hadError）不重发）。abort() 来的
      // 干净销毁不合成（abort-before-end 套件：abort 只发 'abort' 不发 error；
      // abort 恒先置 __aborted 旗）。
      // super.destroy 会走 _destroy（清 socket + 补 'close'）。
      if (__clean && !this.__hadError && this.__aborted !== true &&
          (this.__res === null || this.__res === undefined) && !this.__respDone) {
        const __e = new Error("socket hang up");
        __e.code = "ECONNRESET";
        err = __e;
      }
      // node 口径：干净销毁（用户 destroy，非 abort）+ keepAlive + 无响应 →
      // socket 留用回池（listeners-leak 套件：11 次即时销毁只建 1 连接）。
      // abort 不回池（销毁即杀）；错误销毁照旧杀连接。标记由 _destroy/__attach 消费。
      if (__clean && this.__aborted !== true &&
          this.agent !== null && this.agent !== undefined && this.agent.keepAlive === true &&
          (this.__res === null || this.__res === undefined) && !this.__respDone) {
        this.__poolOnDestroy = true;
      }
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

// Agent keylog 转发补挂（现存 + 后建；socket 侧恰 1，不重复挂）。
Agent.prototype.__armKeylog = function (only, force) {
  if (force !== true && (typeof this.listenerCount !== "function" || this.listenerCount("keylog") === 0)) return;
  const __one = (sock) => {
    if (sock === undefined || sock === null || sock.destroyed) return;
    if (typeof sock.listenerCount === "function" && sock.listenerCount("keylog") > 0) return;
    const self = this;
    try {
      sock.on("keylog", function __agentKeylogFwd(...a) {
        try { self.emit("keylog", ...a); } catch { /* 监听抛错不阻收尾 */ }
      });
    } catch { /* gone */ }
  };
  if (only !== undefined && only !== null) { __one(only); return; }
  for (const map of [this.freeSockets, this.sockets]) {
    if (map === undefined || map === null) continue;
    for (const bucket of Object.values(map)) {
      if (!Array.isArray(bucket)) continue;
      for (const sock of bucket) __one(sock);
    }
  }
};
// Agent：node lib/_http_agent.js 口径的函数式构造器——`http.Agent({...})` 无 new
// 亦合法（keepalive-client/free/override 系套件点名）。键位统一走 getName 形
// （'host:port:localAddress(:family)'，缺省位仍带分隔冒号——agent-getname 套件）。
// node lib/_http_common.js parsers freelist 口径：parser 对象全局回收复用
//（parser-free 套件 maxSockets=1 串行 100 请求恒同一对象）；free 即字段置空
// 回池，attach 即出池接线。
const __wjs2ParserFreeList = [];
Agent.prototype.__takeParser = function () {
  const p = __wjs2ParserFreeList.pop();
  if (p !== undefined) {
    p.__inPool = false;
    return p;
  }
  return {
    onIncoming: null,
    joinDuplicateHeaders: null,
    free() { /* 默认：归池，无可观测 */ },
    close() { /* 默认：关闭，无可观测 */ },
    remove() { /* 默认：摘除，无可观测 */ },
  };
};
Agent.prototype.__freeParser = function (p) {
  if (p === null || p === undefined || p.__inPool === true) return;
  p.__inPool = true;
  p.onIncoming = null;
  p.joinDuplicateHeaders = null;
  __wjs2ParserFreeList.push(p);
};
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
  // node 口径 keylog 转发（keylog-existing-sockets 套件）：agent 挂 keylog
  // 监听即给全部现存 socket（在用 + 空闲）各挂一个转发监听；后建/回池的经
  // __armKeylog 补挂（socket 侧恒恰 1）。
  this.on("newListener", (t) => {
    // newListener 在入表前触发（§4.47），计数仍 0——直驱补挂，不走计数门。
    if (t === "keylog") this.__armKeylog(undefined, true);
  });
  // node 口径 agentKeepAliveTimeoutBuffer（keep-alive-timeout-buffer 套件）：
  // 缺省 1000；非有限数/负数回落 1000。
  {
    const __b = Number(options.agentKeepAliveTimeoutBuffer);
    this.agentKeepAliveTimeoutBuffer = (options.agentKeepAliveTimeoutBuffer !== undefined &&
      Number.isFinite(__b) && __b >= 0) ? __b : 1000;
  }
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
  // 首参数字即端口（socket-encoding-error 套件 createConnection(port, cb) 形）。
  if (typeof options === "number") options = { port: options };
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
// 只读取用（abort-queued 套件：读不得侧效应建空数组，否则
// Object.keys(agent.sockets/requests) 计数污染）。
Agent.prototype.__peek = function (map, key) {
  const arr = map[key];
  return arr === undefined ? [] : arr;
};
// 排空即删键（node 空键即删口径，上条同源）。
Agent.prototype.__dropIfEmpty = function (map, key) {
  if (map[key] !== undefined && map[key].length === 0) delete map[key];
};
Agent.prototype.__liveCount = function (key) {
  const all = this.__peek(this.sockets, key).filter((s) => !s.destroyed);
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
  // 后建 socket 补挂 keylog 转发（keylog 监听先行时）。
  try { this.__armKeylog(sock); } catch { /* gone */ }
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
    // node socketOnEnd 口径：池态 socket 收 EOF 即销毁（close → __poolCleaner
    // → noteClosed 摘池 + 续行队列）——free socket 半死滞留会被下个请求
    // acquire 复用（单发语义服务端永不回包，req 静默挂死）。
    if (this.__inPool) {
      this.destroy();
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
    // 存根供 CONNECT 隧道 detach 摘除（connect 套件 timeout:0 矩阵）。
    sock.__agTimeoutSingleton = function onTimeout() {
      try {
        const __free = __ag.freeSockets;
        for (const __k of Object.keys(__free)) {
          if (__free[__k].includes(sock)) { try { sock.destroy(); } catch { /* gone */ } break; }
        }
      } catch { /* 池表不可读即跳过 */ }
    };
    sock.on("timeout", sock.__agTimeoutSingleton);
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
  // 出池即撤投毒 guard + 重挂解析器（node 复用即取新 parser；free 态的 null
  // 只维持到出池，见 __release）。
  try { sock.__freeGuardArmed = false; } catch { /* gone */ }
  if (sock.parser === undefined || sock.parser === null) {
    sock.parser = {
      onIncoming: null,
      joinDuplicateHeaders: null,
      free() { /* 默认：归池，无可观测 */ },
      close() { /* 默认：关闭，无可观测 */ },
      remove() { /* 默认：摘除，无可观测 */ },
    };
    sock.parser[6] = function () { /* 默认：超时，无可观测 */ };
  }
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
  // node 口径（双探针实测）：内部建连剥离 signal（请求侧已挂单监听，透传即
  // 双挂，agent-abort-controller 套件恰 1；直接调 createConnection 的显式
  // signal 不动）；keepAlive 面仅 true 值透传（缺省 agent 即 absent）。
  delete opts.signal;
  if (this.keepAlive === true) {
    if (opts.keepAlive === undefined) opts.keepAlive = true;
    if (opts.keepAliveInitialDelay === undefined) opts.keepAliveInitialDelay = this.keepAliveMsecs;
  }
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
  // 非池化关闭亦须续行排队请求（get-pipeline-problem 套件：maxSockets=1 下
  // 首连接关后排队请求永不到；此前仅回池路径续行）。
  this.__resumeQueued(key);
};
