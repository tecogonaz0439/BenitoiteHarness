//! リンクを辿らず、呼び出しのスタックを深さに比例させない削除（設計書 01-05「一時ディレクトリ」）。

use std::collections::HashSet;
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rustix::fs::{AtFlags, Dir, Mode, OFlags, fstat, open, openat, unlinkat};

const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// 消すものの種類。根の段で残す項目と、守る名前の扱いが違う。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Target {
    /// ハーネスだけが書く場所（補助の領域、作りかけのセッション）。
    Plain,
    /// 一時ディレクトリ。子プロセスが書けるので、守る名前の項目を残す
    /// （設計書 01-05「一時ディレクトリ」）。
    Temporary,
    /// セッションのディレクトリ。`lock` と `session.jsonl` を最後に消す。
    Session,
    /// セッションの墓標の中身。`lock` とディレクトリを残す（設計書 01-05「記録を残す期間」）。
    SessionContents,
}

pub(crate) fn directory(path: &Path) -> io::Result<()> {
    directory_of(path, Target::Plain)
}

/// 一時ディレクトリをパスで消す。守る名前の項目は残し、誤りとして示す。
pub(super) fn temporary(path: &Path) -> io::Result<()> {
    directory_of(path, Target::Temporary)
}

fn directory_of(path: &Path, target: Target) -> io::Result<()> {
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    // 親はハーネスが管理する根であり、/tmp などの別名も辿る。削除対象とその下は
    // openat の NOFOLLOW で守る（設計書 01-05「一時ディレクトリ」）。
    let parent = File::from(open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    directory_at(&parent, name, target)
}

pub(super) fn directory_at(parent: &File, name: &OsStr, target: Target) -> io::Result<()> {
    let mut removal = Removal::new(parent, name, target)?;
    while removal.advance()? {}
    Ok(())
}

/// 守る名前か。macOS のファイルシステムは大文字と小文字を区別しないことが多いので、
/// macOS では区別せずに比べる（設計書 03-02「方針の要素」）。
fn protected(name: &[u8]) -> bool {
    crate::sandbox::PROTECTED_NAMES.iter().any(|protected| {
        if cfg!(target_os = "macos") {
            name.eq_ignore_ascii_case(protected.as_bytes())
        } else {
            name == protected.as_bytes()
        }
    })
}

/// 守る名前の項目を残したので、一時ディレクトリを消せなかった誤り。示すパスは
/// 最初の高々 20 件で、総数を別に持つ。
#[derive(Debug)]
pub(super) struct ProtectedKept {
    pub(super) shown: Vec<PathBuf>,
    pub(super) total: usize,
}

impl std::fmt::Display for ProtectedKept {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&super::text::protected_kept(&self.shown, self.total))
    }
}

impl std::error::Error for ProtectedKept {}

enum Step {
    Visit(CString),
    Unlink(CString),
    Finish,
}

/// 開いたままにする記述子の上限（設計書 01-05「一時ディレクトリ」）。
const OPEN_LEVELS: usize = 32;

/// 削除の途中の一段。浅い段は記述子を閉じ、(dev, ino) だけを覚える。
struct Level {
    dir: Option<Dir>,
    name: CString,
    identity: (u64, u64),
    /// 下に守る名前の項目を残したので、この段を消さない。
    retained: bool,
    /// 残すと決めた子の名前。閉じた段を開き直して読み直すとき、これを飛ばして
    /// 同じ鎖を下り直さない（設計書 01-05「一時ディレクトリ」）。
    kept_children: HashSet<CString>,
}

struct Removal<'a> {
    parent: &'a File,
    name: &'a OsStr,
    target: Target,
    stack: Vec<Level>,
    /// 最も浅い開いた段の添字。閉じた段は常に先頭の連続した並びである。
    first_open: usize,
    /// 残した守る名前の項目のうち、示す最初の高々 20 件（親からの相対のパス）。
    /// 記憶を残した数に比例させないため、全件は持たない。
    kept: Vec<PathBuf>,
    /// 残した守る名前の項目の総数。
    kept_total: usize,
    pending: Option<Step>,
}

fn identity(fd: impl AsFd) -> io::Result<(u64, u64)> {
    let stat = fstat(fd)?;
    #[cfg(target_os = "macos")]
    let device = u64::from_ne_bytes(i64::from(stat.st_dev).to_ne_bytes());
    #[cfg(not(target_os = "macos"))]
    let device = stat.st_dev;
    Ok((device, stat.st_ino))
}

