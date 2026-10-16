//! 根の記述子から相対にファイルを操作する（設計書 01-03「道具の定義の共通の規則」「書く道具のパスの規則」、01-05「巻き戻し」）。

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod os;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod os;
mod text;

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{File, Metadata};
use std::io::{self, Read};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{self, AtFlags, Mode, OFlags};

/// 範囲の制約の違反と、種類を保った OS の誤り。
#[derive(Debug)]
pub enum SafeFsError {
    /// 相対のパスの構文が不正である。
    InvalidPath,
    /// 解決したパスが根の外へ出る。
    OutsideScope,
    /// 安全に辿れない、または循環するリンクがある。
    IntermediateSymlink(io::Error),
    /// 書く先の最後の要素がシンボリックリンクである。
    FinalSymlink,
    /// 書く先に複数のハードリンクがある。
    MultipleHardLinks,
    /// 書く先が通常のファイルでない。
    NotRegularFile,
    /// 新しく作る先にすでに要素がある。
    AlreadyExists(io::Error),
    /// 要求した要素がない。
    NotFound(io::Error),
    /// そのほかの OS または入出力の誤り。
    Io(io::Error),
}

impl SafeFsError {
    fn from_errno(error: rustix::io::Errno) -> Self {
        io::Error::from(error).into()
    }
}

impl From<io::Error> for SafeFsError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::AlreadyExists {
            Self::AlreadyExists(error)
        } else if error.kind() == io::ErrorKind::NotFound {
            Self::NotFound(error)
        } else {
            Self::Io(error)
        }
    }
}

impl fmt::Display for SafeFsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => f.write_str(text::INVALID_PATH),
            Self::OutsideScope => f.write_str(text::OUTSIDE_SCOPE),
            Self::IntermediateSymlink(error) => write!(f, "{} {error}", text::INTERMEDIATE_SYMLINK),
            Self::FinalSymlink => f.write_str(text::FINAL_SYMLINK),
            Self::MultipleHardLinks => f.write_str(text::MULTIPLE_HARD_LINKS),
            Self::NotRegularFile => f.write_str(text::NOT_REGULAR_FILE),
            Self::AlreadyExists(error) => write!(f, "{} {error}", text::ALREADY_EXISTS),
            Self::NotFound(error) => write!(f, "{} {error}", text::NOT_FOUND),
            Self::Io(error) => write!(f, "{} {error}", text::IO_ERROR),
        }
    }
}

impl Error for SafeFsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IntermediateSymlink(error)
            | Self::AlreadyExists(error)
            | Self::NotFound(error)
            | Self::Io(error) => Some(error),
            Self::InvalidPath
            | Self::OutsideScope
            | Self::FinalSymlink
            | Self::MultipleHardLinks
            | Self::NotRegularFile => None,
        }
    }
}

/// 空の要素、`.`、`..`、NUL を含まない、根からの相対のパス。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeRelPath(PathBuf);

impl ScopeRelPath {
    /// OS のパス正規化で消える要素も、元のバイト列で拒否する。
    pub fn new(path: impl AsRef<Path>) -> Result<Self, SafeFsError> {
        let path = path.as_ref();
        if path
            .as_os_str()
            .as_bytes()
            .split(|byte| *byte == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b".." || part.contains(&0))
        {
            return Err(SafeFsError::InvalidPath);
        }
        Ok(Self(path.to_path_buf()))
    }

    /// 検証した相対のパスを借りる。
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// ディレクトリの一覧に現れる、リンクを辿らない種類。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    /// 通常のファイル。
    File,
    /// ディレクトリ。
    Directory,
    /// シンボリックリンク。
    Symlink,
    /// ソケットなど、ほかの種類。
    Other,
}

impl EntryKind {
    fn from_metadata(metadata: &Metadata) -> Self {
        let kind = metadata.file_type();
        if kind.is_symlink() {
            Self::Symlink
        } else if kind.is_dir() {
            Self::Directory
        } else if kind.is_file() {
            Self::File
        } else {
            Self::Other
        }
    }
}

/// 根またはディレクトリの直下の名前と、リンクを辿らない種類。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryEntry {
    /// UTF-8 と限らない名前。
    pub name: OsString,
    /// 名前が指す要素の種類。
    pub kind: EntryKind,
}

