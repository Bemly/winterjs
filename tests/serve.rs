//! serve 黑盒（tests/serve/ 按 src/serve 对齐；共享见 tests/common）。

mod common;
#[path = "serve/helpers.rs"]
mod helpers;
#[path = "serve/core.rs"]
mod core;
#[path = "serve/ws.rs"]
mod ws;
#[path = "serve/h3.rs"]
mod h3;

