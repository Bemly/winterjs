    __feed(sock, st, chunk) {
      // 当片存根（rawPacket 口径：llhttp rawPacket=触发当片，非累计；空 re-feed
      // 不覆盖——__feedError 按此补齐缺席的 rawPacket）。
      if (chunk !== undefined && chunk !== null && chunk.length > 0) st.__lastPkt = chunk;
      st.buf = __concat(st.buf, chunk);
      while (true) {
        // 头解析门：req 与 framing **双空**才解析新头——体在途的弃体模式
        // （res 完成即 dump：st.req null + framing 存活，node dump 口径）
        // 必须落体泵（msg null 纯跳过），否则体字节进头解析即
        // HPE_INVALID_METHOD（no-read-no-dump 套件 64KB×2 实录）。
        if (st.req === null && (st.framing === null || st.framing === undefined)) {
          // llhttp 口径：消息边界先吞前导空行（管线残段；insecure-parser
          // 套件尾部现形）再找头终结——此前只在头残缺分支吞，前导空行+
          // 完整头即误解析/400（incoming-pipelined 套件多请求连发只到首个）。
          while (st.buf.length >= 2 && st.buf[0] === 13 && st.buf[1] === 10) {
            st.buf = st.buf.slice(2);
          }
          const headEnd = __findHeadEnd(st.buf);
          if (headEnd === -1) {
            // 头段超出 maxHeaderSize：llhttp HPE_HEADER_OVERFLOW（语义计数，
            // 见 __headSemCount；bytesParsed 照旧取 buf 长，header-overflow
            // 套件逐字），默认 431 + 销毁。上限取服务端选项。
            const __lim = this.maxHeaderSize ?? maxHeaderSize;
            if (__headSemCount(st.buf, false) >= __lim) {
              throw __hpeServer("HPE_HEADER_OVERFLOW", "Header overflow", st.buf.length);
            }
            // 消息期 requestTimeout 从消息首字节起算（request-timeout-
            // pipelining 套件：管线第二请求的残缺头也必须在 requestTimeout
            // 内 408；已有计时（headersTimeout 空闲计时）在跑则不叠加，
            // 部分数据不重置——interrupted/delayed 系既有口径）。
            if (st.buf.length > 0 && st.__rqT == null && st.__hdT == null && this.requestTimeout > 0) {
              st.__rqT = setTimeout(() => { st.__rqT = null; this.__reqTimeout(sock); }, this.requestTimeout);
              st.__rqT.unref();
            }
            // headers 计时随首字节起算（残头超 headersTimeout 即 408；
            // 每消息一次——guard 防逐包重置永不触发）。
            if (st.buf.length > 0 && st.__hdT == null && this.headersTimeout > 0) {
              st.__hdT = setTimeout(() => { st.__hdT = null; this.__reqTimeout(sock); }, this.headersTimeout);
              if (typeof st.__hdT.unref === "function") st.__hdT.unref();
            }
            // 头未齐也可先校验请求行（llhttp 增量语义；管线残渣 "hello world"
            // 在 URL 段首字节即 400，等不到行终结——blank-header 套件；
            // 前导空行已在上方统一吞，此处不再重复）。
            __checkRequestLinePrefix(st.buf);
            return;
          }
          // 整头超限（语义计数，见 __headSemCount；一次凑齐/分包凑齐皆无例外；
          // max-http-headers 套件 16KB 分包仍 431）。上限取服务端选项。
          if (__headSemCount(st.buf.slice(0, headEnd + 4), false) >= (this.maxHeaderSize ?? maxHeaderSize)) {
            throw __hpeServer("HPE_HEADER_OVERFLOW", "Header overflow", headEnd + 4);
          }
          const headText = __latin1(st.buf.slice(0, headEnd));
          if (this.insecureHTTPParser !== true && __hasBareCR(headText)) {
            throw __mkParseError("LF expected after CR");
          }
          const { first, headers, rawHeaders, headersDistinct } = __parseHead(headText, this.__inboundMode ?? "strict", this.maxHeadersCount, undefined, this.joinDuplicateHeaders === true);
          __validateRequestHead(first, headers);
          // node 口径 requireHostHeader（缺省 true）：1.1 缺 Host 即静默 400
          //（无 request、无 clientError；request-host-header 套件）。
          // llhttp 头语义错先于 requireHost（llhttp 解析期校验——TE+CL 缺 Host
          // 形：clientError HPE_INVALID_TRANSFER_ENCODING 先到，无 400 直写
          // reject-chunked-with-content-length 套件）。整头已消费
          //（bytesParsed=头长；子节偏移未被套件点名，记档近似），经
          // clientError（默认 400）。rawPacket 由 __feedError 按当片补齐。
          if (headers["transfer-encoding"] !== undefined && headers["content-length"] !== undefined) {
            throw __hpeServer("HPE_INVALID_TRANSFER_ENCODING",
              "Transfer-Encoding can't be present with Content-Length", headEnd + 4);
          }
          {
            let __cln = 0;
            for (let __i = 0; __i < rawHeaders.length; __i += 2) {
              if (String(rawHeaders[__i]).toLowerCase() === "content-length") __cln++;
            }
            if (__cln > 1) {
              throw __hpeServer("HPE_UNEXPECTED_CONTENT_LENGTH", "Duplicate Content-Length", headEnd + 4);
            }
          }
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
          // 背压恢复钩（node IncomingMessage._read → readStart 口径）：消费端
          // 拉数据即解暂停续读（__feed 置 st.__reqPaused 后在此恢复；
          // no-read-no-dump 流控面——只 pause 不恢复即永不续读）。
          // 背压事件由体泵统一管理（emit-only 转换沿，不停读不锁泵）。

          // Node 口径：CONNECT 方法请求不进 request 管线——派发 'connect'
          //（req, socket, head；无监听则销毁连接），socket 停止 HTTP 解析。
          if (req.method === "CONNECT") {
            sock.__upgraded = true;
            // node 口径：劫持即脱离 http 管理——request/headers 计时全撤
            //（request-timeout-upgrade 套件：408 不得打已劫持 socket）。
            this.__clearReqTimers(st);
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
              // node 口径：劫持即脱离 http 管理——request/headers 计时全撤。
              this.__clearReqTimers(st);
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
          // node parserOnIncoming 顺序（1350 行）：Host 校验属 **pipeline 路径**
          // ——upgrade 请求在函数头已 return 0，无 Host 不得 400
          // （request-timeout-upgrade 套件：Host-less GET + Upgrade 形）。
          if (this.requireHostHeader !== false && (first[2] === "HTTP/1.1") &&
              headers.host === undefined) {
            try { sock.write(new TextEncoder().encode("HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")); } catch { /* gone */ }
            try { sock.destroy(); } catch { /* gone */ }
            return;
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
          // node parserOnIncoming 口径（optimize-empty-requests 套件）：选项
          // 开启且无体头（framing none）→ _dumpAndCloseReadable 快路径——
          // 跳过流生命周期（ended/endEmitted/destroyed/closed/closeEmitted
          // 全置位），后挂 data/end 监听永不触发。
          if (this.optimizeEmptyRequests === true && framing.type === "none" &&
              typeof req._dumpAndCloseReadable === "function") {
            req._dumpAndCloseReadable();
            if (typeof req._read === "function") req._read();
          }
          // 管线队列（node state.outgoing 口径）：在途响应未完时新 res 脱钩
          // socket（res.socket/connection 置 null，drain-writable-length 套件
          // 点名），写停靠（_write park），'request' 照常即时派发（eager-parse）；
          // 前响 finish 即 assignSocket 轮转（见 __onDone）。
          if (st.res !== null && st.res !== undefined && !st.res.destroyed) {
            res.__sock = null;
            res.socket = null;
            res.connection = null;
            res.__queued = true;
            (st.outgoing ??= []).push(res);
          } else {
            st.res = res;
          }
          // 新请求解析即复位解析器释放旗（keep-alive 复用逐轮 free）。
          sock.__parserFreed = false;
          // 头已齐、体在途：消息期 requestTimeout 计时。
          this.__armMsgTimer(st, sock);
          res.__onDone = () => {
            // res finish 即释放解析器（freeParser 口径，见 connection 段）。
            try { self.__freeSocketParser(sock); } catch { /* gone */ }
            st.req = null;
            // 体在途（framing 存活且非 none）：framing 必须保留到体完——
            // 丢弃泵按帧丢弃（st.req null 纯跳过，node dump 口径）；置 null
            // 即把体字节当新请求头解析（垃圾 → 400+断连，GET 排队永不到，
            // no-read-no-dump/dump-req-when-res-ends 挂死根因）。
            if (st.framing === null || st.framing === undefined || st.framing.type === "none") {
              st.framing = null;
            }
            st.res = null;
            st.sawRequest = true;
            // 响应完即弃未消费体（node dump 口径）：解背压暂停续读——缓冲
            // 字节回流经丢弃泵（st.req null 纯跳过），体完回空闲续下一请求
            // （no-read-no-dump 套件：pause → res.end → 'something' 弃收 →
            // GET 复用同连接）。
            if (st.__reqPaused === true) {
              st.__reqPaused = false;
              try { sock.emit("resume"); } catch { /* 监听抛错不阻收尾 */ }
            }
            // 管线轮转：下一排空 assignSocket（停靠写排空 + 递补终结 + 'socket'
            // 事件），st.res 移交，后续 re-feed 的新头排其后。
            if (st.outgoing !== undefined && st.outgoing !== null && st.outgoing.length > 0) {
              const __nx = st.outgoing.shift();
              if (__nx !== undefined && !sock.destroyed && !__nx.destroyed) {
                try { __nx.assignSocket(sock); } catch { /* 已挂即跳过 */ }
                st.res = __nx;
              }
            }
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
            // 用户 handler throw 不进 400 通道（node：原样上抛到 uncaught——
            // handler-throw 真机 crash；旧"400+静默 hang"系伪语义）。
            try {
              this.emit(__ev, req, res);
            } catch (e) {
              process.nextTick(() => { throw e; });
            }
          } else {
            // 417 默认路径：响应立即收尾；后续体字节走丢弃泵（st.req 为 null）。
            res.end();
          }
          continue;
        }
        // 体泵：CL / chunked 增量；none 直接完结（st.req 为 null = 417 丢弃泵）。
        const fr = st.framing;
        if (fr.type === "teInvalid") {
          // TE 在场但非整词 chunked：请求已派发（handler ×1）但体不可帧化
          //——data/end 永不发；体字节到达即 HPE_INVALID_TRANSFER_ENCODING
          // → 400 + close（真机 llhttp 口径；te-repeated-chunked 套件）。
          if (st.buf.length > 0) {
            throw __hpeServer("HPE_INVALID_TRANSFER_ENCODING", "invalid transfer encoding", st.buf.length);
          }
          return;
        }
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
        // 体背压事件语义（node parserOnBody 口径的可观测面，状态驱动）：
        // req 缓冲 ≥HWM 发 'pause'（转换沿），落回 HWM 内发 'resume'——
        // 泵**不停读不中断**：停读/早退的耦合在"体一次性到齐"形必死锁
        // （残段/终结段扣在 fr.buf 等再喂而包不会再有——flush-drain 500KB
        // 实录），缓冲有界（≤体长）无此忧。
        if (st.req !== null && st.req._readableState !== undefined &&
            st.req._readableState.highWaterMark > 0) {
          const __rs = st.req._readableState;
          if (__rs.length >= __rs.highWaterMark) {
            if (st.__reqPaused !== true) {
              st.__reqPaused = true;
              try { sock.emit("pause"); } catch { /* 监听抛错不阻收尾 */ }
            }
          } else if (st.__reqPaused === true) {
            st.__reqPaused = false;
            try { sock.emit("resume"); } catch { /* 监听抛错不阻收尾 */ }
          }
        }
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
      // node 口径（client-highwatermark 套件）：req[kHighWaterMark] 记用户 HWM
      // （缺省 getDefaultHighWaterMark），写机构 HWM 同步——Path B（socket 未连
      // 通前的缓冲写）回压判据走它（64KB/100KB 真、2KB/512 假 + drain）。
      let __reqHWM = getDefaultHighWaterMark(false);
      if (options !== null && typeof options === "object" && !(options instanceof URL) &&
          options.highWaterMark !== undefined) {
        const __n = Number(options.highWaterMark);
        if (Number.isFinite(__n) && __n >= 0) __reqHWM = __n;
      }
      this[kHighWaterMark] = __reqHWM;
      if (this._writableState !== undefined && this._writableState !== null) {
        this._writableState.highWaterMark = __reqHWM;
      }
      // node 口径 request.parser（memory-retention 套件）：parser 挂 socket
      //（每连接共享，parser-free 套件 100 请求恒同一对象）；构造期 null，
      // attach 时接线 + 回填 joinDuplicateHeaders（node lib/_http_client.js
      // 1104 行口径）；res 'end' 即置空（见 agent 收尾）。
      this.joinDuplicateHeaders = (options !== null && typeof options === "object" &&
        !(options instanceof URL) && options.joinDuplicateHeaders === true) ? true : null;
      this.parser = null;
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
        // node 口径：URL 实例 expando headers（request-options 套件往 URL 上挂
        // headers；String() 会洗掉，直读原对象）。
        userHeaders = (options !== null && typeof options === "object" &&
          options.headers !== undefined && options.headers !== null &&
          typeof options.headers === "object") ? options.headers : {};
        extra = {};
      } else {
        // node ClientRequest ctor 入口：ObjectAssign({__proto__: null}, input,
        // options)——后续读全走 null-proto 拷贝（null-prototype-options 套件：
        // Object.prototype 上的同名 getter 陷阱不得触发，node 实测零触发）。
        options = Object.assign({ __proto__: null }, options);
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
      // node 口径 signal（agent-abort-controller 套件）：live 即挂单 abort 监听
      //（listenerCount 恰 1；同步挂载），abort 即 AbortError 进 error；预 abort
      // 即异步 error（不挂监听，计数 0）。收尾摘除。
      if (options !== null && typeof options === "object" && !(options instanceof URL) &&
          options.signal !== undefined && options.signal !== null) {
        const __sig = options.signal;
        const __abortErr = () => {
          const e = new Error("This operation was aborted");
          e.name = "AbortError";
          e.code = "ABORT_ERR";
          return e;
        };
        if (__sig.aborted === true) {
          // 预 abort：同步销毁（destroyed 旗同步立，error 照常异步；套件同步断言）。
          try { this.destroy(__abortErr()); } catch { /* gone */ }
        } else if (typeof __sig.addEventListener === "function") {
          const __onAbort = () => {
            try { __sig.removeEventListener("abort", __onAbort); } catch { /* gone */ }
            try { __etRemove(__sig, "abort", __onAbort); } catch { /* gone */ }
            this.__sigCleanup = null;
            if (!this.destroyed) this.destroy(__abortErr());
          };
          try {
            __sig.addEventListener("abort", __onAbort, { once: true });
            __etAdd(__sig, "abort", __onAbort);
            this.__sigCleanup = () => {
              try { __sig.removeEventListener("abort", __onAbort); } catch { /* gone */ }
              try { __etRemove(__sig, "abort", __onAbort); } catch { /* gone */ }
            };
          } catch { /* 异形 signal 即忽略（未定口径） */ }
        }
      }
      // node 口径 maxHeaderSize（缺省 16384；max-header-size-per-stream 套件
      // 客户端逐流覆写）。
      if (options !== null && typeof options === "object" && !(options instanceof URL) &&
          options.maxHeaderSize !== undefined) {
        const __mhs = Number(options.maxHeaderSize);
        if (Number.isFinite(__mhs) && __mhs >= 0) this.maxHeaderSize = __mhs;
      }
      if (this.maxHeaderSize === undefined) this.maxHeaderSize = __defaultMaxHeaderSize();
      // 自设请求头名字门（invalidheaderfield 套件：'testing 123' → TypeError）。
      for (const __k of Object.keys(userHeaders ?? {})) {
        if (!__TOKEN_RE.test(__k)) {
          throw new codes.ERR_INVALID_HTTP_TOKEN("Header name", __k);
        }
      }
      // 每请求宽松解析旗（insecure-parser-per-stream 套件：头值控制字符严格门）。
      // httpValidation 门（client 与 server 同口径：validateOneOf + 互斥，
      // 真机 ERR_INVALID_ARG_VALUE 逐项对拍）。进程旗默认宽松与服务端同理。
      const __cliInsecDefault = options.httpValidation === undefined && options.insecureHTTPParser === undefined &&
        typeof globalThis.__wjs_nodeCompat !== "undefined" &&
        Array.isArray(globalThis.__wjs_nodeCompat) &&
        globalThis.__wjs_nodeCompat.includes("--insecure-http-parser");
      this.__inboundMode = __parseModeOf(__resolveHttpValidation(options.httpValidation, __cliInsecDefault ? true : options.insecureHTTPParser));
      this.__validation = options.httpValidation ?? (options.insecureHTTPParser === true || __cliInsecDefault ? "insecure" : undefined);
      this.insecureHTTPParser = options.insecureHTTPParser ?? __cliInsecDefault;
      this.socket = null;
      this.agent = options.agent === undefined
        ? (typeof flavor.__getDefaultAgent === "function" ? flavor.__getDefaultAgent() : (flavor.defaultAgent ?? null))
        : (options.agent || null);
      this.__agentFalse = options.agent === false;
      this.__defaultPort = this.agent !== null && this.agent.defaultPort !== undefined
        ? this.agent.defaultPort : flavor.defaultPort;
      // node 口径（lib/_http_client.js 真机源码 + host-header-ipv6-fail 套件 +
      // correct-hostname 套件）：Host 拼接比较的是**生效缺省**
      //（options.defaultPort ?? agent.defaultPort ?? flavor 缺省）——URL 无端口
      // 即 port 80 === 80，省略端口；显式缺省不同才拼 `:port`。
      // IPv6 加框：双冒号以上且首字符非 '[' 才加框（'::1'→'[::1]'，
      // 'foo:1234' 单冒号不加框，直接拼端口）。
      const __cfgDp = options.defaultPort ?? (this.agent !== null && this.agent !== undefined ? this.agent.defaultPort : undefined) ?? flavor.defaultPort;
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
            // cookie 对值数组即 '; ' 单行（headers-array 套件；其余照 String）。
            const __rv = Array.isArray(__p[1]) && __k.toLowerCase() === "cookie"
              ? __p[1].join("; ") : String(__p[1]);
            __list.push([__k, __rv]);
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
      // node 口径（headers-array 套件）：数组形 headers 不自动补 Host/Auth
      //（显式值恒赢；数组形只发有序对，自动 connection 照常）。
      if (options.auth && !Array.isArray(this.__headerList)) {
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
      if (!__noDefaults && this.__headers.host === undefined && !Array.isArray(this.__headerList)) {
        // node 口径（lib/_http_client.js 546 行 + connect-default-host-header
        // 套件真机实测）：CONNECT 且 options.path 在场时 Host 取 path 本体
        //（authority），不取连接主机。
        this.__headers.host = (method === "CONNECT" && options.path !== undefined)
          ? String(path)
          : this.__hostHeader;
        this.__headerNames.host = "Host";
        // node 口径（真机 getHeader/wire 双探针）：存储省缺省端口（correct-hostname
        // 套件），wire 在"存储省了端口且无显式 defaultPort"时补 `:port`（batch5
        // `foo:1234:80`；default-port 套件显式缺省即 wire 亦省；已带端口不补）。
        // CONNECT authority 形自带端口，不补。
        this.__hostBarePort = null;
        if (!(method === "CONNECT" && options.path !== undefined) && port === __cfgDp) {
          const __explicitDp = (options !== null && typeof options === "object" && !(options instanceof URL))
            ? (options.defaultPort ?? (this.agent !== null && this.agent !== undefined ? this.agent.defaultPort : undefined))
            : (this.agent !== null && this.agent !== undefined ? this.agent.defaultPort : undefined);
          if (__explicitDp === undefined && port !== undefined && port !== null) {
            this.__hostBarePort = Number(port);
          }
        }
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
      // node 口径 kOutHeaders 对表落账（correct-hostname 套件直读 .host 对；
      // 构造期快照，后续 setHeader 变动不追——套件只读构造态）。
      try {
        const __pairs = {};
        for (const k of Object.keys(this.__headers)) {
          __pairs[k] = [this.__headerNames[k] ?? k, this.__headers[k]];
        }
        this[kOutHeaders] = __pairs;
      } catch { /* 对表永不阻构造 */ }
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
        let settled = false;
        const oncreate = (err, s) => {
          if (settled) return;
          settled = true;
          if (err) {
            // node _http_client 591-607 行口径：createConnection 错误（async
            // cb 错 / sync throw 统一收口）nextTick 异步 emitErrorEvent——
            // 错误永不同步抛出构造器；无监听经 EE ERR_UNHANDLED_ERROR 落
            // uncaught（create-connection-async-error 套件，修前 err 被吞
            // 即 hang）。
            process.nextTick(() => this.emit("error", err));
          } else if (s) {
            this.__attach(s, false);
          }
        };
        try {
          const maybe = this.__createConn(connOpts, oncreate);
          if (!settled && maybe) oncreate(null, maybe);
        } catch (err) {
          oncreate(err);
        }
      } else if (this.agent !== null) {
        this.agent.__acquire(this, host, port, extra, (sock, reused) => this.__attach(sock, reused));
      } else {
        this.__attach(openSocket(host, port, extra), false);
      }
    }
