//! 本体命名空间（prelude 分域；拼接顺序见 mod.rs）。
//! `globalThis.Deno`（冻结只读）+ `globalThis.Bun` / `globalThis.WinterJS`（普通可写）。
//! 全部经已有底座别名：fs 读 `node:fs/promises`、os 读 `node:os`、子进程读
//! `node:child_process`、serve 经 `node:http` 转 Fetch 形、test 经 `node:test`。
//! 无底座故记档另案：`Bun.TOML`/`Bun.YAML`/`Bun.Transpiler`/`Bun.FileSystemRouter`、
//! 独立 `Deno.upgradeWebSocket`（serve 内升级随 serve 轮）。
pub const NAMESPACE_JS: &str = r#"
{
  const __wjs_ns_version = () => {
    try {
      const v = globalThis.process && globalThis.process.versions;
      if (v && typeof v.winterjs === "string") return v.winterjs;
    } catch {}
    return "26.9.27";
  };
  const __wjs_ns_req = (spec) => {
    try {
      const r = globalThis.require;
      if (typeof r !== "function") return null;
      return r(spec);
    } catch { return null; }
  };
  const __wjs_ns_fsp = () => __wjs_ns_req("node:fs/promises");
  const __wjs_ns_fs = () => __wjs_ns_req("node:fs");
  const __wjs_ns_os = () => __wjs_ns_req("node:os");
  const __wjs_ns_child = () => __wjs_ns_req("node:child_process");
  const __wjs_ns_proc = () => globalThis.process || null;
  const __wjs_ns_u8 = (v) => {
    if (v instanceof Uint8Array) return v;
    if (typeof v === "string") return new TextEncoder().encode(v);
    if (v instanceof ArrayBuffer) return new Uint8Array(v);
    if (ArrayBuffer.isView(v)) return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
    throw new TypeError("data must be string, Uint8Array, or ArrayBuffer");
  };
  const __wjs_ns_envstore = () => {
    try {
      const p = globalThis.process;
      if (p && p.env) return p.env;
    } catch {}
    return {};
  };
  const __wjs_ns_envface = () => {
    return {
      get(k) { return __wjs_ns_envstore()[String(k)]; },
      set(k, v) { __wjs_ns_envstore()[String(k)] = String(v); },
      delete(k) { delete __wjs_ns_envstore()[String(k)]; },
      has(k) { return Object.prototype.hasOwnProperty.call(__wjs_ns_envstore(), String(k)); },
      toObject() { return { ...__wjs_ns_envstore() }; },
    };
  };
  const __wjs_ns_serve = (opts, handler) => {
    const http = __wjs_ns_req("node:http");
    if (!http) throw new Error("serve requires node:http");
    let o = opts || {};
    let h = handler;
    if (typeof o === "function") { h = o; o = {}; }
    const port = o.port ?? 8000;
    const hostname = o.hostname ?? o.host ?? "127.0.0.1";
    const server = http.createServer(async (req, res) => {
      try {
        const chunks = [];
        for await (const c of req) chunks.push(c);
        const body = chunks.length ? Buffer.concat(chunks) : null;
        const url = "http://" + (req.headers.host || (hostname + ":" + port)) + req.url;
        const fwd = {};
        for (const [k, v] of Object.entries(req.headers)) {
          if (v !== undefined) fwd[k] = Array.isArray(v) ? v.join(", ") : String(v);
        }
        const frequest = new Request(url, { method: req.method, headers: fwd, body });
        const fresponse = await h(frequest);
        const out = {};
        fresponse.headers.forEach((v, k) => { out[k] = v; });
        res.writeHead(fresponse.status, out);
        const buf = new Uint8Array(await fresponse.arrayBuffer());
        res.end(Buffer.from(buf));
      } catch (e) {
        try { res.writeHead(500); res.end(String((e && e.message) || e)); } catch {}
      }
    });
    return new Promise((resolve, reject) => {
      server.on("error", reject);
      server.listen(port, hostname, () => {
        try { if (typeof o.onListen === "function") o.onListen({ port, hostname }); } catch {}
        resolve({
          addr: { transport: "tcp", hostname, port },
          get finished() { return new Promise((r) => server.on("close", r)); },
          shutdown() { return new Promise((r) => server.close(() => r())); },
          [Symbol.asyncDispose]() { return this.shutdown(); },
        });
      });
    });
  };
  const __wjs_ns_cmd = (file, opts) => {
    const o = opts || {};
    const base = { cwd: o.cwd, env: o.env };
    if (o.stdin !== undefined) base.stdio = o.stdin;
    return {
      output: () => {
        const child = __wjs_ns_child();
        if (!child) throw new Error("Command requires node:child_process");
        const r = child.spawnSync(String(file), (o.args || []).map(String), { ...base, encoding: "buffer" });
        if (r.error) throw r.error;
        return Promise.resolve({ code: r.status ?? 0, success: (r.status ?? 0) === 0, stdout: r.stdout, stderr: r.stderr });
      },
      outputSync: () => {
        const child = __wjs_ns_child();
        if (!child) throw new Error("Command requires node:child_process");
        const r = child.spawnSync(String(file), (o.args || []).map(String), { ...base, encoding: "buffer" });
        if (r.error) throw r.error;
        return { code: r.status ?? 0, success: (r.status ?? 0) === 0, stdout: r.stdout, stderr: r.stderr };
      },
      spawn: () => { throw new Error("Command.spawn: use output()/outputSync() (streaming spawn deferred)"); },
    };
  };
  const __wjs_ns_bunfile = (path) => {
    const p = String(path);
    return {
      name: p,
      get size() {
        try { return __wjs_ns_fs().statSync(p).size; } catch { return 0; }
      },
      exists() {
        try { return __wjs_ns_fs().existsSync(p); } catch { return false; }
      },
      text() { return __wjs_ns_fsp().readFile(p, "utf8"); },
      arrayBuffer() {
        return __wjs_ns_fsp().readFile(p).then((b) => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength));
      },
      json() { return this.text().then((t) => JSON.parse(t)); },
    };
  };
  const __wjs_ns_errs = () => {
    const mk = (name) => class extends Error {
      constructor(m) { super(m === undefined ? "" : String(m)); this.name = name; }
    };
    return {
      AlreadyExists: mk("AlreadyExists"), NotFound: mk("NotFound"),
      PermissionDenied: mk("PermissionDenied"), ConnectionRefused: mk("ConnectionRefused"),
      ConnectionReset: mk("ConnectionReset"), InvalidData: mk("InvalidData"),
      TimedOut: mk("TimedOut"), Interrupted: mk("Interrupted"),
      WriteZero: mk("WriteZero"), UnexpectedEof: mk("UnexpectedEof"),
      BadResource: mk("BadResource"), Busy: mk("Busy"),
    };
  };
  if (globalThis.Deno === undefined) {
    const __wjs_deno_version = () => {
      const w = __wjs_ns_version();
      return { deno: w, v8: "13.6", typescript: "5.9" };
    };
    const __wjs_deno_errors = __wjs_ns_errs();
    globalThis.Deno = {
      get version() { return __wjs_deno_version(); },
      get args() {
        try {
          const a = __wjs_ns_proc() && __wjs_ns_proc().argv;
          return Array.isArray(a) ? a.slice(2) : [];
        } catch { return []; }
      },
      get pid() { try { return __wjs_ns_proc() ? __wjs_ns_proc().pid : 0; } catch { return 0; } },
      get ppid() { try { return (__wjs_ns_proc() && __wjs_ns_proc().ppid) || 0; } catch { return 0; } },
      get mainModule() {
        try { return String((__wjs_ns_proc() && __wjs_ns_proc().argv[1]) || ""); } catch { return ""; }
      },
      get execPath() { try { return String(__wjs_ns_proc().execPath || ""); } catch { return ""; } },
      get build() {
        try {
          const p = __wjs_ns_proc();
          return { target: (p.arch || "aarch64") + "-apple-darwin", arch: p.arch || "aarch64", os: "darwin", vendor: "apple", env: undefined };
        } catch { return { target: "aarch64-apple-darwin", arch: "aarch64", os: "darwin", vendor: "apple", env: undefined }; }
      },
      get arch() { try { return String(__wjs_ns_proc().arch || "aarch64"); } catch { return "aarch64"; } },
      get platform() { try { return String(__wjs_ns_proc().platform || "darwin"); } catch { return "darwin"; } },
      get noColor() { try { return !!process.env.NO_COLOR; } catch { return false; } },
      env: __wjs_ns_envface(),
      errors: __wjs_deno_errors,
      cwd() { return __wjs_ns_proc().cwd(); },
      chdir(d) { __wjs_ns_proc().chdir(String(d)); },
      exit(c) { __wjs_ns_proc().exit(c); },
      hostname() { return __wjs_ns_os().hostname(); },
      osRelease() { return __wjs_ns_os().release(); },
      networkInterfaces() {
        try {
          const os = __wjs_ns_os();
          if (typeof os.networkInterfaces === "function") return os.networkInterfaces();
        } catch {}
        return [];
      },
      systemMemoryInfo() {
        const os = __wjs_ns_os();
        return { total: os.totalmem(), free: os.freemem(), available: os.freemem(), cached: 0, buffers: 0, swapTotal: 0, swapFree: 0 };
      },
      consoleSize() {
        try {
          const s = __wjs_ns_proc().stdout.getWindowSize();
          if (Array.isArray(s)) return { columns: s[0], rows: s[1] };
        } catch {}
        return { columns: 80, rows: 24 };
      },
      get stdin() { return __wjs_ns_proc().stdin; },
      get stdout() { return __wjs_ns_proc().stdout; },
      get stderr() { return __wjs_ns_proc().stderr; },
      readFile(p) { return __wjs_ns_fsp().readFile(String(p)); },
      writeFile(p, d, o) { return __wjs_ns_fsp().writeFile(String(p), __wjs_ns_u8(d), o); },
      readTextFile(p) { return __wjs_ns_fsp().readFile(String(p), "utf8"); },
      writeTextFile(p, t, o) { return __wjs_ns_fsp().writeFile(String(p), String(t), o); },
      open(p, o) { return __wjs_ns_fsp().open(String(p), o && o.read === false ? "w" : "r"); },
      stat(p) { return __wjs_ns_fsp().stat(String(p)); },
      lstat(p) { return __wjs_ns_fsp().lstat(String(p)); },
      mkdir(p, o) { return __wjs_ns_fsp().mkdir(String(p), o); },
      remove(p, o) { return __wjs_ns_fsp().rm(String(p), { recursive: !!(o && o.recursive) }); },
      rename(a, b) { return __wjs_ns_fsp().rename(String(a), String(b)); },
      copyFile(a, b) { return __wjs_ns_fsp().copyFile(String(a), String(b)); },
      symlink(a, b) { return __wjs_ns_fsp().symlink(String(a), String(b)); },
      readlink(p) { return __wjs_ns_fsp().readlink(String(p)); },
      realPath(p) { return __wjs_ns_fsp().realpath(String(p)); },
      readDir(p) {
        return __wjs_ns_fsp().readdir(String(p), { withFileTypes: true }).then((ents) =>
          ents.map((e) => ({ name: e.name, isFile: e.isFile(), isDirectory: e.isDirectory(), isSymlink: e.isSymbolicLink() })));
      },
      makeTempDir(o) { return __wjs_ns_fsp().mkdtemp(((o && o.dir) || "/tmp") + "/deno-"); },
      makeTempFile(o) { return __wjs_ns_fsp().mkdtemp(((o && o.dir) || "/tmp") + "/deno-").then((d) => d + "/tmp"); },
      truncate(p, l) { return __wjs_ns_fsp().truncate(String(p), l); },
      chmod(p, m) { return __wjs_ns_fsp().chmod(String(p), m); },
      chown(p, u, g) { return __wjs_ns_fsp().chown(String(p), u, g); },
      utime(p, a, m) { return __wjs_ns_fsp().utimes(String(p), a, m); },
      watchFs(p, o) { return __wjs_ns_fs().watch(p, o); },
      statFs(p) {
        return __wjs_ns_fsp().statfs
          ? __wjs_ns_fsp().statfs(String(p)).then((s) => ({ type: 0, bsize: 4096, blocks: 0, bfree: 0, bavail: 0, files: 0, ffree: 0 }))
          : Promise.resolve({ type: 0, bsize: 4096, blocks: 0, bfree: 0, bavail: 0, files: 0, ffree: 0 });
      },
      test(name, fn) {
        const t = __wjs_ns_req("node:test");
        if (!t) throw new Error("Deno.test requires node:test");
        if (typeof name === "function") return t.test(name.name || "anonymous", name);
        if (typeof fn === "function") return t.test(String(name), fn);
        return t.test(String((name && name.name) || "anonymous"), (name && name.fn) || (() => {}));
      },
      serve(o, h) { return __wjs_ns_serve(o, h); },
      connect(o) {
        const net = __wjs_ns_req("node:net");
        if (!net) throw new Error("Deno.connect requires node:net");
        const sock = net.connect(o.port, o.hostname);
        return Promise.resolve(sock);
      },
      listen(o) {
        const net = __wjs_ns_req("node:net");
        if (!net) throw new Error("Deno.listen requires node:net");
        const server = net.createServer();
        return new Promise((resolve, reject) => {
          server.on("error", reject);
          server.listen(o.port, o.hostname, () => resolve({
            addr: server.address(),
            accept() { throw new Error("Deno.listen().accept: use node:net server events (documented deviation)"); },
            close() { server.close(); },
            [Symbol.asyncDispose]() { server.close(); },
          }));
        });
      },
      listenDatagram(o) {
        const d = __wjs_ns_req("node:dgram");
        if (!d) throw new Error("Deno.listenDatagram requires node:dgram");
        return d.createSocket(o.transport === "tcp" ? "tcp" : "udp4");
      },
      resolveDns(q, t) {
        const dns = __wjs_ns_req("node:dns");
        const promises = __wjs_ns_req("node:dns/promises");
        const kind = String(t || "A").toUpperCase();
        const run = promises && promises.resolve ? promises.resolve.bind(promises) : null;
        if (!run) throw new Error("Deno.resolveDns requires node:dns/promises");
        return run(String(q), kind);
      },
      upgradeWebSocket() { throw new Error("Deno.upgradeWebSocket: use Deno.serve handler Response upgrade (standalone deferred)"); },
      addSignalListener(s, cb) { __wjs_ns_proc().on(String(s), cb); },
      removeSignalListener(s, cb) { __wjs_ns_proc().off(String(s), cb); },
      Command: class {
        constructor(f, o) { this.__wjs_cmd = __wjs_ns_cmd(f, o); }
        output() { return this.__wjs_cmd.output(); }
        outputSync() { return this.__wjs_cmd.outputSync(); }
        spawn() { return this.__wjs_cmd.spawn(); }
      },
      permissions: { query() { return Promise.resolve({ state: "granted" }); } },
    };
    try {
      Object.defineProperty(globalThis.Deno, Symbol.toStringTag, { value: "Deno" });
    } catch {}
    try { Object.freeze(globalThis.Deno); } catch {}
  }
  if (globalThis.Bun === undefined) {
    let __wjs_bun_version, __wjs_bun_revision;
    globalThis.Bun = {
      get version() { return __wjs_bun_version !== undefined ? __wjs_bun_version : __wjs_ns_version(); },
      set version(v) { __wjs_bun_version = String(v); },
      get revision() { return __wjs_bun_revision !== undefined ? __wjs_bun_revision : __wjs_ns_version(); },
      set revision(v) { __wjs_bun_revision = String(v); },
      get argv() {
        try {
          const a = __wjs_ns_proc() && __wjs_ns_proc().argv;
          return Array.isArray(a) ? a.slice() : [];
        } catch { return []; }
      },
      get main() {
        try { return String((__wjs_ns_proc() && __wjs_ns_proc().argv[1]) || ""); } catch { return ""; }
      },
      get env() { try { return __wjs_ns_proc().env; } catch { return {}; } },
      get stdout() { return __wjs_ns_proc().stdout; },
      get stdin() { return __wjs_ns_proc().stdin; },
      get stderr() { return __wjs_ns_proc().stderr; },
      cwd() { return __wjs_ns_proc().cwd(); },
      file(p) { return __wjs_ns_bunfile(p); },
      write(p, d) {
        if (d instanceof Response) return d.arrayBuffer().then((b) => __wjs_ns_fsp().writeFile(String(p), Buffer.from(b)).then((r) => r));
        if (d instanceof Blob) return d.arrayBuffer().then((b) => __wjs_ns_fsp().writeFile(String(p), Buffer.from(b)));
        return __wjs_ns_fsp().writeFile(String(p), __wjs_ns_u8(d)).then(() => __wjs_ns_u8(d).byteLength);
      },
      spawn(f, o) {
        const child = __wjs_ns_child();
        if (!child || typeof child.spawn !== "function") throw new Error("Bun.spawn: async spawn unavailable, use Bun.spawnSync");
        return child.spawn(String(f), (o && o.args) || [], o);
      },
      spawnSync(f, o) {
        const child = __wjs_ns_child();
        if (!child) throw new Error("Bun.spawnSync requires node:child_process");
        return child.spawnSync(String(f), (o && o.args) || [], o);
      },
      $: (s, ...v) => {
        const child = __wjs_ns_child();
        if (!child) throw new Error("Bun.$ requires node:child_process");
        let cmd = "";
        for (let i = 0; i < s.length; i++) cmd += s[i] + (i < v.length ? String(v[i]) : "");
        const r = child.spawnSync("sh", ["-c", cmd], { encoding: "buffer" });
        return Promise.resolve({ stdout: r.stdout, stderr: r.stderr, exitCode: r.status ?? 0, text() { return Promise.resolve(String(r.stdout)); } });
      },
      sleep(ms) { return new Promise((r) => setTimeout(r, Number(ms))); },
      sleepSync(ms) {
        const end = Date.now() + Number(ms);
        while (Date.now() < end) {}
      },
      nanoseconds() {
        try { return Number(__wjs_ns_proc().hrtime.bigint()); }
        catch {
          try { return Math.floor(__wjs_hrtime_ns()); } catch { return Date.now() * 1000000; }
        }
      },
      randomUUIDv7() {
        try { return __wjs_ns_req("node:crypto").randomUUID(); }
        catch { return globalThis.crypto.randomUUID(); }
      },
      sha(a, d) { return __wjs_ns_req("node:crypto").hash(a, d, "hex"); },
      hash: {
        wyhash(s) { return __wjs_ns_req("node:crypto").hash("sha256", String(s), "hex"); },
        crc32(s) { return __wjs_ns_req("node:zlib").crc32(__wjs_ns_u8(s)); },
      },
      serve(o) { return __wjs_ns_serve(o && { port: o.port, hostname: o.hostname, onListen: o.onListen }, o.fetch); },
      listen(o) {
        const net = __wjs_ns_req("node:net");
        if (!net) throw new Error("Bun.listen requires node:net");
        return net.createServer(o.fetch ? (req, res) => {} : undefined);
      },
      connect(o) {
        const net = __wjs_ns_req("node:net");
        if (!net) throw new Error("Bun.connect requires node:net");
        return net.connect(o.port, o.hostname);
      },
      udpSocket() { return __wjs_ns_req("node:dgram").createSocket("udp4"); },
      fileURLToPath(u) { return __wjs_ns_req("node:url").fileURLToPath(u); },
      pathToFileURL(p) { return __wjs_ns_req("node:url").pathToFileURL(String(p)); },
      resolveSync(s, d) { return __wjs_ns_req("node:module").createRequire(d || "/").resolve(s); },
      which(c) {
        const path = (__wjs_ns_proc().env.PATH || "").split(":");
        const fs = __wjs_ns_fs();
        for (const dir of path) {
          const p = dir + "/" + String(c);
          try {
            fs.accessSync(p);
            return p;
          } catch {}
        }
        return null;
      },
      gc() {},
      shrink() {},
    };
    try {
      Object.defineProperty(globalThis.Bun, Symbol.toStringTag, { value: "Bun" });
    } catch {}
  }
  if (globalThis.WinterJS === undefined) {
    let __wjs_winterjs_version;
    globalThis.WinterJS = {
      get version() { return __wjs_winterjs_version !== undefined ? __wjs_winterjs_version : __wjs_ns_version(); },
      set version(v) { __wjs_winterjs_version = String(v); },
      get versions() {
        try {
          const v = __wjs_ns_proc() && __wjs_ns_proc().versions;
          if (v && typeof v === "object") return { ...v };
        } catch {}
        return { winterjs: __wjs_ns_version() };
      },
      get args() {
        try {
          const a = __wjs_ns_proc() && __wjs_ns_proc().argv;
          return Array.isArray(a) ? a.slice(2) : [];
        } catch { return []; }
      },
      get env() { try { return __wjs_ns_proc().env; } catch { return {}; } },
      cwd() { return __wjs_ns_proc().cwd(); },
      get pid() { try { return __wjs_ns_proc() ? __wjs_ns_proc().pid : 0; } catch { return 0; } },
      get storage() { return globalThis.storage; },
      get localStorage() { return globalThis.localStorage; },
      get Deno() { return globalThis.Deno; },
      get Bun() { return globalThis.Bun; },
    };
    try {
      Object.defineProperty(globalThis.WinterJS, Symbol.toStringTag, { value: "WinterJS" });
    } catch {}
  }
}
"#;
