//! 時点の一覧と戻す操作の公開の値（設計書 01-05「巻き戻し」）。

use super::text;
use crate::safe_fs::{SafeFsError, ScopeRelPath};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 記録の作業量の上限。呼び出す側は用途に応じて小さくできる。
#[derive(Clone, Debug)]
pub struct CheckpointLimits {
    pub max_entries: usize,
    pub max_duration: Duration,
    pub max_file_bytes: u64,
    pub max_content_bytes: u64,
    pub max_ignore_bytes: u64,
    pub max_manifest_bytes: u64,
}
impl Default for CheckpointLimits {
    fn default() -> Self {
        Self {
            max_entries: 200_000,
            max_duration: Duration::from_secs(10),
            max_file_bytes: 8 * 1024 * 1024,
            max_content_bytes: 256 * 1024 * 1024,
            max_ignore_bytes: 1024 * 1024,
            max_manifest_bytes: 256 * 1024 * 1024,
        }
    }
}

/// 一つの時点で実際に読み、保存した量。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointStats {
    pub files_hashed: u64,
    pub bytes_read: u64,
    pub objects_written: u64,
}

/// 記録する直前の操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointReason {
    Request,
    Bash,
    Write,
    Edit,
    BeforeRewind,
}

/// ナノ秒の整数として保存する時刻。秒の部分は再利用の判定にも使う。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(into = "i128")]
pub struct Timestamp {
    pub seconds: i64,
    pub nanoseconds: i64,
}
impl Timestamp {
    pub(super) fn now() -> Self {
        let elapsed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            seconds: i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX),
            nanoseconds: i64::from(elapsed.subsec_nanos()),
        }
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Nanoseconds;
        impl serde::de::Visitor<'_> for Nanoseconds {
            type Value = Timestamp;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(text::TIMESTAMP_EXPECTED)
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Timestamp, E> {
                Timestamp::try_from(i128::from(value)).map_err(E::custom)
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Timestamp, E> {
                Timestamp::try_from(i128::from(value)).map_err(E::custom)
            }
        }
        // flatten が使う中間表現でも整数を読めるよう deserialize_any を使う。
        deserializer.deserialize_any(Nanoseconds)
    }
}
impl From<Timestamp> for i128 {
    fn from(value: Timestamp) -> Self {
        i128::from(value.seconds)
            .saturating_mul(1_000_000_000)
            .saturating_add(i128::from(value.nanoseconds))
    }
}
impl TryFrom<i128> for Timestamp {
    type Error = std::num::TryFromIntError;
    fn try_from(value: i128) -> Result<Self, Self::Error> {
        Ok(Self {
            seconds: i64::try_from(value.div_euclid(1_000_000_000))?,
            nanoseconds: i64::try_from(value.rem_euclid(1_000_000_000))?,
        })
    }
}

/// セッションの行だけで時点を一覧できる記録。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub reason: CheckpointReason,
    pub turn: u64,
    pub call_id: Option<String>,
    pub created_at: Timestamp,
    pub items: Option<usize>,
    pub generation: u64,
    pub manifest: Option<String>,
    pub incomplete: Vec<String>,
}

/// 画面用の時点と、その時点へ会話を戻せるか。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointInfo {
    #[serde(flatten)]
    pub checkpoint: Checkpoint,
    pub conversation_available: bool,
}

/// ファイルと会話のどちらを戻すか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewindScope {
    FilesAndConversation,
    Files,
    Conversation,
}

/// 戻せなかったパスと理由。UTF-8 でない名前も保持する。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewindFailure {
    pub path: PathBuf,
    pub reason: String,
}

/// プロジェクトの記録だけでは効果を戻せない実行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrreversibleOperation {
    pub call_id: String,
    pub summary: String,
}

/// 戻した数と、利用者に示す残る操作。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RewindReport {
    pub restored: u64,
    pub removed: u64,
    pub created: u64,
    pub failures: Vec<RewindFailure>,
    pub irreversible: Vec<IrreversibleOperation>,
    pub before_rewind: Option<String>,
    pub conversation_rewound: bool,
    /// 読んだ版を無効にするための、変更できた相対パス。
    pub changed_paths: Vec<PathBuf>,
}

/// 一覧の種類。特殊なファイルは保存せず、存在だけを記録する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestKind {
    File,
    Directory,
    Symlink,
    Other,
}

/// 読み直しと書き戻し前の競合の判定に使う、リンクを辿らない属性。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Identity {
    pub size: u64,
    pub dev: u64,
    pub ino: u64,
    pub mtime: Timestamp,
    pub ctime: Timestamp,
    pub nlink: u64,
}
impl Identity {
    fn unknown(&self) -> bool {
        self == &Self::default()
    }
    pub(super) fn from_stat(stat: &rustix::fs::Stat) -> Self {
        Self {
            size: u64::try_from(stat.st_size).unwrap_or(0),
            dev: stat_device(stat),
            ino: stat.st_ino,
            mtime: Timestamp {
                seconds: stat.st_mtime,
                nanoseconds: stat_times(stat).0,
            },
            ctime: Timestamp {
                seconds: stat.st_ctime,
                nanoseconds: stat_times(stat).1,
            },
            nlink: stat_links(stat),
        }
    }
    pub(super) fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            size: metadata.len(),
            dev: metadata.dev(),
            ino: metadata.ino(),
            mtime: Timestamp {
                seconds: metadata.mtime(),
                nanoseconds: metadata.mtime_nsec(),
            },
            ctime: Timestamp {
                seconds: metadata.ctime(),
                nanoseconds: metadata.ctime_nsec(),
            },
            nlink: metadata.nlink(),
        }
    }
}

