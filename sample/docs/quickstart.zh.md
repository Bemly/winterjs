---
layout: docs
title: 快速上手
lang: zh
stub: quickstart
permalink: /zh/quickstart/
---
# winterjs 快速上手

> 5 分钟从零跑起 JS。English version: [Quickstart](../en/quickstart/).

## 1. 构建与验证

```bash
cargo build
export SDKROOT="$(xcrun --show-sdk-path)"          # macOS，每个新 shell 都要
export LIBCLANG_PATH="/opt/homebrew/opt/llvm/lib"
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"

./target/debug/winterjs --eval '40 + 2'            # → 42
```

## 2. 跑文件、求值、进 REPL

```bash
./target/debug/winterjs --run sample/web/timers.js  # 跑 JS 文件并打印完成值
./target/debug/winterjs --eval 'await Promise.resolve(7)'  # → 7
./target/debug/winterjs --repl                       # 交互式 REPL（Ctrl-D 退出）
```

规矩（详见 [CLI 参考](cli/)）：

* **一次恰好一个动作**：`--run a.js --eval 1` 直接报错。
* **无裸子命令/无裸位置参数**：`winterjs a.js` 是错的，写 `--run a.js`；
  脚本自己的参数放 `--` 之后：`winterjs --run app.js -- --port 8080`。
* **修饰 flag 只在对应动作下生效**：`--port` 只配 `--serve`，
  `--filter` 只配 `--test`，`--schema` 只配 `--config`，
  `--allow-*` 只配 `--run/--eval/--test/--repl`。配错 exit=1。

## 3. 用 Node API 与 Web 全局

```js
// hello.mjs —— ESM，下例 Web 全局与 node: 导入混用
import { readFileSync } from 'node:fs';
import path from 'node:path';

const text = readFileSync(new URL('./hello.mjs', import.meta.url), 'utf8');
console.log('self bytes:', new TextEncoder().encode(text).length);
console.log('join:', path.join('a', 'b'));
const res = await fetch('data:text/plain,hi');
console.log(await res.text()); // hi
```

```bash
./target/debug/winterjs --run hello.mjs
```

CommonJS 同样可用（`require`、`module.exports`、`__dirname`）。

## 4. 测试、lint、格式化与 serve

```bash
./target/debug/winterjs --test sample/test-runner/   # 发现并跑测试文件
./target/debug/winterjs --test sample/test-runner/ --filter 'basics*'
./target/debug/winterjs --lint -- --help              # 原样转发给 oxlint
./target/debug/winterjs --fmt                         # 转发给 oxfmt
./target/debug/winterjs --serve sample/serve-hello/public --port 8080 \
  --handler sample/serve-hello/handler.mjs            # 静态 + 动态 fetch + WS
```

## 5. 下一步

* [API 参考](api/) —— 全部模块、稳定性与已知偏离。
* [CLI 参考](cli/) —— 全 flag 参考（含动作归属）。
* `sample/<领域>/*.js` —— 50 个可运行样例，每个 API 域一件。
