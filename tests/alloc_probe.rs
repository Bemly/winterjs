//! 分配器探针（docs/dependencies.md §16 风险 ②：smmalloc 超大分配无回退）。
//! 集成测试是独立 crate，不继承 src/main.rs 的 #[global_allocator]，
//! 因此这里显式声明 smmalloc，保证探针测的就是目标分配器。
//! 超大分配用例默认 `#[ignore]`（显式跑：
//! `cargo test --test alloc_probe -- --ignored --nocapture`）。

// 把测试进程的分配器换成 smmalloc，常规路径与超大分配都在它上面跑
#[global_allocator]
static ALLOC: smmalloc::Smalloc = smmalloc::Smalloc::new();

use std::alloc::{Layout, alloc, dealloc};

#[test]
fn alloc_roundtrip_in_size_classes() {
    // 覆盖小/中/大 size class：分配-写入-释放，跑在 #[global_allocator]（smmalloc）上
    for &size in &[1usize, 64, 4096, 1 << 20, 64 << 20] {
        unsafe {
            let layout = Layout::from_size_align(size, 16).unwrap();
            let ptr = alloc(layout);
            assert!(!ptr.is_null(), "alloc {size} bytes failed");
            std::ptr::write_bytes(ptr, 0xAB, size);
            dealloc(ptr, layout);
        }
    }
}

// SAFETY 探针预期：smmalloc 对超出最大 size class 的请求返回空指针（无回退），
// std 的 alloc() 遇 null 会 handle_alloc_error → 进程 abort。此用例就是要把该行为钉在案。
#[test]
#[ignore = "probe: smmalloc oversized alloc (null, no fallback) — run explicitly"]
fn oversized_alloc_above_size_class_cap() {
    // 33 GiB > 最大 size class（约 8 GiB = 4 << 31）
    let layout = Layout::from_size_align(33 << 30, 16).unwrap();
    unsafe {
        let ptr = alloc(layout);
        assert!(!ptr.is_null(), "33 GiB alloc returned null (as documented)");
        std::ptr::write_bytes(ptr, 0, 4096); // 只碰首页，避免整块换页
        dealloc(ptr, layout);
    }
}
