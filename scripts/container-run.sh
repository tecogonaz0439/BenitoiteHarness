#!/bin/bash
# Linux 向けの実行ファイルを、手元と同じパスで走らせる（設計書 07-01「安全の性質のテスト」）。
set -euo pipefail

image=alpine:3.22
if (($# == 0)); then
    printf 'Usage: %s <executable> [arguments...]\n' "$0" >&2
    exit 2
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
executable_dir=$(cd "$(dirname "$1")" && pwd -P)
executable="$executable_dir/$(basename "$1")"
shift

# 対象はともに little-endian の ELF64 なので、e_machine をその形で読む。
header=$(od -An -tx1 -N6 "$executable" | tr -d '[:space:]')
if [[ "$header" != 7f454c460201 ]]; then
    printf 'Expected a little-endian ELF64 executable: %s\n' "$executable" >&2
    exit 2
fi
machine=$(od -An -tx1 -j18 -N2 "$executable" | tr -d '[:space:]')
container_args=(run --rm --interactive)
case "$machine" in
    b700) ;;
    3e00) container_args+=(--arch amd64) ;;
    *) printf 'Unsupported ELF machine: %s\n' "$machine" >&2; exit 2 ;;
esac

if [[ -n "${BENITOITEH_TEST_BENITOITE_AARCH64:-}" ]]; then
    processor_dir=$(cd "$(dirname "$BENITOITEH_TEST_BENITOITE_AARCH64")" && pwd -P)
    container_args+=(-e "BENITOITEH_TEST_BENITOITE=$BENITOITEH_TEST_BENITOITE_AARCH64"
        -v "$processor_dir:$processor_dir:ro")
fi
container_args+=(-v "$repo_root:$repo_root")
if [[ "$executable_dir" != "$repo_root" ]]; then
    container_args+=(-v "$executable_dir:$executable_dir")
fi

# exec により、container が返すプログラムの終了状態をそのまま cargo に渡す。
exec container "${container_args[@]}" \
    --workdir "$(pwd -P)" "$image" "$executable" "$@"
