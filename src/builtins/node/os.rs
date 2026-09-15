//! `node:os`：平台信息（`sysinfo` + `if-addrs` + `uzers` + `sys-locale` + `dirs` 轮子）。
//! 全同步 natives（纯数据/JSON 桥）；偏差文档见各函数注释。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

#[cfg(unix)]
use uzers::os::unix::UserExt as _;

use crate::jsapi_glue::{wrap_cx, Frame};

/// 字符串返回值（`random_uuid` 同款）。
fn set_rval_str(cx: &mut mozjs::context::JSContext, frame: &Frame, s: &str) {
    rooted!(&in(cx) let mut v = UndefinedValue());
    s.to_jsval(cx, v.handle_mut());
    frame.set_rval(v.get());
}

/// Node 平台名（`process.platform` 同源；移动端见注释）。
pub fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "ios") {
        "darwin"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        // linux/ohos（ohos 的 target_os 实为 linux，见 dependencies §1）统一 linux
        "linux"
    }
}

/// Node 架构名。
pub fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else {
        std::env::consts::ARCH
    }
}

/// `__wjs_os_platform()` → 平台名。
pub unsafe extern "C" fn os_platform(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    set_rval_str(&mut cx, &frame, platform());
    true
}

/// `__wjs_os_arch()` → 架构名。
pub unsafe extern "C" fn os_arch(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    set_rval_str(&mut cx, &frame, arch());
    true
}

/// `__wjs_os_info()` → `{type, release, hostname, tmpdir, homedir}` JSON。
pub unsafe extern "C" fn os_info(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let sys_type = if cfg!(target_os = "windows") {
        "Windows_NT".to_string()
    } else {
        sysinfo::System::name().unwrap_or_else(|| std::env::consts::OS.to_string())
    };
    let release = sysinfo::System::kernel_version()
        .or_else(sysinfo::System::os_version)
        .unwrap_or_default();
    let hostname = sysinfo::System::host_name().unwrap_or_else(|| "localhost".to_string());
    let tmpdir = std::env::temp_dir().to_string_lossy().into_owned();
    let homedir = dirs::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let json = serde_json::json!({
        "type": sys_type,
        "release": release,
        "hostname": hostname,
        "tmpdir": tmpdir,
        "homedir": homedir,
    })
    .to_string();
    set_rval_str(&mut cx, &frame, &json);
    true
}

/// `__wjs_os_cpus()` → `[{model, speed, times}]` JSON。
/// 偏差：`sysinfo` 只给总使用率 —— `times.user=usage 百分比取整、idle=100-user`，
/// 其余 0（文档记录；lint 类脚本只读 model/speed）。
pub unsafe extern "C" fn os_cpus(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_all();
    // 首次 usage 恒 0，短睡一次取数（50ms；启动期一次性成本，文档记录）。
    std::thread::sleep(std::time::Duration::from_millis(50));
    sys.refresh_cpu_all();
    let cpus: Vec<_> = sys
        .cpus()
        .iter()
        .map(|c| {
            let usage = c.cpu_usage().round().clamp(0.0, 100.0) as u64;
            serde_json::json!({
                "model": c.brand().trim().to_string(),
                "speed": c.frequency(),
                "times": { "user": usage, "nice": 0, "sys": 0, "idle": 100 - usage.min(100), "irq": 0 },
            })
        })
        .collect();
    set_rval_str(&mut cx, &frame, &serde_json::Value::Array(cpus).to_string());
    true
}

/// `__wjs_os_mem()` → `{total, free}` JSON（字节）。
pub unsafe extern "C" fn os_mem(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let json = serde_json::json!({
        "total": sys.total_memory(),
        "free": sys.free_memory(),
    })
    .to_string();
    set_rval_str(&mut cx, &frame, &json);
    true
}

/// `__wjs_os_net()` → `{iface: [{address, family, internal}]}` JSON。
/// 偏差：mac 恒 `00:00:00:00:00:00`（逐 iface MAC 无可信纯 Rust 轮子，文档记录）。
/// 10f：补 `netmask` + `cidr`（`address/prefixlen`，test-os.js 点名）。
pub unsafe extern "C" fn os_net(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let mut map: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
        std::collections::BTreeMap::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            let (address, family, netmask, prefixlen) = match &iface.addr {
                if_addrs::IfAddr::V4(v4) => (
                    v4.ip.to_string(),
                    "IPv4",
                    v4.netmask.to_string(),
                    v4.prefixlen,
                ),
                if_addrs::IfAddr::V6(v6) => (
                    v6.ip.to_string(),
                    "IPv6",
                    v6.netmask.to_string(),
                    v6.prefixlen,
                ),
            };
            map.entry(iface.name.clone()).or_default().push(serde_json::json!({
                "address": address,
                "netmask": netmask,
                "family": family,
                "mac": "00:00:00:00:00:00",
                "internal": iface.is_loopback(),
                "cidr": format!("{address}/{prefixlen}"),
            }));
        }
    }
    set_rval_str(&mut cx, &frame, &serde_json::Value::Object(
        map.into_iter().map(|(k, v)| (k, serde_json::Value::Array(v))).collect(),
    ).to_string());
    true
}

