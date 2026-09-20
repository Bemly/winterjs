//! Web prelude（一次性全局脚本；§0.9 拆分：按行字节切分、concat 恒等，顺序不可动）。
mod part00;
mod part01;
mod part02;
mod part03;
mod part04;

/// 引擎启动时在全局对象上求值的一次性脚本（§1 路线 Phase 1）。
/// parts 运行时一次拼接（`concat!` 只收字面量，不收 const 路径；LazyLock 进程级单例）。
pub static PRELUDE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    [part00::PART_00, part01::PART_01, part02::PART_02, part03::PART_03, part04::PART_04].concat()
});
