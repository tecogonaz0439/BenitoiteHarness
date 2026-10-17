//! 実のパスで実行ファイルの形とハッシュを確かめる（設計書 03-02「制限モードで実行を許すプログラム」）。

use super::{BenitoiteError, text};
use crate::config::writable::WritableLocations;
use rustix::fs::{Mode, OFlags, open};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Seek};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// 設定を優先し、相対の PATH とプロジェクト内の PATH を除いて処理系を探す。
pub fn discover(
    configured: Option<&Path>,
    path: Option<&OsStr>,
    locations: &WritableLocations,
) -> Result<PathBuf, BenitoiteError> {
    if let Some(path) = configured {
        if !path.is_absolute() {
            return Err(BenitoiteError::Invalid(text::ABSOLUTE));
        }
        return fs::canonicalize(path).map_err(Into::into);
    }
    for directory in path.into_iter().flat_map(std::env::split_paths) {
        if !directory.is_absolute() {
            continue;
        }
        let Ok(directory) = fs::canonicalize(directory) else {
            continue;
        };
        if directory.starts_with(&locations.project) {
            continue;
        }
        match fs::canonicalize(directory.join("benitoite")) {
            Ok(path) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(BenitoiteError::Invalid(text::NOT_FOUND))
}

/// 形と SHA-256 は同じ記述子から読む。名前の禁止は許可プログラムにだけ適用する。
pub fn validate_executable(
    path: &Path,
    locations: &WritableLocations,
    allowed_program: bool,
) -> Result<(PathBuf, String), BenitoiteError> {
    let path = fs::canonicalize(path)?;
    if path.starts_with(&locations.project)
        || path.starts_with(&locations.temporary)
        || locations
            .additional
            .iter()
            .any(|root| path.starts_with(root))
    {
        return Err(BenitoiteError::Invalid(text::INVALID_PROGRAM));
    }
    if allowed_program && path.file_name().is_some_and(banned) {
        return Err(BenitoiteError::Invalid(text::BANNED_PROGRAM));
    }
    // FIFO の相手を待たず、正規化後に差し込まれた末尾のリンクを辿らない。
    // 読む記述子と実のパスの同一性をここで揃える（設計書 04-01「処理系の見つけ方と版の確かめ」）。
    let mut file = File::from(
        open(
            &path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?,
    );
    let opened = file.metadata()?;
    let named = fs::metadata(&path)?;
    if !opened.is_file() || (opened.dev(), opened.ino()) != (named.dev(), named.ino()) {
        return Err(BenitoiteError::Invalid(text::INVALID_PROGRAM));
    }
    let mut header = Vec::new();
    file.by_ref().take(65536).read_to_end(&mut header)?;
    if !native(&header) {
        return Err(BenitoiteError::Invalid(text::INVALID_PROGRAM));
    }
    file.rewind()?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(buffer.get(..count).unwrap_or_default());
    }
    let hash = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((path, hash))
}

fn banned(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    [
        "python", "perl", "ruby", "lua", "php", "ld-linux", "ld-musl", "tclsh", "wish", "expect",
        "vim", "emacs", "lldb", "ksh",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || [
            "sh",
            "bash",
            "zsh",
            "dash",
            "csh",
            "tcsh",
            "fish",
            "busybox",
            "toybox",
            "node",
            "nodejs",
            "bsd-csh",
            "deno",
            "bun",
            "luajit",
            "Rscript",
            "julia",
            "osascript",
            "awk",
            "gawk",
            "mawk",
            "nawk",
            "vi",
            "nvim",
            "gdb",
            "env",
            "xargs",
            "nohup",
            "sudo",
            "doas",
            "open",
            "launchctl",
            "systemd-run",
            "docker",
            "podman",
            "tmux",
            "screen",
            "ssh",
        ]
        .contains(&name.as_ref())
}

fn native(bytes: &[u8]) -> bool {
    #[cfg(target_os = "macos")]
    {
        matches!(
            bytes.get(..4),
            Some(
                [0xfe, 0xed, 0xfa, 0xce]
                    | [0xce, 0xfa, 0xed, 0xfe]
                    | [0xfe, 0xed, 0xfa, 0xcf]
                    | [0xcf, 0xfa, 0xed, 0xfe]
                    | [0xca, 0xfe, 0xba, 0xbe]
                    | [0xbe, 0xba, 0xfe, 0xca]
                    | [0xca, 0xfe, 0xba, 0xbf]
                    | [0xbf, 0xba, 0xfe, 0xca]
            )
        )
    }
    #[cfg(target_os = "linux")]
    {
        static_elf(bytes).unwrap_or(false)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = bytes;
        false
    }
}

// 固定の読み取り範囲に全ヘッダが収まるときだけ受ける。PT_DYNAMIC を持つ static-pie も許す。
#[cfg(any(target_os = "linux", test))]
fn static_elf(bytes: &[u8]) -> Option<bool> {
    if bytes.get(..4)? != b"\x7fELF" {
        return Some(false);
    }
    // 対応する対象は little-endian の ELF64 に限る。別の対象のバイナリを
    // 静的リンクという条件だけで受けない（実装プラン P2-05 の点検の指示）。
    if bytes.get(4) != Some(&2) || bytes.get(5) != Some(&1) {
        return Some(false);
    }
    let number = |start: usize, width: usize| -> Option<u64> {
        let slice = bytes.get(start..start.checked_add(width)?)?;
        Some(
            slice
                .iter()
                .rev()
                .fold(0u64, |n, b| (n << 8) | u64::from(*b)),
        )
    };
    let machine = if cfg!(target_arch = "aarch64") {
        183
    } else if cfg!(target_arch = "x86_64") {
        62
    } else {
        return Some(false);
    };
    if !matches!(number(16, 2)?, 2 | 3) || number(18, 2)? != machine {
        return Some(false);
    }
    let (offset, size, count) = (number(32, 8)?, number(54, 2)?, number(56, 2)?);
    if size != 56 || count == 0 || count == 65535 {
        return Some(false);
    }
    let end = offset.checked_add(size.checked_mul(count)?)?;
    if end > u64::try_from(bytes.len()).ok()? {
        return Some(false);
    }
    for index in 0..count {
        let start = usize::try_from(offset.checked_add(size.checked_mul(index)?)?).ok()?;
        if number(start, 4)? == 3 {
            return Some(false);
        }
    }
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::static_elf;
    use std::error::Error;
    #[test]
    fn elf_static_pie_is_accepted_but_interpreters_and_out_of_range_headers_are_rejected()
    -> Result<(), Box<dyn Error>> {
        let mut elf = vec![0u8; 120];
        // ELF64 little-endian、ET_DYN、プログラムヘッダ一つ、PT_DYNAMIC。
        for (offset, bytes) in [
            (0usize, b"\x7fELF\x02\x01".as_slice()),
            (16, &[3]),
            (
                18,
                &u16::to_le_bytes(if cfg!(target_arch = "aarch64") {
                    183
                } else {
                    62
                }),
            ),
            (32, &[64]),
            (54, &[56]),
            (56, &[1]),
            (64, &[2]),
        ] {
            elf.get_mut(offset..offset.saturating_add(bytes.len()))
                .ok_or("fixture range")?
                .copy_from_slice(bytes);
        }
        assert_eq!(static_elf(&elf), Some(true));
        let mut exec = elf.clone();
        *exec.get_mut(16).ok_or("fixture")? = 2;
        assert_eq!(static_elf(&exec), Some(true));
        // 別の幅・バイト順のヘッダも、その形式自体は正しく作る。
        // 別の欄の不整合による拒否で試験を通さない。
        let machine: u16 = if cfg!(target_arch = "aarch64") {
            183
        } else {
            62
        };
        let mut elf32 = vec![0u8; 84];
        for (offset, bytes) in [
            (0usize, b"\x7fELF\x01\x01".as_slice()),
            (16, &[3]),
            (18, &machine.to_le_bytes()),
            (28, &[52]),
            (42, &[32]),
            (44, &[1]),
            (52, &[2]),
        ] {
            elf32
                .get_mut(offset..offset.saturating_add(bytes.len()))
                .ok_or("fixture")?
                .copy_from_slice(bytes);
        }
        assert_eq!(static_elf(&elf32), Some(false));
        let mut big_endian = vec![0u8; 120];
        for (offset, bytes) in [
            (0usize, b"\x7fELF\x02\x02".as_slice()),
            (16, &[0, 3]),
            (18, &machine.to_be_bytes()),
            (32, &64u64.to_be_bytes()),
            (54, &[0, 56]),
            (56, &[0, 1]),
            (64, &[0, 0, 0, 2]),
        ] {
            big_endian
                .get_mut(offset..offset.saturating_add(bytes.len()))
                .ok_or("fixture")?
                .copy_from_slice(bytes);
        }
        assert_eq!(static_elf(&big_endian), Some(false));
        for (offset, value) in [(4, 1), (5, 2), (16, 1), (18, 0), (54, 32), (54, 57)] {
            let mut invalid = elf.clone();
            *invalid.get_mut(offset).ok_or("fixture")? = value;
            assert_eq!(
                static_elf(&invalid),
                Some(false),
                "offset {offset}, value {value}"
            );
        }
        *elf.get_mut(64).ok_or("fixture")? = 3;
        assert_eq!(static_elf(&elf), Some(false));
        *elf.get_mut(32).ok_or("fixture")? = 120;
        assert_eq!(static_elf(&elf), Some(false));
        assert_eq!(static_elf(b"\xca\xfe\xba\xbe"), Some(false));
        Ok(())
    }
}
