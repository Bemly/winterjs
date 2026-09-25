    __attach(sock, reused) {
      if (this.destroyed) {
        // node 口径：销毁后到达的连接按 __poolOnDestroy 标记回池（listeners-leak
        // 套件；连接中销毁不断连），否则销毁。回池要求干净（零收发）。
        const __pristine = ((sock.bytesWritten ?? 0) === 0) && ((sock.bytesRead ?? 0) === 0);
        if (this.__poolOnDestroy === true && !sock.destroyed && __pristine &&
            this.agent !== null && this.agent !== undefined) {
          this.__poolOnDestroy = false;
          try {
            if (sock.__reqSockOnEnd !== undefined) { try { sock.removeListener("end", sock.__reqSockOnEnd); } catch {} sock.__reqSockOnEnd = undefined; }
            if (sock.__reqSockOnError !== undefined) { try { sock.removeListener("error", sock.__reqSockOnError); } catch {} sock.__reqSockOnError = undefined; }
            if (sock.__reqSockOnClose !== undefined) { try { sock.removeListener("close", sock.__reqSockOnClose); } catch {} sock.__reqSockOnClose = undefined; }
            if (sock.__reqSockOnData !== undefined) { try { sock.removeListener("data", sock.__reqSockOnData); } catch {} sock.__reqSockOnData = undefined; }
            if (sock.__freeSockErr !== undefined) { try { sock.removeListener("error", sock.__freeSockErr); } catch {} }
            sock.__freeSockErr = function freeSocketErrorListener(err) {
              this.destroy();
              this.emit("agentRemove");
            };
            sock.on("error", sock.__freeSockErr);
          } catch { /* 摘除失败即回落销毁 */ }
          if (this.__poolKey !== undefined) {
            try { this.agent.__release(sock, this.__poolKey, this, true); } catch { /* gone */ }
          } else {
            try { sock.destroy(); } catch { /* closed meanwhile */ }
          }
          return;
        }
        try { sock.destroy(); } catch { /* closed meanwhile */ }
        return;
      }
      this.__sock = sock;
      this.socket = sock;
      // node setRequestProps 口径：socket._httpMessage 指回当前请求（connect
      // 套件在 'socket' 事件断言全等）。
      sock._httpMessage = this;
      // node 口径：parser 出全局池（freelist）接线——req.parser 与 socket.parser
      // 同一对象；onIncoming/joinDuplicateHeaders 每请求回填（free 置 null）。
      this.parser = (this.agent !== null && this.agent !== undefined &&
        typeof this.agent.__takeParser === "function")
        ? this.agent.__takeParser()
        : { onIncoming: null, joinDuplicateHeaders: null };
      sock.parser = this.parser;
      sock.parser.onIncoming = () => { /* 入站，无可观测 */ };
      sock.parser.joinDuplicateHeaders = this.joinDuplicateHeaders;
      this.reusedSocket = reused === true;
      // node 口径：复用 socket 的 HWM 按新请求同步（highwatermark-reuse 套件
      // 直读 socket.writableHighWaterMark；新建连接走构造期缺省）。
      if (reused === true && this[kHighWaterMark] !== undefined &&
          sock._writableState !== undefined && sock._writableState !== null) {
        sock._writableState.highWaterMark = this[kHighWaterMark];
      }
      // 防御：上轮请求侧监听残留即先摘（正常路径 __finishSock 已摘）——
      // 必须先于本函数的一切注册，否则会把刚挂的监听当残留摘掉。
      // node 口径 attach 换装四件——socketOnEnd/socketErrorListener/
      // socketCloseListener/socketOnData + 池态 freeSocketErrorListener。
      // connect/secureConnect 同摘（复用不再挂，timeout-connect-listener
      // 套件计数恒 0；once 触发后自摘，此处清未触发残留）。
      if (sock.__reqSockOnConnect !== undefined) {
        try { sock.removeListener("connect", sock.__reqSockOnConnect); } catch { /* gone */ }
        sock.__reqSockOnConnect = undefined;
      }
      if (sock.__reqSockOnSecureConnect !== undefined) {
        try { sock.removeListener("secureConnect", sock.__reqSockOnSecureConnect); } catch { /* gone */ }
        sock.__reqSockOnSecureConnect = undefined;
      }
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
      // node 口径：connect/secureConnect 换装监听只挂给新连接（复用已连通，
      // 再挂即残留累积——timeout-connect-listener 套件断言复用后计数 0）。
      // 去重经 once（触发即摘）+ 上方残留先摘。
      if (reused !== true) {
        sock.once("connect", (sock.__reqSockOnConnect = () => {
          // abort-then-end 形：abort 已销毁请求后连接才到——杀掉孤儿 socket，
          // 不刷盘（abort-before-end 套件：否则服务端收到半截请求 RST，
          // 回 ECONNRESET 进请求 error；且 server 必须零收到）。
          if (this.destroyed) {
            try { sock.destroy(); } catch { /* gone */ }
            return;
          }
          this.__connected = true;
          if (this.__pendingFinal) {
            // end() 已调：整事务一次刷出（CL 决策在 end 时已定），结果回终结
            // 回调（writable-finished 套件 mock 失败形；成功 null 同旧）。
            this.__pendingFinal = false;
            this.__finalStashed = false;
            this.__finishFlush(this.__flushFinal(), this.__pendingFinalCb);
            this.__pendingFinalCb = null;
            return;
          }
          // node _flush 口径：连通即发头（无体请求——如 Expect: 100-continue
          // 等 continue 的形态——头也必须立即出网）。
          this.__tryFlush();
        }));
        sock.once("secureConnect", (sock.__reqSockOnSecureConnect = () => {
          if (this.destroyed) {
            try { sock.destroy(); } catch { /* gone */ }
            return;
          }
          this.__connected = true;
          if (this.__pendingFinal) {
            this.__pendingFinal = false;
            this.__finalStashed = false;
            this.__finishFlush(this.__flushFinal(), this.__pendingFinalCb);
            this.__pendingFinalCb = null;
            return;
          }
          this.__tryFlush();
        }));
      }
      const __sockOnData = (chunk) => {
        try {
          this.__onSockData(chunk);
        } catch (e) {
          // node 口径：响应头解析错（严格门）→ req 'error'（经 destroy(err)）；
          // 用户回调 throw（emit('response') 里的监听异常）不得吞——重抛到
          // uncaught 通道（node 解析错走返回值通道、用户 throw 原样上抛；
          // uncaught-from-request-callback 套件：uncaughtException mustCall 1）。
          if (e !== null && typeof e === "object" && e.__parseErr === true) this.destroy(e);
          else process.nextTick(() => { throw e; });
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
        // 去重：请求已销毁（自身的 destroy(err) 先发过 error）即吞，不二次
        // 递送（max-http-headers 套件 mustCall(1) 形；二次抛会跳过 close 链即 hang）。
        if (this.destroyed) return;
        if (this.listenerCount("error") === 0) throw e;
        // 已发错标记（后到的 Close 不再合成 hangup 走 destroy 幂等门）。
        this.__hadError = true;
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
      } else if (sock.connecting !== true && sock.pending !== true &&
                 (sock.__id === null || sock.__id === undefined)) {
        // node 口径：createConnection 注入的已连通 socket（mock Duplex：无
        // connecting/pending 面、无原生句柄 __id）不走 'connect' 事件等待——
        // 直接视为已连通（writable-finished block2 套件：否则写永停靠、
        // finish 误发）。真新建 socket（含 TLS：connecting 面缺席但有 __id）
        // 仍等事件；序排在 'socket' emit 微任务之后。
        const __self = this;
        queueMicrotask(() => {
          if (__self.destroyed || __self.__connected) return;
          __self.__connected = true;
          if (__self.__pendingFinal) {
            __self.__pendingFinal = false;
            __self.__finalStashed = false;
            __self.__finishFlush(__self.__flushFinal(), __self.__pendingFinalCb);
            __self.__pendingFinalCb = null;
            return;
          }
          __self.__tryFlush();
        });
      }
    }