fn level(fd: OwnedFd, name: CString) -> io::Result<Level> {
    let identity = identity(&fd)?;
    Ok(Level {
        dir: Some(Dir::new(fd)?),
        name,
        identity,
        retained: false,
        kept_children: HashSet::new(),
    })
}

impl<'a> Removal<'a> {
    fn new(parent: &'a File, name: &'a OsStr, target: Target) -> io::Result<Self> {
        let stack = match openat(parent, name, DIRECTORY, Mode::empty()) {
            Ok(root) => vec![level(root, CString::default())?],
            Err(rustix::io::Errno::NOENT) => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            parent,
            name,
            target,
            stack,
            first_open: 0,
            kept: Vec::new(),
            kept_total: 0,
            pending: None,
        })
    }

    fn push(&mut self, level: Level) {
        self.stack.push(level);
        // 深い入れ子で記述子が尽きないよう、最も浅い開いた段を閉じる
        // （設計書 01-05「一時ディレクトリ」）。
        if self.stack.len().saturating_sub(self.first_open) > OPEN_LEVELS
            && let Some(shallow) = self.stack.get_mut(self.first_open)
        {
            shallow.dir = None;
            self.first_open = self.first_open.saturating_add(1);
        }
    }

    /// 親からの相対のパス（残した項目の表示に使う）。
    fn path_of(&self, child: &CString) -> PathBuf {
        let mut path = PathBuf::from(self.name);
        for level in self.stack.iter().skip(1) {
            path.push(OsStr::from_bytes(level.name.to_bytes()));
        }
        path.push(OsStr::from_bytes(child.to_bytes()));
        path
    }

    fn advance(&mut self) -> io::Result<bool> {
        let depth = self.stack.len();
        let Some(top) = self.stack.last_mut() else {
            return Ok(false);
        };
        // 最も深い段は常に開いている。閉じていれば歩行の誤りである。
        let entries = top.dir.as_mut().ok_or(io::ErrorKind::InvalidData)?;
        // 列挙・openat・unlinkat の間で別の掃除が進みうる。各操作を分け、前の操作で
        // 存在した項目も次には消えているものとして扱う（設計書 01-05「記録を残す期間」）。
        match self.pending.take() {
            None => {
                let next = match entries.read() {
                    Some(entry) => Some(entry?.file_name().to_owned()),
                    None => None,
                };
                let Some(child) = next else {
                    self.pending = Some(Step::Finish);
                    return Ok(true);
                };
                let bytes = child.to_bytes();
                // 段の数は閉じた段も含むので、根の判定は記述子の数に依らない。
                let kept_at_root = depth == 1
                    && match self.target {
                        Target::Session => matches!(bytes, b"lock" | b"session.jsonl"),
                        Target::SessionContents => bytes == b"lock",
                        Target::Plain | Target::Temporary => false,
                    };
                // 開き直した段の読み直しでは、残すと決めた子を飛ばして前進させる。
                let already_kept = self
                    .stack
                    .last()
                    .is_some_and(|top| top.kept_children.contains(&child));
                if matches!(bytes, b"." | b"..") || kept_at_root || already_kept {
                    return Ok(true);
                }
                if self.target == Target::Temporary && protected(bytes) {
                    // 一時ディレクトリへ移された .git などを、サンドボックスの外の削除で
                    // 消させない（設計書 01-05「一時ディレクトリ」）。
                    // パスは示す 20 件に入るときだけ作り、ほかは数だけを数える。
                    if self.kept.len() < super::text::PROTECTED_SHOWN {
                        let path = self.path_of(&child);
                        self.kept.push(path);
                    }
                    self.kept_total = self.kept_total.saturating_add(1);
                    if let Some(top) = self.stack.last_mut() {
                        top.retained = true;
                        top.kept_children.insert(child);
                    }
                } else {
                    self.pending = Some(Step::Visit(child));
                }
            }
            Some(Step::Visit(child)) => {
                match openat(entries.fd()?, child.as_c_str(), DIRECTORY, Mode::empty()) {
                    Ok(fd) => {
                        let next = level(fd, child)?;
                        self.push(next);
                    }
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
                let finished = self.stack.pop().ok_or(io::ErrorKind::InvalidData)?;
                let entries = finished.dir.ok_or(io::ErrorKind::InvalidData)?;
                let parent_index = self.stack.len().checked_sub(1);
                if let Some(parent) = self.stack.last_mut() {
                    if parent.dir.is_none() {
                        // 閉じた段へは子から .. で戻り、覚えた (dev, ino) で差し替えを
                        // 見つける。違えば差し替えた先を消さずに止める
                        // （設計書 01-05「一時ディレクトリ」）。
                        let fd = openat(entries.fd()?, c"..", DIRECTORY, Mode::empty())?;
                        if identity(&fd)? != parent.identity {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "directory was replaced during removal",
                            ));
                        }
                        // 開き直した一覧は初めから読む。消し終えた項目は既にない。
                        parent.dir = Some(Dir::new(fd)?);
                        // 閉じた段は先頭の並びなので、開き直した親が最も浅い開いた段になる。
                        self.first_open = parent_index.unwrap_or_default();
                    }
                    if finished.retained {
                        parent.retained = true;
                        parent.kept_children.insert(finished.name);
                    } else {
                        let parent_dir = parent.dir.as_ref().ok_or(io::ErrorKind::InvalidData)?;
                        unlink_existing(
                            parent_dir.fd()?,
                            finished.name.as_c_str(),
                            AtFlags::REMOVEDIR,
                        )?;
                    }
                } else if finished.retained {
                    return Err(io::Error::other(ProtectedKept {
                        shown: std::mem::take(&mut self.kept),
                        total: self.kept_total,
                    }));
                } else {
                    match self.target {
                        Target::Session => {
                            // 途中の削除に失敗しても、次回に錠を取り期限を判定できるよう残す
                            // （設計書 01-05「記録を残す期間」）。
                            for last in ["session.jsonl", "lock"] {
                                unlink_existing(entries.fd()?, last, AtFlags::empty())?;
                            }
                            unlink_existing(self.parent, self.name, AtFlags::REMOVEDIR)?;
                        }
                        // 墓標は錠のファイルだけにして残す。
                        Target::SessionContents => {}
                        Target::Plain | Target::Temporary => {
                            unlink_existing(self.parent, self.name, AtFlags::REMOVEDIR)?;
                        }
                    }
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
        // 本番と同じく、一時の根は別名を辿って開き、sessions は開いた根から session の
        // 扱い（lock と session.jsonl を最後に消す）で消す。
        for (kind, session) in [
            ("temporary", Target::Temporary),
            ("sessions", Target::Session),
        ] {
            let real = fixture.0.join(kind);
            let alias = fixture.0.join(format!("{kind}-alias"));
            fs::create_dir(&real)?;
            symlink(&real, &alias)?;
            fs::create_dir(real.join("remove"))?;
            fs::write(real.join("remove/file"), b"remove")?;
            symlink(&outside, real.join("remove/link"))?;
            let parent = File::open(&alias)?;
            directory_at(&parent, OsStr::new("remove"), session)?;
            assert!(!real.join("remove").exists());
            symlink(&outside, real.join("target-link"))?;
            assert!(directory_at(&parent, OsStr::new("target-link"), session).is_err());
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
            let mut removal = Removal::new(&parent, OsStr::new(stage), Target::Temporary)?;
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
            directory_at(&parent, OsStr::new(stage), Target::Temporary)?;
        }

        // 消えていない項目が増えた場合の ENOTEMPTY は握りつぶさない。
        let root = fixture.0.join("not-empty");
        fs::create_dir(&root)?;
        let mut removal = Removal::new(&parent, OsStr::new("not-empty"), Target::Temporary)?;
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

    // 関門: 32 段の窓の外で閉じた段へ戻るとき、差し替えを (dev, ino) で見つけて止まる。
    // 閉じた段の確かめを外す（覚えた値と比べずに .. を使う）退行で、差し替えた先の
    // keep が消えて失敗する。深い入れ子の成功は tests/session.rs の深い削除で確かめる。
    #[test]
    fn replaced_closed_level_stops_removal_without_touching_the_replacement() -> TestResult {
        let fixture = Fixture::new()?;
        let parent = File::open(&fixture.0)?;
        let mut path = fixture.0.join("deep");
        let mut fifth = None;
        for depth in 0..=40 {
            fs::create_dir(&path)?;
            if depth == 5 {
                fifth = Some(path.clone());
            }
            path.push("d");
        }
        let fifth = fifth.ok_or("missing level")?;
        let outside = fixture.0.join("outside");
        fs::create_dir(&outside)?;
        fs::write(outside.join("keep"), b"keep")?;

        let mut removal = Removal::new(&parent, OsStr::new("deep"), Target::Temporary)?;
        while removal.stack.len() < 41 {
            assert!(removal.advance()?);
        }
        assert!(
            removal
                .stack
                .iter()
                .take(9)
                .all(|level| level.dir.is_none())
        );
        // 5 段目の子（6 段目）を入れ子の外へ移す。戻る先の .. は outside になる。
        fs::rename(fifth.join("d"), outside.join("d"))?;
        let error = loop {
            match removal.advance() {
                Ok(true) => {}
                Ok(false) => return Err("replacement was not detected".into()),
                Err(error) => break error,
            }
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read(outside.join("keep"))?, b"keep");
        assert!(fifth.exists());
        Ok(())
    }

    /// `parent` の下に `name` の段を深さ `depth` だけ連ね、末尾の段を返す。パスで作ると
    /// macOS の長い TMPDIR で上限を越えるので、mkdirat と openat の連なりで作る。
    fn chain(parent: impl AsFd, name: &str, depth: usize) -> io::Result<OwnedFd> {
        rustix::fs::mkdirat(&parent, name, Mode::RWXU)?;
        let mut directory = openat(&parent, name, DIRECTORY, Mode::empty())?;
        for _ in 1..depth {
            rustix::fs::mkdirat(&directory, name, Mode::RWXU)?;
            directory = openat(&directory, name, DIRECTORY, Mode::empty())?;
        }
        Ok(directory)
    }

    /// 末尾に .git/HEAD を置く。
    fn git_at(directory: impl AsFd) -> io::Result<()> {
        rustix::fs::mkdirat(&directory, ".git", Mode::RWXU)?;
        let git = openat(&directory, ".git", DIRECTORY, Mode::empty())?;
        let head = openat(
            &git,
            "HEAD",
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?;
        rustix::io::write(&head, b"ref")?;
        Ok(())
    }

    /// 一時ディレクトリの削除を別のスレッドで走らせ、20 秒で終わらなければ失敗とする。
    /// 終わらない退行（同じ鎖の下り直し）を、テストを止めずに捕まえる。
    fn remove_within_limit(root: &Path, name: &str) -> Result<io::Result<()>, String> {
        let parent = File::open(root).map_err(|error| error.to_string())?;
        let name = name.to_owned();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = directory_at(&parent, OsStr::new(&name), Target::Temporary);
            let _sent = sender.send(result);
        });
        receiver
            .recv_timeout(std::time::Duration::from_secs(20))
            .map_err(|_| "temporary removal did not finish within 20 seconds".to_owned())
    }

    fn occurrences(text: &str, pattern: &str) -> usize {
        text.matches(pattern).count()
    }

    // 関門: 守る名前が 32 段の窓より深いと、閉じた段の読み直しで同じ鎖を下り直し、
    // 削除が終わらない退行を捕まえる。残すと決めた子を飛ばす直しを外すと時間切れで落ちる。
    #[test]
    fn protected_entries_deeper_than_the_window_do_not_stop_removal() -> TestResult {
        let fixture = Fixture::new()?;
        let base = File::open(&fixture.0)?;
        for depth in [33, 40] {
            let name = format!("chain-{depth}");
            rustix::fs::mkdirat(&base, name.as_str(), Mode::RWXU)?;
            let root = openat(&base, name.as_str(), DIRECTORY, Mode::empty())?;
            git_at(chain(&root, "c", depth)?)?;
            let error = remove_within_limit(&fixture.0, &name)?
                .err()
                .ok_or("protected entry was not reported")?;
            let reason = error.to_string();
            assert_eq!(occurrences(&reason, "/.git"), 1, "{reason}");
            let mut head = fixture.0.join(&name);
            for _ in 0..depth {
                head.push("c");
            }
            assert_eq!(fs::read(head.join(".git/HEAD"))?, b"ref");
        }
        Ok(())
    }

    // 関門: 浅い .git を持つ段が窓の外で閉じられ、深い兄弟から戻って読み直すたびに .git を
    // 重ねて示す退行を捕まえる（終わらない退行は上のテストの役）。深い兄弟は消え、.git は残る。
    #[test]
    fn shallow_protected_entry_with_deep_siblings_is_reported_once() -> TestResult {
        let fixture = Fixture::new()?;
        let base = File::open(&fixture.0)?;
        rustix::fs::mkdirat(&base, "mixed", Mode::RWXU)?;
        let root = openat(&base, "mixed", DIRECTORY, Mode::empty())?;
        git_at(&root)?;
        let mixed = fixture.0.join("mixed");
        // 一覧で .git より後に深い兄弟が来ないと、読み直しが .git の後に起きず退行を
        // 捕まえられない。一覧の順はファイルシステムが決めるので、そうなる名前を選ぶ。
        // 新しく作った項目を先に並べるファイルシステム（古い tmpfs など）でも通るよう、
        // 奇数回は .git を兄弟の後に作り直し、作る順も入れ替える。
        let mut ordered = false;
        for attempt in 0..32 {
            let siblings = (0..3)
                .map(|index| format!("s{attempt}-{index}"))
                .collect::<Vec<_>>();
            for sibling in &siblings {
                drop(chain(&root, sibling, 40)?);
            }
            if attempt % 2 == 1 {
                fs::remove_dir_all(mixed.join(".git"))?;
                git_at(&root)?;
            }
            let names = fs::read_dir(&mixed)?
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect::<Result<Vec<_>, _>>()?;
            let git = names.iter().position(|name| name == ".git");
            if names.iter().skip(git.map_or(0, |at| at + 1)).count() > 0 {
                ordered = true;
                break;
            }
            for sibling in &siblings {
                fs::remove_dir_all(mixed.join(sibling))?;
            }
        }
        assert!(ordered, "no sibling was listed after .git");
        let error = remove_within_limit(&fixture.0, "mixed")?
            .err()
            .ok_or("protected entry was not reported")?;
        let reason = error.to_string();
        assert_eq!(occurrences(&reason, "mixed/.git"), 1, "{reason}");
        assert_eq!(fs::read(mixed.join(".git/HEAD"))?, b"ref");
        let mut left = fs::read_dir(&mixed)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()?;
        left.sort();
        assert_eq!(left, [OsStr::new(".git")]);
        Ok(())
    }

    // 関門: 残した項目が多くても、削除の途中で持つパスは最初の 20 件だけで、総数は正しい。
    // 全件を持つ退行は、誤りの型が持つパスの数で捕まえる（文の形だけでは捕まえられない）。
    #[test]
    fn many_kept_entries_hold_twenty_paths_and_the_total() -> TestResult {
        let fixture = Fixture::new()?;
        let base = File::open(&fixture.0)?;
        rustix::fs::mkdirat(&base, "held", Mode::RWXU)?;
        let root = openat(&base, "held", DIRECTORY, Mode::empty())?;
        for index in 0..45 {
            let name = format!("p{index:02}");
            rustix::fs::mkdirat(&root, name.as_str(), Mode::RWXU)?;
            git_at(openat(&root, name.as_str(), DIRECTORY, Mode::empty())?)?;
        }
        let mut removal = Removal::new(&base, OsStr::new("held"), Target::Temporary)?;
        let error = loop {
            assert!(removal.kept.len() <= 20);
            match removal.advance() {
                Ok(true) => {}
                Ok(false) => return Err("protected entries were not reported".into()),
                Err(error) => break error,
            }
        };
        let kept = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<ProtectedKept>())
            .ok_or("error does not carry the kept entries")?;
        assert_eq!(kept.shown.len(), 20);
        assert_eq!(kept.total, 45);
        Ok(())
    }

    // 関門: 一件のパスが長くても、守る名前の側（末尾）を残して示す。
    #[test]
    fn long_kept_paths_keep_their_protected_tail() {
        let long = PathBuf::from(format!("{}/x/.git", "L".repeat(3000)));
        let reason = super::super::text::protected_kept(&[long], 1);
        assert!(
            reason.starts_with("kept protected entries: 1 total; …"),
            "{reason}"
        );
        assert!(reason.ends_with("/x/.git"), "{reason}");
        assert!(reason.chars().count() < 400, "{reason}");
    }

    // 関門: 残した項目が多くても知らせを短く保つ。総数を先頭に、最初の 20 件を後ろに示す。
    #[test]
    fn many_kept_entries_show_twenty_paths_and_the_remaining_count() -> TestResult {
        let fixture = Fixture::new()?;
        let base = File::open(&fixture.0)?;
        rustix::fs::mkdirat(&base, "many", Mode::RWXU)?;
        let root = openat(&base, "many", DIRECTORY, Mode::empty())?;
        for index in 0..25 {
            let name = format!("p{index:02}");
            rustix::fs::mkdirat(&root, name.as_str(), Mode::RWXU)?;
            git_at(openat(&root, name.as_str(), DIRECTORY, Mode::empty())?)?;
        }
        let error = remove_within_limit(&fixture.0, "many")?
            .err()
            .ok_or("protected entries were not reported")?;
        let reason = error.to_string();
        assert_eq!(occurrences(&reason, "/.git"), 20, "{reason}");
        assert!(
            reason.starts_with("kept protected entries: 25 total (5 not shown); "),
            "{reason}"
        );
        Ok(())
    }
}
