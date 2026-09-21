//! tests/node/fs/ — 对齐 src/builtins/node/fs.rs（按面分块）。

#[path = "fs/basic.rs"]
mod basic;
#[path = "fs/sync.rs"]
mod sync;
#[path = "fs/watch.rs"]
mod watch;
#[path = "fs/streams.rs"]
mod streams;
