//! 通常モードの方針の構築を、検査済みの実パスで確かめる（実装プラン P3-02）。

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use benitoiteh::config::{Sandbox, paths::DataPaths, writable::validate_writable_with_executable};
use benitoiteh::sandbox::{ProgramAccess, ReadAccess, normal_policy};
use benitoiteh::session::SessionId;

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

// 作成時の関門: 三つの方針の契約と、存在しない秘密の候補で起動を拒まない条件。
// ここでは方針の生成を守り、OS による拒否の検査は sandbox_* に任せる。
#[test]
fn normal_policies_use_validated_roots_and_existing_denied_candidates() -> Result<(), Box<dyn Error>>
{
    let id = SessionId::generate(SystemTime::now())?;
    let root = std::env::temp_dir().join(format!("benitoiteh-normal-policy-{id}"));
    fs::create_dir(&root)?;
    let f = Fixture(fs::canonicalize(root)?);
    for name in [
        "home/.ssh",
        "project",
        "tmp",
        "additional",
        "config/benitoiteh",
        "state/benitoiteh",
    ] {
        fs::create_dir_all(f.0.join(name))?;
    }
    let paths = DataPaths::from_values(
        Some(&f.0.join("home")),
        Some(&f.0.join("config")),
        Some(&f.0.join("state")),
        Some(&f.0.join("tmp")),
    )?;
    let locations = validate_writable_with_executable(
        &paths,
        &f.0.join("project"),
        &f.0.join("tmp"),
        &[f.0.join("additional")],
        &PathBuf::from(env!("CARGO_BIN_EXE_benitoiteh")),
    )?;
    let candidates = vec![
        paths.home.join(".ssh"),
        paths.home.join(".gnupg"),
        paths.home.join(".aws"),
        paths.config_dir.clone(),
        paths.data_dir.clone(),
    ];
    for (sandbox, network, writable, allow_ip) in [
        (
            Sandbox::ReadOnly,
            true,
            vec![locations.temporary.clone()],
            false,
        ),
        (
            Sandbox::WorkspaceWrite,
            false,
            vec![
                locations.project.clone(),
                locations.temporary.clone(),
                f.0.join("additional"),
            ],
            false,
        ),
        (
            Sandbox::WorkspaceWrite,
            true,
            vec![
                locations.project.clone(),
                locations.temporary.clone(),
                f.0.join("additional"),
            ],
            true,
        ),
    ] {
        let policy =
            normal_policy(sandbox, &locations, network, &candidates)?.ok_or("missing policy")?;
        let config = policy.config();
        assert_eq!(config.writable, writable);
        assert_eq!(config.allow_ip, allow_ip);
        assert_eq!(
            config.denied_read,
            vec![
                paths.home.join(".ssh"),
                paths.config_dir.clone(),
                paths.data_dir.clone()
            ]
        );
        assert_eq!(config.readable, ReadAccess::All);
        assert_eq!(config.programs, ProgramAccess::Unrestricted);
        assert_eq!(config.protected_names, [".git", ".benitoiteh"]);
        assert!(!config.deny_exec_in_writable);
    }
    assert!(normal_policy(Sandbox::DangerFullAccess, &locations, false, &candidates)?.is_none());
    Ok(())
}
