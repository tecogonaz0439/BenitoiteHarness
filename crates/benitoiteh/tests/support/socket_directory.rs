//! Unix ドメインのソケットを短い固有のパスに置く（設計書 07-01「安全の性質のテスト」）。

use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use benitoiteh::session::SessionId;

/// ソケットを閉じた後に削除する、テストごとの短いディレクトリ。
pub struct SocketDirectory(PathBuf);

impl SocketDirectory {
    /// 親の長さもソケットのパスの上限に入るため、TMPDIR に依存しない場所を選ぶ。
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let parent = fs::canonicalize("/tmp")?;
        let path = parent.join(format!(
            "bh-ipc-{}",
            SessionId::generate(SystemTime::now())?
        ));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }

    /// ソケットの名前を付ける実のディレクトリ。
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for SocketDirectory {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}
