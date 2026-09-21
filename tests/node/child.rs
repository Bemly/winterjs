//! tests/node/child/ — 对齐 src/builtins/node/child.rs（spawn/fork/exec；按面分块）。

#[path = "child/spawn.rs"]
mod spawn;
#[path = "child/exec.rs"]
mod exec;
#[path = "child/fork.rs"]
mod fork;
