# winterjs Quickstart

> 5-minute path from zero to running JavaScript. 中文版见 [quickstart.zh.md](./quickstart.zh.md).

## 1. Install & verify

```bash
cargo build
export SDKROOT="$(xcrun --show-sdk-path)"          # macOS, needed every new shell
export LIBCLANG_PATH="/opt/homebrew/opt/llvm/lib"
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"

./target/debug/winterjs --eval '40 + 2'            # → 42
```

## 2. Run a file, evaluate code, start a REPL

```bash
./target/debug/winterjs --run sample/web/timers.js  # run a JS file, print completion value
./target/debug/winterjs --eval 'await Promise.resolve(7)'  # → 7
./target/debug/winterjs --repl                       # interactive REPL (Ctrl-D to exit)
```

Rules (see [cli.en.md](./cli.en.md)):

* **Exactly one action per invocation**: `--run a.js --eval 1` is an error.
* **No bare subcommands / positional actions**: `winterjs a.js` is an error —
  write `winterjs --run a.js`. Only script arguments go after `--`: 
  `winterjs --run app.js -- --port 8080`.
* **Modifiers belong to their action**: `--port` only works with `--serve`,
  `--filter` only with `--test`, `--schema` only with `--config`,
  `--allow-*` only with `--run/--eval/--test/--repl`. Mismatches exit 1.

## 3. Use Node APIs and Web globals

```js
// hello.mjs — ESM, Web globals and node: imports both work
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

CommonJS also works (`require`, `module.exports`, `__dirname`).

## 4. Run tests, lint, format, serve

```bash
./target/debug/winterjs --test sample/test-runner/   # discover + run test files
./target/debug/winterjs --test sample/test-runner/ --filter 'basics*'
./target/debug/winterjs --lint -- --help              # forwarded to oxlint verbatim
./target/debug/winterjs --fmt                         # forwarded to oxfmt
./target/debug/winterjs --serve sample/serve-hello/public --port 8080 \
  --handler sample/serve-hello/handler.mjs            # static + dynamic fetch + WS
```

## 5. Next steps

* [api.en.md](./api.en.md) — every module, stability, and known deviations.
* [cli.en.md](./cli.en.md) — full flag reference with action scope.
* `sample/<area>/*.js` — 46 runnable examples, one per API area.
