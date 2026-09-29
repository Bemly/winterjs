import { EventEmitter } from "node:events";
import { StringDecoder } from "node:string_decoder";
import { __etAdd, __etRemove } from "node:internal/events/abort_listener";
import { kTimeout } from "node:internal/timers";
import { codes } from "node:internal/errors";
const Buffer = globalThis.Buffer;

function __b64dec(s) {
  const bin = atob(s);
  const u8 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  return u8;
}
function __chunkU8(chunk, enc) {
  // 真机逐字（write-arguments 套件）：'The "chunk" argument must be of type string
  // or an instance of Buffer, TypedArray, or DataView.' + invalidArgTypeHelper。
  // encoding 形（odd-hex-write 套件）：字符串 + 显式编码即按编码解码
  // （'ff1'/'hex' → 单字节 0xff，尾 nibble 丢弃，Buffer.from 口径）。
  if (typeof chunk === "string") {
    if (enc !== undefined && enc !== null && enc !== "utf8" && enc !== "utf-8") {
      return Buffer.from(chunk, String(enc));
    }
    return new TextEncoder().encode(chunk);
  }
  if (typeof Buffer !== "undefined" && Buffer.isBuffer(chunk)) return chunk;
  if (ArrayBuffer.isView(chunk) && !(chunk instanceof DataView) || chunk instanceof DataView) {
    if (chunk instanceof DataView) return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
    return chunk;
  }
  if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
  let __got;
  if (chunk === null) __got = "null";
  else if (chunk === undefined) __got = "undefined";
  else if (typeof chunk === "object") __got = `an instance of ${chunk.constructor?.name ?? "Object"}`;
  else __got = `type ${typeof chunk} (${String(chunk)})`;
  const e = new TypeError(`The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ${__got}`);
  e.code = "ERR_INVALID_ARG_TYPE"; throw e;
}
function __toU8(data, what) {
  if (typeof data === "string") return new TextEncoder().encode(data);
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  throw new TypeError(`${what}: data must be string or BufferSource`);
}
function __netErr(code, msg) {
  const e = new Error(msg);
  e.code = code;
  return e;
}

