//! 指示を範囲内の記述子から読み、除外と上限を適用する（設計書 01-04「プロジェクトの指示のファイル」）。

mod text;

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use rustix::fs::{Mode as FileMode, OFlags};

use crate::config::Mode;
use crate::safe_fs::{self, SafeFsError, ScopeRelPath, ScopeRoot};
use crate::session::SkippedInstruction;
use crate::tools::exclude::ExcludeRules;

const FILE_LIMIT: usize = 32 * 1024;
const TOTAL_LIMIT: usize = 64 * 1024;

/// 読んだ指示。パスはリンクを解く前の、探索時の名前である。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionFile {
    /// 探索時の絶対パス。
    pub path: PathBuf,
    /// 上限内の UTF-8 と、切り詰めた場合の末尾の印。
    pub contents: String,
    /// メタデータの大きさから、入れたバイト数を引いた量。
    pub omitted_bytes: u64,
}

/// 環境に入れる指示と、画面・記録に残す除外理由。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectInstructions {
    /// 利用者、リポジトリの根、プロジェクトの順に読んだ指示。
    pub files: Vec<InstructionFile>,
    /// 存在したが読まなかった候補、または探索範囲の根。
    pub skipped: Vec<SkippedInstruction>,
}

#[derive(Debug)]
enum InstructionError {
    Io(io::Error),
    SafeFs(SafeFsError),
    Invalid(&'static str),
}

impl fmt::Display for InstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::SafeFs(error) => error.fmt(f),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl From<io::Error> for InstructionError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<SafeFsError> for InstructionError {
    fn from(error: SafeFsError) -> Self {
        Self::SafeFs(error)
    }
}

/// 通常モードの起動時に指示を集める。読み取りの失敗は起動を止めず、理由を返す。
///
/// project と home は、呼び出し元が確定した実の絶対パスを受け取る。
pub fn collect(
    mode: Mode,
    project: &Path,
    instructions_file: &Path,
    home: &Path,
    exclude: &ExcludeRules,
) -> ProjectInstructions {
    let mut collector = Collector::default();
    if mode == Mode::Restricted {
        return collector.result;
    }
    match rustix::fs::open(
        instructions_file,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC,
        FileMode::empty(),
    ) {
        Ok(fd) => {
            let file = File::from(fd);
            let result = collector.read(&file, false);
            collector.finish(instructions_file.to_path_buf(), result);
        }
        Err(error) if error == rustix::io::Errno::NOENT => {}
        Err(error) => collector.skip(instructions_file.to_path_buf(), io::Error::from(error)),
    }
    let scope = search_root(project, home);
    let root = match ScopeRoot::open(scope) {
        Ok(root) => root,
        Err(error) => {
            collector.skip(scope.to_path_buf(), error);
            return collector.result;
        }
    };
    let mut directories = project
        .ancestors()
        .take_while(|directory| directory.starts_with(scope))
        .collect::<Vec<_>>();
    directories.reverse();
    for directory in directories {
        let relative = directory.strip_prefix(scope).unwrap_or(Path::new(""));
        let agents = relative.join("AGENTS.md");
        let candidate = match ScopeRelPath::new(&agents).and_then(|rel| root.metadata(&rel)) {
            Ok(_) => agents,
            Err(SafeFsError::NotFound(_)) => relative.join("CLAUDE.md"),
            Err(error) => {
                collector.skip(scope.join(agents), error);
                continue;
            }
        };
        let path = scope.join(&candidate);
        let result = (|| {
            let rel = ScopeRelPath::new(candidate)?;
            // 不在は診断しない。壊れたリンクは存在する候補として診断する。
            match root.metadata(&rel) {
                Err(SafeFsError::NotFound(_)) => return Ok(None),
                other => {
                    other?;
                }
            }
            let file = root.open_read(&rel)?;
            let real = safe_fs::resolved_path(&file)?;
            if let Some(reason) = exclude.reason(&real) {
                collector.skip(path.clone(), reason);
                return Ok(None);
            }
            collector.read(&file, true).map(Some)
        })();
        match result {
            Ok(Some(value)) => collector.finish(path, Ok(value)),
            Ok(None) => {}
            Err(error) => collector.skip(path, error),
        }
    }
    collector.result
}

pub(super) fn search_root<'a>(project: &'a Path, home: &Path) -> &'a Path {
    for directory in project.ancestors() {
        if fs::symlink_metadata(directory.join(".git"))
            .is_ok_and(|metadata| metadata.is_file() || metadata.is_dir())
        {
            return if home.starts_with(directory) {
                project
            } else {
                directory
            };
        }
    }
    project
}

#[derive(Default)]
struct Collector {
    result: ProjectInstructions,
    identities: HashSet<(u64, u64)>,
    bytes: usize,
}

impl Collector {
    fn skip(&mut self, path: PathBuf, reason: impl ToString) {
        self.result.skipped.push(SkippedInstruction {
            path,
            reason: reason.to_string(),
        });
    }

    fn finish(&mut self, path: PathBuf, result: Result<(String, u64), InstructionError>) {
        match result {
            Ok((contents, omitted_bytes)) => self.result.files.push(InstructionFile {
                path,
                contents,
                omitted_bytes,
            }),
            Err(error) => self.skip(path, error),
        }
    }

    fn read(&mut self, file: &File, project: bool) -> Result<(String, u64), InstructionError> {
        // File::metadata は、開いた記述子を fstat する。パスを調べ直さない。
        let metadata: Metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(InstructionError::Invalid(text::NOT_REGULAR));
        }
        if project && metadata.nlink() != 1 {
            return Err(InstructionError::Invalid(text::HARD_LINKS));
        }
        let identity = (metadata.dev(), metadata.ino());
        if self.identities.contains(&identity) {
            return Err(InstructionError::Invalid(text::DUPLICATE));
        }
        let limit = FILE_LIMIT.min(TOTAL_LIMIT.saturating_sub(self.bytes));
        if limit == 0 {
            return Err(InstructionError::Invalid(text::TOTAL_LIMIT));
        }
        let mut bytes = Vec::new();
        file.take(u64::try_from(limit.saturating_add(4)).unwrap_or(u64::MAX))
            .read_to_end(&mut bytes)?;
        let valid = match std::str::from_utf8(&bytes) {
            Ok(value) => value,
            Err(error) if error.error_len().is_none() => {
                std::str::from_utf8(bytes.get(..error.valid_up_to()).unwrap_or_default())
                    .map_err(|_| InstructionError::Invalid(text::NOT_UTF8))?
            }
            Err(_) => return Err(InstructionError::Invalid(text::NOT_UTF8)),
        };
        let mut end = limit.min(valid.len());
        while !valid.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        let mut contents = valid.get(..end).unwrap_or_default().to_owned();
        let omitted = metadata
            .len()
            .saturating_sub(u64::try_from(end).unwrap_or(u64::MAX));
        if omitted > 0 {
            contents.push('\n');
            contents.push_str(&text::truncated(omitted));
        }
        self.identities.insert(identity);
        self.bytes = self.bytes.saturating_add(end);
        Ok((contents, omitted))
    }
}
