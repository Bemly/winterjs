//! 本体第二批 JS 面：`WinterJS2.shell/hex/time/retry/graph/git/oauth/transpile/log/mime/cookie/httpdate`
//!（prelude 分域；拼接顺序见 mod.rs）。
//!
//! 薄壳规则：校验在调 native 之前；纯函数面统一改包真 TypeError，
//! 权限类（shell env）按前缀还原类名；token HTTP 走 fetch 栈（见 oauth）。
pub const WSYS_JS: &str = r#"
{
  const __wsys_str = (v, what) => {
    if (typeof v !== "string") throw new TypeError(`${what} requires a string`);
    return v;
  };
  const __wsys_json = (s) => JSON.parse(s);
  // 纯函数面改包真 TypeError；PermissionError 前缀还原类名。
  const __wsys_call = (f) => {
    try { return f(); } catch (e) {
      const m = String((e && e.message) || e);
      if (m.startsWith("PermissionError")) { const p = new Error(m.replace(/^PermissionError:\s*/, "")); p.name = "PermissionError"; throw p; }
      throw new TypeError(m.replace(/^TypeError:\s*/, ""));
    }
  };
  const shell = {
    expand(s) { return __wsys_call(() => __wjs2_wsys_shell_expand(__wsys_str(s, "shell"))); },
  };
  const hex = {
    encode(d) {
      let u8 = d;
      if (typeof d === "string") u8 = new TextEncoder().encode(d);
      else if (d instanceof ArrayBuffer) u8 = new Uint8Array(d);
      else if (ArrayBuffer.isView(d)) u8 = new Uint8Array(d.buffer, d.byteOffset, d.byteLength);
      else throw new TypeError("hex encode requires string, Uint8Array, or ArrayBuffer");
      return __wjs2_wsys_hex_encode(u8);
    },
    decode(s) { return __wsys_call(() => __wjs2_wsys_hex_decode(__wsys_str(s, "hex"))); },
  };
  const time = {
    now() { return __wjs2_wsys_time_now(); },
    parse(s) { return __wsys_call(() => __wjs2_wsys_time_parse(__wsys_str(s, "time"))); },
    format(ms, fmt, tz) {
      if (typeof ms !== "number" || typeof fmt !== "string") throw new TypeError("time format requires (ms, fmt)");
      return __wsys_call(() => __wjs2_wsys_time_format(ms, fmt, tz === undefined ? "" : String(tz)));
    },
  };
  const retry = {
    delay(kind, attempt, o) {
      const opts = o || {};
      const minMs = Number(opts.minMs ?? 100);
      const maxMs = Number(opts.maxMs ?? 5000);
      const factor = Number(opts.factor ?? 2);
      return __wsys_call(() => __wjs2_wsys_retry_delay(String(kind), Number(attempt), minMs, maxMs, factor));
    },
    // 全量 helper（backon 档位数学 + setTimeout 等待；返回 { value, attempts }）。
    async run(fn, o) {
      if (typeof fn !== "function") throw new TypeError("retry run requires a function");
      const opts = o || {};
      const kind = String(opts.kind ?? "exponential");
      const attempts = Math.max(1, Math.min(100, Math.floor(Number(opts.attempts ?? 3))));
      let last = null;
      for (let a = 0; a < attempts; a++) {
        try { return { value: await fn(a), attempts: a + 1 }; }
        catch (e) {
          last = e;
          if (a + 1 >= attempts) break;
          await new Promise((r) => setTimeout(r, retry.delay(kind, a, opts)));
        }
      }
      throw last;
    },
  };
  const graph = {
    create(k) { return __wsys_call(() => __wjs2_wsys_graph_create(__wsys_str(k ?? "directed", "graph"))); },
    addNode(id, label) { return __wsys_call(() => __wjs2_wsys_graph_add_node(Number(id), __wsys_str(label, "graph"))); },
    addEdge(id, a, b, label) { return __wsys_call(() => __wjs2_wsys_graph_add_edge(Number(id), Number(a), Number(b), label === undefined ? "" : String(label))); },
    toposort(id) { return __wsys_json(__wsys_call(() => __wjs2_wsys_graph_toposort(Number(id)))); },
    counts(id) { return __wsys_json(__wsys_call(() => __wjs2_wsys_graph_counts(Number(id)))); },
    free(id) { return __wsys_call(() => __wjs2_wsys_graph_free(Number(id))); },
  };
  const git = {
    revParse(path, rev) { return __wsys_call(() => __wjs2_wsys_git_rev_parse(__wsys_str(path, "git"), __wsys_str(rev ?? "HEAD", "git"))); },
    log(path, rev, n) { return __wsys_json(__wsys_call(() => __wjs2_wsys_git_log(__wsys_str(path, "git"), __wsys_str(rev ?? "HEAD", "git"), n === undefined ? 10 : Number(n)))); },
  };
  const oauth = {
    // 授权 URL 纯构造（oauth2 轮子：state 随机 + PKCE 透传；token HTTP 见 exchangeCode）。
    authorizeUrl(o) {
      if (!o || typeof o !== "object") throw new TypeError("oauth authorizeUrl requires options");
      return __wsys_json(__wsys_call(() => __wjs2_wsys_oauth_authorize_url(
        __wsys_str(o.authUrl, "oauth"), __wsys_str(o.clientId, "oauth"),
        __wsys_str(o.redirectUri, "oauth"), String(o.scope ?? ""),
        o.state === undefined ? "" : String(o.state),
        o.challenge === undefined ? "" : String(o.challenge))));
    },
    pkce() { return __wsys_json(__wjs2_wsys_oauth_pkce()); },
    // token 交换走 fetch 栈（RFC 6749 表单；与 serve/fetch 同 TLS/DNS）。
    async exchangeCode(o) {
      if (!o || typeof o !== "object") throw new TypeError("oauth exchangeCode requires options");
      const body = new URLSearchParams({
        grant_type: "authorization_code", code: String(o.code),
        redirect_uri: String(o.redirectUri), client_id: String(o.clientId),
      });
      if (o.clientSecret !== undefined) body.set("client_secret", String(o.clientSecret));
      if (o.verifier !== undefined) body.set("code_verifier", String(o.verifier));
      const res = await fetch(String(o.tokenUrl), { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" }, body: String(body) });
      if (!res.ok) throw new Error(`oauth exchange failed: HTTP ${res.status}`);
      return res.json();
    },
    async refreshToken(o) {
      if (!o || typeof o !== "object") throw new TypeError("oauth refreshToken requires options");
      const body = new URLSearchParams({
        grant_type: "refresh_token", refresh_token: String(o.refreshToken), client_id: String(o.clientId),
      });
      if (o.clientSecret !== undefined) body.set("client_secret", String(o.clientSecret));
      const res = await fetch(String(o.tokenUrl), { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" }, body: String(body) });
      if (!res.ok) throw new Error(`oauth refresh failed: HTTP ${res.status}`);
      return res.json();
    },
  };
  const transpile = (src, o) => {
    const s = __wsys_str(src, "transpile");
    const filename = (o && o.filename !== undefined) ? String(o.filename) : "input.ts";
    return __wsys_call(() => __wjs2_wsys_transpile(s, filename));
  };
  const log = {
    debug(m) { __wjs2_wsys_log("debug", String(m)); },
    info(m) { __wjs2_wsys_log("info", String(m)); },
    warn(m) { __wjs2_wsys_log("warn", String(m)); },
    error(m) { __wjs2_wsys_log("error", String(m)); },
  };
  const mime = {
    lookup(p) { return __wjs2_wsys_mime_lookup(__wsys_str(p, "mime")); },
  };
  const cookie = {
    parse(h) { return __wsys_json(__wsys_call(() => __wjs2_wsys_cookie_parse(__wsys_str(h, "cookie")))); },
    serialize(name, value, o) {
      return __wsys_call(() => __wjs2_wsys_cookie_serialize(
        __wsys_str(name, "cookie"), String(value),
        JSON.stringify(o === undefined ? null : o)));
    },
  };
  const httpdate = {
    parse(s) { return __wsys_call(() => __wjs2_wsys_httpdate_parse(__wsys_str(s, "httpdate"))); },
    format(ms) {
      if (typeof ms !== "number") throw new TypeError("httpdate format requires ms");
      return __wsys_call(() => __wjs2_wsys_httpdate_format(ms));
    },
  };
  try {
    const W = globalThis.WinterJS2;
    if (W && W.shell === undefined) {
      W.shell = shell;
      W.hex = hex;
      W.time = time;
      W.retry = retry;
      W.graph = graph;
      W.git = git;
      W.oauth = oauth;
      W.transpile = transpile;
      W.log = log;
      W.mime = mime;
      W.cookie = cookie;
      W.httpdate = httpdate;
    }
  } catch {}
}
"#;
