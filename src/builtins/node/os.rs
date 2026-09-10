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
            let (address, family) = match iface.ip() {
                std::net::IpAddr::V4(v4) => (v4.to_string(), "IPv4"),
                std::net::IpAddr::V6(v6) => (v6.to_string(), "IPv6"),
            };
            map.entry(iface.name.clone()).or_default().push(serde_json::json!({
                "address": address,
                "family": family,
                "internal": iface.is_loopback(),
                "mac": "00:00:00:00:00:00",
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

/// 内嵌 ESM 源（JSON 桥拆包；`EOL` 按平台）。
pub const SOURCE: &str = r#"
const __info = JSON.parse(__wjs_os_info());
const __mem = () => JSON.parse(__wjs_os_mem());
const __cpus = () => JSON.parse(__wjs_os_cpus());
const __net = () => JSON.parse(__wjs_os_net());
const __user = () => JSON.parse(__wjs_os_user());
const __load = () => JSON.parse(__wjs_os_load());
const isWin = __wjs_os_platform() === "win32";
export function platform() { return __wjs_os_platform(); }
export function arch() { return __wjs_os_arch(); }
export function release() { return __info.release; }
export function type() { return __info.type; }
export function hostname() { return __info.hostname; }
export function tmpdir() { return __info.tmpdir; }
export function homedir() { return __info.homedir; }
export function totalmem() { return __mem().total; }
export function freemem() { return __mem().free; }
export function cpus() { return __cpus(); }
export function networkInterfaces() { return __net(); }
export function userInfo() { return __user(); }
export function uptime() { return __wjs_os_uptime(); }
export function loadavg() { return __load(); }
export function getLocale() { return __wjs_os_locale(); }
export const EOL = isWin ? "\r\n" : "\n";
export default { platform, arch, release, type, hostname, tmpdir, homedir, totalmem, freemem, cpus, networkInterfaces, userInfo, uptime, loadavg, EOL };
"#;
