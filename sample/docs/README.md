# sample/docs — winterjs 样例与文档索引 / Samples & Docs Index

| 文档 | 内容 |
|---|---|
| [quickstart.en.md](./quickstart.en.md) / [quickstart.zh.md](./quickstart.zh.md) | 5 分钟上手：构建、跑文件、测试、serve |
| [api.en.md](./api.en.md) / [api.zh.md](./api.zh.md) | 全模块 API 参考（nodejs.org/api 体例：稳定性 + 样例 + 已知偏离） |
| [cli.en.md](./cli.en.md) / [cli.zh.md](./cli.zh.md) | CLI 全 flag 参考：动作表、修饰归属表、退出码、node 兼容旗 |

样例（`winterjs --run sample/<area>/<file>`，全部离线可跑，TLS 用本地自签 fixture）：

`web/` 计时/URL/Blob/fetch/流/压缩/WebCrypto/事件/克隆/计时/Buffer/console/WS ·
`path/` `os/` `process/` `events/` `util/`（含 sys） `assert/` `codecs/`（string_decoder/querystring/punycode） ·
`url-timers/`（legacy url + timers/promises） `stream/` `fs/` `crypto/` `zlib/` ·
`test-runner/` `vm/` `module/`（CJS/ESM） `observe/`（channel/ALS/perf/v8/domain/tty） ·
`readline/` `net/` `dgram/` `dns/` `http/` `tls/` `https/` `http2/` `child-process/` ·
`cluster/` `worker-threads/` `sqlite/`（node + bun 双形态） `serve-hello/`（见其 README）。
