  get onexit() { return this.#onexit; }
  set onclose(cb) {
    this.#onclose = (typeof cb === "function") ? ((code, signal) => {
      if (this.exitCode === null && code !== undefined && code !== null) this.exitCode = code;
      if (this.signalCode === null && signal !== undefined && signal !== null) this.signalCode = signal;
      cb(code, signal);
    }) : cb;
  }
  get onclose() { return this.#onclose; }
  set onerror(cb) { this.#onerror = cb; }
  get onerror() { return this.#onerror; }
  set onspawn(cb) { this.#onspawn = cb; }
  get onspawn() { return this.#onspawn; }
  send(message, ...rest) {
    if (!this.__forkChild) {
      throw Object.assign(new Error("ERR_NOT_SUPPORTED: child send() needs an IPC channel (use fork)"), { code: "ERR_NOT_SUPPORTED" });
    }
    // node target.send 口径（send-type-error 套件）：函数位移 + options 对象
    // 校验先行（连接态无关）；message/句柄校验随后。
    let handle, options, cb = null;
    const a = [...rest];
    if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
    else {
      handle = a.shift();
      if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
      else {
        options = a.shift();
        if (a.length > 0 && typeof a[0] === "function") { cb = a.shift(); }
      }
    }
    if (options !== undefined) __validateSendOptions(options);
    if (!this.__connected) {
      // Node 口径：关通道后 send 回 false，并异步报 ERR_IPC_CHANNEL_CLOSED。
      const err = new Error("Channel closed");
      err.code = "ERR_IPC_CHANNEL_CLOSED";
      if (cb) queueMicrotask(() => cb(err));
      else queueMicrotask(() => this.__emitForkError(err));
      return false;
    }
    __validateSendMessage(message, handle);
    try {
      this.__worker.postMessage(message);
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e));
      if (!err.code) err.code = "ERR_IPC_CHANNEL_CLOSED";
      if (cb) queueMicrotask(() => cb(err));
      else queueMicrotask(() => this.__emitForkError(err));
      return false;
    }
    if (cb) queueMicrotask(() => cb(null));
    return true;
  }
  disconnect() {
    if (!this.__forkChild) {
      throw Object.assign(new Error("ERR_NOT_SUPPORTED: child disconnect() needs an IPC channel (use fork)"), { code: "ERR_NOT_SUPPORTED" });
    }
    // node 口径：已断开再调即 'error' 发射 ERR_IPC_DISCONNECTED（无监听即抛，
    // disconnect 套件 assert.throws 形）。
    if (!this.__connected) {
      const err = new Error("IPC channel is already disconnected");
      err.code = "ERR_IPC_DISCONNECTED";
      this.__emitForkError(err);
      return;
    }
    this.__connected = false;
    // 控制信封（单键载荷，子端 shim 解释为 disconnect，不投递给用户）。
    try { this.__worker.postMessage({ __wjs2_fork_ctl: "disconnect" }); } catch {}
    if (typeof this.#ondisconnect === "function") {
      try { this.#ondisconnect(); } catch {}
    }
  }
  __emitForkError(err) {
    if (typeof this.#onerror === "function") this.#onerror(err);
    else throw err;
  }
  __onForkMessage(m) {
    // NODE_ 前缀分流（internal 套件）：cmd 首段 NODE_ 即内部消息，
    // 余下一律普通 message（真机 cluster 协议口径）。
    if (m !== null && typeof m === "object" && !Array.isArray(m) &&
        typeof m.cmd === "string" && m.cmd.startsWith("NODE_")) {
      if (typeof this.#oninternal === "function") this.#oninternal(m);
      return;
    }
    if (typeof this.#onmessage === "function") this.#onmessage(m);
  }
  __onForkExit(code) {
    if (this.__exitDone) return; // abort 路径已 synth（exit(null, killSignal)）
    this.__exitDone = true;
    this.__exitCode = code;
    if (this.__connected) this.__connected = false;
    // node 口径：exit/close 双参 (code, signal)（fork 旧单对象形一并翻转，
    // 与 spawn 派发同形）。
    if (typeof this.onexit === "function") this.onexit(code, null);
    if (typeof this.onclose === "function") this.onclose(code, null);
  }
  get connected() { return !!this.__connected; }
  unref() {
    if (this.__worker) { try { this.__worker.unref(); } catch {} }
    return this;
  }
  ref() {
    if (this.__worker) { try { this.__worker.ref(); } catch {} }
    return this;
  }
}
