//! 信頼の決定の保存の境界を、本物のファイルシステムで確かめる（設計書 03-01「プロジェクトの信頼」）。

use super::*;
use std::error::Error;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random)?;
        let root =
            std::env::temp_dir().join(format!("bh-trust-{:032x}", u128::from_ne_bytes(random)));
        std::fs::create_dir(&root)?;
        Ok(Self(std::fs::canonicalize(root)?))
    }
    fn paths(&self) -> Result<DataPaths, Box<dyn Error>> {
        Ok(DataPaths::from_values(
            Some(&self.0.join("home")),
            None,
            Some(&self.0.join("state")),
            Some(&self.0.join("tmp")),
        )?)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

// 作成時の関門: 決定は根ごとに分かれ、置き換えで上書きでき、次に開いても読める。
// 読む側は、ほかの利用者の形（権限、リンク、別の根の記録）を決定として使わない。
#[test]
fn decisions_are_kept_per_root_and_rejected_when_tampered() -> TestResult {
    let f = Fixture::new()?;
    let paths = f.paths()?;
    let first = f.0.join("first");
    let second = f.0.join("second");
    let store = TrustStore::open(&paths)?;
    assert_eq!(store.get(&first)?, None);
    store.set(&first, true)?;
    store.set(&second, false)?;
    let store = TrustStore::open(&paths)?;
    assert_eq!(store.get(&first)?, Some(true));
    assert_eq!(store.get(&second)?, Some(false));
    store.set(&first, false)?;
    assert_eq!(store.get(&first)?, Some(false));
    let directory = paths.data_dir.join("trust");
    assert_eq!(
        std::fs::metadata(&directory)?.permissions().mode() & 0o7777,
        0o700
    );
    let file = directory.join(name(&first));
    assert_eq!(
        std::fs::metadata(&file)?.permissions().mode() & 0o7777,
        0o600
    );
    let leftovers = std::fs::read_dir(&directory)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(leftovers, 0);
    // 別の根の記録を、この根の名前に置いても使わない。
    std::fs::copy(directory.join(name(&second)), &file)?;
    assert!(store.get(&first).is_err());
    // リンクと、ほかの者が読める権限の記録も使わない。
    std::fs::remove_file(&file)?;
    symlink(directory.join(name(&second)), &file)?;
    assert!(store.get(&first).is_err());
    std::fs::remove_file(&file)?;
    store.set(&first, true)?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
    assert!(store.get(&first).is_err());
    Ok(())
}