/// `__wjs_os_user()` → `{uid, gid, username, homedir, shell}` JSON。
/// 非 unix（Windows 服务场景）uid/gid 置 -1、shell 置空（文档记录）。
pub unsafe extern "C" fn os_user(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    #[cfg(unix)]
    let (uid, gid, username, homedir, shell) = {
        let uid = uzers::get_current_uid();
        let gid = uzers::get_current_gid();
        let user = uzers::get_user_by_uid(uid);
        let username = user
            .as_ref()
            .map(|u| u.name().to_string_lossy().into_owned())
            .unwrap_or_default();
        let homedir = user
            .as_ref()
            .map(|u| u.home_dir().to_string_lossy().into_owned())
            .or_else(|| dirs::home_dir().map(|p| p.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let shell = user
            .as_ref()
            .map(|u| u.shell().to_string_lossy().into_owned())
            .unwrap_or_default();
        (uid as i64, gid as i64, username, homedir, shell)
    };
    #[cfg(not(unix))]
    let (uid, gid, username, homedir, shell): (i64, i64, String, String, String) = {
        let username = std::env::var("USERNAME").unwrap_or_default();
        let homedir = dirs::home_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        (-1, -1, username, homedir, String::new())
    };
    let json = serde_json::json!({
        "uid": uid,
        "gid": gid,
        "username": username,
        "homedir": homedir,
        "shell": shell,
    })
    .to_string();
    set_rval_str(&mut cx, &frame, &json);
    true
}

/// `__wjs_os_uptime()` → 秒（f64；`sysinfo::System::uptime`）。
pub unsafe extern "C" fn os_uptime(
    _cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 仅访问调用帧（无 cx 上的 JSAPI 调用）
    let frame = unsafe { Frame::from_raw(vp, argc) };
    frame.set_rval(mozjs::jsval::DoubleValue(sysinfo::System::uptime() as f64));
    true
}

/// `__wjs_os_load()` → `[1, 5, 15]` JSON（Windows 全 0，文档记录）。
pub unsafe extern "C" fn os_load(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let avg = sysinfo::System::load_average();
    let json = serde_json::json!([avg.one, avg.five, avg.fifteen]).to_string();
    set_rval_str(&mut cx, &frame, &json);
    true
}

/// `__wjs_os_locale()` → BCP47（`sys-locale`；取不到回 `en-US`）。
pub unsafe extern "C" fn os_locale(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    set_rval_str(&mut cx, &frame, &sys_locale::get_locale().unwrap_or_else(|| "en-US".into()));
    true
}

/// 本机原始架构名（`arch()` 的映射前形态；uname -m 口径，纯函数，单元测试覆盖）。
fn machine_raw() -> &'static str {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "ARM64"
        } else {
            "AMD64"
        }
    } else if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
        if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            std::env::consts::ARCH
        }
    } else {
        // linux（含 android/ohos，见 dependencies §1）：uname -m 原样
        std::env::consts::ARCH
    }
}

/// `__wjs_os_machine()` → 原始架构名。
pub unsafe extern "C" fn os_machine(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    set_rval_str(&mut cx, &frame, machine_raw());
    true
}

#[cfg(unix)]
fn uname_version() -> String {
    // SAFETY: 零初始化 buf；`libc::uname` 只写 buf 内；version 恒 NUL 结尾（POSIX）
    unsafe {
        let mut buf: libc::utsname = std::mem::MaybeUninit::zeroed().assume_init();
        if libc::uname(&mut buf) != 0 {
            return String::new();
        }
        std::ffi::CStr::from_ptr(buf.version.as_ptr())
            .to_string_lossy()
            .into_owned()
    }
}

/// `__wjs_os_uname()` → `{"version": <uname -v>}` JSON（unix 经 libc；
/// 非 unix 回空串由 JS 侧回落，win 记档）。
pub unsafe extern "C" fn os_uname(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    #[cfg(unix)]
    let version = uname_version();
    #[cfg(not(unix))]
    let version = String::new();
    set_rval_str(
        &mut cx,
        &frame,
        &serde_json::json!({ "version": version }).to_string(),
    );
    true
}

