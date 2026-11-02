#!/bin/bash
# クラウドの x86_64 の Linux（Claude Code のクラウドのセッションなど、root で動く Ubuntu の使い捨ての環境）で、
# x86_64 の Linux のテストを走らせる準備をする（実装プラン 00-common「検査」）。
#
# 試験に使う x86_64 の機械の podman（scripts/remote-linux-run.sh）の代わりとして、次を用意する。
#   - ホストの bubblewrap と musl-tools（apt）、Rust の musl の対象
#   - Benitoite の処理系 v0.0.12 の x86_64 の musl の静的リンクの実行ファイル（リリースのタグの git archive からビルドする）
#   - alpine:3.22 の rootfs。podman のイメージ（alpine:3.22 + bubblewrap + bash + python3）に近づけるため、
#     ホストの bubblewrap・bash・python3・setpriv とその共有ライブラリを写す。apk の取得先
#     （dl-cdn.alpinelinux.org など）は、この環境の proxy で拒否されたので使わない
#   - 非 root の試験の利用者 bhtest（podman の --userns=keep-id に当たる）
#
# 使い方: sudo なしの root で scripts/cloud-linux-setup.sh を走らせ、最後に示す環境変数を設定して
# cargo test --workspace --target x86_64-unknown-linux-musl を走らせる。runner は scripts/cloud-linux-run.sh。
set -euo pipefail

base=${BENITOITEH_CLOUD_LINUX_DIR:-/opt/benitoiteh-cloud-linux}
processor_tag=v0.0.12
# 設計書 04-01「取り込む文書と版」に記録した v0.0.12 のコミット。
processor_commit=d0678ddb7e82da77715ea2e01ef974d06d41814b
processor_repo=https://github.com/tecogonaz0439/Benitoite.git
# alpine:3.22 の linux/amd64 のマニフェスト（2026-11-02 に Docker Hub と mirror.gcr.io の索引が同じ値を示した。3.22.7）。
# タグは動くので、マニフェストの digest で固定する。
alpine_registry=https://mirror.gcr.io/v2/library/alpine
alpine_manifest=sha256:dc92f017beea8e7f43263073d717f7bc011228c95d5312e440deb4d035bb4eec
test_user=bhtest
target=x86_64-unknown-linux-musl

if [[ $(uname -s) != Linux || $(uname -m) != x86_64 ]]; then
    printf 'This script prepares an x86_64 Linux host only.\n' >&2
    exit 2
fi
if (($(id -u) != 0)); then
    printf 'Run this script as root.\n' >&2
    exit 2
fi
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
mkdir -p "$base"

# 1. ホストの道具
if [[ ! -x /usr/bin/bwrap ]] || ! command -v musl-gcc > /dev/null; then
    apt-get update
    apt-get install -y bubblewrap musl-tools
fi
if [[ -u /usr/bin/bwrap ]]; then
    printf '/usr/bin/bwrap must not be setuid (design 03-02).\n' >&2
    exit 1
fi
toolchain=$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$repo_root/rust-toolchain.toml")
rustup target add --toolchain "$toolchain" "$target"

# 2. 試験の利用者
if ! id "$test_user" > /dev/null 2>&1; then
    useradd -m -s /bin/bash "$test_user"
fi

# 3. 処理系
processor_dir="$base/benitoite-$processor_tag"
processor="$processor_dir/target/$target/release/benitoite"
if [[ ! -x "$processor" ]]; then
    clone="$base/benitoite-clone"
    if [[ ! -d "$clone" ]]; then
        git clone --depth 1 --branch "$processor_tag" "$processor_repo" "$clone"
    fi
    actual=$(git -C "$clone" rev-parse "$processor_tag^{commit}")
    if [[ "$actual" != "$processor_commit" ]]; then
        printf 'Unexpected commit for %s: %s\n' "$processor_tag" "$actual" >&2
        exit 1
    fi
    mkdir -p "$processor_dir"
    git -C "$clone" archive "$processor_tag" | tar -x -C "$processor_dir"
    rustup target add --toolchain "$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$processor_dir/rust-toolchain.toml")" "$target"
    (cd "$processor_dir" && cargo build --release --locked --target "$target")
fi

# 4. alpine:3.22 の rootfs
alpine_dir="$base/alpine"
layer="$alpine_dir/layer.tar.gz"
mkdir -p "$alpine_dir"
if [[ ! -f "$layer" ]]; then
    curl -sSf -H 'Accept: application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json' \
        -o "$alpine_dir/manifest.json" "$alpine_registry/manifests/$alpine_manifest"
    printf '%s  %s\n' "${alpine_manifest#sha256:}" "$alpine_dir/manifest.json" | sha256sum -c --quiet
    layer_digest=$(python3 -I -c 'import json,sys; m=json.load(open(sys.argv[1])); print(m["layers"][0]["digest"])' "$alpine_dir/manifest.json")
    curl -sSfL -o "$layer.partial" "$alpine_registry/blobs/$layer_digest"
    printf '%s  %s\n' "${layer_digest#sha256:}" "$layer.partial" | sha256sum -c --quiet
    mv "$layer.partial" "$layer"
fi
rootfs="$alpine_dir/rootfs"
if [[ ! -d "$rootfs" ]]; then
    mkdir "$rootfs.partial"
    tar -xzf "$layer" -C "$rootfs.partial"
    copy_with_libs() {
        local bin=$1 dest=$2 lib
        install -D -m 0755 "$(readlink -f "$bin")" "$rootfs.partial$dest"
        # glibc の共有ライブラリは /lib/x86_64-linux-gnu と /lib64 に置かれ、musl のローダ（/lib/ld-musl-*）と重ならない。
        while read -r lib; do
            [[ -n "$lib" && -e "$lib" ]] || continue
            install -D -m 0755 "$(readlink -f "$lib")" "$rootfs.partial$lib"
        done < <(ldd "$bin" | awk '/=>/ {print $3} /^\t\/lib64/ {print $1}')
    }
    copy_with_libs /usr/bin/bwrap /usr/bin/bwrap
    copy_with_libs /bin/bash /bin/bash
    copy_with_libs /usr/bin/setpriv /usr/bin/setpriv
    python=$(readlink -f /usr/bin/python3)
    copy_with_libs "$python" /usr/bin/python3
    mkdir -p "$rootfs.partial/usr/lib"
    cp -a "/usr/lib/$(basename "$python")" "$rootfs.partial/usr/lib/"
    uid=$(id -u "$test_user")
    gid=$(id -g "$test_user")
    printf '%s:x:%s:\n' "$test_user" "$gid" >> "$rootfs.partial/etc/group"
    printf '%s:x:%s:%s:%s:/home/%s:/bin/sh\n' "$test_user" "$uid" "$gid" "$test_user" "$test_user" >> "$rootfs.partial/etc/passwd"
    install -d -o "$uid" -g "$gid" "$rootfs.partial/home/$test_user"
    mkdir -p "$rootfs.partial/.oldroot"
    chmod 1777 "$rootfs.partial/tmp"
    mv "$rootfs.partial" "$rootfs"
fi

printf '\nalpine: %s\n' "$(cat "$rootfs/etc/alpine-release")"
printf 'bubblewrap: %s\n' "$(/usr/bin/bwrap --version)"
printf 'processor: %s\n' "$processor"
printf '\nexport BENITOITEH_CLOUD_LINUX_DIR=%s\n' "$base"
printf 'export BENITOITEH_TEST_BENITOITE=%s\n' "$processor"
printf 'export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUNNER=%s/scripts/cloud-linux-run.sh\n' "$repo_root"
