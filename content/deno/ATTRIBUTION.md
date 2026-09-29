# Deno API 语料（REPL `.doc` 命令用，离线内置）

来源：`github.com/denoland/deno`（MIT license）下 `cli/tsc/dts/*.d.ts` 的 TSDoc
注释（源文件进仓见 `vendor/ns-dts/README.md`），经 `scripts/gen-ns-docs.py`
抽取为 MDN 形状页面：

- `vendor/ns-dts/deno.ns.d.ts` ← `lib.deno.ns.d.ts`（主体）
- `vendor/ns-dts/deno_net.d.ts` ← `lib.deno_net.d.ts`
- `vendor/ns-dts/deno.unstable.d.ts` ← `lib.deno.unstable.d.ts`（unstable 面，页内保留注记）

只收 winterjs 已别名的符号（53 页）；沿用上游原文，不编撰。

- 版权：© the Deno authors（MIT）。
- 协议：MIT（随源码分发；本目录文件不换牌）。
- 本仓代码（NPL-1.1）仅**读取**这些文件渲染显示。
- 更新：重拉上游 `.d.ts` 后重跑脚本；输出逐字节确定性。

覆盖偏差（相对真机，文档记录）：`Deno.readLink`（大写 L，真机即此形；
`readlink` 小写是本仓旧别名，已改）；`Deno.statFs` 真机 stable 无此面，未别名、
无页（`node:fs` 的 `statfs` 照常可用）。
