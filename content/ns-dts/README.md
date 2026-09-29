# 命名空间文档源 `.d.ts`（REPL `.doc` 语料上游，原样进仓）

`scripts/gen-ns-docs.py` 的输入，全离线可复现（更新即重拉同名文件覆盖 + 重跑脚本）：

| 文件 | 上游 | 用途 |
|---|---|---|
| `bun.d.ts` | `oven-sh/bun@main:packages/bun-types/bun.d.ts` | `Bun.file`/`write`/`spawn` 等 27 页 |
| `bun.serve.d.ts` | 同上 `packages/bun-types/serve.d.ts` | `Bun.serve` |
| `bun.shell.d.ts` | 同上 `packages/bun-types/shell.d.ts` | `Bun.$` |
| `deno.ns.d.ts` | `denoland/deno@main:cli/tsc/dts/lib.deno.ns.d.ts` | Deno 主体 |
| `deno_net.d.ts` | 同上 `cli/tsc/dts/lib.deno_net.d.ts` | `connect`/`listen`/`resolveDns` |
| `deno.unstable.d.ts` | 同上 `cli/tsc/dts/lib.deno.unstable.d.ts` | `listenDatagram`（unstable） |

- 拉取日期：2026-09-28/29（main 快照；MIT license，两仓皆 MIT）。
- 再生：`scripts/gen-ns-docs.py --bun content/ns-dts/bun.d.ts --bun content/ns-dts/bun.serve.d.ts --bun content/ns-dts/bun.shell.d.ts --deno content/ns-dts/deno.ns.d.ts --deno content/ns-dts/deno_net.d.ts --deno content/ns-dts/deno.unstable.d.ts --out content`（根目录执行；输出逐字节确定性，重跑无 diff）。
