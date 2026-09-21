//! fetch 入口与中止清理（prelude 分域；拼接顺序见 mod.rs）。
pub const FETCH_JS: &str = r#"
globalThis.fetch = (input, init = {}) => {
  const req = new Request(input, init);
  const st = __wjs_reqState.get(req);
  if (st.signal && st.signal.aborted) {
    const reason = st.signal.reason !== undefined
      ? st.signal.reason
      : __wjs_make_fetch_error("AbortError: fetch aborted");
    return Promise.reject(reason);
  }
  const headersJson = JSON.stringify([...st.headers]);
  return new Promise((resolve, reject) => {
    // 监听留到流结束：head 结算只 resolve（流式 body 的 abort 还靠它）；
    // head 失败或 abort 触发或流终结时经 `__wjs_fetchCleanup` 摘除。
    let onAbort = null;
    const cleanup = () => {
      if (onAbort && st.signal) st.signal.removeEventListener("abort", onAbort);
      onAbort = null;
    };
    const id = __wjs_fetch_start(
      st.url, st.method, headersJson, st.bodyU8 ?? undefined,
      (v) => resolve(v),
      (e) => { if (id) __wjs_fetchCleanup(id); else cleanup(); reject(e); },
    );
    if (st.signal && id) {
      onAbort = () => {
        // Rust 侧取消任务 + 拒绝排队 pull（AbortError）；外层按原始 reason 拒绝。
        __wjs_abortedFetch.add(id);
        __wjs_fetch_abort(id);
        __wjs_fetchCleanup(id);
        reject(st.signal.reason);
      };
      st.signal.addEventListener("abort", onAbort);
      __wjs_fetchCleanups.set(id, cleanup);
    }
  });
};
// 已中止的流 id 集（pull 侧直接拒绝，不再进 Rust 状态）。
const __wjs_abortedFetch = new Set();
// 待摘的 abort 监听（流终结/取消时清理，长 signal 不堆积）。
const __wjs_fetchCleanups = new Map();
function __wjs_fetchCleanup(sid) {
  const fn = __wjs_fetchCleanups.get(sid);
  if (fn) {
    __wjs_fetchCleanups.delete(sid);
    try { fn(); } catch {}
  }
}
"#;
