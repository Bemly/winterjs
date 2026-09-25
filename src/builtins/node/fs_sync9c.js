// ---- Phase 9c：同步面增补（link 系/时间戳/权限/access/fd 系/cp/opendir）----
function __fsTimeMs(t, what) {
  if (t instanceof Date) return t.getTime();
  if (typeof t === "number") return t;
  if (typeof t === "string") { const n = Number(t); if (Number.isFinite(n)) return n; }
  throw new TypeError(`${what}: time must be a number, string or Date`);
}
// truncate/ftruncate 的 len（truncate 套件逐字）：undefined → 0；非 number →
// ARG_TYPE 'len'；非整数（±1.5）→ OUT_OF_RANGE 'It must be an integer'。
function __fsLenArg(len) {
  if (len === undefined) return 0;
  if (typeof len !== "number") __vErrType("len", "number", len);
  if (!Number.isInteger(len)) {
    const e = new RangeError(`The value of "len" is out of range. It must be an integer. Received ${len}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  return len;
}
// node lib/internal/fs/utils.js toUnixTimestamp——真机 26.8.2 实测口径：
// 数字串（含 '-1'）→ 数值原样；number 非有限 → ARG_TYPE；number 负 → 当前秒
//（实测 -1 → Date.now()/1000，非 throw）；Date → 秒浮点；日期串可 parse → 秒；
// 其余 ARG_TYPE（timestamp-parsing/utimes 套件）。
function __toUnixTimestamp(time, name = "time") {
  if (typeof time === "number") {
    if (!Number.isFinite(time)) __vErrType(name, "number or Date", time);
    return time < 0 ? Date.now() / 1000 : time;
  }
  if (typeof time === "string" && +time === time) return +time;
  if (time instanceof Date) return time.getTime() / 1000;
  if (typeof time === "string") {
    const d = new Date(time);
    if (!Number.isNaN(d.getTime())) return d.getTime() / 1000;
  }
  __vErrType(name, "number or Date", time);
}
// utimes 族的时间实参（秒口径，y2K38 套件）：Date → ms 直传（精度全保）；
// 其余经 toUnixTimestamp（秒）× 1000。
function __fsUtimeMs(t, what) {
  if (t instanceof Date) return t.getTime();
  return __toUnixTimestamp(t, what) * 1000;
}
function __fsModeNum(mode) {
  // node chmod 族：parseFileMode(mode, 'mode')（无 def；undefined → ARG_TYPE）。
  return __fsParseMode(mode);
}
export function accessSync(p, mode = 0) {
  if (mode !== undefined && typeof mode !== "number") {
    const e = new TypeError(`The "mode" argument must be of type number. Received ${typeof mode} (${JSON.stringify(String(mode))})`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  p = __fsPath(p, "access");
  __fsCall("access", p, () => __wjs_fs_access(p, mode));
}
export function truncateSync(p, len) {
  p = __fsPath(p, "truncate");
  const n = __fsLenArg(len);
  __fsCall("truncate", p, () => __wjs_fs_truncate(p, n));
}
export function utimesSync(p, atime, mtime) {
  p = __fsPath(p, "utimes");
  __fsCall("utimes", p, () => __wjs_fs_utimes(p, __fsUtimeMs(atime, "atime"), __fsUtimeMs(mtime, "mtime")));
}
export function lutimesSync(p, atime, mtime) {
  p = __fsPath(p, "lutimes");
  __fsCall("lutimes", p, () => __wjs_fs_lutimes(p, __fsUtimeMs(atime, "atime"), __fsUtimeMs(mtime, "mtime")));
}
export function chmodSync(p, mode) {
  p = __fsPath(p, "chmod");
  __vModeArg(mode);
  __fsCall("chmod", p, () => __wjs_fs_chmod(p, __fsModeNum(mode)));
}
export function chownSync(p, uid, gid) {
  p = __fsPath(p, "chown");
  // node validateInt32（fchown 套件）：非 number → ARG_TYPE /uid|gid/；
  // 非整数（Infinity/NaN）→ OUT_OF_RANGE 'It must be an integer'；-1 = 不变更。
  __vIntRange(uid, "uid", -1, 4294967295);
  __vIntRange(gid, "gid", -1, 4294967295);
  __fsCall("chown", p, () => __wjs_fs_chown(p, uid, gid));
}
export function fchownSync(fd, uid, gid) {
  __vFd(fd);
  __vIntRange(uid, "uid", -1, 4294967295);
  __vIntRange(gid, "gid", -1, 4294967295);
  __fsCall("fchown", "", () => __wjs_fs_fchown(fd, uid, gid));
}
export function lchownSync(p, uid, gid) {
  p = __fsPath(p, "lchown");
  __vIntRange(uid, "uid", -1, 4294967295);
  __vIntRange(gid, "gid", -1, 4294967295);
  __fsCall("lchown", p, () => __wjs_fs_lchown(p, uid, gid));
}
// node：fs.lchmod 仅 macOS 存在（native 仅 macOS 注册，非 macOS 导出 undefined）；
// mode 走 parseFileMode（lchmod 套件校验矩阵）。
function lchmodSyncImpl(p, mode) {
  p = __fsPath(p, "lchmod");
  const n = __fsModeNum(mode);
  __fsCall("lchmod", p, () => __wjs_fs_lchmod(p, n));
}
export const lchmodSync = typeof __wjs_fs_lchmod === "function" ? lchmodSyncImpl : undefined;

export function linkSync(a, b) {
  a = __fsPath(a, "link");
  b = __fsPath(b, "link");
  __fsCall("link", a, () => __wjs_fs_link(a, b));
}
export function symlinkSync(target, p) {
  target = __fsPath(target, "symlink");
  p = __fsPath(p, "symlink");
  __fsCall("symlink", p, () => __wjs_fs_symlink(target, p));
}
export function readlinkSync(p, opts) {
  p = __fsPath(p, "readlink");
  const enc = __fsEncoding(opts);
  const link = __fsCall("readlink", p, () => __wjs_fs_read_link(p));
  if (enc === "buffer") return Buffer.from(link);
  return link;
}
// node lib/internal/fs/utils.js validateCpOptions 逐字（cp 校验族套件点名）：
// undefined → 缺省；非对象（含函数/数组/null）→ ARG_TYPE 'options'；
// 六布尔逐项校验（property 文案）；mode 走 copyFile 档 [0,7]；
// dereference+verbatimSymlinks 互斥 → INCOMPATIBLE_PAIR；filter 须函数。
function __cpValidateOptions(opts) {
  const def = { dereference: false, errorOnExist: false, filter: undefined, force: true, preserveTimestamps: false, recursive: false, verbatimSymlinks: false };
  if (opts === undefined) return { ...def };
  if (opts === null || typeof opts !== "object" || Array.isArray(opts)) {
    const e = new TypeError(`The "options" argument must be of type object. Received ${__vReceived(opts)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  const o = { ...def, ...opts };
  for (const k of ["dereference", "errorOnExist", "force", "preserveTimestamps", "recursive", "verbatimSymlinks"]) {
    if (typeof o[k] !== "boolean") {
      const e = new TypeError(`The "options.${k}" property must be of type boolean. Received ${__vReceived(o[k])}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
  }
  let mode = o.mode;
  if (mode === undefined || mode === null) mode = 0;
  else {
    if (typeof mode !== "number") __vErrType("mode", "number", mode);
    if (!Number.isInteger(mode)) {
      const e = new RangeError(`The value of "mode" is out of range. It must be an integer. Received ${mode}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    if (mode < 0 || mode > 7) {
      const e = new RangeError(`The value of "mode" is out of range. It must be >= 0 && <= 7. Received ${mode}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
  }
  o.mode = mode;
  if (o.dereference === true && o.verbatimSymlinks === true) {
    const e = new TypeError('Option "dereference" cannot be used in combination with option "verbatimSymlinks"');
    e.code = "ERR_INCOMPATIBLE_OPTION_PAIR"; throw e;
  }
  if (o.filter !== undefined && typeof o.filter !== "function") {
    const e = new TypeError(`The "options.filter" property must be of type function. Received ${__vReceived(o.filter)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  return o;
}
function __cpSameOrSubdir(src, dst) {
  // node cpSyncCheckPaths 近似：同路径或 dst 落在 src 内 → EINVAL。
  // 比对走绝对路径归一（尾斜杠剥离；大小写敏感 posix 口径）。
  const norm = (p) => String(p).replace(/\/+$/, "") || "/";
  const a = norm(src), b = norm(dst);
  if (a === b) return "same";
  if (b.startsWith(a + "/")) return "subdir";
  return null;
}
function __cpEff(p) {
  // C++ checkPaths 穿透链接消解（dest-symlink-points-to-src 套件）：
  // 只消解父链（终段本身是链接时不得跟随——否则两条同目标链接被误判 identical，
  // copy-symlinks-to-existing-symlinks 套件点名）；余段回拼。
  const s = String(p).replace(/\/+$/, "");
  const j = s.lastIndexOf("/");
  const base = j < 0 ? s : s.slice(j + 1);
  let cur = j <= 0 ? (j === 0 ? "/" : ".") : s.slice(0, j);
  const tail = [];
  for (let i = 0; i < 64; i++) {
    let ok = true;
    try { lstatSync(cur); } catch { ok = false; }
    if (ok) break;
    const t = cur.replace(/\/+$/, "");
    const k = t.lastIndexOf("/");
    if (k <= 0) { tail.unshift(cur); cur = k === 0 ? "/" : "."; break; }
    tail.unshift(t.slice(k + 1));
    cur = t.slice(0, k) || "/";
  }
  let rbase;
  try { rbase = realpathSync(cur); } catch { rbase = cur; }
  return String(rbase).replace(/\/+$/, "") + "/" + [...tail, base].join("/");
}
export function cpSync(src, dst, opts) {
  const o = __cpValidateOptions(opts);
  src = __fsPath(src, "cp");
  dst = __fsPath(dst, "cp");
  if (o.filter) {
    const r = o.filter(src, dst);
    if (r && typeof r.then === "function") {
      const e = new TypeError(`The "filter" return value must be of type boolean. Received an instance of Promise`);
      e.code = "ERR_INVALID_RETURN_VALUE"; throw e;
    }
    if (!r) return;
  }
  const rel = __cpSameOrSubdir(src, dst) || __cpSameOrSubdir(__cpEff(src), __cpEff(dst));
  if (rel === "same") {
    const e = new Error(`src and dest cannot be the same ${src}`);
    e.code = "ERR_FS_CP_EINVAL"; throw e;
  }
  if (rel === "subdir") {
    const e = new Error(`cannot copy ${src} to a subdirectory of self ${dst}`);
    e.code = "ERR_FS_CP_EINVAL"; throw e;
  }
  // node getStats 口径：dereference 决定 stat/lstat；dest 恒 lstat（不跟随）。
  const srcStat = (o.dereference ? statSync : lstatSync)(src);
  const destStat = lstatSync(dst, { throwIfNoEntry: false });
  return __cpStats(src, dst, o, srcStat, destStat);
}
// node lib/internal/fs/cp/cp-sync.js getStats 分发（C++ checkPaths 由上层
// __cpSameOrSubdir 近似；EISDIR 非递归目录由本分发抛）。
function __cpStats(src, dst, o, srcStat, destStat) {
  if (srcStat.isDirectory()) {
    // C++ checkPaths 序：dest 存在且非目录 → DIR_TO_NON_DIR（先于递归门，
    // dir-to-file 套件无 opts 即点名此码而非 EISDIR）。
    if (destStat && !destStat.isDirectory()) {
      const e = new Error(`Cannot overwrite non-directory ${dst} with directory ${src}`);
      e.code = "ERR_FS_CP_DIR_TO_NON_DIR"; throw e;
    }
    if (!o.recursive) {
      const e = new Error(`Recursive option not enabled, cannot copy a directory: ${src}/`);
      e.code = "ERR_FS_EISDIR"; throw e;
    }
    return __cpOnDir(src, dst, o, destStat);
  }
  if (srcStat.isFile() || srcStat.isCharacterDevice() || srcStat.isBlockDevice()) {
    return __cpOnFile(src, dst, o, destStat);
  }
  if (srcStat.isSymbolicLink()) {
    return __cpOnLink(src, dst, o, destStat);
  }
  if (srcStat.isSocket()) {
    const e = new Error(`Cannot copy a socket file: ${dst}`);
    e.code = "ERR_FS_CP_SOCKET"; throw e;
  }
  if (srcStat.isFIFO()) {
    const e = new Error(`Cannot copy a FIFO pipe: ${dst}`);
    e.code = "ERR_FS_CP_FIFO_PIPE"; throw e;
  }
  const e = new Error(`Cannot copy an unknown file type: ${dst}`);
  e.code = "ERR_FS_CP_UNKNOWN"; throw e;
}
function __cpEexist(dst) {
  const e = new Error(`Target already exists: cp returned EEXIST (${dst} already exists) ${dst}`);
  e.code = "ERR_FS_CP_EEXIST"; e.syscall = "cp"; e.path = dst; e.errno = 17; throw e;
}
function __cpOnDir(src, dst, o, destStat) {
  if (destStat && !o.force) {
    // 存在即错（内容无冲突也抛，dir-exists-error-on-exist 套件点名）；
    // 无 errorOnExist 则合并（逐项 force 门复用）。
    if (o.errorOnExist) __cpEexist(dst);
  }
  __fsCall("cp", dst, () => __wjs_fs_mkdir(dst, true));
  for (const e of readdirSync(src, { withFileTypes: true })) {
    cpSync(__cpJoin(src, e.name), __cpJoin(dst, e.name), o);
  }
}
// 异步孪生（async-filter 套件：filter 可为 async 函数，逐项 await；
// 文件操作仍同步直调——本地 syscall，无等待点，语义等价）。
async function __cpAsync(src, dst, opts) {
  const o = __cpValidateOptions(opts);
  src = __fsPath(src, "cp");
  dst = __fsPath(dst, "cp");
  if (o.filter) {
    const r = await o.filter(src, dst);
    if (!r) return;
  }
  const rel = __cpSameOrSubdir(src, dst) || __cpSameOrSubdir(__cpEff(src), __cpEff(dst));
  if (rel === "same") {
    const e = new Error(`src and dest cannot be the same ${src}`);
    e.code = "ERR_FS_CP_EINVAL"; throw e;
  }
  if (rel === "subdir") {
    const e = new Error(`cannot copy ${src} to a subdirectory of self ${dst}`);
    e.code = "ERR_FS_CP_EINVAL"; throw e;
  }
  const srcStat = (o.dereference ? statSync : lstatSync)(src);
  const destStat = lstatSync(dst, { throwIfNoEntry: false });
  return __cpStatsA(src, dst, o, srcStat, destStat);
}
async function __cpStatsA(src, dst, o, srcStat, destStat) {
  if (srcStat.isDirectory()) {
    if (destStat && !destStat.isDirectory()) {
      const e = new Error(`Cannot overwrite non-directory ${dst} with directory ${src}`);
      e.code = "ERR_FS_CP_DIR_TO_NON_DIR"; throw e;
    }
    if (!o.recursive) {
      const e = new Error(`Recursive option not enabled, cannot copy a directory: ${src}/`);
      e.code = "ERR_FS_EISDIR"; throw e;
    }
    return __cpOnDirA(src, dst, o, destStat);
  }
  // 非目录分发与 __cpStats 同形（改一处改两处：文件/链接/socket/fifo/未知）。
  if (srcStat.isFile() || srcStat.isCharacterDevice() || srcStat.isBlockDevice()) {
    return __cpOnFile(src, dst, o, destStat);
  }
  if (srcStat.isSymbolicLink()) {
    return __cpOnLink(src, dst, o, destStat);
  }
  if (srcStat.isSocket()) {
    const e = new Error(`Cannot copy a socket file: ${dst}`);
    e.code = "ERR_FS_CP_SOCKET"; throw e;
  }
  if (srcStat.isFIFO()) {
    const e = new Error(`Cannot copy a FIFO pipe: ${dst}`);
    e.code = "ERR_FS_CP_FIFO_PIPE"; throw e;
  }
  const e = new Error(`Cannot copy an unknown file type: ${dst}`);
  e.code = "ERR_FS_CP_UNKNOWN"; throw e;
}
async function __cpOnDirA(src, dst, o, destStat) {
  if (destStat && !o.force) {
    if (o.errorOnExist) __cpEexist(dst);
  }
  __fsCall("cp", dst, () => __wjs_fs_mkdir(dst, true));
  for (const e of readdirSync(src, { withFileTypes: true })) {
    await __cpAsync(__cpJoin(src, e.name), __cpJoin(dst, e.name), o);
  }
}
function __cpOnFile(src, dst, o, destStat) {
  if (!destStat) {
    // Node cp 建缺失父目录（file-to-file 套件：dest 父级不存在仍成功）。
    __fsCall("cp", dst, () => __wjs_fs_mkdir(__cpDirname(dst), true));
    __fsCall("copyfile", src, () => __wjs_fs_copy_file(src, dst));
    return;
  }
  // 文件拷向目录 → NON_DIR_TO_DIR（file-to-dir 套件；直拷报 EISDIR 即错码）。
  if (destStat.isDirectory()) {
    const e = new Error(`Cannot overwrite directory ${dst} with non-directory ${src}`);
    e.code = "ERR_FS_CP_NON_DIR_TO_DIR"; throw e;
  }
  if (o.force) {
    // Node C++ override 语义：dest 为 symlink 时先摘除再拷（dereference 套件：
    // file-over-symlinked-dir 后 dest 为文件非链接；直拷会穿透写进目标目录）。
    let dl = null;
    try { dl = lstatSync(dst); } catch { dl = null; }
    if (dl && dl.isSymbolicLink()) {
      __fsCall("unlink", dst, () => __wjs_fs_unlink(dst));
    }
    __fsCall("copyfile", src, () => __wjs_fs_copy_file(src, dst));
    return;
  }
  if (o.errorOnExist) __cpEexist(dst);
  // !force && !errorOnExist → 静默跳过。
}
// node onLink（cp-sync.js 逐字）：verbatim 关时相对链接消解为绝对；
// 不存在直建；存在分三路（非链接穿透建→EEXIST 门；双向 subdir 检查；否则换链）。
function __cpOnLink(src, dst, o, destStat) {
  let resolvedSrc = readlinkSync(src);
  if (!o.verbatimSymlinks && !__cpIsAbs(resolvedSrc)) {
    resolvedSrc = __cpResolve(__cpDirname(src), resolvedSrc);
  }
  if (!destStat) {
    __fsCall("cp", dst, () => __wjs_fs_mkdir(__cpDirname(dst), true));
    __fsCall("symlink", dst, () => __wjs_fs_symlink(resolvedSrc, dst));
    return;
  }
  let resolvedDest;
  try {
    resolvedDest = readlinkSync(dst);
  } catch (err) {
    if (err && (err.code === "EINVAL" || err.code === "UNKNOWN")) {
      // dest 存在但非链接：Node 原文直调 symlinkSync（不摘除）——
      // 恒 EEXIST（copy-symlink-over-file 套件 force 缺省仍 EEXIST）。
      __fsCall("symlink", dst, () => __wjs_fs_symlink(resolvedSrc, dst));
      return;
    }
    throw err;
  }
  if (!__cpIsAbs(resolvedDest)) {
    resolvedDest = __cpResolve(__cpDirname(dst), resolvedDest);
  }
  // Node 原文门：仅 src 链接指向目录时同址即 EINVAL（文件链接复拷是
  // unlink+重建无操作；copy-symlinks-to-existing-symlinks 套件点名）。
  let __srcIsDir = false;
  try { __srcIsDir = statSync(src).isDirectory(); } catch { __srcIsDir = false; }
  if (__srcIsDir && __cpIsSubdir(resolvedSrc, resolvedDest)) {
    const e = new Error(`cannot copy ${resolvedSrc} to a subdirectory of self ${resolvedDest}`);
    e.code = "ERR_FS_CP_EINVAL"; throw e;
  }
  // dest 链接指向 src 内部且 src 为目录 → 覆盖即删源（先拦）。
  let dstStat = null;
  try { dstStat = statSync(dst); } catch { dstStat = null; }
  if (dstStat && dstStat.isDirectory() && __cpIsSubdir(resolvedDest, resolvedSrc)) {
    const e = new Error(`cannot overwrite ${resolvedDest} with ${resolvedSrc}`);
    e.code = "ERR_FS_CP_SYMLINK_TO_SUBDIRECTORY"; throw e;
  }
  __fsCall("unlink", dst, () => __wjs_fs_unlink(dst));
  __fsCall("symlink", dst, () => __wjs_fs_symlink(resolvedSrc, dst));
}
// posix 路径小件（cp 链接消解专用；.. 不出根）。
function __cpIsAbs(p) { return String(p).startsWith("/"); }
function __cpDirname(p) {
  const s = String(p).replace(/\/+$/, "");
  const i = s.lastIndexOf("/");
  if (i < 0) return ".";
  if (i === 0) return "/";
  return s.slice(0, i);
}
function __cpJoin(a, b) {
  return String(a).replace(/\/+$/, "") + "/" + String(b).replace(/^\/+/, "");
}
function __cpResolve(base, rel) {
  const out = [];
  for (const q of String(base + "/" + rel).split("/")) {
    if (q === "" || q === ".") continue;
    if (q === "..") { out.pop(); continue; }
    out.push(q);
  }
  return "/" + out.join("/");
}
function __cpIsSubdir(parent, child) {
  const norm = (p) => String(p).replace(/\/+$/, "") || "/";
  const a = norm(parent), b = norm(child);
  return b === a || b.startsWith(a + "/");
}
// fd 系（openSync 合成 fd，自 3 起单调，不复用最小号，记档）
export function openSync(p, flags, mode) {
  // node 顺序：stringToFlags → parseFileMode(mode, 'mode', 0o666) → path 校验。
  const f = __fsFlags(flags, "open");
  const m = __fsParseMode(mode, 0o666);
  p = __fsPath(p, "open");
  return __fsCall("open", p, () => Number(__wjs_fs_open(p, f, m)));
}
export function closeSync(fd) {
  __vFd(fd);
  __fsCall("close", "", () => __wjs_fs_close(fd));
}
