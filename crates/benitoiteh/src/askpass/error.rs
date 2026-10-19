//! 仲介の準備の失敗と、補助に返す決まった失敗（設計書 03-04「段 2」）。
use super::text;
use std::{fmt, io};

/// 窓口の準備・通信の失敗。求めの文や値を誤りに含めない。
#[derive(Debug)]
pub enum AskpassError {
    Io(io::Error),
    Unavailable,
    Closed,
    Refused,
    ChangedDirectory,
    UnownedParent(std::path::PathBuf),
}
impl fmt::Display for AskpassError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{} {error}", text::FAILED),
            Self::Unavailable => f.write_str(text::UNAVAILABLE),
            Self::Closed => f.write_str(text::CLOSED),
            Self::Refused => f.write_str(text::REFUSED),
            Self::UnownedParent(path) => f.write_str(&text::unowned_parent(path)),
            Self::ChangedDirectory => f.write_str(text::CHANGED_DIRECTORY),
        }
    }
}
impl std::error::Error for AskpassError {}
impl From<io::Error> for AskpassError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<io::ErrorKind> for AskpassError {
    fn from(kind: io::ErrorKind) -> Self {
        Self::Io(kind.into())
    }
}