/// 一覧の一項目。UTF-8 と生のバイト列は、ちょうど片方だけを持つ。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_hex: Option<String>,
    pub kind: ManifestKind,
    #[serde(flatten, skip_serializing_if = "Identity::unknown")]
    pub identity: Identity,
    /// 実行権限の 3 ビット（mode & 0o111）。
    #[serde(default, skip_serializing_if = "is_zero")]
    pub executable: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stored: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reusable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_target_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enumerated: bool,
}
impl ManifestEntry {
    pub(super) fn new(
        path: &Path,
        kind: ManifestKind,
        identity: Identity,
        executable: u32,
    ) -> Self {
        let (path, path_hex) = encode_path(path);
        Self {
            path,
            path_hex,
            kind,
            identity,
            executable,
            sha256: None,
            stored: false,
            reusable: false,
            link_target: None,
            link_target_hex: None,
            excluded: None,
            error: None,
            enumerated: false,
        }
    }
    /// 一覧のパスを構文と守る名前で検証する。除外の項目も検証してから扱う。
    pub fn relative_path(&self) -> Result<ScopeRelPath, SafeFsError> {
        let path = decode_path(&self.path, &self.path_hex)?;
        let relative = ScopeRelPath::new(path)?;
        if protected(relative.as_path()) {
            return Err(SafeFsError::InvalidPath);
        }
        Ok(relative)
    }
    pub(super) fn raw_path(&self) -> Result<PathBuf, SafeFsError> {
        decode_path(&self.path, &self.path_hex)
    }
    pub(super) fn target(&self) -> Result<PathBuf, SafeFsError> {
        decode_path(&self.link_target, &self.link_target_hex)
    }
}

/// SHA-256 名で保存する、時点のファイル一覧。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub project: PathBuf,
    pub complete: bool,
    pub root_enumerated: bool,
    pub entries: Vec<ManifestEntry>,
}

pub(super) fn protected(path: &Path) -> bool {
    path.components().any(|part| {
        let bytes = part.as_os_str().as_bytes();
        bytes.eq_ignore_ascii_case(b".git") || bytes.eq_ignore_ascii_case(b".benitoiteh")
    })
}
pub(super) fn encode_path(path: &Path) -> (Option<String>, Option<String>) {
    match path.to_str() {
        Some(value) => (Some(value.to_owned()), None),
        None => (None, Some(hex(path.as_os_str().as_bytes()))),
    }
}
pub(super) fn decode_path(
    path: &Option<String>,
    encoded: &Option<String>,
) -> Result<PathBuf, SafeFsError> {
    match (path, encoded) {
        (Some(path), None) if !path.as_bytes().contains(&0) => Ok(path.into()),
        (None, Some(value)) if value.len().is_multiple_of(2) => {
            let mut bytes = Vec::new();
            for pair in value.as_bytes().as_chunks::<2>().0 {
                let pair = std::str::from_utf8(pair).map_err(|_| SafeFsError::InvalidPath)?;
                bytes.push(u8::from_str_radix(pair, 16).map_err(|_| SafeFsError::InvalidPath)?);
            }
            if bytes.contains(&0) {
                return Err(SafeFsError::InvalidPath);
            }
            Ok(PathBuf::from(OsString::from_vec(bytes)))
        }
        _ => Err(SafeFsError::InvalidPath),
    }
}
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn stat_device(stat: &rustix::fs::Stat) -> u64 {
    #[cfg(target_os = "macos")]
    {
        u64::from_ne_bytes(i64::from(stat.st_dev).to_ne_bytes())
    }
    #[cfg(target_os = "linux")]
    {
        stat.st_dev
    }
}

fn stat_times(stat: &rustix::fs::Stat) -> (i64, i64) {
    #[cfg(target_os = "macos")]
    {
        (stat.st_mtime_nsec, stat.st_ctime_nsec)
    }
    #[cfg(target_os = "linux")]
    {
        (
            i64::try_from(stat.st_mtime_nsec).unwrap_or(i64::MAX),
            i64::try_from(stat.st_ctime_nsec).unwrap_or(i64::MAX),
        )
    }
}
// st_nlink の幅は macOS、aarch64 Linux、x86_64 Linux で異なる。
#[allow(clippy::useless_conversion)]
fn stat_links(stat: &rustix::fs::Stat) -> u64 {
    #[cfg(target_os = "macos")]
    {
        u64::from(stat.st_nlink)
    }
    #[cfg(target_os = "linux")]
    {
        u64::from(stat.st_nlink)
    }
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

#[allow(clippy::useless_conversion)]
pub(super) fn stat_mode(stat: &rustix::fs::Stat) -> u32 {
    u32::from(stat.st_mode)
}
