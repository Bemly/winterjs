// ── 10f：Node 精确校验（internal/validators 口径，套件逐字断言）─────────────
function __dnsReceived(v) {
  if (v === null) return 'null';
  if (v === undefined) return 'undefined';
  const t = typeof v;
  // invalidArgTypeHelper 口径（字符串带类型前缀；ARG_VALUE 另用 inspect 形）
  if (t === 'string') return `type string ('${v}')`;
  if (t === 'number' || t === 'boolean' || t === 'bigint') return `type ${t} (${String(v)})`;
  if (t === 'symbol') return `type symbol (${String(v)})`;
  if (t === 'function') return `function ${v.name || '(anonymous)'}`;
  const name = v.constructor?.name;
  if (typeof name === 'string' && name !== '') return `an instance of ${name}`;
  return 'an instance of Object';
}
function __dnsInspectValue(v) {
  // kInspect 口径（ERR_INVALID_ARG_VALUE 的 Received 部分）：字符串单引裸形
  if (typeof v === 'string') return `'${v}'`;
  if (v === null || v === undefined) return String(v);
  const t = typeof v;
  if (t === 'number' || t === 'boolean' || t === 'bigint') return String(v);
  return __dnsReceived(v);
}
// dns 标志位（c-ares AI_* 口径；hints 掩码校验用）
const V4MAPPED = 2048;
const ADDRCONFIG = 1024;
const ALL = 256;
function __dnsValidateLookupOptions(options) {
  // 真机口径：falsy options 整体跳过（`lookup(h, -0, cb)` 绿）；数字 family
  // 由 lookup 入口先转 { family }（custom-lookup 套件 `lookup(h, 4, cb)` 真机绿）。
  if (!options) return undefined;
  if (typeof options !== "object" || options === null) {
    throw __dnsErrInvalidArgType("options", "of type object", options);
  }
  const { family, hints, order } = options;
  let fam = 0;
  if (family !== undefined) {
    if (family === 0 || family === 4 || family === 6 || family === "IPv4" || family === "IPv6") {
      fam = family === "IPv4" ? 4 : family === "IPv6" ? 6 : family;
    } else {
      throw __dnsErrInvalidArgType("family", "of type number", family);
    }
  }
  if (hints !== undefined) {
    if (typeof hints !== "number" || !Number.isInteger(hints) || (hints & ~(V4MAPPED | ADDRCONFIG | ALL)) !== 0) {
      const e = new TypeError(`The argument 'hints' is invalid. Received ${String(hints)}`);
      e.code = "ERR_INVALID_ARG_VALUE";
      throw e;
    }
  }
  if (order !== undefined && order !== "verbatim" && order !== "ipv4first" && order !== "ipv6first") {
    throw __dnsErrInvalidArgType("order", "of type string", order);
  }
  return { ...options, family: fam };
}
function __dnsErrInvalidArgType(name, expected, actual) {
  const e = new TypeError(`The "${name}" argument must be ${expected}. Received ${__dnsReceived(actual)}`);
  e.code = 'ERR_INVALID_ARG_TYPE';
  return e;
}
function __dnsValidateString(v, name) {
  if (typeof v !== 'string') throw __dnsErrInvalidArgType(name, 'of type string', v);
}
function __dnsValidateFunction(v, name) {
  if (typeof v !== 'function') throw __dnsErrInvalidArgType(name, 'of type function', v);
}
// isIP（Node internal/net 口径子集：v4 严格四段 / v6 压缩形 + v4 尾；
// 纯 JS，线性扫描无回溯——REDOS 探针（10 万冒号）安全）
function __dnsIsV4(s) {
  const parts = s.split('.');
  if (parts.length !== 4) return false;
  for (const p of parts) {
    if (p.length === 0 || p.length > 3) return false;
    let n = 0;
    for (let i = 0; i < p.length; i++) {
      const c = p.charCodeAt(i);
      if (c < 48 || c > 57) return false;
      n = n * 10 + (c - 48);
    }
    if (n > 255) return false;
  }
  return true;
}
function __dnsIsV6(s) {
  const dc = s.indexOf('::');
  if (dc !== -1 && s.indexOf('::', dc + 2) !== -1) return false;
  const head = dc === -1 ? s : s.slice(0, dc);
  const tail = dc === -1 ? null : s.slice(dc + 2);
  return __dnsIsV6Inner(head, tail);
}
function __dnsIsV6Inner(head, tail) {
  // 段收集（含 v4 尾：末段 dotted-quad 占两组）
  const collect = (part) => {
    if (part === '') return [];
    const segs = part.split(':');
    const out = [];
    for (let i = 0; i < segs.length; i++) {
      const g = segs[i];
      if (i === segs.length - 1 && g.indexOf('.') !== -1) {
        if (!__dnsIsV4(g)) return null;
        out.push('v4', 'v4');
      } else {
        if (g.length === 0 || g.length > 4) return null;
        for (let j = 0; j < g.length; j++) {
          const c = g.charCodeAt(j);
          if (!((c >= 48 && c <= 57) || (c >= 65 && c <= 70) || (c >= 97 && c <= 102))) return null;
        }
        out.push(g);
      }
    }
    return out;
  };
  const h = collect(head);
  if (h === null) return false;
  if (tail === null) return h.length === 8;
  const t = collect(tail);
  if (t === null) return false;
  // '::' 填充剩余组（含 '::' 本身 total 0；满 8 组时不得再有 '::'）
  return h.length + t.length <= 7;
}
function __dnsIsIP(s) {
  if (typeof s !== 'string') return 0;
  if (__dnsIsV4(s)) return 4;
  if (__dnsIsV6(s)) return 6;
  return 0;
}
function __entries(hostname) {
  let raw;
  try {
    raw = JSON.parse(__wjs_dns_lookup(String(hostname)));
  } catch (e) {
    throw __dnsMakeError("getaddrinfo", "EAI_AGAIN", hostname);
  }
  if (!Array.isArray(raw) || raw.length === 0) {
    throw __dnsMakeError("getaddrinfo", "ENOTFOUND", hostname);
  }
  return raw;
}
function __filter(entries, family) {
  if (family === 4) return entries.filter((e) => e.family === 4);
  if (family === 6) return entries.filter((e) => e.family === 6);
  return entries;
}
function __lookupCore(hostname, options) {
  const all = !!(options && options.all);
  const family = options && (options.family === 4 || options.family === 6) ? options.family : 0;
  const picked = __filter(__entries(hostname), family);
  if (picked.length === 0) {
    throw __dnsMakeError("getaddrinfo", "ENOTFOUND", hostname);
  }
  return all ? picked.map((e) => ({ address: e.address, family: e.family })) : picked[0];
}
export function lookup(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  // 数字 family 重载（custom-lookup 套件：lookup(host, 4, cb) 直通真机；
  // 旧"数字即抛"系伪语义，真机 26.8.2 实测接受）。
  if (typeof options === "number") options = { family: options };
  const norm = __dnsValidateLookupOptions(options);
  // 真机口径：falsy 先行（ARG_VALUE），再类型（ARG_TYPE）——`lookup('')`
  // 系同步抛，非回调错（test-dns.js；旧黑盒按回调错编码，已翻转见 §4.x）
  if (!hostname) {
    const e = new TypeError(`The argument 'hostname' is invalid. Received ${__dnsReceived(hostname)}`);
    e.code = "ERR_INVALID_ARG_VALUE";
    throw e;
  }
  __dnsValidateString(hostname, "hostname");
  __dnsValidateFunction(cb, "callback");
  queueMicrotask(() => {
    try {
      const r = __lookupCore(hostname, norm ?? options);
      if (Array.isArray(r)) cb(null, r);
      else cb(null, r.address, r.family);
    } catch (e) {
      cb(e);
    }
  });
}
// ── 10d 深件：hickory 查询（callback + promise 双形态，Node 错误形状）──
function __qkind(kind, hostname) {
  const text = __wjs_dns_query(kind, String(hostname));
  return JSON.parse(text);
}
// Node DNSException 全文口径：`${syscall} ${code} ${hostname}`（无 detail；
// `foo.onion` 用例逐字钉死，见 test-dns.js）
function __dnsMakeError(syscall, code, hostname) {
  const err = new Error(`${syscall} ${code} ${hostname}`);
  err.code = code;
  err.syscall = syscall;
  err.hostname = String(hostname);
  return err;
}
function __dnsSyscallOf(kind) {
  return `query${kind[0].toUpperCase()}${kind.slice(1)}`;
}
function __qerr(kind, hostname, e) {
  const msg = String((e && e.message) || e);
  const m = msg.match(/^([A-Z_]+):\s*/);
  const code = m ? m[1] : "ENOTFOUND";
  throw __dnsMakeError(__dnsSyscallOf(kind), code, hostname);
}
function __mkQuery(kind, syscall) {
  const fn_ = function (hostname, cb) {
    __dnsValidateString(hostname, "name");
    __dnsValidateFunction(cb, "callback");
    queueMicrotask(() => {
      try { cb(null, __qkind(kind, hostname)); }
      catch (e) {
        try { __qerr(kind, hostname, e); } catch (err) { cb(err); }
      }
    });
  };
  return fn_;
}
export const resolveCname = __mkQuery("cname", "resolveCname");
export const resolveMx = __mkQuery("mx", "resolveMx");
export const resolveNs = __mkQuery("ns", "resolveNs");
export const resolveTxt = __mkQuery("txt", "resolveTxt");
export const resolveSrv = __mkQuery("srv", "resolveSrv");
export const resolvePtr = __mkQuery("ptr", "resolvePtr");
export function resolveAny(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  if (__dnsOverrideSnapshot()) { __moduleQueryCustom("any", "queryAny", hostname, cb, false); return; }
  __mkQuery("any", "resolveAny")(hostname, cb);
}
export function resolve(hostname, rrtype, cb) {
  if (typeof rrtype === "function") { cb = rrtype; rrtype = "A"; }
  __dnsValidateString(hostname, "name");
  if (typeof rrtype !== "string") throw __dnsErrInvalidArgType("rrtype", "of type string", rrtype);
  __dnsValidateFunction(cb, "callback");
  const t = String(rrtype || "A").toUpperCase();
  const map = { CNAME: "cname", MX: "mx", NS: "ns", TXT: "txt", SRV: "srv", PTR: "ptr", ANY: "any", A: "a", AAAA: "aaaa", SOA: "soa" };
  const kind = map[t];
  if (!kind) {
    const err = new TypeError(`dns.resolve: unknown rrtype '${rrtype}'`);
    err.code = "EBADNAME";
    throw err;
  }
  queueMicrotask(() => {
    let raw;
    try { raw = JSON.parse(__wjs_dns_query(kind, hostname)); }
    catch (e) {
      try { __qerr(kind, hostname, e); } catch (err) { cb(err); }
      return;
    }
    const norm = __dnsNormResult(kind, raw);
    if (!Array.isArray(norm) || norm.length === 0) {
      cb(__dnsMakeError(__dnsSyscallOf(kind), "ENODATA", hostname));
      return;
    }
    cb(null, kind === "soa" ? norm[0] : norm);
  });
}
export function reverse(ip, cb) {
  if (typeof cb !== "function") throw new TypeError("dns.reverse: callback must be a function");
  queueMicrotask(() => {
    try { cb(null, __qkind("reverse", ip)); }
    catch (e) {
      try { __qerr("reverse", ip, e); } catch (err) { cb(err); }
    }
  });
}
export function getServers() { return JSON.parse(__wjs_dns_servers_get()); }
// setServers 全套校验（Node internal/dns/utils 口径：forEach 语义跳 holes；
// getter 副作用（length 截断）天然收敛——forEach 只读一次 length；
// 端口 0 视默认；非法 IP → ERR_INVALID_IP_ADDRESS；越界端口 → ERR_SOCKET_BAD_PORT）
function __dnsValidatePort(p) {
  if (!Number.isInteger(p) || p < 0 || p > 65535) {
    const e = new RangeError(`Port should be >= 0 and < 65536. Received ${String(p)}`);
    e.code = "ERR_SOCKET_BAD_PORT";
    throw e;
  }
  return p;
}
function __dnsParseServers(servers) {
  if (!Array.isArray(servers)) {
    throw __dnsErrInvalidArgType("servers", "an instance of Array", servers);
  }
  const out = [];
  servers.forEach((serv, index) => {
    if (typeof serv !== "string") {
      throw __dnsErrInvalidArgType(`servers[${index}]`, "of type string", serv);
    }
    if (__dnsIsIP(serv) !== 0) {
      out.push([__dnsIsIP(serv), serv, 53]);
      return;
    }
    const br = /^\[([^[\]]*)\]/.exec(serv);
    if (br) {
      const v = __dnsIsIP(br[1]);
      if (v !== 0) {
        const rest = serv.slice(br[0].length);
        let port = 53;
        if (rest !== "") {
          const pm = /^:(\d+)$/.exec(rest);
          // 尾巴非 :digits 即 Node 同款宽容（replace 取 $2 为空 → 53）
          port = pm ? __dnsValidatePort(Number(pm[1])) : 53;
          if (port === 0) port = 53;
        }
        out.push([v, br[1], port]);
        return;
      }
    }
    const sp = /(^.+?)(?::(\d+))?$/.exec(serv);
    if (sp) {
      const host = sp[1];
      const v = __dnsIsIP(host);
      if (v !== 0) {
        let port = 53;
        if (sp[2] !== undefined) {
          port = __dnsValidatePort(Number(sp[2]));
          if (port === 0) port = 53;
        }
        out.push([v, host, port]);
        return;
      }
    }
    const e = new TypeError(`Invalid IP address: ${serv}`);
    e.code = "ERR_INVALID_IP_ADDRESS";
    throw e;
  });
  return out;
}
function __dnsPublicForm([ver, ip, port]) {
  if (!port || port === 53) return ip;
  return ver === 6 ? `[${ip}]:${port}` : `${ip}:${port}`;
}
function __dnsWireForm([ver, ip, port]) {
  if (!port || port === 53) return ip;
  return ver === 6 ? `[${ip}]:${port}` : `${ip}:${port}`;
}
export function setServers(servers) {
  const triples = __dnsParseServers(servers);
  __wjs_dns_servers_set(JSON.stringify(triples.map(__dnsPublicForm)));
  // 全局 override 快照（模块级 resolveAny/4/6/Soa 遇 override 走定制单问；
  // 调用时判定，microtask 排空前已 set 即命中——phase10d 行 89/102 序安全）
  __dnsUserServers = triples;
}
let __dnsUserServers = null;
function __dnsOverrideSnapshot() {
  return __dnsUserServers ? __dnsUserServers.map((t) => t.slice()) : null;
}
// 模块级定制查询（全局 override 快照；pending 计数无——模块 setServers 永不抛）
function __moduleQueryCustom(kind, syscall, hostname, cb, wantTtl) {
  const st = { servers: __dnsOverrideSnapshot(), timeout: 5000, tries: 2, pending: 0, jobs: new Map() };
  const wire = JSON.stringify(st.servers.map(__dnsWireForm));
  const id = Number(__wjs_dns_job_start(kind, hostname, wire, "5000", "2"));
  const rec = { poll: 0 };
  st.jobs.set(id, rec);
  const settle = (err, val) => {
    if (!st.jobs.has(id)) return;
    st.jobs.delete(id);
    clearInterval(rec.poll);
    cb(err, val);
  };
  rec.poll = setInterval(() => {
    let r;
    try { r = JSON.parse(__wjs_dns_job_poll(String(id))); }
    catch { return; }
    if (r.status === "pending") return;
    if (r.status === "err") {
      settle(__dnsMakeError(syscall, r.code, hostname), undefined);
      return;
    }
    const norm = __dnsNormResult(kind, r.value);
    if (!Array.isArray(norm) || norm.length === 0) {
      settle(__dnsMakeError(syscall, "ENODATA", hostname), undefined);
      return;
    }
    if (wantTtl && (kind === "a" || kind === "aaaa")) {
      settle(null, norm.map((e) => ({ address: typeof e === "string" ? e : e.address, ttl: (e && e.ttl) || 0 })));
      return;
    }
    settle(null, kind === "soa" ? norm[0] : norm);
  }, 5);
}
export function getDefaultResultOrder() { return __wjs_dns_order_get(); }
export function setDefaultResultOrder(order) { __wjs_dns_order_set(String(order)); }
// 10f：resolve4/6 走 DNS-A/AAAA 查询（hickory 系统路径；`{ttl:true}` 回
// [{address, ttl}]，否则 [address]）。localhost 经 hosts/系统 DNS（phase9d  pin）。
function __resolveAddr(kind, syscall, hostname, options, cb) {
  if (options !== undefined && (typeof options !== "object" || options === null)) {
    throw __dnsErrInvalidArgType("options", "of type object", options);
  }
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  const wantTtl = !!(options && options.ttl);
  __resolverQueryMod(kind, syscall, hostname, cb, wantTtl);
}
// 模块级查询（全局 servers；hickory 同步阻塞语义沿 10d，不动）
function __resolverQueryMod(kind, syscall, hostname, cb, wantTtl) {
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  queueMicrotask(() => {
    let raw;
    try { raw = JSON.parse(__wjs_dns_query(kind, hostname)); }
    catch (e) {
      try { __qerr(kind, hostname, e); } catch (err) { cb(err); }
      return;
    }
    const norm = __dnsNormResult(kind, raw);
    if (!Array.isArray(norm) || norm.length === 0) {
      const cap = syscall.slice(0, 1).toUpperCase() + syscall.slice(1);
      const err = new Error(`ENODATA: query${cap} '${hostname}': no records found`);
      err.code = "ENODATA";
      err.syscall = syscall;
      err.hostname = hostname;
      cb(err);
      return;
    }
    if (wantTtl && (kind === "a" || kind === "aaaa")) {
      cb(null, norm.map((e) => ({ address: typeof e === "string" ? e : e.address, ttl: (e && e.ttl) || 0 })));
      return;
    }
    cb(null, norm);
  });
}
export function resolve4(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  if (__dnsOverrideSnapshot()) {
    const wantTtl = !!(options && options.ttl);
    __moduleQueryCustom("a", "queryA", hostname, cb, wantTtl);
    return;
  }
  __resolveAddr("a", "queryA", hostname, options, cb);
}
export function resolve6(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  if (__dnsOverrideSnapshot()) {
    const wantTtl = !!(options && options.ttl);
    __moduleQueryCustom("aaaa", "queryAaaa", hostname, cb, wantTtl);
    return;
  }
  __resolveAddr("aaaa", "queryAaaa", hostname, options, cb);
}
export function resolveSoa(hostname, options, cb) {
  if (typeof options === "function") { cb = options; options = undefined; }
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  if (__dnsOverrideSnapshot()) { __moduleQueryCustom("soa", "querySoa", hostname, cb, false); return; }
  queueMicrotask(() => {
    let raw;
    try { raw = JSON.parse(__wjs_dns_query("soa", hostname)); }
    catch (e) {
      try { __qerr("soa", hostname, e); } catch (err) { cb(err); }
      return;
    }
    const list = Array.isArray(raw) ? raw : [];
    if (list.length === 0) {
      cb(__dnsMakeError("querySoa", "ENODATA", hostname));
      return;
    }
    cb(null, list[0]);
  });
}
const __as = (fn) => function (...args) { return Promise.resolve().then(() => fn(...args)); };
// 回调转 promise（resolve 系统一包一层，保持 callback 语义单源；
// hostname 同步校验先行——真机口径下 `resolveNs([])` 系同步抛，非 reject）
function __promisifyQuery(kind, syscall) {
  return (hostname) => {
    __dnsValidateString(hostname, "name");
    return new Promise((resolveP, reject) => {
      queueMicrotask(() => {
        try { resolveP(__qkind(kind, hostname)); }
        catch (e) {
          try { __qerr(kind, hostname, e); } catch (err) { reject(err); }
        }
      });
    });
  };
}
// getservbyport 最小静态表（真机查系统库；未知端口回 String(port)，套件接受双形）
const __dnsServices = {
  7: "echo", 9: "discard", 13: "daytime", 19: "chargen", 20: "ftp-data",
  21: "ftp", 22: "ssh", 23: "telnet", 25: "smtp", 37: "time", 42: "nameserver",
  43: "whois", 53: "domain", 67: "bootps", 68: "bootpc", 69: "tftp",
  70: "gopher", 79: "finger", 80: "http", 88: "kerberos", 101: "hostname",
  102: "iso-tsap", 107: "rtelnet", 109: "pop2", 110: "pop3", 111: "sunrpc",
  113: "auth", 115: "sftp", 119: "nntp", 123: "ntp", 143: "imap",
  161: "snmp", 162: "snmptrap", 179: "bgp", 194: "irc", 220: "imap3",
  389: "ldap", 443: "https", 445: "microsoft-ds", 465: "smtps",
  512: "exec", 513: "login", 514: "shell", 515: "printer", 547: "dhcpv6-server",
  548: "dhcpv6-client", 587: "submission", 636: "ldaps", 873: "rsync",
  990: "ftps", 993: "imaps", 995: "pop3s",
};
function __dnsServiceName(port) {
  return __dnsServices[port] ?? String(port);
}
function __dnsMissingArgs(...names) {
  const q = names.map((n) => `"${n}"`);
  const head = q.length > 2 ? `${q.slice(0, -1).join(", ")}, and ${q[q.length - 1]}`
    : q.length === 2 ? `${q[0]} and ${q[1]}` : q[0];
  const e = new TypeError(`The ${head} argument${q.length > 1 ? "s" : ""} must be specified`);
  e.code = "ERR_MISSING_ARGS";
  return e;
}
function __dnsInvalidArgValue(name, value, reason = "is invalid") {
  const e = new TypeError(`The argument '${name}' ${reason}. Received ${__dnsInspectValue(value)}`);
  e.code = "ERR_INVALID_ARG_VALUE";
  return e;
}
export function lookupService(address, port, callback) {
  if (arguments.length !== 3) throw __dnsMissingArgs("address", "port", "callback");
  __lookupServiceImpl(address, port, callback);
}
function __dnsValidatePortLoose(p) {
  // validatePort 宽松形（任意类型先判形，symbol 不求值即拒；-0 归 +0）
  let n;
  if (typeof p === "number") n = p;
  else if (typeof p === "string" && p.trim() !== "") n = Number(p);
  else n = NaN;
  if (!Number.isInteger(n) || n < 0 || n > 65535) {
    const e = new RangeError(`Port should be >= 0 and < 65536. Received ${String(p)}`);
    e.code = "ERR_SOCKET_BAD_PORT";
    throw e;
  }
  return n + 0;
}
function __lookupServiceImpl(address, port, callback) {
  __dnsValidateString(address, "address");
  if (__dnsIsIP(address) === 0) throw __dnsInvalidArgValue("address", address);
  const portCoerced = __dnsValidatePortLoose(port);
  __dnsValidateFunction(callback, "callback");
  queueMicrotask(() => {
    let hostnames;
    try { hostnames = __qkind("reverse", address); }
    catch (e) {
      try { __qerr("reverse", address, e); } catch (err) {
        // getnameinfo 口径：无名即 ENOTFOUND（PTR 查询层的 ENODATA 在此翻译；
        // `reverse()` 本体保持查询语义不动）
        if (err && err.code === "ENODATA") {
          err.code = "ENOTFOUND";
          err.syscall = "getnameinfo";
        }
        callback(err);
        return;
      }
    }
    if (!Array.isArray(hostnames) || hostnames.length === 0) {
      callback(__dnsMakeError("getnameinfo", "ENOTFOUND", address));
      return;
    }
    callback(null, hostnames[0], __dnsServiceName(portCoerced));
  });
}
// ── 10f：Resolver 独立实例（Node internal/dns/utils 口径）──────────────────
// 自有 servers/timeout/tries/maxTimeout；查询经 `__wjs_dns_query_cfg` 定制单问；
// pending 计数 + cancel 代际：cancel 后落定的 in-flight 一律合成 ECANCELLED
// （真机 c-ares 回调语义；线程侧不强杀，只改 JS 侧结算，见模块头注）。
const __resolverPriv = new WeakMap();
function __dnsErrOutOfRange(name, range, value) {
  const e = new RangeError(`The value of "${name}" is out of range. It must be ${range}. Received ${__dnsReceived(value)}`);
  e.code = "ERR_OUT_OF_RANGE";
  return e;
}
function __dnsValidateTimeout(options) {
  const o = options === undefined || options === null ? {} : Object(options);
  const raw = o.timeout === undefined ? -1 : o.timeout;
  if (typeof raw !== "number") throw __dnsErrInvalidArgType("options.timeout", "of type number", o.timeout);
  if (!Number.isInteger(raw) || raw < -1 || raw > 2147483647) {
    throw __dnsErrOutOfRange("options.timeout", ">= -1 && <= 2147483647", raw);
  }
  return raw + 0;
}
function __dnsValidateTries(options) {
  const o = options === undefined || options === null ? {} : Object(options);
  const raw = o.tries === undefined ? 4 : o.tries;
  if (typeof raw !== "number") throw __dnsErrInvalidArgType("options.tries", "of type number", o.tries);
  if (!Number.isInteger(raw) || raw < 1 || raw > 2147483647) {
    throw __dnsErrOutOfRange("options.tries", ">= 1 && <= 2147483647", raw);
  }
  return raw;
}
function __dnsValidateMaxTimeout(options) {
  const o = options === undefined || options === null ? {} : Object(options);
  const raw = o.maxTimeout === undefined ? 0 : o.maxTimeout;
  if (typeof raw !== "number") throw __dnsErrInvalidArgType("options.maxTimeout", "of type number", o.maxTimeout);
  if (!Number.isInteger(raw) || raw < 0 || raw > 4294967295) {
    throw __dnsErrOutOfRange("options.maxTimeout", ">= 0 && <= 4294967295", raw);
  }
  return raw + 0;
}
function __dnsSetServersFailed(servers) {
  const e = new Error(`c-ares failed to set servers: "There are pending queries." ${JSON.stringify(servers)}`);
  e.code = "ERR_DNS_SET_SERVERS_FAILED";
  return e;
}
function __dnsCancelled(syscall, hostname) {
  const cap = syscall.slice(0, 1).toUpperCase() + syscall.slice(1);
  const e = new Error(`ECANCELLED: query${cap} '${hostname}'`);
  e.code = "ECANCELLED";
  e.syscall = syscall;
  e.hostname = hostname;
  return e;
}
// 定制查询双形态归一（kind 见 Rust 单问表；map 归一 hickory/custom 双形）
function __dnsNormResult(kind, raw) {
  if (!Array.isArray(raw)) return raw;
  switch (kind) {
    case "a":
    case "aaaa":
      return raw.map((e) => (typeof e === "string" ? e : e.address));
    case "cname":
    case "ns":
    case "ptr":
    case "reverse":
      return raw.map((e) => (typeof e === "string" ? e : e.value));
    case "txt":
      return raw.map((e) => (Array.isArray(e) ? e : e.entries));
    case "soa":
      return raw;
    default:
      return raw;
  }
}
function __resolverQuery(inst, kind, syscall, hostname, cb) {
  __dnsValidateString(hostname, "name");
  __dnsValidateFunction(cb, "callback");
  const st = __resolverPriv.get(inst);
  st.pending++;
  const wire = JSON.stringify(st.servers.map(__dnsWireForm));
  // 投递即返（helper 线程跑查询，事件循环永不停转——阻塞 native 会饿死
  // stub 回包分发；5ms refed 轮询保活，结算/取消即清）
  const id = Number(__wjs_dns_job_start(kind, hostname, wire, String(st.timeout), String(st.tries), String(st.maxTimeout)));
  const rec = { kind, syscall, hostname, cb, poll: 0 };
  st.jobs.set(id, rec);
  const settle = (err, val) => {
    if (!st.jobs.has(id)) return;
    st.jobs.delete(id);
    clearInterval(rec.poll);
    st.pending--;
    cb(err, val);
  };
  rec.cancel = () => settle(__dnsCancelled(syscall, hostname), undefined);
  rec.poll = setInterval(() => {
    let r;
    try { r = JSON.parse(__wjs_dns_job_poll(String(id))); }
    catch { return; }
    if (r.status === "pending") return;
    if (r.status === "err") {
      settle(__dnsMakeError(syscall, r.code, hostname), undefined);
      return;
    }
    const norm = __dnsNormResult(kind, r.value);
    if (!Array.isArray(norm) || norm.length === 0) {
      settle(__dnsMakeError(syscall, "ENODATA", hostname), undefined);
      return;
    }
    settle(null, norm);
  }, 5);
}
function __resolverQueryP(inst, kind, syscall, hostname) {
  __dnsValidateString(hostname, "name");
  return new Promise((resolveP, reject) => {
    __resolverQuery(inst, kind, syscall, hostname, (e, v) => (e ? reject(e) : resolveP(v)));
  });
}
export class Resolver {
  constructor(options) {
    const st = {
      servers: __dnsParseServers(getServers()),
      timeout: __dnsValidateTimeout(options),
      tries: __dnsValidateTries(options),
      maxTimeout: __dnsValidateMaxTimeout(options),
      pending: 0,
      jobs: new Map(),
      localAddress: null,
    };
    __resolverPriv.set(this, st);
    // _handle 兼容垫片（真机为 c-ares ChannelWrap；getServers гейт经此，
    // 套件覆写 `_handle.getServers` 探活——见 test-dns-get-server）
    this._handle = {
      getServers: () => st.servers.map((t) => t.slice()),
    };
  }
  getServers() {
    const st = __resolverPriv.get(this);
    const raw = this._handle.getServers() || [];
    return raw.map(([ver, ip, port]) => __dnsPublicForm([ver, ip, port]));
  }
  setServers(servers) {
    const st = __resolverPriv.get(this);
    const triples = __dnsParseServers(servers);
    if (st.pending > 0) throw __dnsSetServersFailed(servers);
    st.servers = triples;
  }
  setLocalAddress(ipv4, ipv6) {
    const st = __resolverPriv.get(this);
    __dnsValidateString(ipv4, "ipv4");
    if (ipv6 !== undefined) __dnsValidateString(ipv6, "ipv6");
    // 真机口径（实测 node 26）：坏 IP → ERR_INVALID_ARG_VALUE "Invalid IP address."；
    // 同族双指定 → "Cannot specify two IPvX addresses."；v4+v6 混搭（任意序）OK
    const v4 = __dnsIsIP(ipv4);
    if (v4 === 0) {
      const e = new TypeError("Invalid IP address.");
      e.code = "ERR_INVALID_ARG_VALUE";
      throw e;
    }
    if (ipv6 !== undefined) {
      const v6 = __dnsIsIP(ipv6);
      if (v6 === 0) {
        const e = new TypeError("Invalid IP address.");
        e.code = "ERR_INVALID_ARG_VALUE";
        throw e;
      }
      if (v4 === 4 && v6 === 4) {
        const e = new TypeError("Cannot specify two IPv4 addresses.");
        e.code = "ERR_INVALID_ARG_VALUE";
        throw e;
      }
      if (v4 === 6 && v6 === 6) {
        const e = new TypeError("Cannot specify two IPv6 addresses.");
        e.code = "ERR_INVALID_ARG_VALUE";
        throw e;
      }
    }
    st.localAddress = { ipv4, ipv6 };
  }
  cancel() {
    // 真机 c-ares 语义：未决查询即刻以 ECANCELLED 结算（线程迟归由 forget 摘除，
    // 事件循环不为取消的查询多等一轮超时）
    const st = __resolverPriv.get(this);
    for (const [id, rec] of [...st.jobs]) {
      try { __wjs_dns_job_forget(String(id)); } catch { /* 摘除尽力 */ }
      rec.cancel();
    }
  }
  resolve4(h, cb) { __resolverQuery(this, "a", "queryA", h, cb); }
  resolve6(h, cb) { __resolverQuery(this, "aaaa", "queryAaaa", h, cb); }
  resolveCname(h, cb) { __resolverQuery(this, "cname", "queryCname", h, cb); }
  resolveMx(h, cb) { __resolverQuery(this, "mx", "queryMx", h, cb); }
  resolveNs(h, cb) { __resolverQuery(this, "ns", "queryNs", h, cb); }
  resolveTxt(h, cb) { __resolverQuery(this, "txt", "queryTxt", h, cb); }
  resolveSrv(h, cb) { __resolverQuery(this, "srv", "querySrv", h, cb); }
  resolvePtr(h, cb) { __resolverQuery(this, "ptr", "queryPtr", h, cb); }
  resolveAny(h, cb) { __resolverQuery(this, "any", "queryAny", h, cb); }
  resolveSoa(h, cb) {
    __resolverQuery(this, "soa", "querySoa", h, (e, v) => cb(e, v && v[0]));
  }
  reverse(ip, cb) { __resolverQuery(this, "reverse", "getHostByAddr", ip, cb); }
  resolve(hostname, rrtype, cb) {
    if (typeof rrtype === "function") { cb = rrtype; rrtype = "A"; }
    __dnsValidateString(hostname, "name");
    if (typeof rrtype !== "string") throw __dnsErrInvalidArgType("rrtype", "of type string", rrtype);
    __dnsValidateFunction(cb, "callback");
    const t = String(rrtype).toUpperCase();
    const map = { CNAME: "cname", MX: "mx", NS: "ns", TXT: "txt", SRV: "srv", PTR: "ptr", ANY: "any", A: "a", AAAA: "aaaa", SOA: "soa" };
    const kind = map[t];
    if (!kind) {
      const err = new TypeError(`Unknown rrtype '${rrtype}'`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    const syscall = { cname: "queryCname", mx: "queryMx", ns: "queryNs", txt: "queryTxt", srv: "querySrv", ptr: "queryPtr", any: "queryAny", a: "queryA", aaaa: "queryAaaa", soa: "querySoa" }[kind];
    __resolverQuery(this, kind, syscall, hostname, cb);
  }
}
class PromisesResolver {
  constructor(options) {
    this._r = new Resolver(options);
    this._handle = this._r._handle;
  }
  getServers() { return this._r.getServers(); }
  setServers(s) { this._r.setServers(s); }
  setLocalAddress(a, b) { this._r.setLocalAddress(a, b); }
  cancel() { this._r.cancel(); }
  resolve4(h) { return __resolverQueryP(this._r, "a", "queryA", h); }
  resolve6(h) { return __resolverQueryP(this._r, "aaaa", "queryAaaa", h); }
  resolveCname(h) { return __resolverQueryP(this._r, "cname", "queryCname", h); }
  resolveMx(h) { return __resolverQueryP(this._r, "mx", "queryMx", h); }
  resolveNs(h) { return __resolverQueryP(this._r, "ns", "queryNs", h); }
  resolveTxt(h) { return __resolverQueryP(this._r, "txt", "queryTxt", h); }
  resolveSrv(h) { return __resolverQueryP(this._r, "srv", "querySrv", h); }
  resolvePtr(h) { return __resolverQueryP(this._r, "ptr", "queryPtr", h); }
  resolveAny(h) { return __resolverQueryP(this._r, "any", "queryAny", h); }
  resolveSoa(h) {
    return __resolverQueryP(this._r, "soa", "querySoa", h).then((v) => v[0]);
  }
  reverse(ip) { return __resolverQueryP(this._r, "reverse", "getHostByAddr", ip); }
  resolve(hostname, rrtype) {
    __dnsValidateString(hostname, "name");
    if (rrtype !== undefined && typeof rrtype !== "string") {
      throw __dnsErrInvalidArgType("rrtype", "of type string", rrtype);
    }
    const t = String(rrtype || "A").toUpperCase();
    const map = { CNAME: "cname", MX: "mx", NS: "ns", TXT: "txt", SRV: "srv", PTR: "ptr", ANY: "any", A: "a", AAAA: "aaaa", SOA: "soa" };
    const kind = map[t];
    if (!kind) {
      const err = new TypeError(`Unknown rrtype '${rrtype}'`);
      err.code = "ERR_INVALID_ARG_VALUE";
      throw err;
    }
    const syscall = { cname: "queryCname", mx: "queryMx", ns: "queryNs", txt: "queryTxt", srv: "querySrv", ptr: "queryPtr", any: "queryAny", a: "queryA", aaaa: "queryAaaa", soa: "querySoa" }[kind];
    return __resolverQueryP(this._r, kind, syscall, hostname);
  }
}
function __promisesResolveAddr(kind, syscall, hostname, options) {
  __dnsValidateString(hostname, "name");
  const wantTtl = !!(options && options.ttl);
  return new Promise((resolveP, reject) => {
    __resolverQueryMod(kind, syscall, hostname, (e, v) => (e ? reject(e) : resolveP(v)), wantTtl);
  });
}
export const promises = {
  lookup: (hostname, options) => {
    if (!hostname) return Promise.reject(__dnsInvalidArgValue("hostname", hostname));
    __dnsValidateString(hostname, "hostname");
    __dnsValidateLookupOptions(options);
    return Promise.resolve().then(() => __lookupCore(hostname, options));
  },
  resolve4: (h, o) => {
    __dnsValidateString(h, "name");
    if (__dnsOverrideSnapshot()) {
      return new Promise((resolveP, reject) => {
        __moduleQueryCustom("a", "queryA", h, (e, v) => (e ? reject(e) : resolveP(v)), !!(o && o.ttl));
      });
    }
    return __promisesResolveAddr("a", "queryA", h, o);
  },
  resolve6: (h, o) => {
    __dnsValidateString(h, "name");
    if (__dnsOverrideSnapshot()) {
      return new Promise((resolveP, reject) => {
        __moduleQueryCustom("aaaa", "queryAaaa", h, (e, v) => (e ? reject(e) : resolveP(v)), !!(o && o.ttl));
      });
    }
    return __promisesResolveAddr("aaaa", "queryAaaa", h, o);
  },
  resolveSoa: (h) => {
    __dnsValidateString(h, "name");
    if (__dnsOverrideSnapshot()) {
      return new Promise((resolveP, reject) => {
        __moduleQueryCustom("soa", "querySoa", h, (e, v) => (e ? reject(e) : resolveP(v)), false);
      });
    }
    return new Promise((resolveP, reject) => {
    queueMicrotask(() => {
      let raw;
      try { raw = JSON.parse(__wjs_dns_query("soa", h)); }
      catch (e) {
        try { __qerr("soa", h, e); } catch (err) { reject(err); }
        return;
      }
      const list = Array.isArray(raw) ? raw : [];
      if (list.length === 0) { reject(__dnsMakeError("querySoa", "ENODATA", h)); return; }
      resolveP(list[0]);
    });
    });
  },
  resolveCname: __promisifyQuery("cname", "resolveCname"),
  resolveMx: __promisifyQuery("mx", "resolveMx"),
  resolveNs: __promisifyQuery("ns", "resolveNs"),
  resolveTxt: __promisifyQuery("txt", "resolveTxt"),
  resolveSrv: __promisifyQuery("srv", "resolveSrv"),
  resolvePtr: __promisifyQuery("ptr", "resolvePtr"),
  resolveAny: (h) => {
    __dnsValidateString(h, "name");
    if (__dnsOverrideSnapshot()) {
      return new Promise((resolveP, reject) => {
        __moduleQueryCustom("any", "queryAny", h, (e, v) => (e ? reject(e) : resolveP(v)), false);
      });
    }
    return __promisifyQuery("any", "resolveAny")(h);
  },
  resolve: (hostname, rrtype) => {
    __dnsValidateString(hostname, "name");
    if (rrtype !== undefined && typeof rrtype !== "string") {
      throw __dnsErrInvalidArgType("rrtype", "of type string", rrtype);
    }
    const t = String(rrtype || "A").toUpperCase();
    const map = { CNAME: "cname", MX: "mx", NS: "ns", TXT: "txt", SRV: "srv", PTR: "ptr", ANY: "any", A: "a", AAAA: "aaaa", SOA: "soa" };
    const kind = map[t];
    if (!kind) {
      const err = new TypeError(`dns.resolve: unknown rrtype '${rrtype}'`);
      err.code = "EBADNAME";
      throw err;
    }
    return __promisifyQuery(kind, "resolve")(hostname);
  },
  reverse: (ip) => new Promise((resolveP, reject) => {
    queueMicrotask(() => {
      try { resolveP(__qkind("reverse", ip)); }
      catch (e) {
        try { __qerr("reverse", ip, e); } catch (err) { reject(err); }
      }
    });
  }),
  getServers: () => Promise.resolve().then(() => getServers()),
  // setServers 同步校验先行（`assert.throws` 同步口径）
  setServers: (s) => { setServers(s); return Promise.resolve(); },
  lookupService: function (address, port) {
    if (arguments.length < 2) throw __dnsMissingArgs("address", "port");
    // 同步校验先行（`assert.throws` 同步口径；进 Promise 执行器即变 reject）
    __dnsValidateString(address, "address");
    if (__dnsIsIP(address) === 0) throw __dnsInvalidArgValue("address", address);
    __dnsValidatePortLoose(port);
    return new Promise((resolveP, reject) => {
      __lookupServiceImpl(address, port, (e, h, svc) => (e ? reject(e) : resolveP({ hostname: h, service: svc })));
    });
  },
  Resolver: PromisesResolver,
  getDefaultResultOrder: () => Promise.resolve().then(() => getDefaultResultOrder()),
  setDefaultResultOrder: (o) => Promise.resolve().then(() => setDefaultResultOrder(o)),
};
function resolve4b(hostname) { return __filter(__entries(hostname), 4).map((e) => e.address); }
function resolve6b(hostname) { return __filter(__entries(hostname), 6).map((e) => e.address); }
const __api = { lookup, lookupService, resolve4, resolve6, resolve, resolveCname, resolveMx, resolveNs, resolveTxt, resolveSrv, resolvePtr, resolveAny, resolveSoa, reverse, getServers, setServers, getDefaultResultOrder, setDefaultResultOrder, Resolver, promises, V4MAPPED, ADDRCONFIG, ALL };
export default __api;
export { resolve4b as __resolve4, resolve6b as __resolve6 };