/// 範囲の根として開いて保持したディレクトリと、その実のパス。
#[derive(Debug)]
pub struct ScopeRoot {
    directory: File,
    real_path: PathBuf,
}

impl ScopeRoot {
    /// 根を O_DIRECTORY で開き、開いた記述子から実のパスを得る。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SafeFsError> {
        let path = std::fs::canonicalize(path).map_err(SafeFsError::from)?;
        let directory = File::from(
            fs::open(
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(SafeFsError::from_errno)?,
        );
        let real_path = resolved_path(&directory)?;
        Ok(Self {
            directory,
            real_path,
        })
    }

    /// 開いた根の実のパスを返す。
    pub fn real_path(&self) -> &Path {
        &self.real_path
    }

    /// 範囲内のディレクトリを開き、その記述子を根とする、より狭い範囲を返す。
    ///
    /// 書く道具は親をこの関数で開き、real_path と最後の名前で守る場所を判定し、
    /// 同じ ScopeRoot に最後の名前だけを渡して書く。判定後に親をパスから開き直さない
    /// ためである（設計書 01-03「書く道具のパスの規則」）。
    pub fn open_dir(&self, path: &ScopeRelPath) -> Result<Self, SafeFsError> {
        let directory = os::open(
            &self.directory,
            path.as_path(),
            OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )?;
        let real_path = resolved_path(&directory)?;
        Ok(Self {
            directory,
            real_path,
        })
    }

