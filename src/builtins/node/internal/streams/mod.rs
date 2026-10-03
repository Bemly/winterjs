//! `node:internal/streams/*`（Node lib/internal/streams/* 逐字内嵌，MIT）。
//! require → 垫片（node:internal/* 静态映射）；primordials → 共享静态包装表。
//! 实验旗后路径（iter_classic）与 webstreams/adapters（薄适配）为最小对位。

pub mod add_abort_signal;
pub mod compose;
pub mod destroy;
pub mod duplex;
pub mod duplexify;
pub mod duplexpair;
pub mod end_of_stream;
pub mod from;
pub mod iter_classic;
pub mod iter_types;
pub mod iter_ringbuffer;
pub mod iter_utils;
pub mod iter_from;
pub mod iter_pull;
pub mod iter_push;
pub mod iter_duplex;
pub mod iter_share;
pub mod iter_broadcast;
pub mod iter_transform;
pub mod iter_consumers;
pub mod iter_zlib_binding;
pub mod lazy_transform;
pub mod legacy;
pub mod operators;
pub mod passthrough;
pub mod pipeline;
pub mod readable;
pub mod state;
pub mod transform;
pub mod utils;
pub mod writable;
