#!/bin/bash
# BH-028 の合成のプロジェクトを作り、問い合わせと同じサンドボックスで五回ずつ測る。
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$repo_root"
processor=${1:-${BENITOITEH_TEST_BENITOITE:-}}
if [[ -z "$processor" ]]; then
    printf 'Usage: %s <benitoite> (or set BENITOITEH_TEST_BENITOITE)\n' "$0" >&2
    exit 2
fi
export CARGO_NET_OFFLINE=true
mkdir -p work
cargo build --release -p benitoiteh --bin benitoiteh
exec cargo run --release -p benitoiteh --example bench_write_check -- "$processor" "$repo_root/work"
