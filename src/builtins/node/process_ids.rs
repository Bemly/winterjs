//! `process` 身份族 natives（`getuid/getgid/geteuid/getegid/getgroups`）。
//! 由 `src/builtins/node/process_.rs` 按 §0.9 纯搬移拆出（字节恒等，调用方经
//! `pub use` 原位重导出；`node::process_::getuid` 等路径不变）。

use mozjs::conversions::ToJSValConvertible as _;
use mozjs::jsval::{JSVal, UndefinedValue};
use mozjs::rooted;

use crate::jsapi_glue::{wrap_cx, Frame};

/// 身份族（10f：fs 套件 getuid()===0 守卫点名；libc 直查）。
macro_rules! ids_native {
    ($name:ident, $call:expr) => {
        pub unsafe extern "C" fn $name(
            cx_raw: *mut mozjs::jsapi::JSContext,
            argc: u32,
            vp: *mut JSVal,
        ) -> bool {
            // SAFETY: 引擎回调提供的 raw cx 有效
            let mut _cx = unsafe { wrap_cx(cx_raw) };
            let frame = unsafe { Frame::from_raw(vp, argc) };
            let _ = &mut _cx;
            let v: u32 = $call;
            frame.set_rval(mozjs::jsval::Int32Value(v as i32));
            true
        }
    };
}
ids_native!(getuid, unsafe { libc::getuid() });
ids_native!(getgid, unsafe { libc::getgid() });
ids_native!(geteuid, unsafe { libc::geteuid() });
ids_native!(getegid, unsafe { libc::getegid() });

/// `__wjs2_process_getgroups()` → group id 数组（node 口径：缺 egid 即补）。
pub unsafe extern "C" fn getgroups(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut JSVal,
) -> bool {
    // SAFETY: 引擎回调提供的 raw cx 有效
    let mut cx = unsafe { wrap_cx(cx_raw) };
    let frame = unsafe { Frame::from_raw(vp, argc) };
    let mut ids: Vec<i32> = Vec::new();
    #[cfg(unix)]
    {
        let n = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
        if n > 0 {
            let mut buf: Vec<libc::gid_t> = vec![0; n as usize];
            let n2 = unsafe { libc::getgroups(n, buf.as_mut_ptr()) };
            if n2 > 0 {
                ids.extend(buf[..n2 as usize].iter().map(|g| *g as i32));
            }
        }
        let egid = unsafe { libc::getegid() } as i32;
        if !ids.contains(&egid) {
            ids.push(egid);
        }
    }
    rooted!(&in(cx) let mut arr = UndefinedValue());
    ids.to_jsval(&mut cx, arr.handle_mut());
    frame.set_rval(arr.get());
    true
}
