import { EventEmitter } from "node:events";

// 簿记（ctx id / 快照）一律挂 WeakMap，不落沙箱自有键——真机 contextify 不给
// 沙箱加任何键（ownkeys/ownpropertynames/ownpropertysymbols 三件 + Proxy
// definer-interception 的 trap 计数都点名"沙箱键集不变"）。
const __vmBookkeeping = new WeakMap();
function __vmStdKeys(obj) {
  let rec = __vmBookkeeping.get(obj);
  if (!rec) {
    rec = { id: undefined, std: null, init: new Map() };
    __vmBookkeeping.set(obj, rec);
  }
  return rec;
}
function __vmCtxId(obj) {
  const rec = __vmBookkeeping.get(obj);
  return rec ? rec.id : undefined;
}

function __vmUnwrap(e) {
  const m = String((e && e.message) || e);
  const mm = m.match(/^__wjs2_vm_error:([A-Za-z]+)\n([\s\S]*)$/);
  if (!mm) {
    const err = new Error(m);
    err.code = "ERR_VM_ERROR";
    throw err;
  }
  const [, name, body] = mm;
  // 位置标记（native vm_stk_envelope 附带）：剥出后重建 node displayErrors
  // 形态的栈——`filename:line\n源行\ncaret\n\nName: message\n    at f:l:c`。
  // checkErr 类 `err.stack.startsWith(filename)` 校验靠首行；帧格式引擎口径记档。
  let message = body;
  let stk = null;
  const sm = body.match(/^([\s\S]*)\n__wjs2_vm_stk:(\{.*\})$/);
  if (sm) {
    message = sm[1];
    try { stk = JSON.parse(sm[2]); } catch { stk = null; }
  }
  const Ctor = globalThis[name] || Error;
  const err = new Ctor(message);
  if (stk && stk.f) {
    const caret = " ".repeat(Math.max(0, (stk.c | 0) - 1)) + "^";
    err.stack = `${stk.f}:${stk.l}\n${stk.s ?? ""}\n${caret}\n\n${name}: ${message}\n    at ${stk.f}:${stk.l}:${stk.c}`;
  }
  throw err;
}
function __vmCall(fn) {
  try {
    return fn();
  } catch (e) {
    // native 暂存的原始异常对象优先（保 vm realm 身份/原型/栈——跨域
    // `instanceof vmCtx.SyntaxError` 与栈断言点名）；信封重建只作兜底。
    let orig;
    try { orig = __wjs2_vm_take_error(); } catch { orig = undefined; }
    if (orig !== undefined && orig !== null) {
      // 赋值类 TypeError 文案桥（native bridge_vm_assign_message 同源规则——
      // 原物透传绕过了 native 侧桥，按 node contextify 拦截器口径补齐）。
      if (orig && typeof orig === "object") {
        try {
          const m = orig.message;
          if (typeof m === "string") {
            const am = m.match(/^assignment to undeclared variable (\S+)$/);
            if (am) {
              orig.message = `${am[1]} is not defined`;
            } else {
              const bm = m.match(/^"([^"]+)" is (read-only|non-configurable and can't be redefined)$/);
              if (bm) {
                orig.message = bm[2] === "read-only"
                  ? `Cannot assign to read only property '${bm[1]}' of object '[object Object]'`
                  : `Cannot redefine property: ${bm[1]}`;
              }
            }
          }
        } catch { /* 保留原文案 */ }
        // 信封带位置标记时给原物栈补 node displayErrors 前缀
        //（`f:l\n源行\ncaret\n\n` + 原栈首行 Name: message 同构拼接）。
        const m = String((e && e.message) || e);
        // 组序：1=name、2=message、3=json 栈标记（与 __vmUnwrap 的双组序不同！）
        const sm = m.match(/^__wjs2_vm_error:([A-Za-z]+)\n([\s\S]*)\n__wjs2_vm_stk:(\{.*\})$/);
        if (sm) {
          try {
            const stk = JSON.parse(sm[3]);
            if (stk && stk.f) {
              const caret = " ".repeat(Math.max(0, (stk.c | 0) - 1)) + "^";
              orig.stack = `${stk.f}:${stk.l}\n${stk.s ?? ""}\n${caret}\n\n${orig.stack}`;
            }
          } catch { /* 保留原栈 */ }
        }
      }
      throw orig;
    }
    __vmUnwrap(e);
  }
}

