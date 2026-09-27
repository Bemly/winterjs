//! F2 临时探针：`WINTERJS_TIMING=1` 时累计各阶段耗时/计数并在 run 结束时输出到
//! stderr；F2 落地后删除本文件及全部调用点。无 unsafe，无新依赖，开销：关闭时
//! 每次调用一次 `OnceLock` 读 + `Option` 判断。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var_os("WINTERJS_TIMING").is_some_and(|v| v == "1" || v == "true")
    })
}

fn base() -> &'static Instant {
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now)
}

/// 阶段打点（关闭时零输出）。
pub fn mark(label: &str) {
    if on() {
        eprintln!("[timing] {:>18} +{}ms", label, base().elapsed().as_millis());
    }
}

/// 计时起（关闭时回 None，调用点零分配）。
pub fn start() -> Option<Instant> {
    on().then(Instant::now)
}

/// 计时止（None 即跳过）。
pub fn fin(s: Option<Instant>, n: &AtomicU64, t: &AtomicU64) {
    if let Some(s) = s {
        n.fetch_add(1, Ordering::Relaxed);
        t.fetch_add(s.elapsed().as_micros() as u64, Ordering::Relaxed);
    }
}

/// RAII 计时守卫（覆盖全部 return/`?` 出口；关闭时 Drop 为空操作）。
pub struct Guard {
    s: Option<Instant>,
    n: &'static AtomicU64,
    t: &'static AtomicU64,
}

pub fn guard(n: &'static AtomicU64, t: &'static AtomicU64) -> Guard {
    Guard { s: start(), n, t }
}

impl Drop for Guard {
    fn drop(&mut self) {
        fin(self.s.take(), self.n, self.t);
    }
}

macro_rules! ctr {
    ($n:ident, $t:ident) => {
        pub static $n: AtomicU64 = AtomicU64::new(0);
        pub static $t: AtomicU64 = AtomicU64::new(0);
    };
}

ctr!(N_PREP, T_PREP); // modules::prepare（fetch+transpile，含缓存命中）
ctr!(N_COMP, T_COMP); // modules::compile_source（SM 解析）
ctr!(N_CJS, T_CJS); // modules::cjs_interop 全判定（含包类型查找+探测解析）
ctr!(N_PKG, T_PKG); // require::nearest_pkg_type（逐级 package.json 读+解析）
ctr!(N_RESO, T_RESO); // loader::resolve（specifier 解析）
ctr!(N_CJSN, T_CJSN); // require::cjs_static_names（具名导出静态发现，含递归）

pub static CACHE_MEM: AtomicU64 = AtomicU64::new(0);
pub static CACHE_DISK: AtomicU64 = AtomicU64::new(0);
pub static CACHE_MISS: AtomicU64 = AtomicU64::new(0);

fn show(n: &AtomicU64, t: &AtomicU64, label: &str) {
    let c = n.load(Ordering::Relaxed);
    if c > 0 {
        let us = t.load(Ordering::Relaxed);
        eprintln!(
            "[timing] {:>18} n={:<6} total={:>8.1}ms avg={:>7.1}us",
            label,
            c,
            us as f64 / 1000.0,
            us as f64 / c as f64
        );
    }
}

/// run 结束时输出汇总（关闭时零输出）。
pub fn dump() {
    if !on() {
        return;
    }
    eprintln!("[timing] ---- summary ----");
    show(&N_PREP, &T_PREP, "prepare");
    show(&N_COMP, &T_COMP, "sm-compile");
    show(&N_CJS, &T_CJS, "cjs-interop");
    show(&N_PKG, &T_PKG, "pkgtype-walk");
    show(&N_RESO, &T_RESO, "resolve");
    show(&N_CJSN, &T_CJSN, "cjs-names");
    let m = CACHE_MEM.load(Ordering::Relaxed);
    let d = CACHE_DISK.load(Ordering::Relaxed);
    let x = CACHE_MISS.load(Ordering::Relaxed);
    eprintln!("[timing] transpile-cache mem={m} disk={d} miss={x}");
}
