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
skill_eval_work=
stage=setup

finish() {
    local status=$?
    if [[ -n "$skill_eval_work" ]]; then
        rm -rf -- "$skill_eval_work" || true
    fi
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
        printf '  skill-eval: http_client (loopback listening is blocked in the Codex sandbox)\n'
        printf '  skill-eval: benitoiteh-mock pass (needs loopback and the benitoiteh sandbox outside Codex)\n'
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

begin_stage processors
required_processors=(BENITOITEH_TEST_BENITOITE)
if ! "$inside_codex"; then
    required_processors+=(BENITOITEH_TEST_BENITOITE_AARCH64)
    if [[ -n "${BENITOITEH_X86_64_HOST:-}" ]]; then
        required_processors+=(BENITOITEH_TEST_BENITOITE_X86_64)
    fi
fi
for variable in "${required_processors[@]}"; do
    if [[ -z "${!variable}" ]]; then
        printf '%s is required: set it to a benitoite v0.0.7 executable.\n' "$variable" >&2
        exit 2
    fi
    processor_path=${!variable}
    processor_dir=$(cd "$(dirname "$processor_path")" && pwd -P)
    export "$variable=$processor_dir/$(basename "$processor_path")"
done

processor_identity() {
    local target=$1 report=$2 digest=$3 version
    version=$(printf '%s' "$report" | python3 -c 'import json, sys; print(json.load(sys.stdin)["version"])')
    printf 'processor %s: version=%s sha256=%s\n' "$target" "$version" "$digest"
}

begin_stage fmt
cargo fmt --all --check

begin_stage clippy
cargo clippy --workspace --all-targets
for target in "${linux_targets[@]}"; do
    cargo clippy --workspace --all-targets --target "$target"
done

host_target=$(rustc -vV | sed -n 's/^host: //p')
processor_report=$("$BENITOITEH_TEST_BENITOITE" --version=json)
processor_hash=$(shasum -a 256 "$BENITOITEH_TEST_BENITOITE" | awk '{print $1}')
processor_identity "$host_target" "$processor_report" "$processor_hash"
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

target_dir=${CARGO_TARGET_DIR:-target}
begin_stage skill-eval
python3 -I -m unittest discover -s tools/skill-eval -p 'test_*.py'
skill_eval_args=()
if "$inside_codex"; then
    # Codex のサンドボックスではループバックの待ち受けができない（実装プラン P5-01）。
    for task_file in tools/skill-eval/tasks/*/task.json; do
        task_name=${task_file%/task.json}
        task_name=${task_name##*/}
        if [[ "$task_name" != http_client ]]; then
            skill_eval_args+=(--task "$task_name")
        fi
    done
fi
python3 -I tools/skill-eval/skill_eval.py --self-test --benitoite "$BENITOITEH_TEST_BENITOITE" \
    ${skill_eval_args[@]+"${skill_eval_args[@]}"}
skill_eval_work=$(mktemp -d)
python3 -I tools/skill-eval/skill_eval.py --dry-run --benitoite "$BENITOITEH_TEST_BENITOITE" \
    --work-root "$skill_eval_work" ${skill_eval_args[@]+"${skill_eval_args[@]}"}
warning_count=$(awk '/^\| W[0-9][0-9][0-9][0-9] \|/ { count++ } END { print count+0 }' "$skill_eval_work/record.md")
if [[ "$warning_count" != 5 ]]; then
    printf 'Expected 5 warning rows in the skill-eval record, found %s.\n' "$warning_count" >&2
    exit 1
fi
if ! "$inside_codex"; then
    python3 -I tools/skill-eval/skill_eval.py --harness benitoiteh-mock \
        --benitoite "$BENITOITEH_TEST_BENITOITE" --benitoiteh "$target_dir/debug/benitoiteh" \
        --work-root "$skill_eval_work/mock" --record "$skill_eval_work/mock-record.md"
    python3 -I - "$skill_eval_work/mock-record.jsonl" <<'EOF'
import json
import sys

with open(sys.argv[1], encoding='utf-8') as f:
    rows = [json.loads(line) for line in f if line.strip()]
if len(rows) != 11 or any(row.get('success') is not True or row.get('end') != 'completed'
                          or row.get('shell_output_seen') is not False or row.get('cost_usd') != 0
                          or not isinstance(row.get('wall_seconds'), (int, float)) for row in rows):
    raise SystemExit('Expected 11 successful, completed mock trials without shell output, with zero cost and measured time.')
EOF
fi

begin_stage perf-tools
python3 -I -m unittest discover -s tools/perf -p 'test_*.py'

begin_stage linux
if ! "$inside_codex"; then
    processor_report=$(scripts/container-run.sh "$BENITOITEH_TEST_BENITOITE_AARCH64" --version=json)
    processor_hash=$(shasum -a 256 "$BENITOITEH_TEST_BENITOITE_AARCH64" | awk '{print $1}')
    processor_identity aarch64-unknown-linux-musl "$processor_report" "$processor_hash"
    # aarch64 は container の中で走らせる。container の既定のカーネルは Landlock を持たないので、
    # Landlock を要する統合テストを除き、x86_64 の機械で確かめる（実装プラン 00-common「検査」）。
    cargo test --workspace --target aarch64-unknown-linux-musl --lib --bins
    for manifest in crates/*/Cargo.toml; do
        test_args=()
        for test_file in "${manifest%/Cargo.toml}"/tests/*.rs; do
            test_name=${test_file##*/}
            if [[ "$test_name" == sandbox_linux.rs || "$test_name" == sandbox_benitoite.rs || "$test_name" == sandbox_exec_restricted.rs || "$test_name" == sandbox_benitoite_run.rs || "$test_name" == sandbox_benitoite_test.rs || "$test_name" == sandbox_benitoite_scratch.rs ]]; then
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
        processor_report=$(scripts/remote-linux-run.sh "$BENITOITEH_TEST_BENITOITE_X86_64" --version=json)
        processor_hash=$(scripts/remote-linux-run.sh --sha256 "$BENITOITEH_TEST_BENITOITE_X86_64" | awk '{print $1}')
        processor_identity x86_64-unknown-linux-musl "$processor_report" "$processor_hash"
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
