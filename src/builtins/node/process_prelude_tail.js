// Node 兼容旗语义（CLI 起点剥下，见 cli::strip_node_compat_args）：
// --expose-gc 即暴露 globalThis.gc（async no-op——真收集另案，调用形状先行；
// 无旗不暴露，真机口径）；名单挂内部位供 http 默认宽松等消费（不进 process.env）。
try {
  const __compat = JSON.parse(__wjs2_node_compat_json());
  globalThis.__wjs2_nodeCompat = Array.isArray(__compat) ? __compat : [];
  // gc 门控（--expose-gc/--expose_gc 双拼写， deterioration 套件用下划线形）。
  const __hasGc = globalThis.__wjs2_nodeCompat.includes("--expose-gc") ||
    globalThis.__wjs2_nodeCompat.includes("--expose_gc");
  if (__hasGc && typeof globalThis.gc !== "function") {
    globalThis.gc = async function gc() { return undefined; };
  }
} catch { globalThis.__wjs2_nodeCompat = []; }
// 标题（node 口径：--title=v 末个赢；缺省回 execPath 基名；set 透写同 store）。
try {
  const __tf = globalThis.__wjs2_nodeCompat.filter((a) => a.startsWith("--title="));
  globalThis.__wjs2_processTitle = __tf.length ? __tf[__tf.length - 1].slice("--title=".length) : null;
} catch { globalThis.__wjs2_processTitle = null; }
// 告警旗（node 口径：旗在才定义属性）+ 缺省打印监听（--no-warnings / NODE_NO_WARNINGS=1 不登记）。
{
  const __f = globalThis.__wjs2_nodeCompat;
  const __p = globalThis.process;
  if (__f.includes("--no-deprecation")) __p.noDeprecation = true;
  if (__f.includes("--throw-deprecation")) __p.throwDeprecation = true;
  if (__f.includes("--trace-deprecation")) __p.traceDeprecation = true;
  if (__f.includes("--trace-warnings")) __p.traceProcessWarnings = true;
  const __rw = __f.find((a) => a.startsWith("--redirect-warnings="));
  if (__rw) __p.__wjs2_warningFile = __rw.slice("--redirect-warnings=".length);
  let __nw = false;
  try { __nw = __wjs2_env_get("NODE_NO_WARNINGS") === "1"; } catch {}
  if (!__f.includes("--no-warnings") && !__nw) __p.on("warning", __p.__wjs2_onWarning);
}
// Node 口径：NODE_DEBUG 置位即启动期警告一次（首 section 名；debug.js 套件
// 逐字断言。stderr 直写，不走 warning 通道）。
try {
  const __nd = __wjs2_env_get("NODE_DEBUG");
  if (__nd !== undefined && __nd !== null && String(__nd).trim() !== "") {
    const __sec = String(__nd).split(",")[0].trim();
    __wjs2_stderr_write(`Setting the NODE_DEBUG environment variable to '${__sec}' can expose sensitive data (such as passwords, tokens and authentication headers) in the resulting log.\n`);
  }
} catch { /* 环境不可读即跳过 */ }
// R9：config 冻结 + exitCode 不可删除（真机 bootstrap 口径：config 经 reviver
// 深冻；exitCode configurable:false，严格模式 delete 即抛）。
try {
  const __pc = globalThis.process.config;
  if (__pc && typeof __pc === "object") {
    try { if (__pc.target_defaults) Object.freeze(__pc.target_defaults); } catch {}
    try { if (__pc.variables) Object.freeze(__pc.variables); } catch {}
    Object.freeze(__pc);
  }
} catch {}
try {
  const __d = Object.getOwnPropertyDescriptor(globalThis.process, "exitCode");
  if (__d) Object.defineProperty(globalThis.process, "exitCode", { ...__d, configurable: false });
} catch {}
// R9：SM 删除不可配置属性的文案与 V8 不同（`property "exitCode" is ...` vs
// 真机 `Cannot delete property 'exitCode' of #<process>`，validation 套件点名
// 正则）。Proxy 只拦 deleteProperty，其余默认透传（get/set 经 target，
// this 为 proxy 时读写 __wjs2_* 表经转发一致；PROTO_FIXUP 的 setPrototypeOf
// 亦默认透传）。
try {
  const __target = globalThis.process;
  globalThis.process = new Proxy(__target, {
    deleteProperty(t, p) {
      if (p === "exitCode") {
        throw new TypeError("Cannot delete property 'exitCode' of #<process>");
      }
      return Reflect.deleteProperty(t, p);
    },
  });
} catch {}
