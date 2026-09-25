//! `crypto`：getRandomValues（`getrandom`）+ randomUUID（`uuid`）+ 全算法族。
//! 全 safe（读/写均经 `as_*_slice_safe` + `NoGC` 令牌）；BigInt 视图暂不支持。
//! §0.9 拆分：按算法族（common/symm/rsa/ec/dsa/eddsa/x509/xdh + tests），
//! 调用方路径 `pub use` 原位保持。

#[macro_use]
mod common;
mod dsa;
mod ec;
mod eddsa;
mod rsa;
mod symm;
pub(crate) mod x509;
mod xdh;

#[cfg(test)]
mod tests;

pub use dsa::*;
pub use ec::*;
pub use eddsa::*;
pub use rsa::*;
pub use symm::*;
pub use x509::*;
pub use xdh::*;
