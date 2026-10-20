#!/usr/bin/env bash
# ソースのアーカイブ、private/ の zip、ブランチ・タグの git bundle を work/ に作る。
#
# 使い方:
#   archive.sh archive [<コミット>]
#       コミット（既定は HEAD）から work/BenitoiteHarness-<タグ>-<ブランチ>-<短いコミット>.tar.gz と .zip を作る。
#       タグはコミットに付いたタグ、なければ直近のタグ。タグがなければ名前のタグの部分を省く。
#       短いコミットは 8 文字。中の最上位のディレクトリは、タグの付いたコミットなら
#       BenitoiteHarness-<版>/（タグの v を除く）、ほかは BenitoiteHarness-<短いコミット>/。
#   archive.sh private
#       private/ 全体を work/BenitoiteHarness-private-<ブランチ>-<短いコミット>-<日付>.zip に保存する。
#       リンクは辿らずリンクのまま保存し、元のファイルとリンクの数・名前を zip と照合する。
#   archive.sh bundle <名前> <参照>... [--delete]
#       参照（ブランチ名・タグ名・refs/... の形）を work/BenitoiteHarness-<名前>-<日付>.bundle に保存し、確かめる。
#       --delete を付けると、確かめた後に、保存した手元のブランチとタグを消す（今のブランチ、gamou、
#       リリースのタグ v<数字>... は消さない）。
#   archive.sh bundle <名前> --all-except-main [--delete]
#       gamou とリリースのタグ（v<数字>...）以外のすべての手元のブランチとタグを保存する。
#
# 既にある出力のファイルは上書きしない。リモートには何も送らない。macOS の bash 3.2 で動く。
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
out="$root/work"
main_branch="gamou"

die() { echo "$*" >&2; exit 2; }

refuse_existing() {
    [ ! -e "$1" ] && [ ! -L "$1" ] || die "already exists, not overwritten: $1"
}

short_commit() {
    local full
    full="$(git -C "$root" rev-parse --verify "$1^{commit}")" || die "not a commit: $1"
    # --short=8 は衝突時に長くなるため、指定どおり 8 文字に固定する。
    printf '%.8s' "$full"
}

cmd_archive() (
    [ "$#" -le 1 ] || die "archive accepts at most one commit"
    local commit="${1:-HEAD}" short tag exact branch prefix base
    short="$(short_commit "$commit")"
    if exact="$(git -C "$root" describe --tags --exact-match "$commit" 2>/dev/null)"; then
        tag="$exact"
        prefix="BenitoiteHarness-${tag#v}/"
    else
        tag="$(git -C "$root" describe --tags --abbrev=0 "$commit" 2>/dev/null)" || tag=""
        prefix="BenitoiteHarness-$short/"
    fi
    # ブランチは、公開するブランチがコミットを含むならその名前、次に今のブランチ、どちらでもなければコミットを含むブランチの最初のもの。
    # 作業用のブランチ（`mod/V2` など）の上で作っても、リリースのコミットには公開するブランチの名前を付けるためである。
    # 名前の `/` はファイル名に使えないので `-` に替える。
    if git -C "$root" merge-base --is-ancestor "$commit" "$main_branch" 2>/dev/null; then
        branch="$main_branch"
    else
        branch="$(git -C "$root" branch --show-current)"
    fi
    if [ -z "$branch" ] || ! git -C "$root" merge-base --is-ancestor "$commit" "$branch" 2>/dev/null; then
        branch="$(git -C "$root" branch --contains "$commit" --format='%(refname:short)' | head -1)"
    fi
    [ -n "$branch" ] || die "no branch contains $commit"
    branch="${branch//\//-}"
    tag="${tag//\//-}"
    base="$out/BenitoiteHarness-${tag:+$tag-}$branch-$short"
    mkdir -p "$out"
    refuse_existing "$base.tar.gz"
    refuse_existing "$base.zip"
    refuse_existing "$base.tar.gz.partial"
    refuse_existing "$base.zip.partial"
    local tar_published=0 zip_published=0
    # 照合前の出力を完成品と取り違えないようにし、途中の失敗では今回作ったものだけを消す。
    trap '
        status=$?
        rm -f -- "$base.tar.gz.partial" "$base.zip.partial"
        if [ "$status" -ne 0 ]; then
            [ "$tar_published" -eq 0 ] || rm -f -- "$base.tar.gz"
            [ "$zip_published" -eq 0 ] || rm -f -- "$base.zip"
        fi
        exit "$status"
    ' EXIT
    git -C "$root" archive --format=tar.gz --prefix="$prefix" -o "$base.tar.gz.partial" "$commit"
    git -C "$root" archive --format=zip --prefix="$prefix" -o "$base.zip.partial" "$commit"
    # 中のファイルの数が、コミットの木のファイルの数と一致することを確かめる。
    local expected tar_count zip_count
    expected="$(git -C "$root" ls-tree -r --name-only "$commit" | wc -l | tr -d ' ')"
    tar_count="$(tar tzf "$base.tar.gz.partial" | awk '!/\/$/ { n++ } END { print n+0 }')"
    zip_count="$(unzip -Z1 "$base.zip.partial" | awk '!/\/$/ { n++ } END { print n+0 }')"
    echo "== archive of $commit ($short), tag ${tag:-none}, branch $branch, prefix $prefix"
    echo "   files: tree $expected, tar.gz $tar_count, zip $zip_count"
    [ "$expected" = "$tar_count" ] && [ "$expected" = "$zip_count" ] || die "file counts differ"
    refuse_existing "$base.tar.gz"
    refuse_existing "$base.zip"
    mv "$base.tar.gz.partial" "$base.tar.gz"
    tar_published=1
    mv "$base.zip.partial" "$base.zip"
    zip_published=1
    ls -l "$base.tar.gz" "$base.zip"
)

