# Bun API 语料（REPL `.doc` 命令用，离线内置）

来源：`github.com/oven-sh/bun`（MIT license）下 TypeScript 类型声明的 TSDoc 注释
（源文件进仓见 `vendor/ns-dts/README.md`），经 `scripts/gen-ns-docs.py` 抽取为
MDN 形状页面（散文 + `## Syntax` + `### Parameters`）：

- `vendor/ns-dts/bun.d.ts` ← `packages/bun-types/bun.d.ts`
- `vendor/ns-dts/bun.serve.d.ts` ← `packages/bun-types/serve.d.ts`
- `vendor/ns-dts/bun.shell.d.ts` ← `packages/bun-types/shell.d.ts`

只收 winterjs 已别名的符号（29 页）；沿用上游原文，不编撰。缺页的 `.doc`
主题走未知条目提示（见 `src/repl_doc.rs`）。

- 版权：© Oven-sh / Bun contributors（MIT）。
- 协议：MIT（随源码分发；本目录文件不换牌）。
- 本仓代码（NPL-1.1）仅**读取**这些文件渲染显示。
- 更新：重拉上游 `.d.ts` 后重跑脚本；输出逐字节确定性（重跑无 diff 即可提交）。

覆盖偏差（相对真机，文档记录）：`Bun.cwd` 真机不存在（用 `process.cwd()`），
故无此页；`Bun.TOML`/`YAML`/`Transpiler`/`FileSystemRouter` 无底座，未别名、无页。