/// errno 错误体 JSON（`{"errno","code","message"}`；message 首字母小写贴 libuv 口径，
/// 并剥离 Rust `to_string` 自带的 ` (os error N)` 后缀）。
fn prio_err_json(errno: i32) -> String {
    let e = std::io::Error::from_raw_os_error(errno);
    let mut message = e.to_string();
    let suffix = format!(" (os error {errno})");
    if let Some(stripped) = message.strip_suffix(&suffix) {
        message = stripped.to_string();
    }
    if let Some(first) = message.get_mut(0..1) {
        first.make_ascii_lowercase();
    }
    serde_json::json!({
        "errno": errno,
        "code": super::fs::io_code(&e),
        "message": message,
    })
    .to_string()
}

/// `__wjs_os_prio_get(pid)` → `{"ok": prio}` / 错误体 JSON。
/// pid 由 JS 侧 validateInt32 保证 int32；unix 经 getpriority + 哨兵消毒
/// （先 `close(-1)` 把 errno 钉成 getpriority 永不报的 EBADF：哨兵仍在即真值 -1，
/// 否则为真错；成功 syscall 不动 errno，见 man 契约）；非 unix 回 ENOSYS 桩。
pub unsafe extern "C" fn os_prio_get(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上；close(-1) 必败无副作用；getpriority 只读调度器状态
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let pid = if frame.argc() > 0 && frame.arg(0).is_number() {
        frame.arg(0).to_number() as i32
    } else {
        0
    };
    #[cfg(unix)]
    let text = unsafe {
        libc::close(-1);
        let r = libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t);
        if r == -1 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if errno == libc::EBADF {
                serde_json::json!({ "ok": -1 }).to_string()
            } else {
                prio_err_json(errno)
            }
        } else {
            serde_json::json!({ "ok": r }).to_string()
        }
    };
    #[cfg(not(unix))]
    let text = prio_err_json(38); // ENOSYS（win 记档）
    set_rval_str(&mut cx, &frame, &text);
    true
}

/// `__wjs_os_prio_set(pid, prio)` → `{"ok": true}` / 错误体 JSON
/// （unix setpriority；非 unix ENOSYS 桩）。
pub unsafe extern "C" fn os_prio_set(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上；setpriority 只改目标进程 nice 值
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let pid = if frame.argc() > 0 && frame.arg(0).is_number() {
        frame.arg(0).to_number() as i32
    } else {
        0
    };
    let prio = if frame.argc() > 1 && frame.arg(1).is_number() {
        frame.arg(1).to_number() as i32
    } else {
        0
    };
    #[cfg(unix)]
    let text = unsafe {
        let r = libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, prio as libc::c_int);
        if r == 0 {
            serde_json::json!({ "ok": true }).to_string()
        } else {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            prio_err_json(errno)
        }
    };
    #[cfg(not(unix))]
    let text = prio_err_json(38); // ENOSYS（win 记档）
    set_rval_str(&mut cx, &frame, &text);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_raw_matches_target() {
        if cfg!(target_os = "windows") {
            assert!(matches!(machine_raw(), "AMD64" | "ARM64"));
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            assert_eq!(machine_raw(), "arm64");
        } else {
            assert_eq!(machine_raw(), std::env::consts::ARCH);
        }
    }

    #[test]
    fn prio_err_json_shapes_libuv_message() {
        let v: serde_json::Value =
            serde_json::from_str(&prio_err_json(22)).expect("valid json");
        assert_eq!(v["errno"], 22);
        assert_eq!(v["code"], "EINVAL");
        assert_eq!(v["message"], "invalid argument");
    }
}

