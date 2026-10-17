//! doctor の子プロセスを利用者の設定から隔離する（実装プラン P4-05「テスト」）。

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// 子プロセスに渡すホームと設定と保存先を所有する。
pub struct Fixture {
    pub root: PathBuf,
    pub project: PathBuf,
}

impl Fixture {
    /// テストごとの独立した根を作る。
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let mut random = [0; 16];
        getrandom::fill(&mut random)?;
        let root = std::env::temp_dir().join(format!(
            "bh-doctor-test-{:032x}",
            u128::from_ne_bytes(random)
        ));
        fs::create_dir(&root)?;
        let fixture = Self {
            project: root.join("project"),
            root,
        };
        for name in ["home", "config", "state", "tmp", "project"] {
            fs::create_dir(fixture.root.join(name))?;
        }
        Ok(fixture)
    }

    /// 利用者の設定を専用の場所に置く。
    pub fn write_config(&self, input: &str) -> Result<(), Box<dyn Error>> {
        let directory = self.root.join("config/benitoiteh");
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("config.toml"), input)?;
        Ok(())
    }

    /// 親の設定とキーを引き継がない doctor の起動を作る。
    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_benitoiteh"));
        // 環境全体を消すので、親のキーと利用者の設定への参照を一切引き継がない。
        command
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("TMPDIR", self.root.join("tmp"))
            .env_remove("BENITOITEH_DOCTOR_TEST_KEY")
            .current_dir(&self.project)
            .arg("doctor");
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.root);
    }
}
