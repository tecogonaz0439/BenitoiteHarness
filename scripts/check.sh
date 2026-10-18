#!/bin/bash
# 完了前の検査を、失敗した段が分かる形で揃える（設計書 07-01「完了の前に通す検査」）。
set -euo pipefail
export CARGO_NET_OFFLINE=${CARGO_NET_OFFLINE:-true}
# 起動する処理系はテストごとの設定か、この明示したパスだけにする。
export BENITOITEH_TEST_BENITOITE=${BENITOITEH_TEST_BENITOITE:-}
export BENITOITEH_TEST_BENITOITE_AARCH64=${BENITOITEH_TEST_BENITOITE_AARCH64:-}
export BENITOITEH_TEST_BENITOITE_X86_64=${BENITOITEH_TEST_BENITOITE_X86_64:-}
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
x86_64_skipped=()
aarch64_skipped=()
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
    if ((${#aarch64_skipped[@]} != 0)); then
        printf '\nNot run on aarch64-unknown-linux-musl:\n'
        printf '  %s\n' "${aarch64_skipped[@]}"
    fi
    if ((${#x86_64_skipped[@]} != 0)); then
        printf '\nNot run on x86_64-unknown-linux-musl:\n'
        printf '  %s\n' "${x86_64_skipped[@]}"
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
    # aarch64 は container の中で走らせる。container の既定のカーネルは Landlock を持たないので、
    # Landlock を要する統合テストを除き、x86_64 の機械で確かめる（実装プラン 00-common「検査」）。
    cargo test --workspace --target aarch64-unknown-linux-musl --lib --bins
    for manifest in crates/*/Cargo.toml; do
        test_args=()
        for test_file in "${manifest%/Cargo.toml}"/tests/*.rs; do
            test_name=${test_file##*/}
            if [[ "$test_name" == sandbox_linux.rs || "$test_name" == sandbox_benitoite.rs || "$test_name" == sandbox_exec_restricted.rs || "$test_name" == sandbox_benitoite_run.rs || "$test_name" == sandbox_benitoite_test.rs ]]; then
                aarch64_skipped+=("${test_file}: the container kernel lacks Landlock")
                continue
            fi
            test_args+=(--test "${test_name%.rs}")
        done
        if ((${#test_args[@]} != 0)); then
            cargo test --manifest-path "$manifest" --target aarch64-unknown-linux-musl "${test_args[@]}"
        fi
    done
    cargo test --workspace --target aarch64-unknown-linux-musl --doc
    # x86_64 は、container の Rosetta では走らせない。Rosetta は openat2 を持たず（safe_fs を使うテストが
    # 動かない）、TLS の暗号の実装（graviola）が要する ADX の命令も持たない。BENITOITEH_X86_64_HOST が
    # あれば、本物の x86_64 の Linux の podman で走らせ、なければビルドとリンクまでを確かめる
    # （実装プラン 00-common「検査」）。
    cargo test --workspace --target x86_64-unknown-linux-musl --no-run
    if [[ -n "${BENITOITEH_X86_64_HOST:-}" ]]; then
        if [[ -z "${BENITOITEH_X86_64_DIR:-}" ]]; then
            remote_home=$(ssh -o BatchMode=yes -o LogLevel=ERROR "$BENITOITEH_X86_64_HOST" 'printf %s "$HOME"')
            export BENITOITEH_X86_64_DIR="$remote_home/benitoiteh-test"
        fi
        printf 'Running x86_64 tests on %s (%s)\n' "$BENITOITEH_X86_64_HOST" "$BENITOITEH_X86_64_DIR"
        scripts/remote-linux-run.sh --sync
        CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUNNER="$repo_root/scripts/remote-linux-run.sh" \
            cargo test --workspace --target x86_64-unknown-linux-musl
    else
        x86_64_skipped+=("all test execution (built and linked only; set BENITOITEH_X86_64_HOST to run them)")
    fi
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
