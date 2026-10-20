//! 一時ディレクトリの名前、墓標の名前、再開のときに残った一時ディレクトリの扱い
//! （設計書 01-05「一時ディレクトリ」「記録を残す期間」）。

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use rustix::fs::{AtFlags, FileType, Mode, OFlags, mkdirat, open, renameat, statat};

use super::{SessionError, SessionId, remove, text};

const PREFIX: &str = "benitoiteh-";
const REMOVING: &str = "removing-";
/// 名前を替えられない一時ディレクトリの代わりに使う名前の数（`.1`〜`.9`）。
const ALTERNATES: u8 = 9;

/// 一時の根の項目の種類。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    /// `benitoiteh-<ID>`。
    Primary,
    /// `benitoiteh-<ID>.<n>`（`n` は 1〜9）。
    Alternate,
    /// `benitoiteh-<ID>.removing-<16 桁の 16 進>`。
    Tombstone,
}

/// 一時の根の項目の名前から ID と種類を読む。ほかの名前は掃除の対象にしない。
pub(super) fn parse(name: &str) -> Option<(SessionId, Kind)> {
    let rest = name.strip_prefix(PREFIX)?;
    let (id, kind) = match rest.split_once('.') {
        None => (rest, Kind::Primary),
        Some((id, suffix)) => {
            let kind = if let Some(nonce) = suffix.strip_prefix(REMOVING) {
                is_nonce(nonce).then_some(Kind::Tombstone)?
            } else if matches!(suffix.as_bytes(), [b'1'..=b'9']) {
                Kind::Alternate
            } else {
                return None;
            };
            (id, kind)
        }
    };
    Some((id.parse().ok()?, kind))
}

/// `sessions/.removing-<ID>-<16 桁の 16 進>` の名前から ID を読む。
pub(super) fn parse_session_tombstone(name: &str) -> Option<SessionId> {
    let (id, nonce) = name
        .strip_prefix('.')?
        .strip_prefix(REMOVING)?
        .rsplit_once('-')?;
    if !is_nonce(nonce) {
        return None;
    }
    id.parse().ok()
}

/// 期間を過ぎたセッションのディレクトリを替える墓標の名前。
pub(super) fn session_tombstone(id: &SessionId) -> Result<String, SessionError> {
    Ok(format!(".{REMOVING}{id}-{}", nonce()?))
}

