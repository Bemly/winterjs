//! 全局分配器（docs/dependencies.md §3 备注 + §16 风险 ①）：
//! - 桌面（macOS/Linux/Windows）：`smmalloc`（crate `smmalloc`，lib 名 `smalloc`，纯 Rust）。
//!   已知特性：单次 alloc 超过最大 size class 直接返回空指针、无回退（§16 风险 ②），
//!   超大分配探针见 `tests/alloc_probe.rs`。
//! - 移动端（android/ohos，smmalloc 无支持代码）：`talc` 5 回退，
//!   内存源用 `GlobalAllocSource<System>`（向系统分配器按需取块），锁用 parking_lot RawMutex。

#[cfg(not(any(target_os = "android", target_os = "ohos")))]
#[global_allocator]
static ALLOC: smmalloc::Smalloc = smmalloc::Smalloc::new();

#[cfg(any(target_os = "android", target_os = "ohos"))]
#[global_allocator]
static ALLOC: talc::TalcLock<
    parking_lot::RawMutex,
    talc::source::GlobalAllocSource<std::alloc::System>,
> = talc::TalcLock::new(talc::source::GlobalAllocSource::new(std::alloc::System));
