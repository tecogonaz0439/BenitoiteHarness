//! リンクを辿らず、呼び出しのスタックを深さに比例させない削除（設計書 01-05「一時ディレクトリ」）。

use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io;
use std::os::fd::AsFd;
use std::path::Path;

use rustix::fs::{AtFlags, Dir, Mode, OFlags, open, openat, unlinkat};

const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

pub(crate) fn directory(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    // 親はハーネスが管理する根であり、/tmp などの別名も辿る。削除対象とその下は
    // openat の NOFOLLOW で守る（設計書 01-05「一時ディレクトリ」）。
    let parent = File::from(open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    directory_at(&parent, name, false)
}

pub(super) fn directory_at(parent: &File, name: &OsStr, session: bool) -> io::Result<()> {
    let mut removal = Removal::new(parent, name, session)?;
    while removal.advance()? {}
    Ok(())
}

enum Step {
    Visit(CString),
    Unlink(CString),
    Finish,
}

struct Removal<'a> {
    parent: &'a File,
    name: &'a OsStr,
    session: bool,
    stack: Vec<(Dir, CString)>,
    pending: Option<Step>,
}

impl<'a> Removal<'a> {
    fn new(parent: &'a File, name: &'a OsStr, session: bool) -> io::Result<Self> {
        let stack = match openat(parent, name, DIRECTORY, Mode::empty()) {
            Ok(root) => vec![(Dir::new(root)?, CString::default())],
            Err(rustix::io::Errno::NOENT) => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            parent,
            name,
            session,
            stack,
            pending: None,
        })
    }

    fn advance(&mut self) -> io::Result<bool> {
        let Some((entries, _)) = self.stack.last_mut() else {
            return Ok(false);
        };
        // 列挙・openat・unlinkat の間で別の掃除が進みうる。各操作を分け、前の操作で
        // 存在した項目も次には消えているものとして扱う（設計書 01-05「記録を残す期間」）。
        match self.pending.take() {
            None => {
                if let Some(entry) = entries.read() {
                    let entry = entry?;
                    let child = entry.file_name();
                    if !matches!(child.to_bytes(), b"." | b"..")
                        && !(self.session
                            && self.stack.len() == 1
                            && matches!(child.to_bytes(), b"lock" | b"session.jsonl"))
                    {
                        self.pending = Some(Step::Visit(child.to_owned()));
                    }
                } else {
                    self.pending = Some(Step::Finish);
                }
            }
            Some(Step::Visit(child)) => {
                match openat(entries.fd()?, child.as_c_str(), DIRECTORY, Mode::empty()) {
                    Ok(fd) => self.stack.push((Dir::new(fd)?, child)),
                    Err(rustix::io::Errno::NOENT) => {}
                    Err(rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => {
                        self.pending = Some(Step::Unlink(child));
                    }
                    // EMFILE も誤りとして返す。記述子の不足で主のプロセスを落とさない。
                    Err(error) => return Err(error.into()),
                }
            }
            Some(Step::Unlink(child)) => {
                unlink_existing(entries.fd()?, child.as_c_str(), AtFlags::empty())?;
            }
            Some(Step::Finish) => {
                let (entries, child) = self.stack.pop().ok_or(io::ErrorKind::InvalidData)?;
                if let Some((parent, _)) = self.stack.last() {
                    unlink_existing(parent.fd()?, child.as_c_str(), AtFlags::REMOVEDIR)?;
                } else {
                    if self.session {
                        // 途中の削除に失敗しても、次回に錠を取り期限を判定できるよう残す
                        // （設計書 01-05「記録を残す期間」）。
                        for last in ["session.jsonl", "lock"] {
                            unlink_existing(entries.fd()?, last, AtFlags::empty())?;
                        }
                    }
                    unlink_existing(self.parent, self.name, AtFlags::REMOVEDIR)?;
                }
            }
        }
        Ok(true)
    }
}

