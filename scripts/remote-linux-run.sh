#!/bin/bash
# x86_64 の Linux 向けの実行ファイルを、ssh で届く別の機械の podman で、手元と同じパスに見せて走らせる
# （実装プラン 00-common「検査」）。Apple の container の Rosetta は openat2 と ADX の命令を持たず、
# Landlock も使えないため、x86_64 のテストは本物の x86_64 の Linux で走らせる。
#
#   remote-linux-run.sh --sync                     リポジトリ（target と .git を除く）と、テストが起動する
#                                                  実行ファイル（target/<対象>/debug の直下）を送る
#   remote-linux-run.sh <実行ファイル> [引数...]    cargo の runner として走らせる
#
# 送り先は環境変数で与える。BENITOITEH_X86_64_HOST（ssh の宛先）と BENITOITEH_X86_64_DIR（送り先の
# 機械の絶対パス。手元の絶対パスをこの下に写す）。
set -euo pipefail

image=docker.io/library/alpine:3.22
target=x86_64-unknown-linux-musl
host=${BENITOITEH_X86_64_HOST:?BENITOITEH_X86_64_HOST is required}
remote_dir=${BENITOITEH_X86_64_DIR:?BENITOITEH_X86_64_DIR is required}
if [[ "$remote_dir" != /* ]]; then
    printf 'BENITOITEH_X86_64_DIR must be an absolute path: %s\n' "$remote_dir" >&2
    exit 2
fi
if (($# == 0)); then
    printf 'Usage: %s --sync | <executable> [arguments...]\n' "$0" >&2
    exit 2
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
# テストの実行ファイルごとに ssh をつなぎ直さないよう、接続を共有する。ソケットのパスには長さの上限がある。
ssh_opts=(-o BatchMode=yes -o ControlMaster=auto -o "ControlPath=/tmp/benitoiteh-ssh-%C"
    -o ControlPersist=120 -o LogLevel=ERROR -o VisualHostKey=no)
rsync_ssh="ssh ${ssh_opts[*]}"

if [[ "$1" == --sync ]]; then
    ssh "${ssh_opts[@]}" "$host" "mkdir -p $(printf '%q' "$remote_dir$repo_root")"
    rsync -a --delete --exclude=/target/ --exclude=/.git/ -e "$rsync_ssh" \
        "$repo_root/" "$host:$remote_dir$repo_root/"
    executables=()
    for file in "${CARGO_TARGET_DIR:-$repo_root/target}/$target/debug"/*; do
        if [[ -f "$file" && -x "$file" ]]; then
            executables+=("$file")
        fi
    done
    if ((${#executables[@]} != 0)); then
        rsync -aR -e "$rsync_ssh" "${executables[@]}" "$host:$remote_dir/"
    fi
    exit 0
fi

executable_dir=$(cd "$(dirname "$1")" && pwd -P)
executable="$executable_dir/$(basename "$1")"
shift

# 実行ファイルは、絶対パスを保って送る。doctest の実行ファイルはリポジトリの外に作られる。
rsync -aR -e "$rsync_ssh" "$executable" "$host:$remote_dir/"
podman_args=(run --rm --interactive -v "$remote_dir$repo_root:$repo_root")
case "$executable_dir/" in
    "$repo_root"/*) ;;
    *) podman_args+=(-v "$remote_dir$executable_dir:$executable_dir") ;;
esac

# exec により、ssh が返すプログラムの終了状態をそのまま cargo に渡す。
command=(podman "${podman_args[@]}" --workdir "$(pwd -P)" "$image" "$executable" "$@")
exec ssh "${ssh_opts[@]}" "$host" "$(printf '%q ' "${command[@]}")"