/// 内嵌 ESM 源（JSON 桥拆包；`EOL`/`devNull` 非写，`tmpdir`/`homedir` 读 env 动态值）。
pub const SOURCE: &str = r#"
import { validateInt32 } from 'node:internal/validators';
import errors from 'node:internal/errors';
const { ERR_SYSTEM_ERROR } = errors.codes;
const __info = JSON.parse(__wjs_os_info());
const __mem = () => JSON.parse(__wjs_os_mem());
const __cpus = () => JSON.parse(__wjs_os_cpus());
const __net = () => JSON.parse(__wjs_os_net());
const __user = () => JSON.parse(__wjs_os_user());
const __load = () => JSON.parse(__wjs_os_load());
const __uname = () => JSON.parse(__wjs_os_uname());
const isWin = __wjs_os_platform() === "win32";
// Node lib/os.js signals 表（unix 全集 + Windows 子集；internal/validators 同款）。
const signals = {
  SIGHUP: 1, SIGINT: 2, SIGQUIT: 3, SIGILL: 4, SIGTRAP: 5, SIGABRT: 6, SIGIOT: 6,
  SIGBUS: 7, SIGFPE: 8, SIGKILL: 9, SIGUSR1: 10, SIGSEGV: 11, SIGUSR2: 12,
  SIGPIPE: 13, SIGALRM: 14, SIGTERM: 15, SIGSTKFLT: 16, SIGCHLD: 17, SIGCONT: 18,
  SIGSTOP: 19, SIGTSTP: 20, SIGTTIN: 21, SIGTTOU: 22, SIGURG: 23, SIGXCPU: 24,
  SIGXFSZ: 25, SIGVTALRM: 26, SIGPROF: 27, SIGWINCH: 28, SIGIO: 29, SIGPOLL: 29,
  SIGINFO: 29, SIGPWR: 30, SIGSYS: 31, SIGBREAK: 21,
};
// UV_PRIORITY_*（真机 26.8.2 对码；HIGHEST=-20，注意非 -19）。
const priority = {
  PRIORITY_LOW: 19, PRIORITY_BELOW_NORMAL: 10, PRIORITY_NORMAL: 0,
  PRIORITY_ABOVE_NORMAL: -7, PRIORITY_HIGH: -14, PRIORITY_HIGHEST: -20,
};
const constants = { priority, signals };
Object.freeze(signals);
function sysErr(syscall, body) {
  const err = new ERR_SYSTEM_ERROR(syscall, body.code, body.message);
  err.errno = -body.errno;
  err.syscall = syscall;
  err.info = { errno: -body.errno, code: body.code, message: body.message, syscall };
  return err;
}
export function setPriority(pid, priority) {
  if (priority === undefined) { priority = pid; pid = 0; }
  validateInt32(pid, "pid");
  validateInt32(priority, "priority", -20, 19);
  const r = JSON.parse(__wjs_os_prio_set(pid, priority));
  if (r.errno !== undefined) throw sysErr("uv_os_setpriority", r);
}
export function getPriority(pid) {
  if (pid === undefined) pid = 0;
  else validateInt32(pid, "pid");
  const r = JSON.parse(__wjs_os_prio_get(pid));
  if (r.errno !== undefined) throw sysErr("uv_os_getpriority", r);
  return r.ok;
}
export function platform() { return __wjs_os_platform(); }
export function arch() { return __wjs_os_arch(); }
export function release() { return __info.release; }
export function type() { return __info.type; }
export function hostname() { return __info.hostname; }
export function tmpdir() {
  if (isWin) {
    const p = process.env.TEMP || process.env.TMP ||
      ((process.env.SystemRoot || process.env.windir) + "\\temp");
    if (p.length > 1 && p[p.length - 1] === "\\" && p[p.length - 2] !== ":") return p.slice(0, -1);
    return p;
  }
  const t = process.env.TMPDIR || process.env.TMP || process.env.TEMP || "/tmp";
  let out = t;
  while (out.length > 1 && out[out.length - 1] === "/") out = out.slice(0, -1);
  return out;
}
export function homedir() {
  if (isWin) return process.env.USERPROFILE || __info.homedir;
  return process.env.HOME || __info.homedir;
}
export function totalmem() { return __mem().total; }
export function freemem() { return __mem().free; }
export function cpus() { return __cpus(); }
export function networkInterfaces() { return __net(); }
export function userInfo(options) {
  const u = __user();
  if (options != null && options.encoding === "buffer") {
    return {
      uid: u.uid, gid: u.gid,
      username: Buffer.from(u.username), homedir: Buffer.from(u.homedir), shell: Buffer.from(u.shell),
    };
  }
  return u;
}
export function uptime() { return __wjs_os_uptime(); }
export function loadavg() { return __load(); }
export function getLocale() { return __wjs_os_locale(); }
// 可用并行度（M5 vitest 牵引：真机按 CPU 亲和/线程池上限打折，本仓恒回
// cpus 数——单进程 JS 线程 + tokio 同步多线程，无亲和约束，记档）。
export function availableParallelism() { return __cpus().length; }
export function endianness() {
  return new Uint8Array(new Uint16Array([0x1234]).buffer)[0] === 0x34 ? "LE" : "BE";
}
export function machine() { return __wjs_os_machine(); }
export function version() {
  const v = __uname().version;
  return v || __info.release || __info.type;
}
export const EOL = isWin ? "\r\n" : "\n";
export const devNull = isWin ? "\\\\.\\nul" : "/dev/null";
export { constants };
for (const f of [hostname, homedir, release, type, arch, platform, version,
    machine, endianness, tmpdir, totalmem, uptime, freemem, availableParallelism]) {
  f[Symbol.toPrimitive] = () => f();
}
const __def = { platform, arch, release, type, hostname, tmpdir, homedir, totalmem,
  freemem, cpus, networkInterfaces, userInfo, uptime, loadavg, EOL, devNull,
  constants, endianness, machine, version, setPriority, getPriority, availableParallelism };
Object.defineProperties(__def, {
  EOL: { value: EOL, writable: false, enumerable: true, configurable: true },
  devNull: { value: devNull, writable: false, enumerable: true, configurable: true },
  constants: { value: constants, writable: false, enumerable: true, configurable: false },
});
export default __def;
"#;
