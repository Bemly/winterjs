# winterjs serve 指标（Phase 6-d3）

`GET /metrics` 返回 Prometheus 文本（`text/plain; version=0.0.4`）。
注意：`/metrics` 自身的请求同样被计数与限流。

| 指标名 | 类型 | labels | 含义 |
|---|---|---|---|
| `winterjs_serve_requests_total` | counter | `method`、`path`、`status` | 已完成请求数（`path` 取原始 uri path；静态站路径基数≈文件数，可接受） |
| `winterjs_serve_request_duration_seconds` | histogram（summary 形态导出） | `method`、`path` | 请求耗时秒（含静态文件 IO） |
| `winterjs_serve_in_flight` | gauge | — | 当前处理中请求数 |

限流：`--limit-rps N`（`N=0` 关闭，默认关闭）为全局统一配额（`governor`，
每 `1/N` 秒补 1，burst=1）；超限返回 `429` + `Retry-After` 秒 + `rate limited`
body，同时记一条 `winterjs::serve` DEBUG 日志（429 响应本身**不**计入
`requests_total`，守卫在中间件内先返回）。