fn unlink_existing(
    parent: impl AsFd,
    name: impl rustix::path::Arg,
    flags: AtFlags,
) -> io::Result<()> {
    match unlinkat(parent, name, flags) {
        Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> io::Result<Self> {
            let mut nonce = [0; 16];
            getrandom::fill(&mut nonce).map_err(io::Error::other)?;
            let path =
                std::env::temp_dir().join(format!("bh-remove-{:032x}", u128::from_ne_bytes(nonce)));
            fs::create_dir(&path)?;
            Ok(Self(fs::canonicalize(path)?))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _removed = fs::remove_dir_all(&self.0);
        }
    }

    // 関門: 親の別名を認めても、削除対象とその下のリンクを辿らない境界を保つ。
    #[test]
    fn linked_parents_are_followed_but_targets_and_descendant_links_are_not() -> TestResult {
        let fixture = Fixture::new()?;
        let outside = fixture.0.join("outside");
        fs::create_dir(&outside)?;
        fs::write(outside.join("keep"), b"keep")?;
        for kind in ["temporary", "sessions"] {
            let real = fixture.0.join(kind);
            let alias = fixture.0.join(format!("{kind}-alias"));
            fs::create_dir(&real)?;
            symlink(&real, &alias)?;
            fs::create_dir(real.join("remove"))?;
            fs::write(real.join("remove/file"), b"remove")?;
            symlink(&outside, real.join("remove/link"))?;
            directory(&alias.join("remove"))?;
            assert!(!real.join("remove").exists());
            symlink(&outside, real.join("target-link"))?;
            assert!(directory(&alias.join("target-link")).is_err());
            assert!(fs::symlink_metadata(real.join("target-link"))?.is_symlink());
            assert_eq!(fs::read(outside.join("keep"))?, b"keep");
        }
        Ok(())
    }

    // 関門: 各 syscall の間で実物を消し、競合による NOENT を決まった順で再現する。
    // 本番と同じ歩行を直接進め、スレッドの順番や待ち時間で成功させない。
    #[test]
    fn entries_removed_between_operations_do_not_stop_the_remaining_cleanup() -> TestResult {
        let fixture = Fixture::new()?;
        let parent = File::open(&fixture.0)?;
        for stage in ["listed", "file", "directory", "root"] {
            let root = fixture.0.join(stage);
            let victim = root.join("victim");
            fs::create_dir(&root)?;
            fs::write(root.join("remaining"), b"remaining")?;
            if stage == "directory" {
                fs::create_dir(&victim)?;
            } else if stage != "root" {
                fs::write(&victim, b"victim")?;
            }
            let mut removal = Removal::new(&parent, OsStr::new(stage), false)?;
            let mut removed = false;
            loop {
                let at_stage = match (stage, &removal.pending) {
                    ("listed", Some(Step::Visit(name))) | ("file", Some(Step::Unlink(name))) => {
                        name.to_bytes() == b"victim"
                    }
                    ("directory", Some(Step::Finish)) => removal.stack.len() == 2,
                    ("root", Some(Step::Finish)) => removal.stack.len() == 1,
                    _ => false,
                };
                if at_stage && !removed {
                    match stage {
                        "directory" => fs::remove_dir(&victim)?,
                        "root" => fs::remove_dir(&root)?,
                        _ => fs::remove_file(&victim)?,
                    }
                    removed = true;
                }
                if !removal.advance()? {
                    break;
                }
            }
            assert!(removed, "did not reach {stage}");
            assert!(!root.exists());
            // 開く前に根が消えた場合も成功とする。
            directory_at(&parent, OsStr::new(stage), false)?;
        }

        // 消えていない項目が増えた場合の ENOTEMPTY は握りつぶさない。
        let root = fixture.0.join("not-empty");
        fs::create_dir(&root)?;
        let mut removal = Removal::new(&parent, OsStr::new("not-empty"), false)?;
        while !matches!(removal.pending, Some(Step::Finish)) {
            assert!(removal.advance()?);
        }
        fs::write(root.join("new-entry"), b"new")?;
        let error = removal.advance().err().ok_or("ENOTEMPTY was ignored")?;
        assert_eq!(
            error.raw_os_error(),
            Some(rustix::io::Errno::NOTEMPTY.raw_os_error())
        );
        assert_eq!(fs::read(root.join("new-entry"))?, b"new");
        directory(&root)?;
        Ok(())
    }
}