let __vmFinal = null;
let __vmModFinal = null;
function __vmAutoRelease(obj, id) {
  try {
    if (typeof FinalizationRegistry === "undefined") return;
    if (!__vmFinal) {
      __vmFinal = new FinalizationRegistry((held) => {
        try { __wjs2_vm_release(String(held)); } catch { /* 会话收尾期忽略 */ }
      });
    }
    __vmFinal.register(obj, id);
  } catch { /* 无注册表即会话级存活，记档 */ }
}
function __vmModAutoRelease(obj, id) {
  try {
    if (typeof FinalizationRegistry === "undefined") return;
    if (!__vmModFinal) {
      __vmModFinal = new FinalizationRegistry((held) => {
        try { __wjs2_vm_mod_release(String(held)); } catch { /* 会话收尾期忽略 */ }
      });
    }
    __vmModFinal.register(obj, id);
  } catch { /* 无注册表即会话级存活，记档 */ }
}

function __validateCtx(obj) {
  if ((typeof obj !== "object" && typeof obj !== "function") || obj === null) {
    const err = new TypeError(`The "contextifiedObject" argument must be of type object. Received ${obj === null ? "null" : typeof obj}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const id = __vmCtxId(obj);
  if (typeof id !== "string") {
    // 真机 26 冠词即 "an vm.Context"（怪癖逐字）+ Received 实例描述
    const err = new TypeError(`The "contextifiedObject" argument must be an vm.Context. Received ${__recv(obj)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return id;
}
export function isContext(obj) {
  if ((typeof obj !== "object" && typeof obj !== "function") || obj === null) {
    const err = new TypeError(`The "object" argument must be of type object. Received ${obj === null ? "null" : typeof obj}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return typeof __vmCtxId(obj) === "string";
}

function __vmSnapshot(id, obj) {
  // 创建期快照：目标 global 全部自有字符串键集合 + 各键 SameValue 基线。
  // 值经 CCW 取回主域；跨 compartment 同底层恒同一引用，SameValue 可比（§4.57）。
  // 键枚举用 GetPropertyKeys（OWNONLY+HIDDEN），不可枚举的 defineProperty 产物同样覆盖。
  const rec = __vmStdKeys(obj);
  let std = [];
  try {
    std = JSON.parse(__vmCall(() => __wjs2_vm_keys_all(id)));
  } catch {
    std = JSON.parse(__vmCall(() => __wjs2_vm_keys(id)));
  }
  rec.std = new Set(std);
  rec.init = new Map();
  for (const k of std) {
    rec.init.set(k, __vmCall(() => __wjs2_vm_get(id, k)));
  }
}
// symbol 键通道：键（symbol 本体作值）与完整描述符经字符串暂存位过域，
// 目标域内 defineProperty 落定（defineProperty 不触发访问器，无 mustCall 污染；
// 描述符对象过域照常工作，p38 实证）。
const __kTmpKey = "__wjs2_vm_tmp_key";
const __kTmpDesc = "__wjs2_vm_tmp_desc";
function __vmStageAndDefine(id, key, dd) {
  __vmCall(() => __wjs2_vm_set(id, __kTmpKey, key));
  __vmCall(() => __wjs2_vm_set(id, __kTmpDesc, dd));
  __vmCall(() => __wjs2_vm_run(id,
    `Object.defineProperty(globalThis, globalThis[${JSON.stringify(__kTmpKey)}], globalThis[${JSON.stringify(__kTmpDesc)}]); delete globalThis[${JSON.stringify(__kTmpKey)}]; delete globalThis[${JSON.stringify(__kTmpDesc)}];`,
    "vm-sync-in.js"));
}
// 描述符读取（可抛）：SM proxy 不变量文案 → V8 口径（套件按 trap 名子串
// 断言；仅限本模块 sync 路径的窄桥，不改引擎全局文案）。
function __vmDescOrThrow(obj, k) {
  try {
    return Object.getOwnPropertyDescriptor(obj, k);
  } catch (e) {
    if (e && typeof e.message === "string") {
      const m = e.message.match(/^proxy can't report a non-existent property '"(.*)"' as non-configurable$/);
      if (m) {
        const err = new TypeError(`'getOwnPropertyDescriptor' on proxy: trap reported non-configurability for property '${m[1]}' which is either non-existent or configurable in the proxy target`);
        throw err;
      }
    }
    throw e;
  }
}
function __syncIn(id, obj) {
  // 主域侧全部自有字符串键（含不可枚举；与目标 global 的 HIDDEN 枚举口径对齐）。
  // 纯 Object.keys 会漏沙箱不可枚举种子（defineProperty value 形），vm 内即 undefined。
  // 描述符携带：数据描述符传 d.value（值拷贝，此时求值）；访问器传描述符对象本身——
  // 经 __wjs2_vm_set 过 CCW 后在目标域内 Object.defineProperty 落定，getter/setter
  // 身份由引擎 CCW 透明代理（真机"访问器活绑定"同效：主域改 getter 实现即 vm 内可见；
  // 实证见 p38：描述符对象过沙箱属性中转后 define 照常工作）。
  // 只写目标缺席键（目标已有——首轮 define 产物或标准内建——即跳过）：
  // define+赋值双失败（目标只读）由 vm_set 静默跳过，但预检可省一次跨域调用；
  // 更重要的是预检以目标描述符为准，不以源值为准。
  // 注意：__wjs2_vm_keys 只回可枚举键；不可枚举目标键不在此集——
  // 该分支漏检时 vm_set 的静默跳过是最后一道防线（本行注释钉住两层关系）。
  // 【10f 修订】预检取消：真机 contextify 的拦截器让 sandbox 自有键**遮蔽**
  // vm realm 内建（harmony-symbols/proxies：sandbox {Symbol} 后 vm 内读
  // Symbol 得主域 Symbol）——源键必须每次重落。目标已有的只读/不可配置键
  // 由 vm_set 的 define 失败→赋值失败→静默跳过兜底（global-non-writable
  // 的 vm 侧只读 x 不被沙箱可写值覆盖）。
  const keys = Object.getOwnPropertyNames(obj);
  for (const k of keys) {
    // 主域 globalThis 自指（globalThis/global）永不同步——值是主域 global 本体，
    // define 进目标即把目标 globalThis 换成主域 CCW，后续一切 set 全写错域
    //（BQ1：set globalThis 后 probe 即 undefined，而 set global 无事）。
    // 真机口径：目标自有 globalThis 恒为自身（AL1 mc-exists 同源），无需同步。
    if (k === "globalThis" || k === "global") continue;
    // 沙箱自指键（ctx.window = ctx）：真机沙箱即 global proxy，this/window 恒同
    // 身份——目标域内挂 vm global 本体（CCW 缓存保证 thisVal === windowVal），
    // 并记账 selfRefs 让 syncOut 不回写（回写会把 sandbox.window 换成 CCW）。
    let selfRef = false;
    try { selfRef = obj[k] === obj; } catch { selfRef = false; }
    if (selfRef) {
      try {
        const g = __vmCall(() => __wjs2_vm_global(id));
        __vmCall(() => __wjs2_vm_set(id, k, g));
        const rec0 = __vmStdKeys(obj);
        (rec0.selfRefs ??= new Set()).add(k);
      } catch { /* 跳过该键 */ }
      continue;
    }
    // globalThis 等宿主对象有"名无描述符"键（getOwnPropertyNames 列得出、
    // getOwnPropertyDescriptor 回 undefined）：无描述符即按值语义走 obj[k]。
    // 描述符读取错误必须容错——真机 contextify 不在 sync 期查询属性描述符
    //（proxy-failure-CP：trap 恒抛的 sandbox 照常 create/run）；set 期的
    // 不变量错误由 syncOut 的传播路径负责（set-property-proxy）。
    let d = null;
    try { d = __vmDescOrThrow(obj, k); } catch { d = null; }
    if (d && ("get" in d || "set" in d)) {
      // 键形态以"有无可调用 get/set"为准，不以 key 存在为准——
      // 宿主懒访问器（MessageChannel 等）可能是 {get: fn, set: undefined}，
      // set: undefined 必须剔除，否则目标域 defineProperty 读到
      // set: undefined 即判"描述符非对象"（AB3 实证；真机侧同键是数据描述符）。
      const hasGet = "get" in d && typeof d.get === "function";
      const hasSet = "set" in d && typeof d.set === "function";
      if (!hasGet && !hasSet) {
        // 伪访问器（get/set 皆不可调用）：按值语义走 obj[k]（此时求值）。
        try { __vmCall(() => __wjs2_vm_set(id, k, obj[k])); } catch { /* 跳过该键 */ }
        continue;
      }
      // 真访问器：描述符对象暂存 + 目标域内 defineProperty 落定后删暂存。
      // 不删则数据暂存遮蔽访问器（setter 永不触发，p45 实证）。
      const t = `__wjs2_vm_tmp_${k}`;
      const dd = { enumerable: false, configurable: true };
      if (hasGet) dd.get = d.get;
      if (hasSet) dd.set = d.set;
      dd.enumerable = !!d.enumerable;
      dd.configurable = !!d.configurable;
      __vmCall(() => __wjs2_vm_set(id, t, dd));
      __vmCall(() => __wjs2_vm_run(id, `Object.defineProperty(globalThis, ${JSON.stringify(k)}, globalThis[${JSON.stringify(t)}]); delete globalThis[${JSON.stringify(t)}]`, "vm-sync-in.js"));
    } else if (d && "value" in d) {
      if (d.writable === true && d.enumerable === true && d.configurable === true) {
        // 默认属性快路径（define_prop 即 {w,e,c}=true，无损失）。
        __vmCall(() => __wjs2_vm_set(id, k, d.value));
      } else {
        // 非默认属性（nonWritableProp 等）走描述符 staging：真机按源描述符落定，
        // vm 侧 writable:false 不可写/不可枚举都要保形（global-setter descriptor10）。
        const dd = { value: d.value, writable: !!d.writable, enumerable: !!d.enumerable, configurable: !!d.configurable };
        try { __vmStageAndDefine(id, k, dd); } catch { /* 跳过该键 */ }
      }
    }
    else if (d) __vmCall(() => __wjs2_vm_set(id, k, obj[k]));
    else {
      // 无描述符键：读值失败即跳过（globalThis 宿主键），不中断整表。
      try { __vmCall(() => __wjs2_vm_set(id, k, obj[k])); } catch { /* 跳过该键 */ }
    }
  }
  // symbol 键同步（真机 contextify 转发 symbol 面；ownkeys/ownpropertysymbols/
  // global-setter 的 symbol 描述符都点名）。不可重定义等失败跳过该键，不中断。
  const syms = Object.getOwnPropertySymbols(obj);
  for (const s of syms) {
    let d = null;
    try { d = Object.getOwnPropertyDescriptor(obj, s); } catch { d = null; }
    if (!d) continue;
    const dd = { enumerable: !!d.enumerable, configurable: !!d.configurable };
    const hasGet = "get" in d && typeof d.get === "function";
    const hasSet = "set" in d && typeof d.set === "function";
    if (hasGet || hasSet) {
      if (hasGet) dd.get = d.get;
      if (hasSet) dd.set = d.set;
    } else {
      dd.value = d.value;
      dd.writable = !!d.writable;
    }
    try {
      __vmStageAndDefine(id, s, dd);
    } catch { /* 跳过该键 */ }
  }
}
function __syncOut(id, obj) {
  const rec = __vmBookkeeping.get(obj);
  const std = rec ? rec.std : null;
  const init = rec ? rec.init : null;
  const snap = JSON.parse(__vmCall(() => __wjs2_vm_keys_all(id)));
  for (const entry of snap) {
    // symbol 占位无跨 realm 身份：只维护存在性（ownkeys 计数口径），不做值同步。
    if (entry !== null && typeof entry === "object") continue;
    const k = entry;
    // global 自有只读常量（undefined/NaN/Infinity，非枚举、不可写、值恒同）：永不同步。
    // 旧 keys（仅可枚举）路径从未见过它们；keys_all 含 HIDDEN 后必须显式跳过，
    // 否则 DONT_CONTEXTIFY（obj 即 vm global 本体）写只读属性直接抛。
    //（簿记键自 symbol 化起不再进字符串快照，无需再跳过。）
    if (k === "undefined" || k === "NaN" || k === "Infinity") continue;
    // syncIn 记账的自指键（window 等）：不回写（值是 vm global 本体，
    // 回写会把 sandbox 侧同键换成 CCW，破坏沙箱自指身份）。
    if (rec && rec.selfRefs && rec.selfRefs.has(k)) continue;
    // 源端访问器键：不读不写——syncIn 已装同款访问器，syncOut 再读/写即各多触发
    // 一次 getter/setter（global-setter 的 mustCall 精确计数口径）；值面归源端管。
    // 描述符查询语义：trap 自身抛的异常容错（proxy-failure-CP：不意外查询属性）；
    // 引擎不变量 TypeError 照真机传播（set-property-proxy：trap 返回 {} 报
    // non-configurability）。
    let d = null;
    try {
      d = __vmDescOrThrow(obj, k);
    } catch (e) {
      if (e && e.name === "TypeError") throw e;
      d = null;
    }
    if (d && ("get" in d || "set" in d)) continue;
    const cur = __vmCall(() => __wjs2_vm_get(id, k));
    if (std !== null && std.has(k)) {
      // 快照内键：仅当与创建快照发生 SameValue 变化时回写（this.Symbol = Symbol 等）；
      // 未改即跳过，防标准构造器污染沙箱。SameValue 经引擎比较（NaN 自等，±0 区分）。
      const before = init.get(k);
      if (__vmCall(() => __wjs2_vm_same(cur, before))) continue;
    }
    // 目标描述符优先：主域侧已有同名只读数据（源端 defineProperty 默认不可写不可配置，
    // 首轮 sync-in 的 define 产物即如此）则赋值抛——只读数据即跳过。
    // 可写数据才赋值（保留既有描述符，10c-3 回落语义）；目标缺席（全新键）直接挂载。
    if (d && d.writable === false) continue;
    try {
      obj[k] = cur;
    } catch {
      // 主域 getter-only（无 setter）赋值抛：真机静默不写（VV），此处同效跳过。
      // 有 setter 但 setter 内抛则会误吞——setter 抛的用例另案（套件无此形，记档）。
    }
  }
}

function __normStr(v, what, dflt) {
  if (v === undefined) return dflt;
  if (typeof v !== "string") {
    const err = new TypeError(`The "options.${what}" property must be of type string. Received type ${typeof v}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return v;
}
function __normUint(v, what) {
  if (v === undefined) return undefined;
  if (typeof v !== "number") {
    const err = new TypeError(`The "options.${what}" property must be of type number. Received ${__recv(v)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!Number.isInteger(v) || v < 0 || v > 4294967295) {
    const err = new RangeError(`The "options.${what}" property must be an integer in the range 0 to 4294967295. Received ${String(v)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}
// timeout：真机 validateUint32(…, positive=true)——0/负数/NaN 也 RangeError。
function __normTimeout(v, what) {
  if (v === undefined) return undefined;
  if (typeof v !== "number") {
    const err = new TypeError(`The "options.${what}" property must be of type number. Received ${__recv(v)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!Number.isInteger(v) || v <= 0 || v > 4294967295) {
    const err = new RangeError(`The "options.${what}" property must be an integer in the range 1 to 4294967295. Received ${String(v)}`);
    err.code = "ERR_OUT_OF_RANGE";
    throw err;
  }
  return v;
}
function __normBool(v, what) {
  if (v === undefined) return undefined;
  if (typeof v !== "boolean") {
    const err = new TypeError(`The "options.${what}" property must be of type boolean. Received type ${typeof v}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  return v;
}
// ERR_INVALID_ARG_TYPE 的 Received 描述（node 真机口径：null → "null"、
// 原始值 → "type T (值)"、字符串带引号）。
function __recv(v) {
  if (v === null) return "null";
  if (typeof v === "string") return `type string ('${v}')`;
  if (typeof v === "function") return "type function";
  if (typeof v === "object") {
    if (Array.isArray(v)) return "an instance of Array";
    return "an instance of Object";
  }
  return `type ${typeof v} (${String(v)})`;
}

function __runArgs(contextifiedObject, options) {
  if (typeof options === "string") options = { filename: options };
  options = options ?? {};
  if ((typeof options !== "object" && typeof options !== "function") || options === null) {
    const err = new TypeError(`The "options" argument must be of type object. Received ${options === null ? "null" : typeof options}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const filename = __normStr(options.filename, "filename", "evalmachine.<anonymous>");
  __normTimeout(options.timeout, "timeout");
  __normBool(options.displayErrors, "displayErrors");
  __normBool(options.breakOnSigint, "breakOnSigint");
  return { filename };
}

export function createContext(contextObject = {}, options = {}) {
  // Node 24+ DONT_CONTEXTIFY（真机实测语义）：新建独立 context，返回其 global
  // 对象本体——不等于主 globalThis、写入不穿透主域、runInContext("this")===返回值。
  // jsdom 29（vitest jsdom 环境）拿它当 window 直装 DOM 全局。
  // 注意：新 global 只有 SpiderMonkey 标准内建（Object/Array/Symbol 等），
  // 无 winterjs2 主域扩展（process/console/Buffer 等）——真机 vanilla 口径（§4.90 同源）。
  if (contextObject === __dontCtx) {
    const id = __vmCall(() => __wjs2_vm_create());
    const g = __vmCall(() => __wjs2_vm_global(id));
    __vmStdKeys(g).id = id;
    return g;
  }
  if (contextObject !== null && (typeof contextObject !== "object" && typeof contextObject !== "function")) {
    const err = new TypeError(`The "contextObject" argument must be of type object. Received type ${typeof contextObject}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (options === null || ((typeof options !== "object" && typeof options !== "function"))) {
    const err = new TypeError(`The "options" argument must be of type object. Received ${__recv(options)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  // name/origin 字符串校验（真机 ERR_INVALID_ARG_TYPE 文案逐字，basic 套件点名）。
  for (const k of ["name", "origin"]) {
    if (options[k] !== undefined && typeof options[k] !== "string") {
      const err = new TypeError(`The "options.${k}" property must be of type string. Received ${__recv(options[k])}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
  }
  if (isContext(contextObject)) return contextObject;
  const name = typeof options.name === "string" ? options.name : undefined;
  void name;
  if (options.microtaskMode !== undefined && options.microtaskMode !== "afterEvaluate") {
    const err = new TypeError(`The "options.microtaskMode" property must be one of 'afterEvaluate'. Received '${String(options.microtaskMode)}'`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  const id = __vmCall(() => __wjs2_vm_create());
  __vmStdKeys(contextObject).id = id;
  __vmSnapshot(id, contextObject);
  if (contextObject !== null && contextObject !== undefined) __syncIn(id, contextObject);
  __vmAutoRelease(contextObject, id);
  return contextObject;
}

export function runInContext(code, contextifiedObject, options) {
  const id = __validateCtx(contextifiedObject);
  const { filename } = __runArgs(contextifiedObject, options);
  __syncIn(id, contextifiedObject);
  const r = __vmCall(() => __wjs2_vm_run(id, String(code), filename));
  __syncOut(id, contextifiedObject);
  return r;
}

export function runInNewContext(code, contextObject, options) {
  if (typeof options === "string") options = { filename: options };
  options = options ?? {};
  // contextName/contextOrigin（createContext name/origin 的 run 侧别名）字符串校验。
  for (const k of ["contextName", "contextOrigin"]) {
    if (options[k] !== undefined && typeof options[k] !== "string") {
      const err = new TypeError(`The "options.${k}" property must be of type string. Received ${__recv(options[k])}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
  }
  if (contextObject === undefined) return runInContext(code, createContext({}, options ?? {}), options);
  const ctx = createContext(contextObject, options ?? {});
  return runInContext(code, ctx, options);
}

export function runInThisContext(code, options) {
  const { filename } = __runArgs(null, options);
  return __vmCall(() => __wjs2_vm_run_this(String(code), filename));
}

export class Script {
  constructor(code, options = {}) {
    code = String(code);
    if (typeof options === "string") options = { filename: options };
    if (options === null || (typeof options !== "object" && typeof options !== "function")) {
      const err = new TypeError(`The "options" argument must be of type object. Received ${__recv(options)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    this.__code = code;
    this.__filename = __normStr(options.filename, "filename", "evalmachine.<anonymous>");
    void __normUint(options.lineOffset, "lineOffset");
    void __normUint(options.columnOffset, "columnOffset");
    void __normTimeout(options.timeout, "timeout");
    void __normBool(options.displayErrors, "displayErrors");
    void __normBool(options.breakOnSigint, "breakOnSigint");
    void __normBool(options.produceCachedData, "produceCachedData");
    // cachedData 类型门（真机 validateBufferish：Buffer/TypedArray/DataView）；
    // 字节码本体接受忽略（无缓存引擎，记档），类型不对仍按真机拒。
    if (options.cachedData !== undefined &&
        !(typeof ArrayBuffer.isView === "function" && ArrayBuffer.isView(options.cachedData)) &&
        !(options.cachedData instanceof ArrayBuffer)) {
      const err = new TypeError('The "options.cachedData" property must be one of Buffer, TypedArray, or DataView');
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    __vmCall(() => __wjs2_vm_compile(code, this.__filename));
  }
  // Script 方法层 options 只收 object/function/undefined（真机 assertErrors：
  // 'bad'/42/null 即 TypeError）。
  __normOpts(options) {
    if (options !== undefined &&
        (options === null || (typeof options !== "object" && typeof options !== "function"))) {
      const err = new TypeError(`The "options" argument must be of type object. Received ${__recv(options)}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    return options ?? {};
  }
  runInContext(contextifiedObject, options) {
    return runInContext(this.__code, contextifiedObject, { ...this.__normOpts(options), filename: this.__filename });
  }
  // 真机形态：`this.runInContext(...)` 的成员访问先于实参求值——this 非对象时
  // 即报 `this.runInContext is not a function`（new-script-new-context 末块
  // `.call('hello')` 点名）；options 形状由 createContext 侧校验。
  runInNewContext(contextObject, options) {
    return this.runInContext(createContext(contextObject, options), options);
  }
  runInThisContext(options) {
    return runInThisContext(this.__code, { ...this.__normOpts(options), filename: this.__filename });
  }
  createCachedData() {
    return Buffer.alloc(0);
  }
  get cachedDataRejected() { return undefined; }
  get cachedDataProduced() { return false; }
  get sourceMapURL() { return undefined; }
}

export function createScript(code, options) {
  return new Script(code, options);
}

export function compileFunction(code, params, options = {}) {
  if (typeof code !== "string") {
    const err = new TypeError(`The "code" argument must be of type string. Received type ${typeof code}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (options === null || (typeof options !== "object" && typeof options !== "function")) {
    const err = new TypeError(`The "options" argument must be of type object. Received ${options === null ? "null" : typeof options}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (params !== undefined) {
    if (!Array.isArray(params)) {
      const err = new TypeError(`The "params" argument must be of type string array. Received type ${typeof params}`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
    for (const p of params) {
      if (typeof p !== "string") {
        const err = new TypeError("The \"params\" array must only contain strings.");
        err.code = "ERR_INVALID_ARG_TYPE";
        throw err;
      }
    }
  }
  const filename = __normStr(options.filename, "filename", "");
  let ctxId = "";
  if (options.parsingContext !== undefined) {
    ctxId = __validateCtx(options.parsingContext);
  }
  // undefined → 默认 []；null/非数组 → 抛（真机 null 不给 ?? 吞掉）。
  const extOpt = options.contextExtensions;
  if (extOpt !== undefined && !Array.isArray(extOpt)) {
    const err = new TypeError(`The "options.contextExtensions" property must be an instance of Array. Received ${__recv(extOpt)}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const exts = extOpt ?? [];
  for (let ei = 0; ei < exts.length; ei++) {
    const ext = exts[ei];
    if (ext === null || (typeof ext !== "object" && typeof ext !== "function")) {
      const err = new TypeError(`The "options.contextExtensions[${ei}]" property must be of type object. Received type ${typeof ext} (${String(ext)})`);
      err.code = "ERR_INVALID_ARG_TYPE";
      throw err;
    }
  }
  const paramsCsv = (params ?? []).join(",");
  const fn = __vmCall(() => __wjs2_vm_compile_fn(ctxId, paramsCsv, code, filename));
  if (ctxId === "") {
    for (const ext of exts) Object.assign(globalThis, ext);
  } else {
    for (const ext of exts) __syncIn(ctxId, ext);
  }
  return fn;
}

export function measureMemory(options = {}) {
  if (typeof process !== "undefined" && typeof process.emitWarning === "function") {
    process.emitWarning("vm.measureMemory is experimental", "ExperimentalWarning");
  }
  void options;
  const err = new Error("vm.measureMemory requires a context with memory measurement support");
  err.code = "ERR_CONTEXT_NOT_INITIALIZED";
  return Promise.reject(err);
}

const __useMainLoader = Symbol("USE_MAIN_CONTEXT_DEFAULT_LOADER");
const __dontCtx = Symbol("DONT_CONTEXTIFY");
export const constants = Object.freeze({
  USE_MAIN_CONTEXT_DEFAULT_LOADER: __useMainLoader,
  DONT_CONTEXTIFY: __dontCtx,
});

// ── 9i-1 模块系（真机口径：node --experimental-vm-modules 实测，见模块头注）──
// SourceTextModule：Rust 侧编译/link/evaluate（零导入全链；带导入 link 即
// ERR_VM_MODULE_LINK_FAILURE，linker 切片后续）；SyntheticModule 纯 JS。

let __vmModSeq = 0;

function __modErr(code, message) {
  const err = new Error(message);
  err.code = code;
  throw err;
}

export class Module {
  link(linker) {
    if (typeof linker !== "function") {
      __modErr("ERR_INVALID_ARG_TYPE", `The "linker" argument must be of type function. Received ${linker === null ? "null" : typeof linker}`);
    }
    return this.__doLink(linker);
  }
  evaluate() {
    return this.__doEvaluate();
  }
}

export class SourceTextModule extends Module {
  constructor(code, options = {}) {
    super();
    if (typeof code !== "string") {
      __modErr("ERR_INVALID_ARG_TYPE", `The "code" argument must be of type string. Received type ${typeof code}`);
    }
    if (options === null || (typeof options !== "object" && typeof options !== "function")) {
      __modErr("ERR_INVALID_ARG_TYPE", `The "options" argument must be of type object. Received ${options === null ? "null" : typeof options}`);
    }
    const identifier = __normStr(options.identifier, "identifier", `vm:module(${__vmModSeq++})`);
    let ctxId;
    if (options.context !== undefined) {
      ctxId = __validateCtx(options.context);
      this.__context = options.context;
    } else {
      ctxId = __vmCall(() => __wjs2_vm_create());
      const holder = {};
      __vmStdKeys(holder).id = ctxId;
      __vmSnapshot(ctxId, holder);
      __vmAutoRelease(holder, ctxId);
      this.__context = undefined;
    }
    this.__ctxId = ctxId;
    this.__identifier = identifier;
    this.__status = "unlinked";
    this.__error = null;
    this.__ns = undefined;
    // importModuleDynamically/initializeImportMeta 接受忽略（v1 未接线，记档）。
    this.__id = __vmCall(() => __wjs2_vm_compile_mod(ctxId, identifier, code));
    __vmModAutoRelease(this, this.__id);
  }
  get status() { return this.__status; }
  get identifier() { return this.__identifier; }
  get context() { return this.__context; }
  get dependencySpecifiers() {
    return JSON.parse(__vmCall(() => __wjs2_vm_mod_deps(this.__id)));
  }
  get namespace() {
    if (this.__status !== "evaluated") {
      __modErr("ERR_VM_MODULE_STATUS", "Module status must be evaluated");
    }
    return this.__ns;
  }
  get error() {
    if (this.__status !== "errored") {
      __modErr("ERR_VM_MODULE_STATUS", "Module status must be errored");
    }
    return this.__error;
  }
  __doLink(linker) {
    if (this.__status !== "unlinked") {
      return Promise.reject(Object.assign(new Error("Module status must be unlinked"), { code: "ERR_VM_MODULE_STATUS" }));
    }
    void linker;
    return Promise.resolve().then(() => {
      __vmCall(() => __wjs2_vm_link(this.__id));
      this.__status = "linked";
    });
  }
  __doEvaluate() {
    if (this.__status !== "linked" && this.__status !== "evaluated" && this.__status !== "errored") {
      return Promise.reject(Object.assign(new Error("Module status must be linked"), { code: "ERR_VM_MODULE_STATUS" }));
    }
    if (this.__status === "evaluated") return Promise.resolve(undefined);
    return Promise.resolve().then(() => {
      // native 同步抛错（参数/link 前置）即失败落定；完成值可能是跨域 promise
      // （§4.57：`instanceof Promise` 跨 compartment 恒 false，必须按 thenable 认领，
      // 否则落定被丢弃、报错变 unhandled rejection）。
      const r = __vmCall(() => __wjs2_vm_evaluate(this.__id));
      const done = () => {
        __vmCall(() => __wjs2_vm_mod_settled(this.__id));
        this.__status = "evaluated";
        this.__ns = __vmCall(() => __wjs2_vm_mod_ns(this.__id));
        return undefined;
      };
      const failed = (e) => {
        this.__status = "errored";
        this.__error = e;
        throw e;
      };
      if (r !== null && (typeof r === "object" || typeof r === "function") && typeof r.then === "function") {
        return r.then(done, failed);
      }
      try {
        return done();
      } catch (e) {
        return failed(e);
      }
    });
  }
}

export class SyntheticModule extends Module {
  constructor(exportNames, evaluateCallback, options = {}) {
    super();
    if (!Array.isArray(exportNames)) {
      __modErr("ERR_INVALID_ARG_TYPE", `The "exportNames" argument must be of type array. Received type ${typeof exportNames}`);
    }
    if (typeof evaluateCallback !== "function") {
      __modErr("ERR_INVALID_ARG_TYPE", `The "evaluateCallback" argument must be of type function. Received type ${typeof evaluateCallback}`);
    }
    if (options === null || (typeof options !== "object" && typeof options !== "function")) {
      __modErr("ERR_INVALID_ARG_TYPE", `The "options" argument must be of type object. Received ${options === null ? "null" : typeof options}`);
    }
    this.__exports = {};
    for (const n of exportNames) this.__exports[String(n)] = undefined;
    this.__cb = evaluateCallback;
    this.__identifier = __normStr(options.identifier, "identifier", `vm:module(${__vmModSeq++})`);
    this.__status = "linked";
    this.__error = null;
    this.__ns = undefined;
  }
  get status() { return this.__status; }
  get identifier() { return this.__identifier; }
  get dependencySpecifiers() { return undefined; }
  get namespace() {
    if (this.__status !== "evaluated") {
      __modErr("ERR_VM_MODULE_STATUS", "Module status must be evaluated");
    }
    return this.__ns;
  }
  get error() {
    if (this.__status !== "errored") {
      __modErr("ERR_VM_MODULE_STATUS", "Module status must be errored");
    }
    return this.__error;
  }
  setExport(name, value) {
    if (this.__status === "evaluated") {
      __modErr("ERR_VM_MODULE_STATUS", "Module status must not be evaluated");
    }
    this.__exports[String(name)] = value;
  }
  // Synthetic 的 linker 可选（真机口径：无参 link 即过）。
  link(linker) {
    if (linker !== undefined) return super.link(linker);
    return this.__doLink(undefined);
  }
  __doLink(linker) {
    void linker;
    // Synthetic 出生即 linked；重复 link 照真机保持 linked（无操作成功）。
    return Promise.resolve(undefined);
  }
  __doEvaluate() {
    if (this.__status === "evaluated") return Promise.resolve(undefined);
    return Promise.resolve().then(() => {
      try {
        this.__cb(this.__exports);
      } catch (e) {
        this.__status = "errored";
        this.__error = e;
        throw e;
      }
      this.__status = "evaluated";
      this.__ns = Object.freeze({ ...this.__exports });
      return undefined;
    });
  }
}

const __api = {
  Script, createContext, createScript, runInContext, runInNewContext,
  runInThisContext, isContext, compileFunction, measureMemory, constants,
  Module, SourceTextModule, SyntheticModule,
};
export default __api;