    /// プロジェクトからの相対または絶対の要求を、入口で早く拒否するために変換する。
    ///
    /// 存在する最も長い前の部分を正規化する。最後のリンクは書くときの拒否に
    /// 必要なので、最後の名前を保存する。安全の根拠は各操作の記述子からの解決に置く。
    pub fn resolve_path(
        &self,
        project: &Path,
        requested: &Path,
    ) -> Result<ScopeRelPath, SafeFsError> {
        if requested.is_absolute() {
            // 先頭の / だけを除き、内部の空や dot の要素を正規化前に調べる。
            let bytes = requested.as_os_str().as_bytes();
            let relative = bytes.strip_prefix(b"/").ok_or(SafeFsError::InvalidPath)?;
            ScopeRelPath::new(Path::new(OsStr::from_bytes(relative)))?;
        } else {
            ScopeRelPath::new(requested)?;
        }
        let absolute = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            std::fs::canonicalize(project)
                .map_err(SafeFsError::from)?
                .join(requested)
        };
        let checked = canonicalize_existing_prefix(&absolute)?;
        if !checked.starts_with(&self.real_path) {
            return Err(SafeFsError::OutsideScope);
        }
        let parent = absolute.parent().ok_or(SafeFsError::InvalidPath)?;
        let name = absolute.file_name().ok_or(SafeFsError::InvalidPath)?;
        let normalized = canonicalize_existing_prefix(parent)?.join(name);
        let relative = normalized
            .strip_prefix(&self.real_path)
            .map_err(|_| SafeFsError::OutsideScope)?;
        ScopeRelPath::new(relative)
    }

    /// 範囲の中に解決できるパスを、読むために開く。
    pub fn open_read(&self, path: &ScopeRelPath) -> Result<File, SafeFsError> {
        os::open(
            &self.directory,
            path.as_path(),
            OFlags::RDONLY | OFlags::NONBLOCK,
            Mode::empty(),
        )
    }

    /// 最後のリンクを辿らずにメタデータを返す。
    ///
    /// std::os::unix::fs::MetadataExt から dev、ino、mtime、ctime、nlink、mode も取得できる。
    pub fn metadata(&self, path: &ScopeRelPath) -> Result<Metadata, SafeFsError> {
        let (parent, name) = self.parent(path, false)?;
        entry_metadata(&parent, &name)
    }

    /// ディレクトリの直下を列挙する。None は根そのものを表す。
    ///
    /// 最後のリンクは辿らない。一覧を得ている間に要素が消えたときは誤りを返す。
    pub fn read_dir(
        &self,
        path: Option<&ScopeRelPath>,
    ) -> Result<Vec<DirectoryEntry>, SafeFsError> {
        let directory = match path {
            Some(path) => os::open(
                &self.directory,
                path.as_path(),
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                Mode::empty(),
            )?,
            None => os::open(
                &self.directory,
                Path::new("."),
                OFlags::RDONLY | OFlags::DIRECTORY,
                Mode::empty(),
            )?,
        };
        let mut entries = Vec::new();
        let iterator = fs::Dir::read_from(&directory).map_err(SafeFsError::from_errno)?;
        for entry in iterator {
            let entry = entry.map_err(SafeFsError::from_errno)?;
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let name = OsString::from_vec(bytes.to_vec());
            // d_type が未知の FS でも、同じ no-follow の意味にする（設計書 01-05「巻き戻し」）。
            let metadata = entry_metadata(&directory, Path::new(&name))?;
            entries.push(DirectoryEntry {
                name,
                kind: EntryKind::from_metadata(&metadata),
            });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    /// 同じ親の一時ファイルを書いて名前を付け替える。既存の権限は保つ。
    pub fn write(
        &self,
        path: &ScopeRelPath,
        contents: &[u8],
        create_parents: bool,
    ) -> Result<(), SafeFsError> {
        let mut source = contents;
        self.write_from(path, &mut source, create_parents)
    }

    /// ファイルなどの入力から、同じ親の一時ファイルへ書いて置き換える。
    ///
    /// 入力や書き込みが失敗した場合、元のファイルを保ち、一時ファイルを削除する。
    pub fn write_from(
        &self,
        path: &ScopeRelPath,
        source: &mut impl Read,
        create_parents: bool,
    ) -> Result<(), SafeFsError> {
        let (parent, name) = self.parent(path, create_parents)?;
        let mode = writable_mode(&parent, &name)?.unwrap_or(0o644);
        let mut temporary = TemporaryFile::create(&parent)?;
        io::copy(source, &mut temporary.file).map_err(SafeFsError::from)?;
        // mode は 0o7777 以下であり、macOS の u16 と Linux の u32 のどちらにも収まる。
        fs::fchmod(&temporary.file, Mode::from_raw_mode(mode as _))
            .map_err(SafeFsError::from_errno)?;
        // 書いている間に最後の要素がリンクに変わった場合も、置き換える前に拒否する。
        // renameat 自体は最後のリンクを辿らない（設計書 01-03「書く道具のパスの規則」）。
        let _mode = writable_mode(&parent, &name)?;
        fs::renameat(&parent, &temporary.name, &parent, &name).map_err(SafeFsError::from_errno)?;
        temporary.armed = false;
        Ok(())
    }

    /// 新しいディレクトリを作る。すでにある場合は誤りにする。
    pub fn create_dir(&self, path: &ScopeRelPath) -> Result<(), SafeFsError> {
        let (parent, name) = self.parent(path, false)?;
        fs::mkdirat(parent, name, Mode::from_raw_mode(0o755)).map_err(SafeFsError::from_errno)
    }

    /// 最後のリンクを辿らずに、ファイルまたはリンクを消す。
    pub fn remove_file(&self, path: &ScopeRelPath) -> Result<(), SafeFsError> {
        let (parent, name) = self.parent(path, false)?;
        fs::unlinkat(parent, name, AtFlags::empty()).map_err(SafeFsError::from_errno)
    }

    /// 最後のリンクを辿らずに、空のディレクトリを消す。
    pub fn remove_dir(&self, path: &ScopeRelPath) -> Result<(), SafeFsError> {
        let (parent, name) = self.parent(path, false)?;
        fs::unlinkat(parent, name, AtFlags::REMOVEDIR).map_err(SafeFsError::from_errno)
    }

    /// 先の文字列をそのまま持つリンクを作る。先は開かない。
    pub fn symlink(&self, target: &Path, path: &ScopeRelPath) -> Result<(), SafeFsError> {
        let (parent, name) = self.parent(path, false)?;
        fs::symlinkat(target, parent, name).map_err(SafeFsError::from_errno)
    }

    /// 最後のリンクを辿らずに、その先の文字列を返す。
    pub fn read_link(&self, path: &ScopeRelPath) -> Result<PathBuf, SafeFsError> {
        let (parent, name) = self.parent(path, false)?;
        let target = fs::readlinkat(parent, name, Vec::new()).map_err(SafeFsError::from_errno)?;
        Ok(PathBuf::from(OsString::from_vec(target.into_bytes())))
    }

    fn parent(&self, path: &ScopeRelPath, create: bool) -> Result<(File, PathBuf), SafeFsError> {
        let name = path.as_path().file_name().ok_or(SafeFsError::InvalidPath)?;
        let parent_path = path.as_path().parent().ok_or(SafeFsError::InvalidPath)?;
        if create {
            let mut prefix = PathBuf::new();
            for component in parent_path.components() {
                prefix.push(component);
                match os::open(
                    &self.directory,
                    &prefix,
                    OFlags::RDONLY | OFlags::DIRECTORY,
                    Mode::empty(),
                ) {
                    Ok(_directory) => {}
                    Err(SafeFsError::NotFound(_)) => {
                        let relative = ScopeRelPath::new(&prefix)?;
                        match self.create_dir(&relative) {
                            Ok(()) | Err(SafeFsError::AlreadyExists(_)) => {}
                            Err(error) => return Err(error),
                        }
                        // 競合した作成を信用せず、根からの制限を掛け直す。
                        let _directory = os::open(
                            &self.directory,
                            &prefix,
                            OFlags::RDONLY | OFlags::DIRECTORY,
                            Mode::empty(),
                        )?;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        let parent_path = if parent_path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent_path
        };
        let parent = os::open(
            &self.directory,
            parent_path,
            OFlags::RDONLY | OFlags::DIRECTORY,
            Mode::empty(),
        )?;
        Ok((parent, PathBuf::from(name)))
    }
}

/// 開いたものの実のパスを、パスの再解決をせず記述子から得る。
///
/// Linux では /proc が必要である。呼び出す側は、要求の字面ではなくこの値で
/// 拡張子と守る場所を判定する（設計書 01-03「書く道具のパスの規則」）。
pub fn resolved_path(file: &File) -> Result<PathBuf, SafeFsError> {
    os::resolved_path(file)
}

fn entry_metadata(parent: &File, name: &Path) -> Result<Metadata, SafeFsError> {
    os::metadata_file(parent, name)?
        .metadata()
        .map_err(SafeFsError::from)
}

fn writable_mode(parent: &File, name: &Path) -> Result<Option<u32>, SafeFsError> {
    let file = match os::write_target(parent, name) {
        Ok(file) => file,
        Err(SafeFsError::NotFound(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata().map_err(SafeFsError::from)?;
    if metadata.file_type().is_symlink() {
        return Err(SafeFsError::FinalSymlink);
    }
    if !metadata.is_file() {
        return Err(SafeFsError::NotRegularFile);
    }
    if metadata.nlink() > 1 {
        return Err(SafeFsError::MultipleHardLinks);
    }
    Ok(Some(metadata.permissions().mode() & 0o7777))
}

fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf, SafeFsError> {
    let mut prefix = path;
    let mut suffix = Vec::new();
    loop {
        match std::fs::canonicalize(prefix) {
            Ok(mut real) => {
                for name in suffix.into_iter().rev() {
                    real.push(name);
                }
                return Ok(real);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                suffix.push(prefix.file_name().ok_or(SafeFsError::InvalidPath)?);
                prefix = prefix.parent().ok_or(SafeFsError::InvalidPath)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TemporaryFile<'a> {
    parent: &'a File,
    name: OsString,
    file: File,
    armed: bool,
}

impl<'a> TemporaryFile<'a> {
    fn create(parent: &'a File) -> Result<Self, SafeFsError> {
        for _attempt in 0..128 {
            let counter = TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = OsString::from(format!(".benitoiteh-tmp-{}-{counter}", std::process::id()));
            match os::open(
                parent,
                Path::new(&name),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
                Mode::from_raw_mode(0o600),
            ) {
                Ok(file) => {
                    return Ok(Self {
                        parent,
                        name,
                        file,
                        armed: true,
                    });
                }
                Err(SafeFsError::AlreadyExists(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Err(SafeFsError::AlreadyExists(io::Error::new(
            io::ErrorKind::AlreadyExists,
            text::TEMPORARY_COLLISIONS,
        )))
    }
}

impl Drop for TemporaryFile<'_> {
    fn drop(&mut self) {
        if self.armed {
            // 元の誤りを保ち、途中の失敗でも一時ファイルを片付ける。
            // 親のパスを再解決しない（設計書 01-03「書く道具のパスの規則」）。
            let _cleanup = fs::unlinkat(self.parent, &self.name, AtFlags::empty());
        }
    }
}
