//! POSIX 身份设置 natives（setuid/setgid/seteuid/setegid/setgroups/initgroups）。
//! 从 `process_.rs` 拆出（§0.9 单文件 ≤1000 行；纯搬移，行为不变，
//! 调用方经 `node::process_cred::` 原位改址）。

use mozjs::jsval::JSVal;

use crate::jsapi_glue::{report_error, value_to_string, wrap_cx, Frame};

/// POSIX credential 查询：用户名→uid（unknown 即 None；数字直通 u32）。
#[cfg(unix)]
fn cred_user_id(id: &serde_json::Value) -> Option<u32> {
    match id {
        serde_json::Value::Number(n) => n.as_u64().and_then(|v| u32::try_from(v).ok()),
        serde_json::Value::String(s) => {
            // SAFETY: name 为临时 CString，getpwnam 同步读后即取 uid，无别名；
            // 返回的静态区指针只读 uid 字段，不持有。
            let c = std::ffi::CString::new(s.as_str()).ok()?;
            let pwd = unsafe { libc::getpwnam(c.as_ptr()) };
            if pwd.is_null() {
                return None;
            }
            Some(unsafe { (*pwd).pw_uid })
        }
        _ => None,
    }
}

/// 组名→gid（unknown 即 None；数字直通 u32）。
#[cfg(unix)]
fn cred_group_id(id: &serde_json::Value) -> Option<u32> {
    match id {
        serde_json::Value::Number(n) => n.as_u64().and_then(|v| u32::try_from(v).ok()),
        serde_json::Value::String(s) => {
            // SAFETY: 同上（getgrnam 静态区只读 gr_gid）。
            let c = std::ffi::CString::new(s.as_str()).ok()?;
            let grp = unsafe { libc::getgrnam(c.as_ptr()) };
            if grp.is_null() {
                return None;
            }
            Some(unsafe { (*grp).gr_gid })
        }
        _ => None,
    }
}

/// set*id 系收敛：0 成功 / 1 未知身份 / 负值为 -errno（JS 侧取反成错；
/// EPERM 本体即 1，不可用正数传 errno）。
#[cfg(unix)]
fn cred_set_id(
    id_json: &str,
    is_user: bool,
    call: unsafe extern "C" fn(u32) -> libc::c_int,
) -> i32 {
    let Ok(id) = serde_json::from_str::<serde_json::Value>(id_json) else {
        return -(libc::EINVAL as i32);
    };
    let sys = if is_user { cred_user_id(&id) } else { cred_group_id(&id) };
    let Some(sys_id) = sys else { return 1 };
    // SAFETY: 纯数值 syscall，无指针，无别名；errno 当场取。
    if unsafe { call(sys_id) } == 0 {
        0
    } else {
        -std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EPERM)
    }
}

macro_rules! cred_set_native {
    ($name:ident, $is_user:expr, $call:expr) => {
        pub unsafe extern "C" fn $name(
            cx_raw: *mut mozjs::jsapi::JSContext,
            argc: u32,
            vp: *mut JSVal,
        ) -> bool {
            // SAFETY: 引擎回调提供的 raw cx 有效；文档许可由此构造 wrapper
            let mut cx = unsafe { wrap_cx(cx_raw) };
            let frame = unsafe { Frame::from_raw(vp, argc) };
            if frame.argc() < 1 {
                report_error(&mut cx, "TypeError: credential setter needs an id");
                return false;
            }
            let id_json = value_to_string(&mut cx, frame.arg(0));
            frame.set_rval(mozjs::jsval::Int32Value(cred_set_id(&id_json, $is_user, $call)));
            true
        }
    };
}

#[cfg(unix)]
cred_set_native!(setuid, true, libc::setuid);
#[cfg(unix)]
cred_set_native!(setgid, false, libc::setgid);
#[cfg(unix)]
cred_set_native!(seteuid, true, libc::seteuid);
#[cfg(unix)]
cred_set_native!(setegid, false, libc::setegid);

