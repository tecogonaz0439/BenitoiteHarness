#!/bin/bash
# 完了前の検査を、失敗した段が分かる形で揃える（設計書 07-01「完了の前に通す検査」）。
set -euo pipefail
shopt -s nullglob

inside_codex=false
case "$#:${1:-}" in
    0:) ;;
    1:--inside-codex) inside_codex=true ;;
    *) printf 'Usage: %s [--inside-codex]\n' "$0" >&2; exit 2 ;;
esac

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$repo_root"
linux_targets=(aarch64-unknown-linux-musl x86_64-unknown-linux-musl)
skipped_tests=(crates/*/tests/sandbox_*.rs)
rosetta_skipped=()
stage=setup

finish() {
    local status=$?
    if ((status != 0)); then
        printf 'FAILED: %s (exit %s)\n' "$stage" "$status" >&2
    fi
    if "$inside_codex"; then
        printf '\nSkipped with --inside-codex:\n'
        if ((${#skipped_tests[@]} == 0)); then
            printf '  sandbox integration tests: none found (tests/sandbox_*.rs)\n'
        else
            printf '  %s\n' "${skipped_tests[@]}"
        fi
        printf '  linux: container tests for %s\n' "${linux_targets[@]}"
    fi
    if ((${#rosetta_skipped[@]} != 0)); then
        printf '\nNot run on x86_64-unknown-linux-musl under Rosetta:\n'
        printf '  %s\n' "${rosetta_skipped[@]}"
    fi
    exit "$status"
}
trap finish EXIT

begin_stage() {
    stage=$1
    printf '\n== %s ==\n' "$stage"
}

begin_stage fmt
cargo fmt --all --check

begin_stage clippy
cargo clippy --workspace --all-targets
for target in "${linux_targets[@]}"; do
    cargo clippy --workspace --all-targets --target "$target"
done

begin_stage test
cargo build -p sandbox-probe
if [[ $(uname -s) == Darwin ]]; then
    cargo rustc -p sandbox-probe --lib --crate-type cdylib
fi
for target in "${linux_targets[@]}"; do
    cargo build -p sandbox-probe --target "$target"
done
if "$inside_codex"; then
    # 名前のフィルタでは、sandbox のファイルにある別名のテストを除けないため、対象を列挙する。
    # 配置は実装プラン 10-interfaces「ファイルの配置」に従う。
    cargo test --workspace --lib --bins
    for manifest in crates/*/Cargo.toml; do
        test_args=()
        for test_file in "${manifest%/Cargo.toml}"/tests/*.rs; do
            test_name=${test_file##*/}
            case "$test_name" in
                sandbox_*.rs) continue ;;
            esac
            test_args+=(--test "${test_name%.rs}")
        done
        if ((${#test_args[@]} != 0)); then
            cargo test --manifest-path "$manifest" "${test_args[@]}"
        fi
    done
    cargo test --workspace --doc
else
    cargo test --workspace
fi

begin_stage linux
if ! "$inside_codex"; then
    cargo test --workspace --target aarch64-unknown-linux-musl -- --show-output
    # x86_64 は container の Rosetta で動く。Rosetta は openat2 を持たず、記述子も開いているので、
    # カーネルの機能を確かめるテスト（safe_fs と sandbox_*）を除く（実装プラン P0-05「x86_64 の扱い」）。
    # TLS の暗号の実装（graviola）は CPU の ADX の命令を要し、Rosetta はそれを持たない。
    # TLS を使うテストは名前に tls を含める約束とし、ここで除く（実装プラン 00-common「検査」）。
    cargo test --workspace --target x86_64-unknown-linux-musl --lib --bins -- --skip tls --test-threads=1
    rosetta_skipped+=("unit tests whose names contain 'tls' (graviola needs ADX)")
    rosetta_skipped+=("sandbox_*: Rosetta opens extra descriptors and translates syscall architectures; native x86_64 sandbox validation is still required" "safe_fs: Rosetta does not implement openat2")
    for manifest in crates/*/Cargo.toml; do
        test_args=()
        for test_file in "${manifest%/Cargo.toml}"/tests/*.rs; do
            [[ -e "$test_file" ]] || continue
            test_name=${test_file##*/}
            case "$test_name" in
                sandbox_*.rs | safe_fs.rs)
                    rosetta_skipped+=("$test_file")
                    continue
                    ;;
            esac
            test_args+=(--test "${test_name%.rs}")
        done
        if ((${#test_args[@]} != 0)); then
            cargo test --manifest-path "$manifest" --target x86_64-unknown-linux-musl "${test_args[@]}" -- --skip tls --test-threads=1
        fi
    done
fi

begin_stage deny
if "$inside_codex"; then
    cargo deny --offline check licenses
else
    cargo deny check licenses
fi

begin_stage size
host_target=$(rustc -vV | sed -n 's/^host: //p')
target_dir=${CARGO_TARGET_DIR:-target}
cargo build --release -p benitoiteh
bytes=$(wc -c < "$target_dir/release/benitoiteh" | tr -d '[:space:]')
printf '%s: %s bytes\n' "$host_target" "$bytes"
for target in "${linux_targets[@]}"; do
    cargo build --release -p benitoiteh --target "$target"
    bytes=$(wc -c < "$target_dir/$target/release/benitoiteh" | tr -d '[:space:]')
    printf '%s: %s bytes\n' "$target" "$bytes"
done

printf '\nAll checks passed.\n'
