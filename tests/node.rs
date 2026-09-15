//! node: 内建黑盒测试（tests/node/ 按 src/builtins/node 对齐；共享见 tests/common）。

mod common;
#[path = "node/helpers.rs"]
mod helpers;
#[path = "node/assert.rs"]
mod assert;
#[path = "node/async_hooks.rs"]
mod async_hooks;
#[path = "node/buffer.rs"]
mod buffer;
#[path = "node/child.rs"]
mod child;
#[path = "node/console.rs"]
mod console;
#[path = "node/crypto.rs"]
mod crypto;
#[path = "node/dgram.rs"]
mod dgram;
#[path = "node/diagnostics_channel.rs"]
mod diagnostics_channel;
#[path = "node/dns.rs"]
mod dns;
#[path = "node/events.rs"]
mod events;
#[path = "node/fs.rs"]
mod fs;
#[path = "node/http.rs"]
mod http;
#[path = "node/http2.rs"]
mod http2;
#[path = "node/https.rs"]
mod https;
#[path = "node/inspector.rs"]
mod inspector;
#[path = "node/net.rs"]
mod net;
#[path = "node/nodemodule.rs"]
mod nodemodule;
#[path = "node/os.rs"]
mod os;
#[path = "node/path.rs"]
mod path;
#[path = "node/path_posix.rs"]
mod path_posix;
#[path = "node/perf_hooks.rs"]
mod perf_hooks;
#[path = "node/process_.rs"]
mod process_;
#[path = "node/punycode.rs"]
mod punycode;
#[path = "node/querystring.rs"]
mod querystring;
#[path = "node/readline.rs"]
mod readline;
#[path = "node/require.rs"]
mod require;
#[path = "node/stream.rs"]
mod stream;
#[path = "node/string_decoder.rs"]
mod string_decoder;
#[path = "node/testmod.rs"]
mod testmod;
#[path = "node/timers_promises.rs"]
mod timers_promises;
#[path = "node/tls.rs"]
mod tls;
#[path = "node/trace_events.rs"]
mod trace_events;
#[path = "node/tty.rs"]
mod tty;
#[path = "node/url.rs"]
mod url;
#[path = "node/util.rs"]
mod util;
#[path = "node/util_types.rs"]
mod util_types;
#[path = "node/v8.rs"]
mod v8;
#[path = "node/vm.rs"]
mod vm;
#[path = "node/worker.rs"]
mod worker;
#[path = "node/zlib.rs"]
mod zlib;
