//! `node:internal/test/binding`——`internalBinding()` 测试绑定表。
//!
//! 源：nodejs/node（MIT）对应 internal 件的最小对位实现。表项：
//! `http_parser`（`HTTPParser` 类，与 `_http_common` 同源；monkey-patch 即改
//! 全局注册，`parsers.alloc()` 经注册取新类，parser-lazy-loaded 套件）、
//! `uv`（`UV_ENETUNREACH = -51`，真机实测；immediate-error 套件）。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
import { HTTPParser } from "node:_http_common";
// 绑定注册表（parser-lazy-loaded 套件改写 binding.HTTPParser 后，
// parsers.alloc 经全局注册取新类；_http_common 侧同注册）。
const __table = {
  http_parser: { HTTPParser },
  uv: { UV_ENETUNREACH: -51 },
  // tcp_wrap：localaddress 套件经 common/net.hasMultiLocalhost 探环回多地址。
  // 保守口径：仅标准环回（127.0.0.1/localhost/::1）回 0，其余非零即跳过套件
  // （与真机在不支持平台的行为一致；有 127.0.0.2 的机器上保守跳过，记档）。
  tcp_wrap: (() => {
    function TCP() {}
    TCP.prototype.bind = function (addr) {
      if (addr === "127.0.0.1" || addr === "localhost" || addr === "::1") return 0;
      return -22;
    };
    TCP.prototype.close = function () { return undefined; };
    return { TCP, constants: { SOCKET: 1 } };
  })(),
};
try {
  globalThis.__wjs2_bindHttpParser = __table.http_parser;
} catch {}
export function internalBinding(name) {
  const __e = __table[name];
  if (__e === undefined) throw new Error(`No such binding: ${name}`);
  return __e;
}
export default { internalBinding };
"#;