cmd_private() (
    [ "$#" -eq 0 ] || die "private accepts no arguments"
    [ -d "$root/private" ] && [ ! -L "$root/private" ] || die "private/ must be a directory, not a symlink"
    git -C "$root" check-ignore -q private/ || die "private/ must be ignored by git"
    # 内容を読まずに、秘密の値を置きやすい名前を断る。リンクの参照先は辿らない。
    local forbidden
    forbidden="$(find "$root/private" \( -name .env -o -name '*.pem' -o -name settings.local.json \) -print)"
    [ -z "$forbidden" ] || die "private/ contains a forbidden name: .env, *.pem or settings.local.json"
    local branch short file
    branch="$(git -C "$root" branch --show-current)"
    [ -n "$branch" ] || die "private needs a checked-out branch"
    branch="${branch//\//-}"
    short="$(short_commit HEAD)"
    file="$out/BenitoiteHarness-private-$branch-$short-$(date +%Y-%m-%d).zip"
    mkdir -p "$out"
    refuse_existing "$file"
    refuse_existing "$file.partial"
    local published=0
    trap '
        status=$?
        rm -f -- "$file.partial"
        if [ "$status" -ne 0 ] && [ "$published" -eq 1 ]; then
            rm -f -- "$file"
        fi
        exit "$status"
    ' EXIT
    # private/ だけを指定し、隠しファイルと空のディレクトリも含める。-y で外へのリンクも辿らない。
    # 環境の ZIP と ZIPOPT でリンクの扱いや対象が変わらないよう、両方を取り除く。
    (cd "$root" && env -u ZIP -u ZIPOPT zip -q -r -y -MM "$file.partial" private)
    # 行単位の一覧では、改行を含む名前の件数を誤るため、zip の項目を直接読む。
    python3 - "$root" "$file.partial" <<'PY'
import os
import stat
import sys
import zipfile

root, archive = sys.argv[1:]
expected = {}

def collect(path):
    mode = os.lstat(os.path.join(root, path)).st_mode
    if stat.S_ISDIR(mode):
        with os.scandir(os.path.join(root, path)) as entries:
            for entry in entries:
                collect(path + '/' + entry.name)
    elif stat.S_ISREG(mode) or stat.S_ISLNK(mode):
        expected[path] = stat.S_ISLNK(mode)
    else:
        raise SystemExit('unsupported file type in private/: ' + path)

collect('private')
with zipfile.ZipFile(archive) as saved:
    if saved.testzip() is not None:
        raise SystemExit('private zip integrity check failed')
    files = [entry for entry in saved.infolist() if not entry.is_dir()]
    # macOS 標準 zip は UTF-8 の印を付けないため、その場合は元のファイル名のバイト列を復元する。
    actual = {
        (entry.filename if entry.flag_bits & 0x800 else os.fsdecode(entry.filename.encode('cp437'))):
        stat.S_ISLNK(entry.external_attr >> 16)
        for entry in files
    }
    print('   files (including symlinks): source %d, zip %d' % (len(expected), len(files)))
    if len(expected) != len(files) or expected != actual:
        raise SystemExit('private zip file counts, names or symlink types differ')
PY
    refuse_existing "$file"
    mv "$file.partial" "$file"
    published=1
    echo "== private archive of working tree (HEAD $short), branch $branch"
    ls -l "$file"
)