fn is_nonce(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn nonce() -> Result<String, SessionError> {
    let mut random = [0; 8];
    getrandom::fill(&mut random).map_err(SessionError::Random)?;
    Ok(format!("{:016x}", u64::from_be_bytes(random)))
}

/// 一時の根を開く。根は /tmp などの別名でもよく、その下は NOFOLLOW で守る。
pub(super) fn open_root(path: &Path) -> io::Result<File> {
    Ok(File::from(open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?))
}

/// 一時ディレクトリを墓標の名前に替えて消した結果。
pub(super) enum Discard {
    /// 項目がなかった。
    Absent,
    /// 消した。
    Removed,
    /// 名前を替えられなかった（macOS の `uchg`、削除を拒む ACL など）。
    Pinned(io::Error),
    /// 墓標の名前に替えたが、消せなかった。
    Failed { tombstone: String, error: io::Error },
}

/// リンクでなく、自分が持ち主のディレクトリか。`None` は項目がない。
fn owned_directory(root: &File, name: &str) -> Result<Option<bool>, SessionError> {
    let stat = match statat(root, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(Some(
        FileType::from_raw_mode(stat.st_mode) == FileType::Directory
            && stat.st_uid == rustix::process::geteuid().as_raw(),
    ))
}

/// 自分の持ち物のディレクトリを、権限によらず墓標の名前に替えてから消す。
/// 持ち主が違うものとリンクは触らずに誤りとする（設計書 01-05「一時ディレクトリ」）。
pub(super) fn discard(root: &File, name: &str, id: &SessionId) -> Result<Discard, SessionError> {
    match owned_directory(root, name)? {
        None => return Ok(Discard::Absent),
        Some(false) => return Err(SessionError::Invalid(text::UNSAFE_PATH)),
        Some(true) => {}
    }
    let tombstone = format!("{PREFIX}{id}.{REMOVING}{}", nonce()?);
    match renameat(root, name, root, tombstone.as_str()) {
        Ok(()) => {}
        Err(rustix::io::Errno::NOENT) => return Ok(Discard::Absent),
        // ENOENT のほかの誤り（uchg の EPERM、ACL の EACCES など）は、代わりの名前へ
        // 進めるよう、替えられないものとして扱う（設計書 01-05「一時ディレクトリ」）。
        Err(error) => return Ok(Discard::Pinned(error.into())),
    }
    Ok(
        match remove::directory_at(root, OsStr::new(&tombstone), remove::Target::Temporary) {
            Ok(()) => Discard::Removed,
            Err(error) => Discard::Failed { tombstone, error },
        },
    )
}

/// このセッションの一時の根の項目（墓標を含む）を消し、消した数を返す。
/// 期間を過ぎたセッションを錠を持ったまま消す前に使う（設計書 01-05「記録を残す期間」）。
pub(super) fn remove_all(
    root: &File,
    root_path: &Path,
    id: &SessionId,
) -> Result<u64, SessionError> {
    let mut removed = 0_u64;
    for entry in std::fs::read_dir(root_path)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some((entry_id, kind)) = name.to_str().and_then(parse) else {
            continue;
        };
        if &entry_id != id {
            continue;
        }
        let Some(name) = name.to_str() else {
            continue;
        };
        if kind == Kind::Tombstone {
            // discard と同じく、リンクと持ち主の違うものには触らない。
            if owned_directory(root, name)? != Some(true) {
                continue;
            }
            remove::directory_at(root, OsStr::new(name), remove::Target::Temporary)?;
        } else {
            match discard(root, name, id)? {
                Discard::Absent => continue,
                Discard::Removed => {}
                Discard::Pinned(error) | Discard::Failed { error, .. } => {
                    return Err(error.into());
                }
            }
        }
        removed = removed.saturating_add(1);
    }
    Ok(removed)
}

/// 再開のときに残った一時ディレクトリを消し、新しい一時ディレクトリを 0700 で作る。
/// 名前を替えられなければ `.1`〜`.9` の代わりの名前を使い、知らせを返す。
pub(super) fn prepare(
    root_path: &Path,
    id: &SessionId,
) -> Result<(PathBuf, Vec<String>), SessionError> {
    let root = open_root(root_path)?;
    let primary = format!("{PREFIX}{id}");
    let mut notices = Vec::new();
    let names =
        std::iter::once(primary.clone()).chain((1..=ALTERNATES).map(|n| format!("{primary}.{n}")));
    for name in names {
        match discard(&root, &name, id)? {
            Discard::Absent | Discard::Removed => {}
            Discard::Pinned(error) => {
                notices.push(text::temporary_pinned(
                    &root_path.join(&name),
                    &error.to_string(),
                ));
                continue;
            }
            Discard::Failed { tombstone, error } => notices.push(text::temporary_not_removed(
                &root_path.join(tombstone),
                &error.to_string(),
            )),
        }
        mkdirat(&root, name.as_str(), Mode::RWXU)?;
        return Ok((root_path.join(name), notices));
    }
    Err(SessionError::Invalid(text::UNSAFE_PATH))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 関門: 掃除で消す名前を三つに限る。ほかの名前（似た接尾辞を含む）を消す退行と、
    // 代わりの名前・墓標を読み落として残し続ける退行を、名前の表で捕まえる。
    #[test]
    fn temporary_and_tombstone_names_are_parsed_strictly() {
        let id = "20200101T000000Z-0000000000000001";
        for (name, expected) in [
            (format!("benitoiteh-{id}"), Some(Kind::Primary)),
            (format!("benitoiteh-{id}.1"), Some(Kind::Alternate)),
            (format!("benitoiteh-{id}.9"), Some(Kind::Alternate)),
            (
                format!("benitoiteh-{id}.removing-0123456789abcdef"),
                Some(Kind::Tombstone),
            ),
            (format!("benitoiteh-{id}.0"), None),
            (format!("benitoiteh-{id}.10"), None),
            (format!("benitoiteh-{id}.removing-0123456789ABCDEF"), None),
            (format!("benitoiteh-{id}.removing-0123"), None),
            (format!("benitoiteh-{id}.tmp"), None),
            (format!("bh-{id}"), None),
            ("benitoiteh-unrelated".to_owned(), None),
        ] {
            assert_eq!(parse(&name).map(|(_, kind)| kind), expected, "{name}");
        }
        assert_eq!(
            parse_session_tombstone(&format!(".removing-{id}-0123456789abcdef"))
                .as_ref()
                .map(SessionId::as_str),
            Some(id)
        );
        for name in [
            id.to_owned(),
            format!(".removing-{id}"),
            format!(".removing-{id}-xyz"),
            format!("removing-{id}-0123456789abcdef"),
        ] {
            assert!(parse_session_tombstone(&name).is_none(), "{name}");
        }
    }
}
