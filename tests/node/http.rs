//! tests/node/http/ — 对齐 src/builtins/node/http.rs（按面分块）。

#[path = "http/loopback.rs"]
mod loopback;
#[path = "http/keepalive.rs"]
mod keepalive;
#[path = "http/parity.rs"]
mod parity;
#[path = "http/mapper.rs"]
mod mapper;
#[path = "http/surface.rs"]
mod surface;
#[path = "http/timeout.rs"]
mod timeout;
#[path = "http/upgrade.rs"]
mod upgrade;
