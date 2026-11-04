#!/bin/bash
# x86_64 の Linux 向けのテストの実行ファイルを、scripts/cloud-linux-setup.sh が用意した alpine:3.22 の rootfs で、
# 手元と同じパスに見せ、非 root で走らせる cargo の runner（実装プラン 00-common「検査」）。
# scripts/remote-linux-run.sh の podman（--userns=keep-id、--security-opt unmask=ALL）の形に倣う。
#
# chroot の中では非特権の user namespace を作れず（unshare(2) の EPERM の条件）、bubblewrap が動かないので、
# 新しいマウントの名前空間で pivot_root する。PID の名前空間も分け、/proc には試験のプロセスだけを見せる。
# 能力（capability）は setpriv で uid を変えるときに落ち、サンドボックスの補助の能力の検査の前提を満たす。
set -euo pipefail

base=${BENITOITEH_CLOUD_LINUX_DIR:-/opt/benitoiteh-cloud-linux}
rootfs="$base/alpine/rootfs"
test_user=bhtest
if (($# == 0)); then
    printf 'Usage: %s <executable> [arguments...]\n' "$0" >&2
    exit 2
fi
if [[ ! -d "$rootfs" ]]; then
    printf 'Missing %s; run scripts/cloud-linux-setup.sh first.\n' "$rootfs" >&2
    exit 2
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
executable_dir=$(cd "$(dirname "$1")" && pwd -P)
executable="$executable_dir/$(basename "$1")"
shift
binds=("$repo_root")
if [[ -n "${BENITOITEH_TEST_BENITOITE:-}" ]]; then
    binds+=("$(cd "$(dirname "$BENITOITEH_TEST_BENITOITE")" && pwd -P)")
fi
case "$executable_dir/" in
    "$repo_root"/*) ;;
    *) binds+=("$executable_dir") ;;
esac

exec unshare --mount --pid --fork --propagation private -- /bin/bash -c '
set -euo pipefail
rootfs=$1; user=$2; uid=$3; gid=$4; cwd=$5; nbinds=$6; shift 6
mount --bind "$rootfs" "$rootfs"
for ((i = 0; i < nbinds; i++)); do
    mkdir -p "$rootfs$1"
    mount --rbind "$1" "$rootfs$1"
    shift
done
mount -t proc proc "$rootfs/proc"
mount --rbind /dev "$rootfs/dev"
mount -t sysfs sysfs "$rootfs/sys"
mount -t tmpfs -o mode=1777 tmpfs "$rootfs/tmp"
cd "$rootfs"
pivot_root . .oldroot
umount -l /.oldroot
exec /usr/bin/setpriv --reuid="$uid" --regid="$gid" --clear-groups -- /usr/bin/env -i \
    HOME="/home/$user" USER="$user" LOGNAME="$user" \
    PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    ${BENITOITEH_TEST_BENITOITE:+BENITOITEH_TEST_BENITOITE=$BENITOITEH_TEST_BENITOITE} \
    ${RUST_BACKTRACE:+RUST_BACKTRACE=$RUST_BACKTRACE} \
    /bin/sh -c "cd \"\$0\" && exec \"\$@\"" "$cwd" "$@"
' _ "$rootfs" "$test_user" "$(id -u "$test_user")" "$(id -g "$test_user")" "$(pwd -P)" "${#binds[@]}" "${binds[@]}" "$executable" "$@"
