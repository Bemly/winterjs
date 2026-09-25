#!/bin/bash
# §0.9 单文件 ≤1000 行守门：src/tests/benches 的 .rs + src 的 .js；超限即列出并 exit 1。
cd "$(dirname "$0")/.." || exit 2
bad=$(git ls-files '*.rs' 'src/**/*.js' | grep -v '^sample/' | xargs wc -l | awk '$2!="total" && $1>1000 {print $1, $2}')
[ -z "$bad" ] && { echo "check-lines: ok（最大 $(git ls-files '*.rs' 'src/**/*.js' | grep -v '^sample/' | xargs wc -l | grep -v total | sort -rn | head -1)）"; exit 0; }
echo "check-lines: 超 1000 行："; echo "$bad"; exit 1