/// `__wjs2_setgroups(jsonArr)` → 0 成功 / (idx+1) 未知组 / 负 errno。
pub unsafe extern "C" fn setgroups(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 1 {
        report_error(&mut cx, "TypeError: setgroups needs an array");
        return false;
    }
    let list_json = value_to_string(&mut cx, frame.arg(0));
    let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&list_json) else {
        report_error(&mut cx, "TypeError: setgroups needs an array");
        return false;
    };
    #[cfg(unix)]
    {
        let mut gids: Vec<libc::gid_t> = Vec::with_capacity(list.len());
        for (i, g) in list.iter().enumerate() {
            match cred_group_id(g) {
                Some(id) => gids.push(id as libc::gid_t),
                None => {
                    frame.set_rval(mozjs::jsval::Int32Value((i + 1) as i32));
                    return true;
                }
            }
        }
        // SAFETY: gids 存活到调用返回；errno 当场取。
        if unsafe { libc::setgroups(gids.len() as libc::c_int, gids.as_ptr()) } == 0 {
            frame.set_rval(mozjs::jsval::Int32Value(0));
        } else {
            let e = -std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EPERM);
            frame.set_rval(mozjs::jsval::Int32Value(e));
        }
        true
    }
    #[cfg(not(unix))]
    {
        let _ = &list;
        report_error(&mut cx, "Error: setgroups is not available on this platform");
        false
    }
}

/// `__wjs2_initgroups(userJson, extraJson)` → 0 成功 / 1 用户未知 / 2 组未知 / 负 errno。
pub unsafe extern "C" fn initgroups(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 同上
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    if frame.argc() < 2 {
        report_error(&mut cx, "TypeError: initgroups needs user and group");
        return false;
    }
    let (u_json, g_json) = (
        value_to_string(&mut cx, frame.arg(0)),
        value_to_string(&mut cx, frame.arg(1)),
    );
    #[cfg(unix)]
    {
        let (Ok(u), Ok(g)) = (
            serde_json::from_str::<serde_json::Value>(&u_json),
            serde_json::from_str::<serde_json::Value>(&g_json),
        ) else {
            report_error(&mut cx, "TypeError: initgroups needs user and group");
            return false;
        };
        // node 口径：先解组（未知即 2），再解用户（未知即 1）——双未知报组错，
        // 见 test-process-initgroups.js 末段。
        let gid = match cred_group_id(&g) {
            Some(id) => id as libc::gid_t,
            None => {
                frame.set_rval(mozjs::jsval::Int32Value(2));
                return true;
            }
        };
        // initgroups 取用户名串（数字 id 无名可查即未知）。
        let user = match &u {
            serde_json::Value::String(s) => s.clone(),
            _ => {
                frame.set_rval(mozjs::jsval::Int32Value(1));
                return true;
            }
        };
        // SAFETY: user 为临时 CString，initgroups 同步调用，无别名。
        if let Ok(c) = std::ffi::CString::new(user) {
            // SAFETY: 同上；errno 当场取（macOS 组参为 int，其余为 gid_t）。
            let rc = unsafe {
                #[cfg(target_vendor = "apple")]
                {
                    libc::initgroups(c.as_ptr(), gid as libc::c_int)
                }
                #[cfg(not(target_vendor = "apple"))]
                {
                    libc::initgroups(c.as_ptr(), gid)
                }
            };
            if rc == 0 {
                frame.set_rval(mozjs::jsval::Int32Value(0));
                return true;
            }
            let e = -std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EPERM);
            frame.set_rval(mozjs::jsval::Int32Value(e));
            return true;
        }
        frame.set_rval(mozjs::jsval::Int32Value(1));
        true
    }
    #[cfg(not(unix))]
    {
        let _ = (&u_json, &g_json);
        report_error(&mut cx, "Error: initgroups is not available on this platform");
        false
    }
}