cmd_bundle() {
    local name="${1:-}"
    [ -n "$name" ] || die "bundle needs a name"
    case "$name" in */*|.|..) die "bundle name must be a filename component" ;; esac
    shift
    local delete=0 refs="" r
    while [ $# -gt 0 ]; do
        case "$1" in
            --delete) delete=1 ;;
            --all-except-main)
                refs="$refs $(git -C "$root" for-each-ref --format='%(refname)' refs/heads refs/tags | awk -v main="refs/heads/$main_branch" '$0 != main && !/^refs\/tags\/v[0-9]/' | tr '\n' ' ')" ;;
            *)
                # 短い名前を、ブランチかタグの完全な名前にする。
                if git -C "$root" show-ref --verify -q "refs/heads/$1"; then refs="$refs refs/heads/$1"
                elif git -C "$root" show-ref --verify -q "refs/tags/$1"; then refs="$refs refs/tags/$1"
                elif git -C "$root" show-ref --verify -q "$1"; then refs="$refs $1"
                else die "no such branch or tag: $1"
                fi ;;
        esac
        shift
    done
    [ -n "${refs// /}" ] || die "no refs to bundle"
    local file="$out/BenitoiteHarness-$name-$(date +%Y-%m-%d).bundle"
    mkdir -p "$out"
    refuse_existing "$file"
    # shellcheck disable=SC2086
    git -C "$root" bundle create "$file" $refs
    git -C "$root" bundle verify "$file" >/dev/null
    echo "== bundle $file"
    ls -l "$file"
    # 保存した参照ごとに、bundle の中の値が手元の値と一致することを確かめる。
    local heads missing=0
    heads="$(git -C "$root" bundle list-heads "$file")"
    for r in $refs; do
        if printf '%s\n' "$heads" | grep -qx "$(git -C "$root" rev-parse "$r") $r"; then
            echo "   saved $r"
        else
            echo "   NOT saved $r" >&2
            missing=1
        fi
    done
    [ "$missing" -eq 0 ] || die "some refs are not in the bundle; nothing deleted"
    echo "   restore: git fetch $file '<ref>:<ref>'  (e.g. 'refs/heads/NAME:refs/heads/NAME')"
    if [ "$delete" -eq 1 ]; then
        local current
        current="$(git -C "$root" branch --show-current)"
        for r in $refs; do
            case "$r" in
                "refs/heads/$main_branch"|"refs/heads/$current")
                    echo "   kept $r (public or current branch)" ;;
                refs/heads/*)
                    # 作業ディレクトリが使っているブランチは消せない（git が拒む）ので、その旨を示して残す。
                    git -C "$root" branch -D "${r#refs/heads/}" >/dev/null 2>&1 && echo "   deleted $r" || echo "   kept $r (in use by a worktree?)" ;;
                refs/tags/v[0-9]*)
                    echo "   kept $r (release tag)" ;;
                refs/tags/*)
                    git -C "$root" tag -d "${r#refs/tags/}" >/dev/null && echo "   deleted $r" ;;
                *) echo "   kept $r" ;;
            esac
        done
    fi
}

case "${1:-}" in
    archive) shift; cmd_archive "$@" ;;
    private) shift; cmd_private "$@" ;;
    bundle) shift; cmd_bundle "$@" ;;
    -h|--help|"") sed -n '2,/^set -/p' "$0" | sed '$d' ;;
    *) die "unknown command: $1 (archive, private, bundle)" ;;
esac
