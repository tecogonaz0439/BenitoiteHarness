//! 設定・データ・一時領域の置き場所を決める（設計書 05-01「データの置き場所」）。

use std::path::{Path, PathBuf};

use super::{ConfigError, text};

/// 起動時に確定する利用者単位の置き場所。
#[derive(Clone, Debug)]
pub struct DataPaths {
    /// 利用者のホーム。
    pub home: PathBuf,
    /// benitoiteh の設定ディレクトリ。
    pub config_dir: PathBuf,
    /// 利用者の設定ファイル。
    pub config_file: PathBuf,
    /// 利用者の指示。
    pub instructions_file: PathBuf,
    /// benitoiteh の状態ディレクトリ。
    pub data_dir: PathBuf,
    /// セッションの根。
    pub sessions_dir: PathBuf,
    /// プロジェクトごとの入力の履歴。
    pub history_dir: PathBuf,
    /// ChatGPT の許可の保存先。
    pub auth_dir: PathBuf,
    /// 一時ディレクトリを作る親。
    pub temporary_root: PathBuf,
}

impl DataPaths {
    /// 起動環境の値を一度だけ読み取る。
    pub fn from_env() -> Result<Self, ConfigError> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from);
        let temporary = std::env::var_os("TMPDIR").map(PathBuf::from);
        Self::from_values(
            home.as_deref(),
            config.as_deref(),
            state.as_deref(),
            temporary.as_deref(),
        )
    }

    /// 環境変数を変更せず、起動時に採った値から置き場所を決める。
    pub fn from_values(
        home: Option<&Path>,
        xdg_config: Option<&Path>,
        xdg_state: Option<&Path>,
        temporary: Option<&Path>,
    ) -> Result<Self, ConfigError> {
        let home = home
            .filter(|path| path.is_absolute())
            .ok_or(ConfigError::Invalid(text::HOME_REQUIRED))?;
        let config_dir = nonempty(xdg_config)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| home.join(".config"))
            .join("benitoiteh");
        let data_dir = nonempty(xdg_state)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| home.join(".local/state"))
            .join("benitoiteh");
        let temporary_root = nonempty(temporary)
            .unwrap_or(Path::new("/tmp"))
            .to_path_buf();
        if [&config_dir, &data_dir, &temporary_root]
            .iter()
            .any(|path| !path.is_absolute())
        {
            return Err(ConfigError::Invalid(text::INVALID_PATH));
        }
        Ok(Self {
            home: home.to_path_buf(),
            config_file: config_dir.join("config.toml"),
            instructions_file: config_dir.join("AGENTS.md"),
            sessions_dir: data_dir.join("sessions"),
            history_dir: data_dir.join("history"),
            auth_dir: data_dir.join("auth"),
            config_dir,
            data_dir,
            temporary_root,
        })
    }

    /// まだ作成していないセッションの一時ディレクトリを返す。
    pub fn temporary_dir(&self, id: &str) -> Result<PathBuf, ConfigError> {
        let relative = crate::safe_fs::ScopeRelPath::new(id)
            .map_err(|_| ConfigError::Invalid(text::INVALID_ID))?;
        if relative.as_path().components().count() != 1 {
            return Err(ConfigError::Invalid(text::INVALID_ID));
        }
        Ok(self.temporary_root.join(format!("benitoiteh-{id}")))
    }
}

fn nonempty(path: Option<&Path>) -> Option<&Path> {
    path.filter(|path| !path.as_os_str().is_empty())
}
