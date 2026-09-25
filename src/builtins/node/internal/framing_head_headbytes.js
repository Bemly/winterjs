  __headBytes() {
    if (this.__headSent) return new Uint8Array(0);
    this.__headSent = true;
    this.headersSent = true;
    // 用户头预处理（node matchHeader 口径）：
    // connection close → _last；connection 其他 → shouldKeepAlive=true；
    // TE chunked → chunked 帧；CL → raw；keep-alive 头在场 → 抑制自动 Keep-Alive。
    const __st = { conn: false, cl: false, te: false, trailer: false };
    for (const k of Object.keys(this.__headers)) {
      const lk = k.toLowerCase();
      const v = String(this.__headers[k]);
      if (lk === "connection") {
        __st.conn = true;
        if (/(?:^|\W)close(?:\W|$)/i.test(v)) this.__last = true;
        else this.__keepAlive = true;
      } else if (lk === "transfer-encoding") {
        __st.te = true;
        if (/(?:^|\W)chunked/i.test(v)) this.__chunked = true;
      } else if (lk === "content-length") {
        __st.cl = true;
        this.__rawCL = true;
      } else if (lk === "trailer") {
        __st.trailer = true;
      } else if (lk === "keep-alive") {
        this.__defaultKA = false;
      }
    }
    // 204/304 + chunked → 抑制零块 + 强制关连接（node _storeHeader 开头口径，
    // chunked-304 套件：响应带 Connection: close 且无 0\r\n 零块）。
    // wire 状态码以 writeHead 快照为准（__storedStatus；无 writeHead 即活读）。
    const __sc = this.__headStored && this.__storedStatus !== undefined ? this.__storedStatus : this.statusCode;
    if (__sc === 204 || __sc === 304) {
      if (this.__chunked) { this.__chunked = false; this.__keepAlive = false; }
      this.__noBody = true;
    }
    const reason = this.statusMessage ?? STATUS_CODES[__sc] ?? "unknown";
    const head = [`HTTP/1.1 ${__sc} ${reason}`.trimEnd()];
    // node 口径：用户头（含 writeHead 合并头）恒在自动头（Date/Connection/
    // Keep-Alive/CL/TE）之前（真机三探针：plain/set/set+wh 全同序）。
    const __auto = [];
    const __user = [];
    const __autoKey = (k) => {
      if (k === "connection" && this.__autoConn) return true;
      if (k === "date" && this.__autoDate) return true;
      if (k === "keep-alive" && this.__autoKA) return true;
      // 本轮新补的自动 CL/TE（用户未显式设置时）。
      if ((k === "content-length" || k === "transfer-encoding") && this.__headerNames[k] === undefined) return true;
      return false;
    };
    // 自动 Date 头（node 口径：响应缺 date 即补 UTC 串；删掉的不补；
    // sendDate === false 不补——test-http-1.0 套件 curl 形断言无 Date 行）。
    // 顺序：Date 恒在 Connection/Keep-Alive 之前（真机 wire 顺序，chunked-304
    // 套件 `/^Connection: close\r\n$/m` 钉住 Connection 紧贴头终结）。
    if (this.sendDate !== false && this.__headers["date"] === undefined &&
        !(this._removedHeader !== undefined && this._removedHeader.date)) {
      this.__headers["date"] = new Date().toUTCString();
      this.__autoDate = true;
    }
    // Connection 自动决策（node keep-alive logic 口径）：
    // shouldSendKeepAlive = shouldKeepAlive && (用户CL || UCED)；
    // maxRequestsPerSocket 达标 → close；否则 keep-alive（+Keep-Alive: timeout）；
    // 否则 close + _last。
    // _removedHeader 口径：删掉的 connection 不再自动补（remove-header 套件）。
    if (!__st.conn && !(this._removedHeader !== undefined && this._removedHeader.connection)) {
      const shouldSendKeepAlive = this.__keepAlive && (__st.cl || this.__uced);
      if (shouldSendKeepAlive && this.__maxReqReached) {
        this.__headers["connection"] = "close";
        this.__autoConn = true;
        this.__last = true;
      } else if (shouldSendKeepAlive) {
        this.__headers["connection"] = "keep-alive";
        this.__autoConn = true;
        if ((this.__kaTimeout ?? 0) > 0 && this.__defaultKA) {
          const t = Math.floor(this.__kaTimeout / 1000);
          const max = this.__maxReq > 0 ? `, max=${this.__maxReq}` : "";
          this.__headers["keep-alive"] = `timeout=${t}${max}`;
          this.__autoKA = true;
        }
      } else {
        this.__headers["connection"] = "close";
        this.__autoConn = true;
        this.__last = true;
      }
    }
    // 帧决策（node：!contLen && !te 分支）：无用户 CL/TE 时按
    // noBody/UCED/__contentLength（end() 快路径预置）决定 auto CL 或 chunked；
    // 1.0（UCED false）→ _last（close-delimited）。CL/TE 被删掉时不自动补
    // CL/chunked，直接 _last（remove-header 套件：close-delimited 体）。
    if (!__st.cl && !__st.te) {
      const __frSup = this._removedHeader !== undefined &&
        !!(this._removedHeader["content-length"] || this._removedHeader["transfer-encoding"]);
      if (__frSup) {
        this.__chunked = false;
        this.__last = true;
      } else if (this.__noBody || this.__headOnly) {
        this.__chunked = false;
      } else if (!this.__uced) {
        this.__last = true;
      } else if (!__st.trailer && this.__contentLength !== undefined) {
        this.__headers["content-length"] = String(this.__contentLength);
        this.__rawCL = true;
      } else {
        this.__headers["transfer-encoding"] = "chunked";
        this.__chunked = true;
      }
    }
    // 自设头按用户拼写输出（node verbatim；本仓内部统一小写存取）；自动头的
    // 真机输出是规范大写（Transfer-Encoding/Content-Length/自动 Connection/
    // Date/Keep-Alive）。
    const canon = { "transfer-encoding": "Transfer-Encoding", "content-length": "Content-Length" };
    const __autoCase = (k) => (k === "connection" && this.__autoConn) || (k === "date" && this.__autoDate) ||
      (k === "keep-alive" && this.__autoKA);
    for (const [k, v] of Object.entries(this.__headers)) {
      const user = this.__headerNames[k];
      const name = user !== undefined ? user
        : (__autoCase(k) ? (k === "keep-alive" ? "Keep-Alive" : k.charAt(0).toUpperCase() + k.slice(1)) : (canon[k] ?? k));
      // node 口径：数组值逐行发出（set-cookie/多值头）；uniqueHeaders 名单内
      // 用 '; ' 合并单行（multiple-headers 套件；发送侧合并，解析侧天然单元素）。
      // cookie 数组恒单行 '; ' 合并（真机 26.8.2 实测，双端同）。
      if (Array.isArray(v)) {
        const __uniq = this.__uniqueHeaders;
        if (k === "cookie") {
          (__autoKey(k) ? __auto : __user).push(`${name}: ${v.join("; ")}`);
          continue;
        }
        if (Array.isArray(__uniq) && __uniq.includes(k)) {
          (__autoKey(k) ? __auto : __user).push(`${name}: ${v.join("; ")}`);
          continue;
        }
        for (const __e of v) (__autoKey(k) ? __auto : __user).push(`${name}: ${__e}`);
        continue;
      }
      (__autoKey(k) ? __auto : __user).push(`${name}: ${v}`);
    }
    for (const __line of __user) head.push(__line);
    for (const __line of __auto) head.push(__line);
    return __latin1Bytes(head.join("\r\n") + "\r\n\r\n");
  }
  __sendHead() {
    // node 口径：header 独立成 write；首块合并只发生在 _final/flushFinal 快捷路。
    // socket 已销毁即静默丢弃（incoming-pipelined 套件：管线中连接被毁后
    // 续行响应的写不抛，node 写毁 socket 回 false 口径）。
    if (this.__sock === null || this.__sock.destroyed) return;
    const head = this.__headBytes();
    if (head.length > 0) this.__sockWrite(head);
  }
  // chunked 终结块：`0\r\n` + trailer 行 + 空行（addTrailers 的 trailer 跟尾）。
  __chunkTerminator() {
    return "0\r\n" + (this.__trailer ?? "") + "\r\n";
  }
  // node _send 粒度（lib/_http_outgoing.js write_ 原文 + response-cork 套件
  // socket.write spy 恰 5 次）：chunked 帧四发——hex / CRLF / 体 / CRLF，
  // 每次 _send 独立 conn.write；CL/裸体一发（头 prepend 首块，见
  // __sendHeadWithFirst）。字节流恒等，write 调用次数与真机对齐。
  __frame(u8, box) {
    if (this.__noBody || this.__headOnly) return;
    if (u8.length === 0) return;
    if (this.__sock === null || this.__sock.destroyed) return;
    if (this.__chunked && !this.__rawCL) {
      // node _send 粒度：尺寸行 hex **不含 CRLF**（crlf_buf 独立一发）——
      // hex 带 CRLF 再发 __CRLF 即双 CRLF，整条 chunked 流错位（实锤坑）。
      this.__sockWrite(new TextEncoder().encode(u8.length.toString(16)), box);
      this.__sockWrite(__CRLF, box);
      this.__sockWrite(u8, box);
      this.__sockWrite(__CRLF, box);
    } else {
      this.__sockWrite(u8, box);
    }
  }
  // node _http_outgoing _send 内部面（test-http-1.0 套件直调 res._send('')）：
  // 头未发即发头（'' 调用 = 强制冲头，_headerSent 口径）；已发且 data 非空即
  // 直写（套件只用 ''；本仓帧化归 __frame，_send 不做 chunk 帧化）。
  _send(data) {
    const __d = data === undefined || data === null ? "" : String(data);
    // 先记账后落盘（_send 先于首 _write 时头尚未计数；已计数即只递减）。
    if (!this.__headCounted && !this.__headSent) {
      this.__headCounted = true;
      this.__wlen += this.__predictHeadLen();
    }
    if (!this.__headSent) {
      if (this.__sock !== null && !this.__sock.destroyed) {
        const head = this.__headBytes();
        if (head.length > 0) this.__sockWrite(head);
        if (__d.length > 0) this.__sockWrite(new TextEncoder().encode(__d));
      }
      return this;
    }
    if (__d.length > 0 && this.__sock !== null && !this.__sock.destroyed) {
      this.__sockWrite(new TextEncoder().encode(__d));
    }
    return this;
  }
  // 首块带头发（node _send 的 _header prepend 口径：头未发即拼进首个 _send
  //——chunked 拼 hex、CL/裸体拼体；空块/无体头独立一发）。
  __sendHeadWithFirst(b, box) {
    if (this.__sock === null || this.__sock.destroyed) return;
    const head = this.__headBytes();
    const __doChunk = this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly;
    if (__doChunk && b !== null && b !== undefined && b.length > 0) {
      // hex 不含 CRLF（见 __frame 注）：head+hex / CRLF / 体 / CRLF。
      const hex = new TextEncoder().encode(b.length.toString(16));
      this.__sockWrite(head.length > 0 ? __concat(head, hex) : hex, box);
      this.__sockWrite(__CRLF, box);
      this.__sockWrite(b, box);
      this.__sockWrite(__CRLF, box);
    } else if (head.length > 0 && b !== null && b !== undefined && b.length > 0) {
      this.__sockWrite(__concat(head, b), box);
    } else if (head.length > 0) {
      this.__sockWrite(head, box);
    } else if (b !== null && b !== undefined && b.length > 0) {
      this.__frame(b, box);
    }
  }
  _write(chunk, encoding, cb) {
    const u8 = chunk instanceof Uint8Array ? chunk : __toU8(String(chunk));
    this.__sawWrite = true;
    // 管线停靠（drain-writable-length 套件）：socket 未就位的入列响应停靠
    // chunk，cb 暂扣（流机构自然等待，背压天然成立）；assignSocket 回放。
    // 独立构造（未入列）仍直落旧路（丢弃 + 回调，零行为差）。
    if (this.__sock === null && this.__queued === true && !this.destroyed) {
      (this.__parked ??= []).push([u8, cb]);
      return;
    }
    // 记账在 write/end 包装层同步完成（见上），此处不再计。
    // node 口径：首个 write 即算发头（setheaders-after-sent 套件 write 后
    // setHeader 即 HEADERS_SENT；holdback 只延迟落盘，旗同步立）。
    this.headersSent = true;
    // 帧决策已知（显式 CL/TE 或 writeHead 已存）即直发，不 holdback（spurious-
    // aborted 套件：holdback 停首包 + 服务端即时 destroy 即字节全丢，对端
    // hangup；真机无 holdback，直发）。未知才停靠待 CL 快捷判定。
    const __framingKnown = this.__headers["content-length"] !== undefined ||
      this.__headers["transfer-encoding"] !== undefined || this.__headStored;
    if (this.__buf1 === null && !this.__headSent && !__framingKnown) {
      // 首字节 holdback：一拍内 end 到达且此前无 writeHead 则走 CL 快捷，
      // 否则转 chunked 流式（1.0 裸写）。cb 经 microtask 回（base 依此排
      // _final，抢在 timer(0) 前保 CL 快捷；defer 到落盘后会反让 timer 先赢，
      // 实测回退）。
      this.__buf1 = u8;
      this.__holdTimer = setTimeout(() => {
        this.__holdTimer = null;
        if (this.__buf1 !== null && !this.destroyed) {
          if (!this.__headSent) {
            if (this.__uced && this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined && !this.__frameSuppressed()) this.__chunked = true;
            const b = this.__buf1;
            this.__buf1 = null;
            this.__sendHeadWithFirst(b);
          } else {
            // _send('') 已冲头：滞留首块补帧（1.0 套件 write→_send('') 序）。
            const b = this.__buf1;
            this.__buf1 = null;
            this.__frame(b);
          }
        }
      }, 0);
      // node 口径：_write 完成异步回（socket 层 flush 节奏），同步回即
      // writableLength 即时清零、write 恒 true、背压永不触发
      // （outgoing-finish 系无限 while hang 根因）。
      queueMicrotask(cb);
      return;
    }
    // node 口径：socket 写错回传写回调（writable-finished 套件 mock 形；
    // 确认计数等待，见顶层 __afterSockFlush）。成功路回调 undefined
    //（旧 cb() 同值）。
    const __box = this.__sockCap();
    if (this.__headSent && this.__buf1 !== null) {
      // _send('') 已冲头：滞留首块先行（保序——1.0 套件 write→_send('')→write 序）。
      const b0 = this.__buf1;
      this.__buf1 = null;
      this.__frame(b0, __box);
    }
    if (!this.__headSent) {
      if (this.__uced && this.__headers["content-length"] === undefined && this.__headers["transfer-encoding"] === undefined && !this.__frameSuppressed()) this.__chunked = true;
      const b = this.__buf1;
      this.__buf1 = null;
      // node _header prepend：头未发即拼进首个 _send（头+hex 或 头+体一体；
      // 直发首块（CL/TE 已知）同样合并，standalone 套件单 write 断言）。
      if (b !== null && b !== undefined) {
        this.__sendHeadWithFirst(b, __box);
        this.__frame(u8, __box);
      } else {
        this.__sendHeadWithFirst(u8, __box);
      }
    } else {
      this.__frame(u8, __box);
    }
    // 出错显式递送同一 err + destroy() 收尾（silent；单次 error 被常驻吞错接住）；
    // 成功 microtask 回 undefined（旧 cb() 同值）。
    __afterSockFlush(this, __box, (err) => {
      if (err !== null && err !== undefined) { this.__failFlush(err, (e) => cb(e)); return; }
      cb(undefined);
    });
  }
  _final(cb) {
    if (this.__holdTimer !== null) {
      clearTimeout(this.__holdTimer);
      this.__holdTimer = null;
    }
    // node 口径：已销毁（多为先前写错）时 end 回调带已记错误（writable-
    // finished 套件 end-again 形）；无错沿旧路（参缺席，与旧 cb() 同值）。
    if (this.destroyed) {
      const __de = this._writableState !== null && this._writableState !== undefined ? this._writableState.errored : null;
      cb(__de ?? undefined);
      return;
    }
    // node 口径：socket 写错回传终结回调（writable-finished 套件 mock 形；
    // 确认计数等待，见顶层 __afterSockFlush）。成功路回调 undefined
    //（旧 cb() 同值）。
    const __box = this.__sockCap();
    if (this.__sockGone || this.__sock === null || this.__sock.destroyed) {
      // 入列停靠中：终结 parked（流等待 assign 回放，Node 管线口径；回放后重
      // 走本函数正常收尾，__onDone 照常轮转）。
      if (this.__sock === null && this.__queued === true && !this.destroyed) {
        this.__finalParked = cb;
        return;
      }
      // 无处可送：挂起计数清零（finish 口径 writableLength 恒 0；
      // socket 已死时 pending 同清，未落盘不再落盘）。
      this.__wlen = 0;
      try {
        if (this.__sock !== null && typeof this.__sock.__bwSub === "function") {
          this.__sock.__bwPend = 0;
        }
      } catch { /* 计数永不阻收尾 */ }
      cb();
      return;
    }
    if (!this.__headSent) {
      // CL 快捷：end() 为首个头触发点（无 writeHead/write 前置）且 UCED 时，
      // __contentLength 预置（node _contentLength 口径；end() 裸调 = 0）；
      // write/writeHead 在前 → chunked；1.0 无 TE → 裸体（_last close-delimited）。
      const total = this.__buf1 !== null ? this.__buf1.length : 0;
      if (!this.__noBody && !this.__headOnly &&
          this.__headers["content-length"] === undefined &&
          this.__headers["transfer-encoding"] === undefined &&
          !this.__frameSuppressed()) {
        if (this.__uced && !this.__headStored && (!this.__sawWrite || this.__endHadData)) {
          this.__contentLength = this.__endHadData ? total : 0;
        }
      }
      const head = this.__headBytes();
      if (this.__sock !== null) {
        if (this.__buf1 !== null) {
          const b = this.__buf1;
          this.__buf1 = null;
          const __chunkedFrame = this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly;
          if (__chunkedFrame && b.length > 0) {
            // chunked：头拼首帧 hex（node _send 的 _header prepend 口径）+
            // CRLF/体/CRLF + 终结块独立发——response-cork 套件 socket.write
            // spy 恰 5 次（头+hex/CRLF/体/CRLF/终结）。hex 不含 CRLF
            //（crlf_buf 独立一发，真机 _send 链口径）。
            const hex = new TextEncoder().encode(b.length.toString(16));
            this.__sockWrite(head.length > 0 ? __concat(head, hex) : hex, __box);
            this.__sockWrite(__CRLF, __box);
            this.__sockWrite(b, __box);
            this.__sockWrite(__CRLF, __box);
          } else if (head.length === 0) {
            this.__frame(b, __box);
          } else if (!this.__noBody && !this.__headOnly && b.length > 0) {
            // node 口径：CL/raw 快捷时头 + 首块合并为一次 write（standalone 套件）。
            this.__sockWrite(__concat(head, b), __box);
          } else {
            this.__sockWrite(head, __box);
            this.__frame(b, __box);
          }
          if (__chunkedFrame) {
            this.__sockWrite(new TextEncoder().encode(this.__chunkTerminator()), __box);
          }
        } else if (head.length > 0) {
          // end() 无数据：node _send prepend——chunked 头拼终结块一发，
          // 非 chunked 头独立一发（真机 writeHead+end() 口径）。
          if (this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly) {
            this.__sockWrite(__concat(head, new TextEncoder().encode(this.__chunkTerminator())), __box);
          } else {
            this.__sockWrite(head, __box);
          }
        }
      }
    } else if (this.__chunked && !this.__rawCL && !this.__noBody && !this.__headOnly) {
      this.__sockWrite(new TextEncoder().encode(this.__chunkTerminator()), __box);
    }
    const cont = this.__onDone;
    this.__onDone = null;
    // _final 落盘全量同步完成（CL 快捷真值可能覆盖写时预测）：收尾计数恒清零，
    // 与 socket 实际落盘对齐（finish 口径 writableLength 恒 0；pending 预测
    // 偏差一并清零，base 持有全部实发）。
    this.__wlen = 0;
    try {
      if (this.__sock !== null && typeof this.__sock.__bwSub === "function") {
        this.__sock.__bwPend = 0;
      }
    } catch { /* 计数永不阻收尾 */ }
    // cont（re-feed 解析已读管线字节→503/408 等错误响应）先排，__last 的 FIN
    // 随后排：错误响应写落定时 socket 仍活，否则撞上已 end 即 "write after end"
    // 丢失（GET 管线超 maxRequests 形；POST 形靠体 pacing 碰巧，递延后确定性）。
    // node _last 口径：响应后关连接（close-delimited/显式 close/1.0 裸体）。
    // 终结回调确认计数等待（mock 错异步到；成功 undefined 与旧 cb() 同值）；
    // 出错显式递送 + destroy() 收尾；cont/FIN 调度保持同步。
    __afterSockFlush(this, __box, (err) => {
      if (err !== null && err !== undefined) { this.__failFlush(err, (e) => cb(e)); return; }
      cb(undefined);
    });
    // autoDestroy 接管 close（finish→destroy→close；req-res-close 套件时序）——
    // 此处不再手动发 close（否则与流机构双发）。req 唤醒在 _destroy 内。
    if (cont !== null) queueMicrotask(cont);
    if (this.__last) {
      const __s = this.__sock;
      queueMicrotask(() => { try { __s.end(); } catch { /* closed meanwhile */ } });
    }
  }
  // node 口径：destroy(err) 不外发 msg 'error'（outgoing-destroyed 要求吞错、
  // capture-rejection 经 socket 递错误；基类 trampoline 发 error 时序不可靠，
  // 吞错常驻——用户自有 error 监听仍可达，仅永不无监听抛错）。
  // 真机实测：err 只落 errored（同步可读），不发 'error' 事件。
  destroy(err) {
    if (this.destroyed) return this;
    this.on("error", () => {});
    if (err !== undefined && err !== null) this.__resErrored = err;
    return super.destroy();
  }
  // errored 优先记 destroy(err) 的错（真机同步可读），否则读流机构；
  // 无错时回 undefined（OutgoingMessage 口径，outgoing-destroyed:89）。
  get errored() {
    if (this.__resErrored !== undefined && this.__resErrored !== null) return this.__resErrored;
    const __v = this._writableState ? this._writableState.errored : null;
    return __v === null || __v === undefined ? undefined : __v;
  }
  _destroy(err, cb) {
    if (this.__holdTimer !== null) {
      clearTimeout(this.__holdTimer);
      this.__holdTimer = null;
    }
    // 销毁即无后续落盘：挂起计数/停靠 drain/停靠写/停靠终结全清
    // （destroy 不走 _final；停靠回调永不递送；socket pending 同清）。
    this.__wlen = 0;
    this.__parkedDrain = false;
    this.__parked = [];
    this.__finalParked = null;
    try { this._closed = true; } catch {}
    // node 口径：暂停的服务端 req 随 res 收尾唤醒（req-res-close 套件：无 data
    // 监听时 req 'end' 在 res-close 之后；有监听即 flowing 不受影响。升级/
    // 劫持形无 st 即跳过）。
    try {
      const __ss = this.__sock;
      const __st = __ss !== null && __ss !== undefined ? __ss.__httpState : null;
      const __rq = __st !== null && __st !== undefined ? __st.req : null;
      if (__rq !== null && __rq !== undefined && !__rq.destroyed &&
          typeof __rq.resume === "function") {
        try { __rq.resume(); } catch { /* 唤醒永不阻收尾 */ }
      }
    } catch { /* 无 st 即跳过 */ }
    // node 口径：干净自动收尾（无错、无已记错、finish-hook 手动销毁）只
    // detach 不杀 socket（keep-alive 复用）。显式/错误销毁照旧杀连接。
    // capture-rejection 套件：destroy(err) 透传 socket——仅有用户 error 监听
    // 才带 err（裸杀配 err 会无监听抛错；常驻 __httpSockOnError 兜底不算数，
    // 否则兜底把 err 当用户错重抛即 uncaught——outgoing-destroyed 套件实录）。
    const __noRecErr = this.__resErrored === undefined || this.__resErrored === null;
    const __cleanAuto = (err === undefined || err === null) &&
      __noRecErr && this.__autoTeardown === true;
    if (!__cleanAuto) {
      try {
        const __s = this.__sock;
        let __userErr = 0;
        try {
          const __ls = typeof __s.listeners === "function" ? __s.listeners("error") : [];
          __userErr = __ls.filter((l) => l !== __s.__httpSockOnError && l !== __s.__freeSockErr).length;
        } catch { /* 读表失败即按无用户监听 */ }
        // destroy(err) 包装层把 err 只记 __resErrored（不进流机构防 res
        // 'error' 外发），此处必须从 __resErrored 找回——capture-rejection
        // 套件：res.destroy(err) 后 socket 'error' 收**同一 err 对象**
        // （真机 OutgoingMessage.destroy 直 socket.destroy(error) 口径）；
        // 只认 _destroy 入参 err 恒 undefined 即永裸杀（修前挂死根因）。
        const __carryErr = err !== undefined && err !== null ? err
          : (__noRecErr ? null : this.__resErrored);
        if (__carryErr !== null && __userErr > 0) __s.destroy(__carryErr);
        else __s.destroy();
      } catch { /* closed meanwhile */ }
    }
    cb(err);
  }
}