class Socket extends EventEmitter {
  constructor(options) {
    super();
    // node Socket 构造（socket-constructor 套件）：number 形即 {fd: options}；
    // fd 校验 validateInt32(fd, 'fd', 0) 逐字——'foo' → ARG_TYPE、-1 → ERR_OUT_OF_RANGE。
    if (typeof options === "number") options = { fd: options };
    if (options !== null && typeof options === "object" && options.fd !== undefined) {
      if (typeof options.fd !== "number") {
        const e = new TypeError(`The "fd" argument must be of type number. Received ${__netGot(options.fd)}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (!Number.isInteger(options.fd) || options.fd < 0 || options.fd > 2147483647) {
        const e = new RangeError(`The value of "fd" is out of range. It must be >= 0 && <= 2147483647. Received ${options.fd}`);
        e.code = "ERR_OUT_OF_RANGE"; throw e;
      }
    }
    // node 口径：new Socket({ handle: bound }) 消费 BoundSocket（adopt）。
    if (options && typeof options === "object" && options.handle !== undefined) {
      const h = options.handle;
      if (h && typeof h === "object" && typeof h.address === "function" && h.__boundPort !== undefined) {
        if (h.__adopted) {
          const e = new Error("The bound socket has already been adopted by a server or socket");
          e.code = "ERR_SOCKET_HANDLE_ADOPTED"; throw e;
        }
        h.__adopted = true;
        if (h.__udsPath !== undefined) __boundPaths.delete(h.__udsPath);
        if (h.__holdToken) { try { __wjs2_net_unhold(h.__holdToken); } catch {} }
        this.__adoptHost = h.__boundHost ?? null;
        this.__adoptPort = h.__boundPort ?? 0;
        this.__adoptUds = h.__isPipe ? h.__udsPath : null;
        options = { ...options };
        delete options.handle;
      }
    }
    this.__id = 0;
    this.__enc = null;
    this.__dec = null;
    this.__peerFin = false;
    // signal 选项（abort-controller 套件 testConstructor* 三形）：构造即 aborted
    // → 异步 destroy(AbortError)（once('close') 以 error reject）；live → 注册
    // abort→destroy（直调 addEventListener 须入侧表供 events.listenerCount 读）。
    if (options !== null && typeof options === "object" && options.signal !== undefined) {
      const __sig = options.signal;
      if (__sig.aborted) {
        queueMicrotask(() => {
          const e = new Error("The operation was aborted"); e.name = "AbortError"; e.code = "ABORT_ERR";
          try { this.destroy(e); } catch {}
        });
      } else {
        const __sigHandler = () => {
          __etRemove(__sig, "abort", __sigHandler);
          const e = new Error("The operation was aborted"); e.name = "AbortError"; e.code = "ABORT_ERR";
          // 同 connect 侧：套件在 abort 之后才挂 once('close')，destroy 推 microtask。
          queueMicrotask(() => { try { this.destroy(e); } catch {} });
        };
        __sig.addEventListener("abort", __sigHandler, { once: true });
        __etAdd(__sig, "abort", __sigHandler);
        // 同 signal 重复 connect 即复用此监听（真机恰 1；connect 侧认领）。
        this.__sockSig = __sig;
        this.__sockSigHandler = __sigHandler;
      }
    }
    // node 口径（remote-address 双套件点名）：连接完成前 remote* 全 undefined
    // （够不上 null；发布点在 __ev-connect，不在 __realConnect）。
    this.remoteAddress = undefined;
    this.remotePort = undefined;
    this.remoteFamily = undefined;
    this.localAddress = null;
    this.localPort = null;
    this.readable = true;
    this.writable = true;
    this.destroyed = false;
    // node 口径 bytesWritten（byteswritten 套件）：socket.write 调用即同步计。
    // res 写经流机构异步落盘——同步读会漏计数，故 base（已落盘）+ pend（已写
    // 未落盘，res 包装层按 __wlen 预测同步加、落盘核销）分家；直接读值与真机同。
    this.__bwBase = 0;
    this.__bwPend = 0;
    Object.defineProperty(this, "bytesWritten", {
      get: () => (this.__bwBase ?? 0) + (this.__bwPend ?? 0),
      set: (v) => { this.__bwBase = Number(v) || 0; },
      enumerable: true, configurable: true,
    });
    // pending 记账（res 写预测同步加，不阻写；落盘/收尾核销，钳零）。
    this.__bwAdd = (n) => { this.__bwPend = (this.__bwPend ?? 0) + n; };
    this.__bwSub = (n) => { this.__bwPend = Math.max(0, (this.__bwPend ?? 0) - n); };
    this.bytesRead = 0;
    this.__connected = false;   // 完成连接（connect/attach 后 true）
    this.__pendW = [];          // 连接完成前的缓冲写（node write 语义）
    // Node 默认 allowHalfOpen=false：收到远端 FIN（'end'）后自动 end 本端
    this.allowHalfOpen = !!(options && options.allowHalfOpen);
    // node 口径：_handle 只在连接存活期非空（构造时/close 后恒 null，真机 26 实测；
    // after-close 套件点名 `c._handle === null`）。连接建立（__realConnect/__attach*）
    // 时建桩，destroy/__ev-close 置空。setNoDelay/setKeepAlive 恒可调（无柄只缓存）。
    this._handle = null;
    this.__tos = 0;            // getTypeOfService 缓存（真机默认 0；连接前设置同样缓存）
    this.__kaState = null;     // setKeepAlive 去重缓存 [enable, delaySec, intervalSec, count]
    this.__hadError = false;   // close(hadError) 口径：error 发过即 true
    // transfer-guards 套件：Socket 不可经 MessagePort transfer（node kTransferList
    // 断言族的最保守近似：任何 Socket 在 transfer list 即 ERR_WORKER_HANDLE_NOT_
    // TRANSFERABLE；worker 侧 __normTransfer 认领，成功转移面本就另案）。
    try { (globalThis.__wjs2_netXfer ??= new Map()).set(this, "net.Socket"); } catch {}
    this.__handleClosed = false;
    // node _handle 表面（10f：套件直接打补丁观测 setNoDelay/setKeepAlive 调用；
    // write-after-close 套件点名 _handle.close()；unref-timer 套件点名 _unrefTimer）。
    this.__makeHandle = () => {
      const self = this;
      const __h = {
        setNoDelay: (enable) => { self.__noDelayApplied = enable; },
        setKeepAlive: (enable, delay, interval, count) => { self.__keepAliveApplied = [enable, delay, interval, count]; },
        close: () => { self.__handleClosed = true; queueMicrotask(() => self.destroy()); },
      };
      // 句柄→socket 注册（parser consume() 经 handle 回查 socket；timeout-reset
      // 套件。WeakMap 无泄漏）。
      try {
        globalThis.__wjs2_sockByHandle ??= new WeakMap();
        globalThis.__wjs2_sockByHandle.set(__h, self);
      } catch { /* 注册失败即 consume 空转 */ }
      return __h;
    };
    if (options && typeof options === "object") {
      if (options.readable !== undefined) this.readable = !!options.readable;
      if (options.writable !== undefined) this.writable = !!options.writable;
    }
    // node 读写 HWM（真机 26.8.2 实测：缺省双 65536；readableHighWaterMark/
    // writableHighWaterMark/highWaterMark 逐级；incoming-message-options 套件
    // 点名可配 readableHWM）。写背压沿用 __hwm（hwm 0 恒 false）。
    this.__hwm = options && (options.highWaterMark !== undefined || options.writableHighWaterMark !== undefined)
      ? Number(options.highWaterMark ?? options.writableHighWaterMark) || 0 : 65536;
    this.__rhwm = options && (options.readableHighWaterMark !== undefined || options.highWaterMark !== undefined)
      ? Number(options.readableHighWaterMark ?? options.highWaterMark) || 0 : 65536;
    this.__pendBytes = 0;
    // socket 写队列记账（Slice C：reuse-drained 套件要 socket.writableLength
    // 可观测——同步落盘 + microtask 排空 + HWM 累计判定 + 背压后 'drain'；
    // 原生写本身同步（内核缓冲），记账层只管可观测语义，传输不动）。
    this.__sockQ = 0;
    this.__needSockDrain = false;
    this.__sockQFlush = null;
    this.__sockQAdd = (n) => {
      this.__sockQ = (this.__sockQ ?? 0) + n;
      if (this.__sockQ > this._writableState.highWaterMark) this.__needSockDrain = true;
      if (this.__sockQFlush === null || this.__sockQFlush === undefined) {
        this.__sockQFlush = true;
        queueMicrotask(() => {
          this.__sockQFlush = null;
          this.__sockQ = 0;
          if (this.__needSockDrain) {
            this.__needSockDrain = false;
            if (!this.destroyed) { try { this.emit("drain"); } catch { /* 监听抛错不阻收尾 */ } }
          }
        });
      }
      return this.__sockQ < this._writableState.highWaterMark;
    };
    // _writableState 最小桩：HWM 存储随套件可变（response-drain-cork 套件
    // `socket._writableState.highWaterMark = 1000` 后 res 侧经
    // writableHighWaterMark 读到；真缓冲归 net.Socket 流式化另轮）。
    this._writableState = { highWaterMark: this.__hwm };
    Object.defineProperty(this, "writableHighWaterMark", { get: () => this._writableState.highWaterMark, enumerable: true });
    Object.defineProperty(this, "readableHighWaterMark", { get: () => this.__rhwm, enumerable: true });
    // node 口径：bufferSize = 待刷写字节（本仓同步写队列，连接中缓冲计入，完成即 0）。
    Object.defineProperty(this, "bufferSize", { get: () => this.__pendBytes, enumerable: true });
    // 事件循环派发钩子：dispatch 以 global 为 this 调用，须预绑定（self 语义）
    this.__ev = this.__ev.bind(this);
    // 无监听到达的数据暂存（upgrade-body 系：101 先到、data 监听后挂即丢——
    // 真机缓冲至读；冲刷见 __flushData + newListener 钩）。
    this.__dataBuf = [];
    this.__flushData = () => {
      while (this.__dataBuf.length > 0) {
        if (typeof this.listenerCount !== "function" || this.listenerCount("data") === 0) return;
        const __u = this.__dataBuf.shift();
        try { this.emit("data", this.__dec ? this.__dec.write(__u) : Buffer.from(__u)); } catch { /* 监听抛错不阻收尾 */ }
      }
    };
    this.on("newListener", (__evName) => {
      // 入表前触发（§4.47），只递延冲刷不读表。
      if (__evName === "data") queueMicrotask(() => this.__flushData());
    });
    // Node Writable/Readable 内部面（ws 等 npm 库直接翻字段/调用）：
    // cork/uncork no-op（JS 层写本就不聚合，行为等价）；setNoDelay/
    // setKeepAlive no-op（tokio 写半直通，无 Nagle 可关）；_readableState
    // 最小桩（socketOnClose/socketOnEnd 读 endEmitted/length 判收尾路径）；
    // pause/resume no-op（读流无 JS 侧缓冲，整包即达）；
    // read 恒 null（数据已全经 data 事件投递，无缓冲可取——M5 dev 实测
    // `stream.resume is not a function`，缺桩即 TypeError）。
    // setTimeout 真实现见下（10f timers 对拍）。
    this.__corkCnt = 0;
    this.cork = () => { this.__corkCnt++; return this; };
    this.uncork = () => { if (this.__corkCnt > 0) this.__corkCnt--; return this; };
    Object.defineProperty(this, "writableCorked", { get: () => this.__corkCnt, enumerable: true });
    // _handle 为空（未连接/已关闭）时 no-op 只缓存（after-close 套件：close 后调不抛）。
    this.setNoDelay = (enable) => { if (this._handle && typeof this._handle.setNoDelay === "function") { try { this._handle.setNoDelay(enable !== false); } catch {} } else { this.__noDelayApplied = enable !== false; } return this; };
    // node 口径（真机 26 实测）：setKeepAlive(enable, initialDelay, interval, count) /
    // setKeepAlive({enable, initialDelay, interval, count})；ms→s 下取整转发
    // （5000→5），缺省 interval/count 转发 undefined（JSON 呈 null，typeof 仍 undefined）；
    // 与上次四元组全同即跳过转发（server-keepalive 套件：同值首调被吞）。
    this.setKeepAlive = (enable, initialDelay, interval, count) => {
      if (enable !== null && typeof enable === "object") {
        const o = enable;
        enable = o.enable; initialDelay = o.initialDelay; interval = o.interval; count = o.count;
      }
      enable = enable === undefined ? false : !!enable;
      const toSec = (ms) => ms === undefined ? undefined : Math.floor(Number(ms) / 1000);
      const dSec = initialDelay === undefined ? 0 : toSec(initialDelay);
      const iSec = toSec(interval);
      const st = [enable, dSec, iSec, count];
      const pv = this.__kaState;
      const same = !!pv && pv[0] === st[0] && pv[1] === st[1] && pv[2] === st[2] && pv[3] === st[3];
      this.__kaState = st;
      if (!same && this._handle && typeof this._handle.setKeepAlive === "function") {
        try { this._handle.setKeepAlive(enable, dSec, iSec, count); } catch {}
      }
      return this;
    };
    // node 口径（真机 26 实测）：setTypeOfService 校验逐字（invalidArgTypeHelper 形/
    // OUT_OF_RANGE 双文案：非整数 "must be an integer"、越界 "must be >= 0 && <= 255"），
    // 链式返回自身；getTypeOfService 读缓存（连接前设置同样生效，tos 套件 2a 项）。
    this.setTypeOfService = (tos) => {
      if (typeof tos !== "number" || Number.isNaN(tos)) {
        const got = typeof tos === "string" ? `type string ('${tos}')` : `type ${typeof tos} (${String(tos)})`;
        const e = new TypeError(`The "tos" argument must be of type number. Received ${got}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (!Number.isInteger(tos)) {
        const e = new RangeError(`The value of "tos" is out of range. It must be an integer. Received ${tos}`);
        e.code = "ERR_OUT_OF_RANGE"; throw e;
      }
      if (tos < 0 || tos > 255) {
        const e = new RangeError(`The value of "tos" is out of range. It must be >= 0 && <= 255. Received ${tos}`);
        e.code = "ERR_OUT_OF_RANGE"; throw e;
      }
      this.__tos = tos;
      return this;
    };
    this.getTypeOfService = () => this.__tos ?? 0;
    // 最小 pipe 面（write-connect-write 套件：server 侧 socket.pipe(socket) 回显）；
    // unpipe/_unrefTimer 桩（_parent 链安全，unref-timer 套件点名不抛）。
    this.pipe = (dest, options) => {
      this.on("data", (chunk) => { try { dest.write(chunk); } catch {} });
      if (!options || options.end !== false) this.on("end", () => { try { dest.end(); } catch {} });
      return dest;
    };
    this.unpipe = (dest) => this;
    this._unrefTimer = () => {};
    // pause/resume 真语义（server-pause-on-connect 套件）：paused 期 data 分节
    // 缓存（bytesRead 不进），resume 即冲刷。'pause'/'resume' 事件随转换异步
    // 发（node Readable emitPauseStreamEvent/emitResumeStreamEvent 口径；
    // no-read-no-dump 套件：服务端 handler 借 'pause' 感知体背压）。
    this.__paused = false;
    this.__pauseBuf = [];
    this.pause = () => {
      const __was = this.__paused;
      this.__paused = true;
      if (!__was) {
        queueMicrotask(() => { try { this.emit("pause"); } catch { /* 监听抛错不阻暂停 */ } });
      }
      return this;
    };
    this.resume = () => {
      const __was = this.__paused;
      this.__paused = false;
      if (__was) {
        queueMicrotask(() => { try { this.emit("resume"); } catch { /* 监听抛错不阻续流 */ } });
      }
      // node 流语义：resume 异步续流（同步冲刷会抢在调用方 resume 之后的
      // 语句前发 data——pause-on-connect 套件 stopped 旗现形）。
      queueMicrotask(() => {
        const buf = this.__pauseBuf;
        this.__pauseBuf = [];
        for (const u8 of buf) {
          this.bytesRead += u8.length;
          this.emit("data", this.__dec ? this.__dec.write(u8) : Buffer.from(u8));
        }
      });
      return this;
    };
    // 10f timers 对拍：setTimeout(ms[, cb]) 真实现——单发内部 timer 到期
    // emit('timeout')（Node 口径：不关连接、不杀 socket；cb 注册为 once 监听；
    // 0/负值 = 解除）。内部 timer 恒 unref：连接生死不归它管，socket 在场时
    // 事件循环照常泵到点（fire 不因 unrefed 豁免）。活动重置（node 收包即重置
    // idle 计时）未做——整收口径记档。
    // node 口径 kTimeout（timeout-on-connect 套件）：无计时 null，有计时即
    // timer 对象（_idleTimeout 可读）。
    this[kTimeout] = null;
    this.setTimeout = (ms, cb) => {
      const delay = Number(ms) || 0;
      if (this.__wjs2_stimer) { clearTimeout(this.__wjs2_stimer); this.__wjs2_stimer = null; }
      this[kTimeout] = null;
      // node 口径：socket.timeout 反映最后一次 setTimeout（client-set-timeout
      // 套件断言；旧"不发布"偏差作废，真机 26 实测 socket.timeout 即 ms 值）。
      this.timeout = delay > 0 ? delay : 0;
      if (delay > 0) {
        const t = setTimeout(() => { this.__wjs2_stimer = null; this.emit("timeout"); }, delay);
        t.unref();
        this.__wjs2_stimer = t;
        this[kTimeout] = t;
      }
      if (typeof cb === "function") this.once("timeout", cb);
      return this;
    };
    // 可读侧注入（readable.push 口径，真机 26.8.2 实测）：native 到包与
    // 用户 push 走同一 ingest（bytesRead/暂停/升级直调/暂存冲刷全同）；
    // push(null) = 可读 EOF（先冲暂存再 'end'；无传输（__id 0）即收尾 'close'）。
    this.__ingestData = (u8) => {
      // 空闲池投毒 guard（free-socket-data-guard 套件）：回池后到达的首个
      // 数据即销毁（监听之外，data/readable 计数恒 0）。
      if (this.__freeGuardArmed === true) {
        this.__freeGuardArmed = false;
        try { this.destroy(); } catch { /* gone */ }
        return;
      }
      // 服务端升级接管（upgrade-body 系）：体字节走服务端直调喂体，不经
      // emitter（同表双发会使用户收到原始体 + spill 双份）；用户只收 spill。
      if (this.__srvUpgraded === true && typeof this.__srvFeed === "function") {
        this.bytesRead += u8.length;
        try { this.__srvFeed(u8); } catch { /* 喂体错由服务端收口 */ }
        return;
      }
      if (this.__paused) { this.__pauseBuf.push(u8); return; }
      this.bytesRead += u8.length;
      // 先暂存后冲刷（迟挂监听不丢字节；挂载竞态下仍保序——直发会反超暂存）。
      this.__dataBuf.push(u8);
      this.__flushData();
    };
    this.push = (chunk, encoding) => {
      if (chunk === null || chunk === undefined) {
        if (this.__pushEOF === true || this.readable === false) return false;
        this.__pushEOF = true;
        try { this.__flushData(); } catch { /* 监听抛错不阻收尾 */ }
        if (this.__dec) {
          const rest = this.__dec.end();
          if (rest) this.emit("data", rest);
        }
        this.readable = false;
        this.emit("end");
        // 无传输即整流收尾（真机：未连接 socket push(null) 后 END + CLOSE）。
        if (!this.__id && !this.destroyed) {
          this.destroyed = true; this.writable = false; this._handle = null;
          this.__dataBuf = [];
          this.emit("close", false);
        }
        return false;
      }
      if (this.destroyed || this.readable === false || this.__pushEOF === true) return false;
      let u8;
      if (typeof chunk === "string") {
        u8 = encoding !== undefined && encoding !== null
          ? Buffer.from(chunk, String(encoding)) : new TextEncoder().encode(chunk);
      } else {
        u8 = __chunkU8(chunk);
      }
      this.__ingestData(u8);
      return true;
    };
    this.read = () => null;
    this._readableState = { endEmitted: false, length: 0 };
  }
  connect(...args) {
    // node 口径：首参数组即参数表本身（agent 调 socket.connect([options]) 形，
    // nodelay 套件 patched-connect 按 args[0].noDelay 断言）。
    if (args.length === 1 && Array.isArray(args[0])) args = args[0];
    if (args.length === 0 || (args.length === 1 && typeof args[0] === "object" && args[0] !== null && args[0].port === undefined && args[0].path === undefined)) {
      // node ERR_MISSING_ARGS（connect-no-arg 套件逐字）
      const e = new TypeError('The "options" or "port" or "path" argument must be specified');
      e.code = "ERR_MISSING_ARGS"; throw e;
    }
    // node 口径（Socket.connect destroyed 分支 + initSocketHandle._undestroy）：
    // destroyed 后 connect 即整流复位（destroyed/ending/errored 全清）——
    // boundsocket reconnect-after-destroy 块：close → connect → end 必须可用。
    if (this.destroyed) {
      this.destroyed = false;
      this.readable = true; this.writable = true;
      this.__connected = false;
      this.__ended = false; this.__finSent = false; this.__endAfterFlush = false;
      this.__hadError = false; this.__handleClosed = false; this.__peerFin = false;
      // 重连即 fresh 柄（闩锁随旧柄消亡，server close() 同理）。
      this.__unrefLatched = false;
      this._handle = null;
      this.__pendW = []; this.__pendBytes = 0;
      this.__id = 0;
    }
    let port, host, cb, __noDelay, signal, sockPath = null, __blockList = null, __lookup = null, __halfOpen, __famOpt = 0;
    if (typeof args[0] === "object" && args[0] !== null) {
      if (args[0].fd !== undefined) {
        // node 口径：listen({fd}) 非法 fd 即异步 EINVAL（error 事件；真机实证）。
        const cbFd = typeof args[1] === "function" ? args[1] : null;
        if (cbFd) this.once("listening", cbFd);
        queueMicrotask(() => {
          const e = new Error(`listen EINVAL: invalid argument`);
          e.code = "EINVAL"; e.syscall = "listen"; e.errno = -4071;
          this.emit("error", e);
        });
        return this;
      }
      if (args[0].path !== undefined) {
        // node 口径：{path} 非串 → ERR_INVALID_ARG_TYPE（逐字形）；{path} 形走 unix socket。
        if (typeof args[0].path !== "string") {
          const __got = args[0].path === null ? "null" : (Array.isArray(args[0].path) ? "an instance of Array" : (typeof args[0].path === "object" ? `an instance of ${args[0].path.constructor?.name ?? "Object"}` : `type ${typeof args[0].path} (${String(args[0].path)})`));
          const e = new TypeError(`The "options.path" property must be of type string. Received ${__got}`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        sockPath = String(args[0].path); ({ noDelay: __noDelay, signal } = args[0]);
        cb = typeof args[1] === "function" ? args[1] : undefined;
      } else {
        ({ port, host = "127.0.0.1", family: __famOpt, noDelay: __noDelay, signal, blockList: __blockList, lookup: __lookup, allowHalfOpen: __halfOpen } = args[0]);
        // autoSelectFamily 校验（HE 校验族套件真机口径）：非 boolean → ARG_TYPE；
        // attemptTimeout 仅在生效 autoSelectFamily 下验 int [1,60000] → OUT_OF_RANGE。
        if (args[0].autoSelectFamily !== undefined && typeof args[0].autoSelectFamily !== "boolean") {
          const e = new TypeError(`The "options.autoSelectFamily" property must be of type boolean. Received type ${typeof args[0].autoSelectFamily} (${String(args[0].autoSelectFamily)})`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        if ((args[0].autoSelectFamily ?? __autoSelectFamily) && args[0].autoSelectFamilyAttemptTimeout !== undefined) {
          const __att = args[0].autoSelectFamilyAttemptTimeout;
          if (typeof __att !== "number" || !Number.isInteger(__att) || __att < 1 || __att > 60000) {
            const e = new RangeError(`The value of "options.autoSelectFamilyAttemptTimeout" is out of range. It must be an integer >= 1 && <= 60000. Received ${String(__att)}`);
            e.code = "ERR_OUT_OF_RANGE"; throw e;
          }
        }
        // node 口径：connect(server.address()) 形——address 对象（{address/family/port}）
        // 直作 options，host 缺省时取 address 键（ready-without-cb 套件点名）。
        if ((args[0].host === undefined || args[0].host === null) && typeof args[0].address === "string") host = args[0].address;
        if (__halfOpen !== undefined) this.allowHalfOpen = !!__halfOpen;
        // host 校验（真机逐字）：非串→ARG_TYPE（Array 显实例形）；含 \0→ARG_VALUE。
        if (host !== undefined && typeof host !== "string") {
          const __got = Array.isArray(host) ? "an instance of Array" : (host !== null && typeof host === "object" ? `an instance of ${host.constructor?.name ?? "Object"}` : `type ${typeof host} (${String(host)})`);
          const e = new TypeError(`The "options.host" property must be of type string. Received ${__got}`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        if (typeof host === "string" && host.includes("\0")) {
          const e = new TypeError(`The property 'options.host' must be a string without null bytes. Received '${host.replaceAll("\0", "\\x00")}'`);
          e.code = "ERR_INVALID_ARG_VALUE"; throw e;
        }
        // 不支持键（真机逐字；lib/net.js 黑名单）。
        for (const __k of ["objectMode", "readableObjectMode", "writableObjectMode"]) {
          if (args[0][__k] !== undefined) {
            const e = new TypeError(`The property 'options.${__k}' is not supported. Received ${String(args[0][__k])}`);
            e.code = "ERR_INVALID_ARG_VALUE"; throw e;
          }
        }
        cb = typeof args[1] === "function" ? args[1] : undefined;
      }
    } else if (typeof args[0] === "string" && (typeof args[1] !== "string" || args[1] === "")) {
      // connect(path[, cb])：首参串 + 次参非 host 串即 path 形。
      sockPath = args[0];
      cb = typeof args[1] === "function" ? args[1] : (typeof args[2] === "function" ? args[2] : undefined);
    } else {
      port = args[0];
      if (typeof args[1] === "string") { host = args[1]; cb = typeof args[2] === "function" ? args[2] : undefined; }
      else { host = "127.0.0.1"; cb = typeof args[1] === "function" ? args[1] : undefined; }
    }
    // adopt-UDS + connect({path}) 恒走 UDS（真机口径：path 在即 pipe，不看 adopt）。
    if (sockPath === null) {
      // node lookupAndConnect 校验序（localerror/boundsocket 套件真机逐字）：
      // adopt 门 → localAddress(isIP) → localPort(number) → port(type/range)。
      if (this.__adoptPort !== undefined &&
          (args[0].localAddress !== undefined || args[0].localPort !== undefined)) {
        const e = new TypeError(`The argument 'options' is invalid. localAddress and localPort cannot be used with an adopted bound socket. Received ${__netInspect(args[0])}`);
        e.code = "ERR_INVALID_ARG_VALUE"; throw e;
      }
      const __la = args[0].localAddress, __lp = args[0].localPort;
      if (__la && !isIP(__la)) {
        const e = new TypeError(`Invalid IP address: ${__la}`);
        e.code = "ERR_INVALID_IP_ADDRESS"; throw e;
      }
      if (__lp && typeof __lp !== "number") {
        const e = new TypeError(`The "options.localPort" property must be of type number. Received ${__netGot(__lp)}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (port !== undefined) { __vPortType(port); __vPort(port); }
      // node：host 非 IP 才走 lookup 系校验（IP 捷径跳过 dns 全链）——
      // lookup 函数型（options-lookup 套件）+ hints 掩码（connect-options-port）。
      // node 缺省 host = options.host || 'localhost'（connect({port}) 无 host 也走
      // 校验）；本仓底层连接面维持 127.0.0.1 缺省（remote* 表面记档），仅校验门
      // 按 node 有效 host 判定。掩码 1024|2048|256 与 dns 模块同值。
      const __effHost = (args[0].host === undefined || args[0].host === null || args[0].host === "")
        ? "localhost" : host;
      if (!isIP(__effHost)) {
        if (__lookup !== null && __lookup !== undefined && typeof __lookup !== "function") {
          const e = new TypeError(`The "options.lookup" property must be of type function. Received ${__netGot(__lookup)}`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        const __hv = args[0].hints || 0;
        if ((__hv & ~(1024 | 2048 | 256)) !== 0) {
          const e = new TypeError(`The argument 'hints' is invalid. Received ${Number(__hv) || 0}`);
          e.code = "ERR_INVALID_ARG_VALUE"; throw e;
        }
      }
    }
    if (cb) this.once("connect", cb);
    if (signal) {
      if (typeof signal.addEventListener !== "function") {
        const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      if (signal.aborted) {
        const e = new Error("The operation was aborted"); e.name = "AbortError"; e.code = "ABORT_ERR";
        queueMicrotask(() => this.destroy(e));
        return this;
      }
      {
        // 直调 addEventListener 须入侧表（abort-controller 套件 listenerCount 口径；
        // 原生忽略 once，handler 自摘）。构造期同 signal 已挂即复用（真机恰 1）。
        if (signal === this.__sockSig && this.__sockSigHandler !== undefined) {
          // 已监听，无需重复挂载。
        } else {
          const __connAbort = () => {
            __etRemove(signal, "abort", __connAbort);
            const e = new Error("The operation was aborted"); e.name = "AbortError"; e.code = "ABORT_ERR";
            // postAbort 形：套件在 abort 之后才挂 once('close')——destroy 的
            // error/close 必须推 microtask（node destroy 发射为 nextTick）。
            queueMicrotask(() => this.destroy(e));
          };
          signal.addEventListener("abort", __connAbort, { once: true });
          __etAdd(signal, "abort", __connAbort);
        }
      }
    }
    // node 口径：blockList 命中即 ERR_IP_BLOCKED（connect 前，不建连接）；
    // lookup 形：自定义解析（(host, opts, cb)；cb(null, addr[, family] | [{address, family}])）。
    const __doConnect = (finalHost) => {
      if (__blockList && typeof __blockList.check === "function" && finalHost !== null && __blockList.check(finalHost)) {
        const e = new Error(`IP(${finalHost}) is blocked by net.BlockList`);
        e.code = "ERR_IP_BLOCKED"; e.syscall = "connect";
        // HE 链中：blockList 命中即该地址尝试失败（不走 task，无 close 事件），
        // 直接推进下一地址；末位命中由 __heAdvance 收口 error+close。
        if (this.__heOnErr) { this.__heLast = e; this.__heAdvance(); return this; }
        queueMicrotask(() => this.destroy(e));
        return this;
      }
      this.__realConnect(finalHost, port, cb, __noDelay, signal, sockPath);
      return this;
    };
    // node 口径 localAddress（localaddress 套件）：本端源地址预 bind，
    // 经 native 第 4 参透传（UDS 形不用）。校验已在上游完成，此处只透传。
    this.__localAddrOpt = (args[0] !== null && typeof args[0] === "object" &&
      typeof args[0].localAddress === "string" && args[0].localAddress !== "")
      ? args[0].localAddress : null;
    // node 口径：预置 _handle 自带 connect 即走假柄短路（immediate-error 套件：
    // 注入假柄强制立即错；返回非零即 UV errno，异步 error + 销毁）。
    // 仅 TCP 形（UDS 沿旧路）；置于 lookup/HE 之前。
    if (sockPath === null && this._handle !== null && this._handle !== undefined &&
        typeof this._handle.connect === "function") {
      let __rc;
      try {
        __rc = this._handle.connect(null, host !== undefined ? String(host) : "", port);
      } catch (__e) {
        queueMicrotask(() => this.destroy(__e instanceof Error ? __e : new Error(String(__e))));
        return this;
      }
      if (__rc !== 0 && __rc !== undefined && __rc !== null) {
        const __code = { "-51": "ENETUNREACH" }[String(__rc)] ?? "UNKNOWN";
        const __e = new Error(`connect ${__code} ${host ?? ""}:${port ?? ""}`);
        __e.code = __code;
        __e.syscall = "connect";
        queueMicrotask(() => {
          this.__hadError = true;
          try { this.emit("error", __e); } catch { /* 无监听即抛，由调用方承接 */ }
          try { this.destroy(); } catch { /* gone */ }
        });
        return this;
      }
    }
    if (sockPath !== null) return __doConnect(null);
    if (typeof __lookup === "function") {
      let called = false;
      // autoSelectFamily 生效时以 all:true 拉全地址（autoselectfamily-default
      // 套件的 mocked lookup 只在 all:true 下给数组）→ 多地址走 __heTry 串行回落。
      const __heAll = (args[0].autoSelectFamily ?? __autoSelectFamily) === true;
      try {
        __lookup(String(host), { family: __famOpt || 0, hints: 0, all: __heAll }, (err, addr, family) => {
          if (called) return; called = true;
          if (err) { queueMicrotask(() => this.destroy(err)); return; }
          // node onlookup：family ∉ {4,6} → ERR_INVALID_ADDRESS_FAMILY（异步 error 事件，
          // 错误带 host/port 属性；options-lookup 套件 message 逐字）。
          const fam = Array.isArray(addr) ? addr[0].family : family;
          if (fam !== 4 && fam !== 6) {
            const e = new RangeError(`Invalid address family: ${fam} ${host}:${port}`);
            e.code = "ERR_INVALID_ADDRESS_FAMILY"; e.host = host; e.port = port;
            queueMicrotask(() => this.destroy(e)); return;
          }
          const first = Array.isArray(addr) ? addr[0].address : addr;
          if (__heAll && Array.isArray(addr) && addr.length > 1) {
            this.__heStart(addr, __doConnect, cb);
            return;
          }
          __doConnect(String(first));
        });
      } catch (e) { queueMicrotask(() => this.destroy(e)); return this; }
      return this;
    }
    return __doConnect(String(host));
  }
  // Happy Eyeballs 串行回落（autoSelectFamily-default 套件）：按 lookup 数组序
  // 逐地址尝试，中间失败（error/close）被 __heOnErr 钩吞掉，close 后复位重试
  // 下一地址；connect 成功即拆钩。记档：attemptTimeout 竞速未实现（回环
  // ECONNREFUSED 即时失败，套件不经超时路径）。
  __heStart(addrs, doConnect, cb) {
    this.__heSeq = { addrs, doConnect, cb, i: 0 };
    this.__heOnErr = () => {};
    this.__heTry();
  }
  __heTry() {
    const seq = this.__heSeq;
    // 统一走 __doConnect 闭包：blockList 校验每地址都生效（blocklist 套件
    // 多 IP 全屏蔽形——直接 __realConnect 会绕过拦截并停摆回落链）。
    seq.doConnect(seq.addrs[seq.i].address);
  }
  __heReset() {
    // 与 connect() 的 destroyed 复位分支同款（boundsocket reconnect-after-destroy 口径），
    // 但保留 __pendW——HE 失败尝试期间的用户写要带到最终连接（default 套件
    // write('request') 先于 connect 的缓冲形）。
    this.destroyed = false;
    this.readable = true; this.writable = true;
    this.__connected = false;
    this.__ended = false; this.__finSent = false; this.__endAfterFlush = false;
    this.__hadError = false; this.__handleClosed = false; this.__peerFin = false;
    this._handle = null;
    this.__id = 0;
  }
  __heAdvance() {
    const seq = this.__heSeq;
    seq.i++;
    if (seq.i < seq.addrs.length) {
      this.__heReset();
      this.__heOnErr = () => {};
      this.__heTry();
    } else {
      const last = this.__heLast;
      this.__heSeq = null; this.__heOnErr = null;
      this.destroyed = true; this._handle = null;
      queueMicrotask(() => { this.emit("error", last); this.emit("close", true); });
    }
  }
  // 真连接段（blockList/lookup 前置之后；adopt 预置 local 面）。
  __realConnect(finalHost, port, cb, __noDelay, signal, sockPath) {
    // UDS 面：remoteAddress/localAddress 恒 undefined（真机实证），address() 回 {}。
    // adopt 面：localAddress/localPort 预置 bound 值（connect 前后一致，真机实证）。
    // remote* 不在此发布（连接完成前恒 undefined，见构造注）；目标另存供报错整形。
    this.__targetHost = sockPath !== null ? null : String(finalHost);
    this.__targetPort = sockPath !== null ? null : Number(port);
    this.remoteAddress = undefined; this.remotePort = undefined; this.remoteFamily = undefined;
    if (this.__adoptPort !== undefined && sockPath === null) {
      this.localAddress = this.__adoptUds || this.__adoptHost;
      this.localPort = this.__adoptPort;
    }
    // node 口径：connect 即读写可达（write 缓冲至连接完成）
    this.readable = true; this.writable = true;
    // noDelay 经 native 直达 setsockopt（http agent 默认 true；Node net 默认 false）。
    // adopt-UDS：本端源 path 透 native 预 bind（localAddress 预置源 path）。
    if (sockPath !== null) this.__udsTarget = sockPath;
    // 建柄（_handle 存活期起点；连接前缓存的 keepAlive 随建即直通新柄）。
    this.__handleClosed = false;
    this._handle = this.__makeHandle();
    if (this.__kaState) { const [ke, kd, ki, kc] = this.__kaState; try { this._handle.setKeepAlive(ke, kd, ki, kc); } catch {} }
    if (sockPath !== null && this.__adoptUds) {
      this.localAddress = this.__adoptUds;
      this.__id = Number(__wjs2_net_connect(sockPath, "", this, this.__adoptUds));
    } else this.__id = sockPath !== null
      ? Number(__wjs2_net_connect(sockPath, "", this, false))
      : (this.__localAddrOpt !== null && this.__localAddrOpt !== undefined
        ? Number(__wjs2_net_connect(this.__targetHost, this.__targetPort, this, this.__localAddrOpt, __noDelay === true))
        : Number(__wjs2_net_connect(this.__targetHost, this.__targetPort, this, __noDelay === true)));
    // unref 闩锁结算（connect 前 unref 过即补调）。
    if (this.__unrefLatched === true && this.__id) {
      try { __wjs2_net_unref(this.__id); } catch { /* entry gone 即无事 */ }
    }
  }
  // 事件循环派发钩子（Rust dispatch 调用；kind/data 均为字符串）
  __ev(kind, payload) {
    switch (kind) {
      case "connect": {
        try {
          const o = JSON.parse(payload || "{}");
          // serde SocketAddr → "ip:port"（IPv6 为 "[ip]:port"）
          // adopt-TCP 不回填（真机 fd 复用：local 恒为 bound 值；OS 重分漂移时以预置为准）。
          if (typeof o.local === "string" && this.__adoptPort === undefined) {
            const m = o.local.match(/^\[?([^\]]+?)\]?:(\d+)$/);
            if (m) { this.localAddress = m[1]; this.localPort = Number(m[2]); }
          }
          if (this.localAddress !== undefined && this.localAddress !== null)
            this.localFamily = String(this.localAddress).includes(":") ? "IPv6" : "IPv4";
        } catch {}
        // 远端面在此发布（UDS 恒 undefined；TCP 取 __realConnect 存的目标）。
        this.remoteAddress = this.__targetHost ?? undefined;
        this.remotePort = this.__targetPort ?? undefined;
        this.remoteFamily = this.remoteAddress === undefined ? undefined
          : (String(this.remoteAddress).includes(":") ? "IPv6" : "IPv4");
        this.__connected = true;
        this.readable = true; this.writable = true;
        const pend = this.__pendW; this.__pendW = [];
        this.__pendBytes = 0;
        for (const [u8, cb2] of pend) {
          this.__nativeWrite(u8);
          // bytesWritten 已在 write 时同步计入 base，此处只补写队列记账。
          this.__sockQAdd(u8.length);
          if (cb2) queueMicrotask(cb2);
        }
        if (this.__endAfterFlush) {
          this.__endAfterFlush = false;
          if (this.__id) __wjs2_net_end(this.__id);
        }
        // HE 成功：拆回落钩（此后 close 走正常路径）。
        this.__heOnErr = null; this.__heSeq = null;
        this.emit("connect");
        // node 口径：connect 后同步发 'ready'（onconnection 原文；已接受端不走
        // 此分支故不发）。旧"暂不发射"记档（§4.126 并行 -10 强相关）作废：
        // 欠账清零要求语义到位；§4.126 禁并行压力负载后常规验证无复现。
        this.emit("ready");
        break;
      }
      case "data": {
        const u8 = __b64dec(payload);
        this.__ingestData(u8);
        break;
      }
      case "end": {
        this.readable = false;
        this.__peerFin = true;
        // 暂存先行（FIN 前到的字节先于 end 交付；无监听即留待迟挂冲刷）。
        try { this.__flushData(); } catch { /* 监听抛错不阻收尾 */ }
        // 池化空闲 socket 见 FIN 即销毁（半关不可复用；否则写端永活、条目永泄，
        // 10b https 保活案；Node 同样把 end 掉的 socket 踢出池）。
        if (this.__inPool) {
          this.destroy();
          break;
        }
        // setEncoding 残余字节 flush（分包切断的多字节尾在 end 前补齐）
        if (this.__dec) {
          const rest = this.__dec.end();
          if (rest) this.emit("data", rest);
        }
        this.emit("end");
        // Node 口径：非 allowHalfOpen 时收 FIN 即自动回 FIN（'close' 随后）
        if (!this.allowHalfOpen) this.__nativeEnd();
        // G11 半开案：allowHalfOpen 持有半开即不再续命（真机同款——读停转后
        // 空闲句柄不 ref 循环；k9/k12 实证：ref() 也留不住，真机照常退出）。
        // 递延一轮：end 监听内同步 destroy/auto-end 的走正常收尾通道，此处只收
        // 无动作的稳定半开；写侧仍可用，Close 派发照常 purge 结算。
        if (this.allowHalfOpen && !this.destroyed && this.__id) {
          const __s = this;
          queueMicrotask(() => {
            if (__s.allowHalfOpen && !__s.destroyed && __s.__id && __s.__peerFin) {
              try { __wjs2_net_halfhold(__s.__id); } catch { /* entry gone 即无事 */ }
            }
          });
        }
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        const se = __netErr(o.code, o.msg);
        this.__hadError = true;
        // 已销毁 socket 的迟到 teardown 噪声不 chạm 用户监听（真机口径：destroy 后
        // 底层 RST/EOF 竞速错不再派发；write-after-close 套件双块并发下必现 flaky）。
        if (this.destroyed) break;
        // node connect 系标配：syscall + errno（uv 负值；ENOENT=-2/EACCES=-13/ECONNREFUSED=-61
        // /ENOTSOCK=-38/EADDRNOTAVAIL=-49；未知 -4094；ENOTFOUND 系 getaddrinfo -3008）。
        se.syscall = "connect";
        se.errno = { ENOENT: -2, EACCES: -13, ECONNREFUSED: -61, ENOTSOCK: -38, EADDRNOTAVAIL: -49, EINVAL: -22, EADDRINUSE: -48 }[o.code] ?? -4094;
        if (o.code === "ENOTFOUND") {
          se.syscall = "getaddrinfo";
          se.errno = -3008;
        }
        // node connect 错误消息形："connect CODE <target>"（target=host:port 或 path）。
        // native msg 已是 "CODE: <os>"，此处按目标重塑（expectsError 逐字断言面）；
        // getaddrinfo 形保留原文（dns-error 套件 node 口径）。
        if (typeof o.msg === "string" && !o.msg.startsWith("connect ") && !o.msg.startsWith("IP(") &&
            !o.msg.startsWith("getaddrinfo ")) {
          const rh = this.__targetHost ?? this.remoteAddress;
          const rp = this.__targetPort ?? this.remotePort;
          const tgt = this.__udsTarget ?? ((rh !== undefined && rh !== null && rp !== undefined && rp !== null) ? `${rh}:${rp}` : null);
          if (tgt) se.message = `connect ${o.code} ${tgt}`;
        }
        // HE 串行回落：中间地址的连接失败被钩吞（不落用户监听），close 后重试。
        if (this.__heOnErr) { this.__heLast = se; break; }
        // node：socket 错误经 destroy(err) 递送——'error' 监听里 destroyed 已为 true
        // （修前 false；tls/net 回环套件 `socket.destroyed` 断言现形）。
        this.destroyed = true; this.writable = false; this.readable = false;
        this.emit("error", se);
        break;
      }
      case "close":
        // HE：失败尝试的 close → 推进下一地址（或末位失败收口）。
        if (this.__heOnErr) { this.__heAdvance(); break; }
        this.destroyed = true; this._handle = null; this.__dataBuf = [];
        this.emit("close", this.__hadError === true); break;
    }
  }
  // node 口径：pending = 尚无可用句柄——连接中 true、连接完成 false、
  // close 后**仍为 true**（test-net-connect-buffer 'close' 处理器点名）。
  // node 口径：net.Socket 亦有 writableEnded（remove-header 套件点名
  // response.socket.writableEnded；end() 后 true）。
  // node 口径 writableLength（socket 写队列积压 + 连接前缓冲；reuse-drained
  // 套件点名 req.socket.writableLength === 0 / > 0）。
  get writableLength() { return (this.__sockQ ?? 0) + (this.__pendBytes ?? 0); }
  get writableEnded() { return this.__ended === true; }
  get pending() { return !this.__connected || this.destroyed; }
  get connecting() { return !this.__connected && !this.destroyed && this.__id > 0; }
  get readyState() {
    if (this.destroyed) return "closed";
    // 真机：new Socket() 未连接即 "open"（构造 readable/writable 初始真；connect 前即 open）。
    // 连接中（__id 已发）才 "opening"。
    if (!this.__connected && this.__id) return "opening";
    if (this.readable && this.writable) return "open";
    return this.readable ? "readOnly" : "writeOnly";
  }
  // node Socket async iterable（for await over data；end/close 终结、error 拒绝）
  [Symbol.asyncIterator]() {
    const st = { q: [], wake: null, done: false, err: null };
    const push = (fn, v) => { st[fn] && 0; };
    const onData = (c) => { st.q.push({ v: c }); if (st.wake) { st.wake(); st.wake = null; } };
    const onDone = () => { st.done = true; if (st.wake) { st.wake(); st.wake = null; } };
    const onErr = (e) => { st.err = e; st.done = true; if (st.wake) { st.wake(); st.wake = null; } };
    this.on("data", onData);
    this.on("end", onDone);
    this.on("close", onDone);
    this.once("error", onErr);
    const cleanup = () => { this.off("data", onData); this.off("end", onDone); this.off("close", onDone); this.off("error", onErr); };
    return {
      next: () => new Promise((resolve, reject) => {
        const step = () => {
          if (st.q.length) resolve({ value: st.q.shift().v, done: false });
          else if (st.err) { const e = st.err; st.err = null; cleanup(); reject(e); }
          else if (st.done) { cleanup(); resolve({ done: true }); }
          else st.wake = step;
        };
        step();
      }),
      return: () => { cleanup(); this.destroy(); return Promise.resolve({ done: true }); },
      throw: (e) => { cleanup(); this.destroy(); return Promise.reject(e); },
    };
  }
  // node 口径（lib/net.js writeGeneric + stream Writable.write）：
  // destroyed/!writable → 有 cb 走 cb(err)+error 事件（返回 false），无 cb 才同步抛；
  // err 形状：write after end / connect 未完成 → ERR_STREAM_WRITE_AFTER_END（writableLength 0），
  // 其余 destroyed → ERR_STREAM_DESTROYED。§4.119 同源：包装/状态检查不吞场景口径。
  __writeErr(cb2) {
    const ended = this.__ended === true || this.__finSent === true;
    const code = ended ? "ERR_STREAM_WRITE_AFTER_END" : "ERR_STREAM_DESTROYED";
    const e = __netErr(code, ended ? "write after end" : "Cannot call write after a stream was destroyed");
    if (typeof cb2 === "function") { queueMicrotask(() => { try { cb2.call(this, e); } catch {} }); return false; }
    throw e;
  }
  write(data, enc, cb) {
    const cb2 = typeof enc === "function" ? enc : cb;
    // net 口径（write-after-end-nt 套件真机形）：本地已 end 且对端已 FIN 后再写
    // → Error EPIPE 'This socket has been ended by the other party'——cb 与 error
    // 事件都下一 tick。两条件缺一不可：仅对端 FIN（writable 套件 'end' 后写）
    // 与仅本地 end（G7 write-after-end 形 STREAM_WRITE_AFTER_END）都不走此路。
    if (this.__peerFin === true && this.__ended === true && this.allowHalfOpen !== true) {
      const e = new Error("This socket has been ended by the other party");
      e.code = "EPIPE"; e.errno = 32; e.syscall = "write";
      if (typeof cb2 === "function") queueMicrotask(() => { try { cb2.call(this, e); } catch {} });
      queueMicrotask(() => this.emit("error", e));
      return false;
    }
    // 真机逐字（writable.js _write）：仅 null → ERR_STREAM_NULL_VALUES（undefined 落
    // ARG_TYPE 'Received undefined'）；chunk 类型校验先于 after-end/destroyed 状态检查。
    if (data === null) {
      const e = new TypeError("May not write null values to stream");
      e.code = "ERR_STREAM_NULL_VALUES";
      if (typeof cb2 === "function") { queueMicrotask(() => { try { cb2.call(this, e); } catch {} }); return false; }
      throw e;
    }
    const u8 = __chunkU8(data, typeof enc === "string" ? enc : undefined);
    if (this.destroyed || !this.writable) return this.__writeErr(cb2);
    // node 口径（write-after-close 套件双形，真机 26 实测均为异步 error 事件非同步抛）：
    // 已连接但 _handle 被置空后写 → ERR_SOCKET_CLOSED('Socket is closed')；
    // _handle.close() 后（柄关而对象在）写 → Error('write EBADF'，win 系 EPIPE)。
    if (this.__connected && this._handle === null) {
      const e2 = new Error("Socket is closed"); e2.code = "ERR_SOCKET_CLOSED";
      if (typeof cb2 === "function") queueMicrotask(() => { try { cb2.call(this, e2); } catch {} });
      queueMicrotask(() => this.emit("error", e2));
      return false;
    }
    if (this.__handleClosed === true) {
      const e = new Error(`write ${typeof process !== "undefined" && process.platform === "win32" ? "EPIPE" : "EBADF"}`);
      if (typeof cb2 === "function") queueMicrotask(() => { try { cb2.call(this, e); } catch {} });
      queueMicrotask(() => this.emit("error", e));
      return false;
    }
    this.__bwBase += u8.length;
    this.__bwSub(u8.length);
    if (!this.__connected) {
      // node 口径：连接完成前 write 缓冲（connect 完成时按序冲刷）
      this.__pendW.push([u8, cb2]);
      this.__pendBytes += u8.length;
      return u8.length + this.__pendBytes - u8.length <= this._writableState.highWaterMark;
    }
    this.__nativeWrite(u8);
    // 记账：底层同步写队列，无 flush 语义，回调即刻（排空/drain 见 __sockQAdd）。
    const __wret = this.__sockQAdd(u8.length);
    if (cb2) queueMicrotask(cb2);
    return __wret;
  }
