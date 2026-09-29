import { EventEmitter } from "node:events";
import errors from 'node:internal/errors';
import { validatePort } from 'node:internal/validators';
import __dnsDefault from "node:dns";
const Buffer = globalThis.Buffer;

const {
  codes: {
    ERR_MISSING_ARGS,
  },
} = errors;

function __b64dec(s) {
  const bin = atob(s);
  const u8 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  return u8;
}
function __dgramBufErr(what, data) {
  // send 首参校验（真机逐字）：
  // scalar → `"buffer"` + 自身 Received；数组元非法 → `"buffer list arguments"` +
  // 外层数组 Received（`Received an instance of Array`）。
  const e = new TypeError(`The ${what} argument must be of type string or an instance ` +
    `of Buffer, TypedArray, or DataView. Received ${__dgramReceived(data)}`);
  e.code = "ERR_INVALID_ARG_TYPE";
  return e;
}
function __toU8(data) {
  // node 口径：send 首参收 string/Buffer/视图/**数组**（逐段拼接，空数组即 0 字节，
  // send-callback-multi-buffer 系套件）。
  if (Array.isArray(data)) {
    const parts = [];
    for (const m of data) {
      if (typeof m === "string") parts.push(new TextEncoder().encode(m));
      else if (m instanceof Uint8Array) parts.push(m);
      else if (m instanceof ArrayBuffer) parts.push(new Uint8Array(m));
      else if (ArrayBuffer.isView(m)) parts.push(new Uint8Array(m.buffer, m.byteOffset, m.byteLength));
      else throw __dgramBufErr('"buffer list arguments"', data);
    }
    const total = parts.reduce((n, p) => n + p.length, 0);
    const out = new Uint8Array(total);
    let at = 0;
    for (const p of parts) { out.set(p, at); at += p.length; }
    return out;
  }
  if (typeof data === "string") return new TextEncoder().encode(data);
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  throw __dgramBufErr('"buffer"', data);
}
// offset/length 越界（真机逐字，send/sendto 双收口）。
function __dgramBounds(u8, off, len) {
  const o = off ?? 0;
  if (o < 0 || o > u8.length) {
    const e = new RangeError('"offset" is outside of buffer bounds');
    e.code = "ERR_BUFFER_OUT_OF_BOUNDS"; throw e;
  }
  if (len !== undefined && (len < 0 || o + len > u8.length)) {
    const e = new RangeError('"length" is outside of buffer bounds');
    e.code = "ERR_BUFFER_OUT_OF_BOUNDS"; throw e;
  }
  return u8.subarray(o, len === undefined ? u8.length : o + len);
}
function __netErr(code, msg) {
  const e = new Error(msg);
  e.code = code;
  return e;
}
// validators 口径 Received 形（ttl 校验逐字点名）。
function __dgramReceived(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  const t = typeof v;
  if (t === "string") return `type string ('${v}')`;
  if (t === "boolean") return `type boolean (${v})`;
  if (t === "number") return `type number (${v})`;
  if (t === "object") return Array.isArray(v) ? "an instance of Array" : `an instance of ${v.constructor?.name ?? "Object"}`;
  if (t === "function") return `function ${v.name}`;
  return `type ${t} (${String(v)})`;
}
function __dgramTtl(name, ttl, min, max) {
  if (typeof ttl !== "number") {
    const e = new TypeError(`The "ttl" argument must be of type number. Received ${__dgramReceived(ttl)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  if (!Number.isInteger(ttl) || ttl < min || ttl > max) throw __sysErr(name, 'EINVAL');
  return ttl;
}
// 系统调用失败形（真机 `EINVAL: addMembership EINVAL` 口径；task 侧失败走
// Error 事件见模块头注，此处只做 JS 侧可同步判定的格式校验）。
function __sysErr(syscall, code) {
  const e = new Error(`${syscall} ${code}`);
  e.code = code;
  return e;
}
function __parseIPv4(s) {
  const parts = String(s).split('.');
  if (parts.length !== 4) return null;
  const nums = [];
  for (const p of parts) {
    if (!/^\d+$/.test(p)) return null;
    const n = Number(p);
    if (n > 255) return null;
    nums.push(n);
  }
  return nums;
}
// v6 字面量粗验（够 setMulticastInterface 非法形判定；route 用原文）。
// 注：手写 hex 判定，不用正则字面量（本模块内该位置正则字面量曾伴随
// require 链 crash 高频现形；根因为 require_cjs_file 的 make_fn 裸值窗口，
// 已修，此处保守形态保留）。
function __parseIPv6Hex(g) {
  if (g.length < 1 || g.length > 4) return false;
  for (let i = 0; i < g.length; i++) {
    const c = g.charCodeAt(i);
    const dig = c >= 48 && c <= 57;
    const lo = c >= 97 && c <= 102;
    const hi = c >= 65 && c <= 70;
    if (!dig && !lo && !hi) return false;
  }
  return true;
}
function __parseIPv6(s) {
  if (typeof s !== "string" || !s.includes(":")) return false;
  const chk = (arr) => {
    for (const g of arr) { if (!__parseIPv6Hex(g)) return false; }
    return true;
  };
  if (s.includes("::")) {
    if (s.indexOf("::") !== s.lastIndexOf("::")) return false;
    const parts = s.split("::");
    const lg = parts[0] === "" ? [] : parts[0].split(":");
    const rg = parts[1] === "" ? [] : parts[1].split(":");
    if (lg.length + rg.length > 7) return false;
    return chk(lg) && chk(rg);
  }
  const gs = s.split(":");
  return gs.length === 8 && chk(gs);
}
// 组播地址校验（v4 限 224/4，v6 限 ff00::/8；跨类型错配即 EINVAL，真机口径）。
function __membershipAddrs(multi, iface, type, syscall) {
  if (multi === undefined) throw new ERR_MISSING_ARGS('multicastAddress');
  const v4 = (typeof multi === 'string') ? __parseIPv4(multi) : null;
  let v6 = false;
  if (v4) {
    if (v4[0] < 224 || v4[0] > 239) throw __sysErr(syscall, 'EINVAL');
  } else if (typeof multi === 'string' && multi.includes(':')) {
    v6 = true;
    if (!/^ff/i.test(multi)) throw __sysErr(syscall, 'EINVAL');
  } else {
    throw __sysErr(syscall, 'EINVAL');
  }
  if (type === 'udp4' && v6) throw __sysErr(syscall, 'EINVAL');
  if (type === 'udp6' && !v6) throw __sysErr(syscall, 'EINVAL');
  return { multi: String(multi), iface: iface === undefined ? (v6 ? '0' : '0.0.0.0') : String(iface) };
}

class Socket extends EventEmitter {
  constructor(typeOrOptions, cb) {
    super();
    let type;
    if (typeof typeOrOptions === "string") type = typeOrOptions;
    else if (typeOrOptions && typeof typeOrOptions === "object") type = typeOrOptions.type;
    if (type !== "udp4" && type !== "udp6") {
      const e = new TypeError(`Bad socket type specified. Valid types are: udp4, udp6`);
      e.code = "ERR_SOCKET_BAD_TYPE";
      throw e;
    }
    this.type = type;
    this.__id = 0;
    this.__bound = false;
    // 构造缓冲选项校验（createSocket-type 套件：非 number 即 ARG_TYPE）。
    if (typeOrOptions && typeof typeOrOptions === "object") {
      for (const k of ["recvBufferSize", "sendBufferSize"]) {
        const v = typeOrOptions[k];
        if (v !== undefined && typeof v !== "number") {
          const e = new TypeError(`The "${k}" argument must be of type number. Received ${__dgramReceived(v)}`);
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
      }
    }
    // 连接/关闭状态（connect 连接机 + close 后 NOT_RUNNING 门，见下）。
    this.__connecting = false;
    this.__closed = false;
    // 绑定窗口旗（bind 错误走 ExceptionWithHostPort 形，见 __evErrorBind）。
    this.__binding = false;
    // 绑定失败旗（bind-error-repeat：错误处理器内可立即重绑；陈旧 Close
    // 按"重绑与否"分流，见 __ev close）。
    this.__bindFailed = false;
    this.__opts = (typeOrOptions && typeof typeOrOptions === "object") ? typeOrOptions : null;
    this.__addr = null;
    this.__connected = false;
    this.__remote = null;
    this.__pending = [];
    this.__blockList = (typeOrOptions && typeof typeOrOptions === "object") ? (typeOrOptions.sendBlockList ?? null) : null;
    this.__recvBlockList = (typeOrOptions && typeof typeOrOptions === "object") ? (typeOrOptions.receiveBlockList ?? null) : null;
    // 自定义 DNS（custom-lookup 套件）：非函数即同步 ARG_TYPE。
    if (typeOrOptions && typeof typeOrOptions === "object" && typeOrOptions.lookup !== undefined) {
      if (typeof typeOrOptions.lookup !== "function") {
        const e = new TypeError(`The "lookup" argument must be of type function. Received ${__dgramReceived(typeOrOptions.lookup)}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      this.__lookup = typeOrOptions.lookup;
    } else {
      this.__lookup = null;
    }
    this.__sendSeq = 0;
    this.__sendCbs = new Map();
    this.__sendTargets = new Map();
    // 发送队列记账（getSendQueueSize/Count 套件）：seq→字节数，在途 +
    // __pending 待刷（bind 前挂起）合并统计；完成（sendok/senderr）即摘。
    this.__sendQueue = new Map();
    // 引用计数（`process.getActiveResourcesInfo` 口径：缺省 ref，unref 即摘；
    // unref 先于 bind 时 bind 完成不登记，见 unref-in-cluster 套件）。
    this.__ref = true;
    // signal 面（close-signal 套件）：非法即同步 ARG_TYPE；abort 即关；
    // 预 abort 即微任务关（close 事件仍异步到）。
    if (typeOrOptions && typeof typeOrOptions === "object" && typeOrOptions.signal !== undefined) {
      const sig = typeOrOptions.signal;
      if (!sig || typeof sig !== "object" || typeof sig.aborted !== "boolean" || typeof sig.addEventListener !== "function") {
        const e = new TypeError(`The "options.signal" property must be of type AbortSignal. Received ${String(sig)}`);
        e.code = "ERR_INVALID_ARG_TYPE";
        throw e;
      }
      if (sig.aborted) {
        queueMicrotask(() => this.close());
      } else {
        sig.addEventListener("abort", () => this.close(), { once: true });
      }
    }
    if (typeof cb === "function") this.on("message", cb);
    // 派发钩子预绑定（dispatch 以 global 为 this 调用，§4.34 坑一）
    this.__ev = this.__ev.bind(this);
  }
  bind(...args) {
    // node 口径（test-dgram-bind）：已绑（含绑定窗口）再 bind 同步抛
    // ERR_SOCKET_ALREADY_BOUND "Socket is already bound"；成功返回 this。
    // 失败后重绑（bind-error-repeat）：上一代 task 已死，清旧旗旧柄再起
    //（含 __binding 窗口旗，否则失败后重绑被窗口门误拦报 ALREADY_BOUND）。
    if (this.__bindFailed) {
      this.__bindFailed = false;
      this.__binding = false;
      this.__closed = false;
      this.__id = 0;
    }
    if (this.__id || this.__bound || this.__binding) {
      throw __netErr("ERR_SOCKET_ALREADY_BOUND", "Socket is already bound");
    }
    let port = 0, address = null, cb = null;
    if (typeof args[0] === "object" && args[0] !== null) {
      port = args[0].port ?? 0;
      address = args[0].address ?? null;
      cb = typeof args[1] === "function" ? args[1] : null;
    } else {
      // bind([port][, address][, cb])——缺省位依次前移
      let i = 0;
      if (typeof args[i] === "number") { port = args[i]; i++; }
      if (typeof args[i] === "string") { address = args[i]; i++; }
      if (typeof args[i] === "function") cb = args[i];
    }
    // node 口径：回调挂 'listening'，失败（errorMonitor）即摘除——否则失败重绑
    // 逐次累积监听，第 11 次触发 MaxListenersExceededWarning（bind-error-repeat）。
    if (cb) {
      const removeListeners = () => {
        this.removeListener(EventEmitter.errorMonitor, removeListeners);
        this.removeListener("listening", onListening);
      };
      const onListening = () => {
        removeListeners();
        Reflect.apply(cb, this, []);
      };
      this.on(EventEmitter.errorMonitor, removeListeners);
      this.on("listening", onListening);
    }
    this.__binding = true;
    // 地址解析（真机 handle.lookup 口径：自定义 lookup 必经，默认走同步族匹配；
    // 通配符在自定义 lookup 下同样过一遍，custom-lookup 套件点名）。
    const rawAddr = address === null ? (this.type === "udp6" ? "::" : "0.0.0.0") : String(address);
    const finishBind = (err, ip) => {
      // 关后即弃（真机 handle 置空同口径），并复位窗口旗以免卡死重绑。
      if (this.__closed) { this.__binding = false; return; }
      if (err) {
        this.__binding = false;
        this.__evError(err);
        return;
      }
      // 族匹配解析（node 口径：udp4 socket bind('localhost') 落 127.0.0.1——tokio
      // 直接 bind('localhost') 会挑 ::1，family 错乱即后续 send EINVAL）。
      this.__addr = this.__resolveAddr(ip, false);
      // 用户原文地址（bind 错误形 e.address 用；归一化串在字面量下同值）。
      this.__bindAddr = address === null ? null : String(address);
      this.__id = Number(__wjs2_dgram_bind(Number(port), this.__addr, this, this.__bindFlags()));
      // bind 前的 unref()（无句柄时只记旗）落到原生句柄（unref-in-cluster 套件）。
      if (this.__id && this.__ref === false) __wjs2_net_unref(this.__id);
    };
    if (this.__lookup) {
      try {
        this.__lookup.call(this, rawAddr, this.type === "udp4" ? 4 : 6, (e, ip) => {
          if (e) finishBind(e);
          else finishBind(null, ip);
        });
      } catch (e) { finishBind(e); }
    } else if (this.__isNumericIP(rawAddr)) {
      finishBind(null, rawAddr);
    } else {
      // 默认经 JS dns.lookup（custom-lookup 第二块：全局 mock 必经；
      // Node handle.lookup 异步口径；数字 IP 上已直通免一跳）。
      let dnsLookup = null;
      try { dnsLookup = __dnsDefault.lookup; } catch {}
      if (typeof dnsLookup !== "function") {
        finishBind(null, rawAddr);
      } else {
        try {
          dnsLookup.call(this, rawAddr, this.type === "udp4" ? 4 : 6, (e, ip) => {
            if (e) finishBind(e);
            else finishBind(null, ip);
          });
        } catch (e) { finishBind(e); }
      }
    }
    return this;
  }
  // 绑定预选项位（bit0 reusePort/bit1 ipv6Only/bit2 reuseAddr；随 bind 进内核）。
  __bindFlags() {
    const o = this.__opts;
    return ((o && o.reusePort ? 1 : 0) | (o && o.ipv6Only ? 2 : 0) | (o && o.reuseAddr ? 4 : 0));
  }
  // 数字 IP 判定（bindSync/connectSync 不做 DNS；v6 允许 %scope 后缀）。
  __isNumericIP(s) {
    if (typeof s !== "string") return false;
    if (__parseIPv4(s)) return true;
    return __parseIPv6(s.split("%")[0]);
  }
  bindSync(...args) {
    // 校验先于状态门（已绑 socket 传非法参仍报参数错，bind-sync 套件点名）；
    // 校验失败不落状态、可重调。
    let port = 0, address = null;
    if (args[0] !== undefined) {
      const o = args[0];
      if (!o || typeof o !== "object" || Array.isArray(o)) {
        const e = new TypeError(`The "options" argument must be of type object. Received ${__dgramReceived(o)}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
      port = o.port ?? 0;
      address = o.address ?? null;
    }
    if (address !== null && typeof address !== "string") {
      const e = new TypeError(`The "address" argument must be of type string. Received ${__dgramReceived(address)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    // bind 端口容 0（临时端口；-1 照旧 BAD_PORT）。
    validatePort(port, 'Port', true);
    if (address !== null && !this.__isNumericIP(address)) {
      const e = new TypeError(`The argument 'address' must be a numeric IP address. Received '${address}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    // 已绑（含绑定窗口）再绑即 ALREADY_BOUND。
    if (this.__id || this.__bound) {
      throw __netErr("ERR_SOCKET_ALREADY_BOUND", "Socket is already bound");
    }
    const bindAddr = address ?? (this.type === "udp6" ? "::" : "0.0.0.0");
    const res = JSON.parse(__wjs2_dgram_bind_sync(Number(port), bindAddr, "", this, this.__bindFlags()));
    if (res.error) {
      const e = new Error(`${res.error}: ${bindAddr}`);
      e.code = res.error;
      e.syscall = 'bind';
      throw e;
    }
    this.__id = res.id;
    this.__bound = true;
    this.__binding = false;
    this.__closed = false;
    this.__resAdd();
    this.__rinfo = { address: res.addr, port: res.port };
    // 挂起队列留待 task listening 事件刷出（单刷，不与本函数双发）；
    // 'listening' 同样由该事件派发（仍异步；close 抢先即被 __closed 门吞）。
    return { address: res.addr, family: res.addr.includes(":") ? "IPv6" : "IPv4", port: res.port };
  }
  connectSync(port, address) {
    validatePort(port, 'Port', false);
    if (this.__connected || this.__connecting) {
      throw __netErr('ERR_SOCKET_DGRAM_IS_CONNECTED', 'Already connected');
    }
    if (address === undefined || address === null) {
      address = this.type === "udp4" ? "127.0.0.1" : "::1";
    }
    if (typeof address !== "string") {
      const e = new TypeError(`The "address" argument must be of type string. Received ${__dgramReceived(address)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (!this.__isNumericIP(address)) {
      const e = new TypeError(`The argument 'address' must be a numeric IP address. Received '${address}'`);
      e.code = "ERR_INVALID_ARG_VALUE"; throw e;
    }
    if (this.__blockList) {
      const fam = address.includes(":") ? "ipv6" : "ipv4";
      let blocked = false;
      try { blocked = this.__blockList.check(address, fam); } catch {}
      if (blocked) {
        const e = new Error(`connect ${address} blocked`);
        e.code = "ERR_IP_BLOCKED"; throw e;
      }
    }
    if (!this.__id && !this.__bound) this.bindSync();
    else if (!this.__bound) throw __netErr("ERR_SOCKET_ALREADY_BOUND", "Socket is already bound");
    const family = address.includes(":") ? "IPv6" : "IPv4";
    this.__remote = { address, port, family };
    this.__connecting = false;
    this.__connected = true;
    __wjs2_dgram_sockopt(this.__id, JSON.stringify({ op: 'connect', addr: `${address}:${port}` }));
    // 'connect' 仍递延（先关即抑制，与 bindSync listening 同口径）。
    queueMicrotask(() => { if (!this.__closed) this.emit("connect"); });
  }
  send(...args) {
    // 尾函数恒为回调（各形态通用；Node 位置舞步的等价形）。
    let cb = null;
    if (typeof args[args.length - 1] === "function") cb = args.pop();
    const msg = args[0];
    if (this.__connected) {
      // 已连接：(msg[, offset, length])——port/address 禁止（IS_CONNECTED）。
      // 校验序（真机）：msg buffer 形态先行（`send(23)` 报 ARG_TYPE buffer，
      // 非 IS_CONNECTED），再判连接态，最后 offset/length 越界。
      const u8 = __toU8(msg);
      let off = 0, len;
      if (args.length >= 2) {
        if (typeof args[1] === "function") { /* (msg, cb)——cb 已摘 */ }
        else if (args.length === 2) { /* (msg, 误放回调)——Node 忽略 */ }
        else { off = args[1] ?? 0; len = args[2]; }
      }
      if ((args[3] !== undefined && args[3] !== null) || (args[4] !== undefined && args[4] !== null)) {
        throw __netErr('ERR_SOCKET_DGRAM_IS_CONNECTED', 'Already connected');
      }
      // (msg, offset, address-string) 形：位置 2 是串即地址（非 length），同抛。
      if (args.length >= 3 && typeof args[2] === "string") {
        throw __netErr('ERR_SOCKET_DGRAM_IS_CONNECTED', 'Already connected');
      }
      const body = __dgramBounds(u8, off, len);
      if (!this.__bound) {
        this.__pending.push({ __send: true, msg: body, port: undefined, address: undefined, cb });
        if (!this.__id && !this.__binding) this.bind();
        return this;
      }
      return this.__doSend(body, undefined, undefined, cb);
    }
    // 未连接：位置 3/4 判形（Node 原文规则）——有 port/address 即
    // (msg, offset, length, port, address)，否则 (msg, port, address)。
    let off, len, port, address;
    // Node 原文真值规则：address(pos4) 真，或 port(pos3) 真且非函数 → 五元形。
    if (args[4] || (args[3] && typeof args[3] !== "function")) {
      off = args[1]; len = args[2]; port = args[3]; address = args[4];
    } else {
      port = args[1]; address = args[2];
    }
    if (typeof address === "function") address = undefined;
    const body = __dgramBounds(__toU8(msg), off, len);
    // node 口径：端口同步校验先于地址形态（`(buf,0,6)` 报 BAD_PORT 而非
    // address 错；RangeError 即时抛，不进挂起队列）。
    // 无目标且未 connect → validatePort(undefined) 先炸 ERR_SOCKET_BAD_PORT。
    // 隐式绑定只发生在"有目标"的 send 上。
    if (port !== undefined) {
      validatePort(port, 'Port', false);
    } else if (address === undefined && !this.__connected) {
      validatePort(port, 'Port', false);
    }
    if (address !== undefined && address !== null && typeof address !== "string") {
      const e = new TypeError(`The "address" argument must be of type string. Received ${__dgramReceived(address)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (!this.__bound) {
      // node/libuv 口径：未绑 socket send 即隐式 bind（port 0），send 参数挂起
      // 到 listening 刷出（真机：cb 后 address().port > 0）。绑定窗口内（__id
      // 已有）直接挂起不重复 bind。
      this.__pending.push({ __send: true, msg: body, port, address, cb });
      if (!this.__id && !this.__binding) this.bind();
      return this;
    }
    return this.__doSend(body, port, address, cb);
  }
  // legacy sendto（严格六元形；校验逐字，sendto 套件点名）。
  sendto(buffer, offset, length, port, address, callback) {
    const needNum = (name, v) => {
      if (typeof v !== "number") {
        const e = new TypeError(`The "${name}" argument must be of type number. Received ${__dgramReceived(v)}`);
        e.code = "ERR_INVALID_ARG_TYPE"; throw e;
      }
    };
    needNum("offset", offset);
    needNum("length", length);
    needNum("port", port);
    if (typeof address !== "string") {
      const e = new TypeError(`The "address" argument must be of type string. Received ${__dgramReceived(address)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    return this.send(buffer, offset, length, port, address, callback);
  }
  __doSend(msg, port, address, cb) {
    let target;
    if (port === undefined && address === undefined) {
      // connect 后的无地址发送走默认远端；未 connect 即端口校验错（真机口径）。
      if (!this.__connected) validatePort(port, 'Port', false);
      target = "";
    } else {
      validatePort(port, 'Port', false);
      target = `${this.__resolveAddr(address ?? 'localhost')}:${port}`;
    }
    // 发送黑名单（blocklist 套件：错经回调/事件异步到，不抛同步）。
    {
      const tip = target === "" ? this.__remote?.address : String(address ?? 'localhost');
      let dip = String(tip ?? "");
      try {
        const entries = JSON.parse(__wjs2_dns_lookup(dip));
        if (Array.isArray(entries) && entries.length) dip = entries[0].address;
      } catch {}
      const clean = dip.startsWith("[") && dip.endsWith("]") ? dip.slice(1, -1) : dip;
      if (this.__blockList && typeof this.__blockList.check === "function") {
        let blocked = false;
        try { blocked = this.__blockList.check(clean, clean.includes(":") ? "ipv6" : "ipv4"); } catch {}
        if (blocked) {
          const err = __netErr('ERR_IP_BLOCKED', `send ${clean} blocked`);
          if (cb) queueMicrotask(() => cb(err));
          else this.__evError(err);
          return this;
        }
      }
    }
    const u8 = __toU8(msg);
    // send 失败按 seq 路由回本回调（无回调才走 error 事件，node 口径）；
    // 目标随 seq 记录（senderr 的 e.address/e.port 回填）。
    const seq = ++this.__sendSeq;
    this.__sendTargets.set(seq, { address: address ?? this.__remote?.address, port: port ?? this.__remote?.port });
    this.__sendQueue.set(seq, u8.length);
    __wjs2_dgram_send(this.__id, u8, target, seq);
    if (cb) this.__sendCbs.set(seq, cb);
    return this;
  }
  // 发送队列（真机语义：在途 + bind 前挂起合并；关闭即清零见 close）。
  getSendQueueSize() {
    let n = 0;
    for (const l of this.__sendQueue.values()) n += l;
    for (const p of this.__pending) if (p.__send) n += p.msg.length;
    return n;
  }
  getSendQueueCount() {
    let n = this.__sendQueue.size;
    for (const p of this.__pending) if (p.__send) n++;
    return n;
  }
  // fire-and-forget sockopt（失败走 Error 事件）。未 bind（__id 未分配）即
  // 同步 EBADF（真机口径 `setMulticastLoopback EBADF`；静默挂起是偏差——
  // 绑定窗口内 __id 已有，照常发 task）。文案用 JS 侧方法名。
  __sockopt(op) {
    this.__healthCheck();
    if (!this.__id) {
      const name = { setBroadcast: 'setBroadcast', setTtl: 'setTTL', setMulticastTtl: 'setMulticastTTL', setMulticastLoop: 'setMulticastLoopback', join: 'addMembership', leave: 'dropMembership', connect: 'connect', disconnect: 'disconnect' }[op.op] ?? op.op;
      throw __sysErr(name, 'EBADF');
    }
    __wjs2_dgram_sockopt(this.__id, JSON.stringify(op));
  }
  connect(...args) {
    let port, address = 'localhost', cb = null;
    if (args.length === 1 && typeof args[0] === 'object' && args[0] !== null) {
      port = args[0].port;
      if (args[0].address !== undefined) address = args[0].address;
    } else {
      port = args[0];
      if (args[1] !== undefined) {
        if (typeof args[1] === 'function') cb = args[1];
        else address = args[1];
      }
      if (typeof args[2] === 'function') cb = args[2];
    }
    validatePort(port, 'Port', false);
    // node 口径（lib/dgram.js）：非 DISCONNECTED 即 IS_CONNECTED（含 CONNECTING 窗口）。
    if (this.__connected || this.__connecting) {
      throw __netErr('ERR_SOCKET_DGRAM_IS_CONNECTED', 'Already connected');
    }
    // 地址解析后即查发送黑名单（blocklist 套件：错经回调/事件异步到，不抛同步）。
    let dispAddr = String(address);
    let family = dispAddr.includes(':') ? 'IPv6' : 'IPv4';
    try {
      const entries = JSON.parse(__wjs2_dns_lookup(dispAddr));
      if (Array.isArray(entries) && entries.length) {
        dispAddr = entries[0].address;
        family = entries[0].family === 6 ? 'IPv6' : 'IPv4';
      }
    } catch { /* keep verbatim */ }
    if (this.__blockList && typeof this.__blockList.check === "function") {
      let blocked = false;
      try { blocked = this.__blockList.check(dispAddr, family === 'IPv6' ? 'ipv6' : 'ipv4'); } catch {}
      if (blocked) {
        const err = __netErr('ERR_IP_BLOCKED', `connect ${dispAddr} blocked`);
        if (cb) queueMicrotask(() => cb(err));
        else this.__evError(err);
        return;
      }
    }
    this.__connecting = true;
    if (cb) this.once('connect', cb);
    if (!this.__id && !this.__binding) this.bind();
    // 展示用远端（发送时 task 再解，见模块头注；解析已在黑名单检查前完成）。
    this.__remote = { address: dispAddr, port, family };
    // 窗口期内（lookup 未归）排队，listening 刷出；不直调（__id 未落）。
    if (this.__id) __wjs2_dgram_sockopt(this.__id, JSON.stringify({ op: 'connect', addr: `${this.__resolveAddr(String(address))}:${port}` }));
    else this.__pending.push({ op: 'connect', addr: `${this.__resolveAddr(String(address))}:${port}` });
  }
  disconnect() {
    if (!this.__connected) {
      throw __netErr('ERR_SOCKET_DGRAM_NOT_CONNECTED', 'Not connected');
    }
    this.__connected = false;
    this.__connecting = false;
    this.__remote = null;
    if (this.__id) __wjs2_dgram_sockopt(this.__id, JSON.stringify({ op: 'disconnect' }));
  }
  remoteAddress() {
    if (!this.__connected || !this.__remote) {
      throw __netErr('ERR_SOCKET_DGRAM_NOT_CONNECTED', 'Not connected');
    }
    return { ...this.__remote };
  }
  // close 后调用即 NOT_RUNNING（membership 套件点名；close 本体幂等静默系既有记档）。
  __healthCheck() {
    if (this.__closed) throw __netErr('ERR_SOCKET_DGRAM_NOT_RUNNING', 'Not running');
  }
  addMembership(multicastAddress, multicastInterface) {
    this.__healthCheck();
    const { multi, iface } = __membershipAddrs(multicastAddress, multicastInterface, this.type, 'addMembership');
    if (!this.__id && !this.__binding) this.bind();
    if (this.__id) __wjs2_dgram_sockopt(this.__id, JSON.stringify({ op: 'join', multi, iface }));
    else this.__pending.push({ op: 'join', multi, iface });
  }
  dropMembership(multicastAddress, multicastInterface) {
    this.__healthCheck();
    const { multi, iface } = __membershipAddrs(multicastAddress, multicastInterface, this.type, 'dropMembership');
    if (!this.__id && !this.__binding) this.bind();
    if (this.__id) __wjs2_dgram_sockopt(this.__id, JSON.stringify({ op: 'leave', multi, iface }));
    else this.__pending.push({ op: 'leave', multi, iface });
  }
  // SSM 入组/退组（membership 套件点名校验；成功路径走 tasksetsockopt）。
  __ssmAddrs(sourceAddress, groupAddress, syscall) {
    if (typeof sourceAddress !== "string") {
      const e = new TypeError(`The "sourceAddress" argument must be of type string. Received ${__dgramReceived(sourceAddress)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    if (typeof groupAddress !== "string") {
      const e = new TypeError(`The "groupAddress" argument must be of type string. Received ${__dgramReceived(groupAddress)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    // 组须为本族组播地址（'0' 等非组播即 EINVAL，真机口径）。
    const v4 = __parseIPv4(groupAddress);
    let v6 = false;
    if (v4) {
      if (v4[0] < 224 || v4[0] > 239) throw __sysErr(syscall, 'EINVAL');
    } else if (groupAddress.includes(':')) {
      v6 = true;
      if (!/^ff/i.test(groupAddress)) throw __sysErr(syscall, 'EINVAL');
    } else {
      throw __sysErr(syscall, 'EINVAL');
    }
    if (this.type === 'udp4' && v6) throw __sysErr(syscall, 'EINVAL');
    if (this.type === 'udp6' && !v6) throw __sysErr(syscall, 'EINVAL');
    return { source: sourceAddress, group: groupAddress, v6 };
  }
  addSourceSpecificMembership(sourceAddress, groupAddress, interfaceAddress) {
    this.__healthCheck();
    const { source, group, v6 } = this.__ssmAddrs(sourceAddress, groupAddress, 'addSourceSpecificMembership');
    if (!this.__id && !this.__binding) this.bind();
    if (this.__id) __wjs2_dgram_sockopt(this.__id, JSON.stringify({ op: 'joinSource', source, group, iface: interfaceAddress ?? (v6 ? '0' : '0.0.0.0') }));
    else this.__pending.push({ op: 'joinSource', source, group, iface: interfaceAddress ?? (v6 ? '0' : '0.0.0.0') });
  }
  dropSourceSpecificMembership(sourceAddress, groupAddress, interfaceAddress) {
    this.__healthCheck();
    const { source, group, v6 } = this.__ssmAddrs(sourceAddress, groupAddress, 'dropSourceSpecificMembership');
    if (!this.__id && !this.__binding) this.bind();
    if (this.__id) __wjs2_dgram_sockopt(this.__id, JSON.stringify({ op: 'leaveSource', source, group, iface: interfaceAddress ?? (v6 ? '0' : '0.0.0.0') }));
    else this.__pending.push({ op: 'leaveSource', source, group, iface: interfaceAddress ?? (v6 ? '0' : '0.0.0.0') });
  }
  setMulticastInterface(interfaceAddress) {
    this.__healthCheck();
    if (typeof interfaceAddress !== "string") {
      const e = new TypeError(`The "interfaceAddress" argument must be of type string. Received ${__dgramReceived(interfaceAddress)}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
    // 族错配同步 EINVAL（udp4 配 v6 形；真机平台三态之一，此处取确定态）。
    // 其余非法形（''/'undefined'/非组播）同样 EINVAL；合法交 task。
    const s = interfaceAddress;
    const v6 = s.includes(':');
    if (this.type === 'udp4' && v6) throw __sysErr('setMulticastInterface', 'EINVAL');
    if (this.type === 'udp4') {
      const v = __parseIPv4(s);
      if (!v || v[0] >= 224) throw __sysErr('setMulticastInterface', 'EINVAL');
    } else {
      const bare = s.split('%')[0];
      if (bare === '' || !__parseIPv6(bare)) throw __sysErr('setMulticastInterface', 'EINVAL');
    }
    this.__sockopt({ op: 'multicastInterface', addr: s });
  }
  // buffer size 四方法（node 口径：未绑即 ERR_SOCKET_BUFFER_SIZE，文案
  // "Could not get or set buffer size: uv_recv/send_buffer_size returned
  // EBADF (bad file descriptor)" 逐字；绑后 get/setsockopt 同步直调）。
  __bufSizeErr(kind) {
    return __netErr("ERR_SOCKET_BUFFER_SIZE", `Could not get or set buffer size: uv_${kind}_buffer_size returned EBADF (bad file descriptor)`);
  }
  __bufSize(kind, size) {
    if (!this.__bound) throw this.__bufSizeErr(kind);
    const v = __wjs2_dgram_bufsize(this.__id, kind, size);
    if (v === "") throw this.__bufSizeErr(kind);
    return Number(v);
  }
  getRecvBufferSize() { return this.__bufSize("recv"); }
  setRecvBufferSize(size) { this.__bufSize("recv", size); }
  getSendBufferSize() { return this.__bufSize("send"); }
  setSendBufferSize(size) { this.__bufSize("send", size); }
  setBroadcast(flag) {
    this.__sockopt({ op: 'setBroadcast', v: Boolean(flag) });
  }
  setTTL(ttl) {
    __dgramTtl('setTTL', ttl, 1, 255);
    this.__sockopt({ op: 'setTtl', v: ttl });
    return ttl;
  }
  setMulticastTTL(v) {
    __dgramTtl('setMulticastTTL', v, 0, 255);
    this.__sockopt({ op: 'setMulticastTtl', v });
    return v;
  }
  setMulticastLoopback(flag) {
    // 真机回显原值（16→16，非 Boolean 归一；loopback 套件点名）。
    this.__sockopt({ op: 'setMulticastLoop', v: Boolean(flag) });
    return flag;
  }
  // 事件循环派发钩子（Rust dispatch 以 global 为 this 调用，须预绑定）
  __ev(kind, payload) {
    switch (kind) {
      case "listening": {
        // close 后到的迟到事件丢弃（close-is-not-callback：send 即关，
        // 绑定完成事件后到，此时挂起队列早随 task 消亡，刷出即对空表抛错）。
        if (this.__closed) break;
        this.__binding = false;
        const o = JSON.parse(payload);
        this.__bound = true;
        this.__resAdd();
        this.__rinfo = { address: o.addr, port: o.port };
        for (const p of this.__pending) {
          if (p.__send) this.__doSend(p.msg, p.port, p.address, p.cb);
          else __wjs2_dgram_sockopt(this.__id, JSON.stringify(p));
        }
        this.__pending = [];
        // 构造选项 buffer sizes（node：选项在 handle 创建期生效；此处绑后即设，
        // macOS getsockopt 精确回读，Linux 回读翻倍记平台差）。
        if (this.__opts) {
          if (this.__opts.recvBufferSize !== undefined) __wjs2_dgram_bufsize(this.__id, "recv", this.__opts.recvBufferSize);
          if (this.__opts.sendBufferSize !== undefined) __wjs2_dgram_bufsize(this.__id, "send", this.__opts.sendBufferSize);
        }
        this.emit("listening");
        break;
      }
      case "connect": {
        if (this.__closed) break;
        this.__connecting = false;
        this.__connected = true;
        this.emit("connect");
        break;
      }
      case "message": {
        const o = JSON.parse(payload);
        // 接收黑名单：命中即静默丢弃（blocklist 套件；check 副作用由调用方承载）。
        if (this.__recvBlockList && typeof this.__recvBlockList.check === "function") {
          let blocked = false;
          try { blocked = this.__recvBlockList.check(o.address, o.family === 6 ? "ipv6" : "ipv4"); } catch {}
          if (blocked) break;
        }
        const msg = Buffer.from(__b64dec(o.data));
        const rinfo = { address: o.address, port: o.port, family: o.family, size: msg.length };
        this.emit("message", msg, rinfo);
        break;
      }
      case "error": {
        const o = JSON.parse(payload);
        this.__evErrorBind(o.code, o.msg);
        break;
      }
      case "sendok": {
        // send 完成：回调 (null, bytes) 异步触发（§4.74——同步完成也走微任务）。
        const o = JSON.parse(payload);
        const cb = this.__sendCbs.get(o.seq);
        if (cb) {
          this.__sendCbs.delete(o.seq);
          queueMicrotask(() => cb(null, o.bytes));
        }
        this.__sendTargets.delete(o.seq);
        this.__sendQueue.delete(o.seq);
        break;
      }
      case "senderr": {
        // send 失败：有回调走回调（msgsize 套件 EMSGSIZE 形），无回调走 error 事件
        //（node 口径）；e.address/e.port 由 JS 侧按 seq 记录的发送目标回填。
        const o = JSON.parse(payload);
        const rec = this.__sendTargets.get(o.seq);
        const e = __netErr(o.code, o.msg);
        if (rec) {
          e.address = rec.address;
          e.port = rec.port;
        }
        const cb = this.__sendCbs.get(o.seq);
        if (cb) {
          this.__sendCbs.delete(o.seq);
          queueMicrotask(() => cb(e));
        } else {
          this.__evError(e);
        }
        this.__sendTargets.delete(o.seq);
        this.__sendQueue.delete(o.seq);
        break;
      }
      case "close": {
        // 陈旧代 Close（失败后已重绑）：跳过状态改写与派发，不吞新代；
        // purge 照旧由分发侧按 id 做（见 net dispatch）。
        if (this.__binding && !this.__bindFailed) break;
        this.__bound = false;
        this.__binding = false;
        this.__bindFailed = false;
        this.__connected = false;
        this.__connecting = false;
        this.__closed = true;
        this.__remote = null;
        this.emit("close");
        break;
      }
    }
  }
  // 目标地址归一：主机名经 DNS 取本 socket 族匹配的地址（node 口径——udp4 socket
  // 发 'localhost' 落 127.0.0.1，macOS localhost 首选 ::1，不归一即 EINVAL）；
  // v6 字面量加方括号（tokio ToSocketAddrs 需 "[::1]:port" 形）。
  __resolveAddr(addr, bracket = true) {
    let s = String(addr);
    try {
      const entries = JSON.parse(__wjs2_dns_lookup(s));
      if (Array.isArray(entries) && entries.length) {
        const fam = this.type === "udp4" ? 4 : 6;
        const hit = entries.find((e) => e.family === fam) ?? entries[0];
        s = hit.address;
      }
    } catch { /* 字面量/解析失败保留原文 */ }
    // bind 路径禁括号：tokio ("[::1]", port) 元组经 ToSocketAddrs 走 DNS 而非
    // 字面量，即 EADDRNOTAVAIL；裸 "::"/"::1" 才直解（address 套件现形）。
    // send/connect 目标串仍要括号（"ip:port" 拼接口径）。
    if (!bracket) return s;
    return s.includes(":") && !s.startsWith("[") ? `[${s}]` : s;
  }
  address() {
    // node 口径（test-dgram-address 末块）：未绑 address() 即
    // `Error EBADF "getsockname EBADF"`（uv getsockname 直透，非 NOT_RUNNING）；
    // 已关闭（曾绑）即 NOT_RUNNING（async-dispose 套件点名）。
    if (this.__closed) throw __netErr('ERR_SOCKET_DGRAM_NOT_RUNNING', 'Not running');
    if (!this.__bound) {
      const e = __netErr("EBADF", "getsockname EBADF");
      throw e;
    }
    return { address: this.__rinfo.address, port: this.__rinfo.port, family: String(this.__rinfo.address).includes(":") ? "IPv6" : "IPv4" };
  }
  // error 事件无监听时经 nextTick 抛（运行时 uncaughtException 探针收敛；
  // 分发内直抛绕过监听走 fatal，bind-error-callback 套件点名）。
  __evError(e) {
    if (this.listenerCount("error") > 0) { this.emit("error", e); return; }
    process.nextTick(() => { throw e; });
  }
  // 绑定期错误整形（ExceptionWithHostPort 口径 `bind CODE addr` + address/port
  // 属性，error-message-address 套件逐字点名；非绑定期错误原样）。
  // 失败旗为重绑留门（bind-error-repeat）。
  __evErrorBind(code, msg) {
    if (this.__binding) {
      this.__bindFailed = true;
      const e = new Error(`bind ${code} ${this.__bindAddr ?? this.__addr}`);
      e.code = code;
      e.syscall = 'bind';
      e.address = this.__bindAddr ?? this.__addr;
      e.port = undefined;
      this.__evError(e);
      return;
    }
    this.__evError(__netErr(code, msg));
  }
  // 显式处置（async-dispose 套件）：关后决议；重复处置照决议（幂等）。
  async [Symbol.asyncDispose]() {
    this.close();
  }
  [Symbol.dispose]() {
    this.close();
  }
  close(cb) {
    if (typeof cb === "function") this.once("close", cb);
    // 同步落关闭旗（后继 addMembership/connect 等健康检查即时生效，不等 task）。
    this.__closed = true;
    this.__binding = false;
    this.__resDel();
    // 在途发送计数随 socket 消亡（完成事件永不到）；__pending 挂起保留
    // （bind-error-repeat 重绑后刷出，既有行为不动）。
    this.__sendQueue.clear();
    // 从未绑定（无 task）即微任务派 close（真机未绑 close 仍异步派发；
    // 有 task 走 task Close 事件独派，不双发）。
    if (!this.__id) queueMicrotask(() => this.emit("close"));
    else __wjs2_net_destroy(this.__id);
    return this;
  }
  ref() {
    this.__ref = true;
    if (this.__bound && !this.__closed) this.__resAdd();
    if (this.__id) __wjs2_net_ref(this.__id);
    return this;
  }
  unref() {
    this.__ref = false;
    this.__resDel();
    if (this.__id) __wjs2_net_unref(this.__id);
    return this;
  }
  // 存活资源登记（`process.getActiveResourcesInfo()` 读全局表；UDPWrap）。
  __resAdd() {
    if (this.__ref !== false && !this.__closed) {
      globalThis.__wjs2ActiveResources ??= new Map();
      globalThis.__wjs2ActiveResources.set(this, "UDPWrap");
    }
  }
  __resDel() {
    globalThis.__wjs2ActiveResources?.delete(this);
  }
}

export function createSocket(options, cb) {
  return new Socket(options, cb);
}
export { Socket };
const __api = { Socket, createSocket };
export default __api;
