# plan4 → master 合并交接（给 plan3 agent）

> **存档（2026-09-25）**：plan4-serve 已于 2026-09-21 合入 master，交接事项完结。 当前进度见 `docs/plan3.md` §0，索引 `docs/README.md`。


> plan4（独立 serve，WinterCG handler + H1/H2/H3 + WS）已收官。
> 本分支 `plan4-serve` 基于 `04ff117`（与 master 共享基线），27 提交，工作区干净。
> 合并预演（`git merge-tree 04ff117 master plan4-serve`，只读）结论先行：
> **唯一手工 hunk 是 `AGENTS.md` 尾部**（§4.165–169 双边撞号，见 §2 表），其余自动合。

## 1. 本分支内容（27 提交，`git log master..plan4-serve`）

- 拆分收官（6）：`dd5b023` CLI `--handler` → `dc9128f` prelude 拆分 →
  `4a92f71` crypto → `3dd98e1` state → `ebf39ae` runtime → `fc52e15` napi/api →
  `359b343` 收尾。纯搬移 + `pub use` 原位，外部零改动。
- T1（5）：`ae1d7b1` fallback 修（§4.165）→ `727a4c4` 记档 → `de4b901` 黑盒 →
  `d8d5a86` 64KB 分片 + `9ba7ec5` 停机 Wake（§4.166）→ `cafeb2a`/`863adbe` 黑盒。
- T2（2）：`8b610d8` TLS-ALPN + scheme → `66eb3ec` 黑盒。
- T3（3）：`3ea7ba5` H3 同 Router → `d3754a5` 记档（§4.167）→ `86fcb01` 黑盒。
- T4（5）：`7043cc2` WS 决策桥 → `330c9c8` 升级接管（+`hyper-util` 直引）→
  `96e34c5` prelude 工厂 → `29f58ad` 黑盒 → `17f39ce` 记档（§4.168）。
- 收尾（3）：`76a9e05` 依赖记档 → `aaa4c45` flake 观察（§4.169）→
  `7ead278` sample + 双语 README。
- 验收：`tests/serve.rs` **27/27**（phase11 计 11），`--bin` 单测绿，
  node **234/234**（单独），其余 suites 绿，冒烟 5/5。唯一全并行 flake 见 §4.169
  （mutex 解锁失败，单跑+整套件皆绿，非回归）。

## 2. 合并唯一手工活：AGENTS 尾部 Renumber

master 侧已有 §4.161–164（dgram/child）与 §4.165–169（fs 残簇/读流），
本分支写了**同号不同内容**的 §4.165–169。master 的保留，本分支的按此表改号：

| 本分支旧号 | 新号 | 标题 |
|---|---|---|
| §4.165 | §4.170 | tower-http fallback 状态改写双坑 |
| §4.166 | §4.171 | serve 停机 Wake + 响应构造快照边界 |
| §4.167 | §4.172 | H3 半关闭 FIN + h3-axum 请求体整收 |
| §4.168 | §4.173 | T4 WS 五坑 |
| §4.169 | §4.174 | 全并行 mutex flake 观察 |

另：§0.9（单文件≤1000 行铁律，本分支加的 §0 第 9 条）自动合，无需动手；
master 侧 §4 条目里的行号引用若指向被拆文件（`builtins/mod.rs` 等），抽查即可。

## 3. 自动合部分（已逐项核对，无需动手）

- `src/builtins/mod.rs`：prelude 抽离（删巨型 `PRELUDE` const）胜出；
  master 侧 native 表增补逐行保留。`pub use prelude::PRELUDE` 重导出在位，
  master 无外部 `PRELUDE` 引用（`git grep` 实证），无语义断裂。
  注意类型变了：`&str` const → `LazyLock<String>`（调用方照旧 `&*` 即行，本分支已如此）。
- `src/serve.rs`（本分支重写级，1135 行）：master 未碰，原样采用。
- `src/serve_bridge.rs`（新建 617 行）、`src/state/serve.rs`、
  `src/runtime/serve_session.rs`、`src/builtins/prelude/*`、`src/builtins/crypto/*`、
  `src/state/*`、`src/runtime/*`、`src/napi/api/*`：master 未碰，原样采用。
- `Cargo.toml`/`Cargo.lock`：仅 `hyper-util 0.1` 直引（`tokio` 特性；
  axum 传递已在树内 0.1.20，同版零新增传递，见 `docs/dependencies.md` 备注）。
- `tests/serve.rs`（+657）、`tests/cli.rs`（+22）：master 未碰，原样采用。

## 4. 合并后必跑（按序，参 AGENTS §0/§3）

```bash
export SDKROOT="$(xcrun --show-sdk-path)"
export LIBCLANG_PATH="/opt/homebrew/opt/llvm/lib"
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
git merge plan4-serve --no-commit   # 解 §2 hunk 后再继续
cargo build                          # 2 存量警告（http2.rs:362/273）属 master 旧账，不管
./target/debug/winterjs --eval '40 + 2'   # 冒烟 5/5 跑全（见 AGENTS §3）
cargo test --test serve              # 27/27
cargo test --bin winterjs serve::    # 改结构体必跑（§4.173 坑三：build 不编 cfg(test)）
cargo test                           # 后台跑；红了先看 §4.174（单跑+整套件两档复核再定回归）
```

## 5. 未竟事项（另案，不挡合并）

1. **`src/serve.rs` 1135 行，超 §0.9（≤1000）**：合并后第一件事建议拆
   （如 `serve.rs` 核心 + `serve_ws.rs` + `serve_h3.rs`，沿 T 切分）。
   口径更新（2026-09-21）：§0.9 已扩到项目内全部 `.rs` 无豁免，存量超限
   24 件清单见 AGENTS §0.9（`tests/serve.rs` 1151 等 6 件测试 + `node/` 17 件
   同样在拆分范围，不止 serve 一件）。
2. 记档偏离三件（AGENTS §4.171–173 尾部）：通道 unbounded（背压）、H3 上传整收、
   client 侧缺 close-flush。
3. 本机 curl 无 http3（SecureTransport），H3 以 harness 验收（测试注释已记）。
4. `sample/serve-hello/` 为演示目录（handler + 静态 + 双语 README，实测全通），
   不进测试矩阵。

## 6. 回滚点

- 整轮回滚：`git revert -n 04ff117..plan4-serve`（基线含 h3-axum 采购，去掉则 H3 编译失败，注意）。
- 功能级回滚：T4（`7043cc2` 起 5 提交）、T3（`3ea7ba5` 起 3 提交）各自连续可摘；
  拆分提交（`dc9128f` 起 6 提交）动文件结构，摘了必重编全量。
