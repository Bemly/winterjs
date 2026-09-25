  end(data, enc, cb) {
    // node 语义：end([chunk][, enc][, cb])——首参函数即 cb（async-iter 套件
    // `end(resolve)` 形；不识别则回调被当 chunk 落校验 TypeError）。
    if (typeof data === "function") { cb = data; data = undefined; enc = undefined; }
    else if (typeof enc === "function") { cb = enc; enc = undefined; }
    if (data !== undefined && data !== null) this.write(data, typeof enc === "string" ? enc : undefined);
    const cb2 = cb;
    this.writable = false; this.__ended = true;
    if (this.__id && this.__connected) __wjs_net_end(this.__id);
    else this.__endAfterFlush = true; // node 口径：FIN 排队到连接完成+缓冲写冲刷之后
    // node 口径：写侧刷完即 'finish'（早于 close；bytes-stats/bytes-read 套件点名）。
    // 本仓同步写队列：FIN 已发即 microtask 派发 finish。
    queueMicrotask(() => this.emit("finish"));
    // node 流语义：end 的回调挂 'finish'（非 close——半开对端不回 FIN 时
    // close 永不来，async-iter 套件 `end(resolve)` 卡死）。
    if (cb2) this.once("finish", cb2);
    return this;
  }
  // node 口径：resetAndDestroy() = RST 硬关（本端无 error 即 close；
  // 对端读侧 ECONNRESET）。本仓 TCP 无 RST 面：本端走 destroy 无 error，
  // 对端侧由传输 FIN 收尾（ECONNRESET 偏离，见 bun-parity net 节）。
  resetAndDestroy() { return this.destroy(); }
  // node 口径：error 事件只在 destroy(err) 带参时发（显式 destroy() 无参不发）。
  // 校验/状态 write 失败走 cb（__writeErr），不进 error 事件——lib/net.js 原文口径。
  destroy(err) {
    if (!this.destroyed) {
      this.destroyed = true;
      this.writable = false; this.readable = false;
      this._handle = null;
      this.__dataBuf = [];
      // 写队列记账清零（排空 microtask 见 destroyed 门，不再递送 drain）。
      // bytesWritten pending 同清（预测未落盘，销毁即不再落盘；base 保留实发）。
      this.__sockQ = 0; this.__needSockDrain = false;
      this.__bwPend = 0;
      if (this.__id) __wjs_net_destroy(this.__id);
      // node 口径 emitErrorNT：error 经 nextTick 异步发（同步抛错会把
      // uncaughtException 语义压成同步异常——upgrade body-error 套件；
      // tick 回调带 uncaught 路由，无监听即交付 uncaughtException）。
      if (err !== undefined && err !== null) {
        this.__hadError = true;
        const __e = err;
        process.nextTick(() => { this.emit("error", __e); });
      }
    }
    return this;
  }
  address() {
    // UDS：真机 address() 回 {}（local/remote 全 undefined）。
    if (this.localAddress === null || this.localAddress === undefined) {
      return this.remoteAddress === undefined && this.__connected ? {} : null;
    }
    return { address: this.localAddress, port: this.localPort, family: String(this.localAddress).includes(":") ? "IPv6" : "IPv4" };
  }
  setEncoding(enc) {
    // node 口径：HTTP 服务端 socket 禁改编码（socket-encoding-error 套件；
    // RFC7230 原始字节面）。纯 net socket 照常。
    if (this.__httpServerSocket === true) {
      throw new codes.ERR_HTTP_SOCKET_ENCODING();
    }
    this.__enc = enc === null || enc === undefined ? null : String(enc);
    // 持久解码器（large-string 套件：分包多字节必须跨 chunk 保态——
    // 每 chunk 新建 TextDecoder 会把切断的序列各吐一个 U+FFFD）。
    this.__dec = this.__enc ? new StringDecoder(this.__enc) : null;
    return this;
  }
  // 10a：ref 真计数（net/dgram 共用 natives；__id 为 0 时静默 no-op）。
  // unref 闩锁（connect 前 unref 同 server 侧：落定即补调，真机同）。
  ref() { this.__unrefLatched = false; if (this.__id) __wjs_net_ref(this.__id); return this; }
  unref() {
    this.__unrefLatched = true;
    if (this.__id) __wjs_net_unref(this.__id);
    return this;
  }
}
