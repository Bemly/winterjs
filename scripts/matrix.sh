#!/bin/sh
# 本机 10-target 编译矩阵（plan Phase 0/8 验收；可原样搬进 GitHub Actions）。
#
# 每列两级：
#   tree  —— §2 特性门控红线审计：`cargo tree -i aws-lc-rs / native-tls / openssl`
#            必须为空（配错 feature 把 ✅ 变 ⚠️/❌ 的最终证明）。
#   check —— `cargo check --target <t>`：全依赖在该 target 的 cfg/特性下可编译
#            （含 mozjs_sys 按 TARGET 下载 servo release 预构建引擎）。
#
# 运行层说明：macA64 原生全测（cargo test）；macX64 经 Rosetta 跑测试；
# Linux/Android/OHOS 的运行层需 Docker/模拟器/真机，属 CI 阶段（脚本记录）。
#
# 用法：scripts/matrix.sh [target ...]   无参数 = 全 10 列。
# 环境要求：AGENTS §3 的 export（SDKROOT/LIBCLANG_PATH/PATH）。
#
# 工具链：本机 /opt/homebrew/bin/cargo 是 brew rust（sysroot 仅原生 std，无法
# 装交叉 target）——矩阵统一切到 rustup 管理的 stable 工具链（可装任意
# target，rustup target add 后即为交叉 std）。日常原生构建不受影响。
set -u

RUSTUP_BIN="$(dirname "$(rustup which --toolchain stable rustc)")"
export PATH="$RUSTUP_BIN:$PATH"
echo "toolchain: $(rustc --version) ($(rustc --print sysroot | sed 's|.*toolchains/||;s|/bin||'))"

ALL_TARGETS="aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu aarch64-pc-windows-msvc x86_64-pc-windows-msvc aarch64-linux-android x86_64-linux-android aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos"
TARGETS="${*:-$ALL_TARGETS}"
cd "$(dirname "$0")/.."

fail=0
for t in $TARGETS; do
    echo "=== [$t] tree audit ==="
    banned=""
    for pkg in aws-lc-rs aws-lc-sys native-tls openssl; do
        # -i 按反查：命中即违规（输出非空 = 该包进了依赖闭包）
        if cargo tree --target "$t" -i "$pkg" >/dev/null 2>&1; then
            echo "  ✗ $pkg FOUND in tree"
            banned="$banned $pkg"
        else
            echo "  ✓ no $pkg"
        fi
    done
    if [ -n "$banned" ]; then
        echo "=== [$t] TREE AUDIT FAILED:$banned ==="
        fail=1
        continue
    fi

    echo "=== [$t] cargo check ==="
    if cargo check --target "$t" 2> "target/check-$t.log"; then
        echo "  ✓ check ok ($(grep -c '^warning' "target/check-$t.log" 2>/dev/null || echo 0) warnings)"
    else
        echo "  ✗ check FAILED (log: target/check-$t.log, tail:)"
        tail -5 "target/check-$t.log"
        fail=1
    fi
done

echo "=== matrix done (fail=$fail) ==="
exit $fail
