//! 巻き戻しの保存領域を本物の子と OS のサンドボックスから守る（設計書 01-05、07-01）。

#[cfg(any(target_os = "macos", all(target_os = "linux", target_arch = "x86_64")))]
#[path = "support/bash.rs"]
mod fixture;

// 作成時の関門: 主プロセスが objects/ を書けても、同じセッションの子が書ける退行を
// 実際の bash の実行から捕まえる。宣言した方針の文字列だけを比べない。
#[cfg(any(target_os = "macos", all(target_os = "linux", target_arch = "x86_64")))]
#[test]
fn sandboxed_child_cannot_change_checkpoint_objects_or_session()
-> Result<(), Box<dyn std::error::Error>> {
    use benitoiteh::config::Sandbox;
    use std::fs;
    use std::sync::atomic::AtomicBool;
    let f = fixture::Fixture::new(Sandbox::WorkspaceWrite)?;
    let (hash, _) = f
        .session
        .store_object(b"saved checkpoint")?
        .ok_or("budget")?;
    let object = f.session.directory().join("objects").join(hash);
    let log = f.session.directory().join("session.jsonl");
    let before = fs::read(&log)?;
    let inside = f.execute(
        "inside",
        serde_json::json!({"command":"printf allowed > allowed"}),
        &AtomicBool::new(false),
    )?;
    assert!(!inside.is_error, "{}", inside.text);
    assert_eq!(fs::read(f.project.join("allowed"))?, b"allowed");
    for (id, path) in [
        ("object", object.clone()),
        ("session", log.clone()),
        ("new-object", f.session.directory().join("objects/new")),
    ] {
        let quoted = format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        let output = f.execute(
            id,
            serde_json::json!({"command":format!("printf corrupt > {quoted}")}),
            &AtomicBool::new(false),
        )?;
        assert!(output.is_error, "{}", output.text);
    }
    assert_eq!(fs::read(object)?, b"saved checkpoint");
    assert_eq!(fs::read(log)?, before);
    assert!(!f.session.directory().join("objects/new").exists());
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_no_symlinks_rejects_inside_and_intermediate_links()
-> Result<(), Box<dyn std::error::Error>> {
    use benitoiteh::safe_fs::{ScopeRelPath, ScopeRoot, verify_platform_support};
    let id = benitoiteh::session::SessionId::generate(std::time::SystemTime::now())?;
    let path = std::env::temp_dir().join(format!("bh-rewind-no-symlinks-{id}"));
    std::fs::create_dir(&path)?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _removed = std::fs::remove_dir_all(&self.0);
        }
    }
    let cleanup = Cleanup(path);
    verify_platform_support(&cleanup.0)?;
    std::fs::create_dir(cleanup.0.join("directory"))?;
    std::fs::write(cleanup.0.join("directory/file"), "inside")?;
    std::os::unix::fs::symlink("directory", cleanup.0.join("link"))?;
    let root = ScopeRoot::open(&cleanup.0)?.no_follow()?;
    assert!(
        root.open_read(&ScopeRelPath::new("directory/file")?)
            .is_ok()
    );
    // 一つの open_read が全パスを openat2 に渡す。最終要素だけの NOFOLLOW や
    // 一要素ずつの事前の解決では拒まれないリンクを、NO_SYMLINKS 自体で拒む。
    assert!(root.open_read(&ScopeRelPath::new("link/file")?).is_err());
    assert!(root.open_dir(&ScopeRelPath::new("link")?).is_err());
    Ok(())
}
