//! マウントの順序と、起動前後に所有する資源を決める（設計書 03-02「Linux のファイルシステムの見え方」）。

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::super::{SandboxError, SandboxPolicy, policy, text};

pub(super) struct MountPlan {
    pub args: Vec<OsString>,
    pub cleanup: Cleanup,
}

#[derive(Default)]
pub(super) struct Cleanup {
    pub directories: Vec<PathBuf>,
    pub cover: Option<PathBuf>,
}

impl Cleanup {
    pub fn finish(&mut self) -> io::Result<()> {
        let mut failure = None;
        // 同じ根の並行実行と強制終了の残骸の管理は BH-095 に残す。
        // Drop では呼ばず、子の wait が成功した後か、spawn の失敗時だけに呼ぶ。
        for path in self.directories.drain(..).rev() {
            if let Err(error) = fs::remove_dir(path)
                && !matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::DirectoryNotEmpty
                        | io::ErrorKind::NotADirectory
                )
            {
                failure.get_or_insert(error);
            }
        }
        if let Some(path) = self.cover.take()
            && let Err(error) = fs::remove_file(path)
            && error.kind() != io::ErrorKind::NotFound
        {
            failure.get_or_insert(error);
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

pub(super) fn build(
    policy: &SandboxPolicy,
    cwd: &Path,
    scratch: &Path,
) -> Result<MountPlan, SandboxError> {
    let config = policy.config();
    let mut plan = MountPlan {
        args: [
            "--unshare-user",
            "--unshare-pid",
            "--as-pid-1",
            "--unshare-ipc",
            "--disable-userns",
            "--new-session",
            "--die-with-parent",
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
        cleanup: Cleanup::default(),
    };
    if !config.allow_ip {
        plan.args.push("--unshare-net".into());
    }
    plan.args
        .extend(["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"].map(OsString::from));
    let mut roots = config.writable.clone();
    roots.sort();
    roots.dedup();
    for root in &roots {
        bind(&mut plan.args, "--bind", root, root);
    }
    let mut protected = BTreeSet::new();
    let mut gitdirs = BTreeSet::new();
    for root in &roots {
        scan(policy, root, 1, &mut protected)?;
        for name in &config.protected_names {
            let path = root.join(name);
            check_links(&path, &roots)?;
            if metadata(&path)?.is_none() {
                plan.cleanup.directories.push(path);
            }
        }
        let git = root.join(".git");
        if metadata(&git)?.is_some_and(|metadata| metadata.is_file()) {
            let contents = fs::read_to_string(&git).map_err(SandboxError::Io)?;
            let target = contents
                .strip_prefix("gitdir:")
                .map(str::trim)
                .filter(|target| !target.is_empty() && !target.contains(['\n', '\r']))
                .ok_or(SandboxError::InvalidPolicy(text::INVALID_GITDIR))?;
            let target = root.join(target);
            check_links(&target, &roots)?;
            let real = fs::canonicalize(&target).map_err(SandboxError::Io)?;
            // commondir と入れ子の .git ファイルの指す先は辿らない（実装プラン P0-07）。
            if roots.iter().any(|root| real.starts_with(root)) {
                gitdirs.insert(real);
            }
        }
    }
    for path in protected {
        bind(&mut plan.args, "--ro-bind", &path, &path);
    }
    for path in &plan.cleanup.directories {
        tmpfs(&mut plan.args, "555", path);
    }
    for path in gitdirs {
        bind(&mut plan.args, "--ro-bind", &path, &path);
    }
    let mut denied = config.denied_read.clone();
    denied.sort();
    denied.dedup();
    let mut outer = Vec::<PathBuf>::new();
    let mut files = Vec::new();
    for path in denied {
        check_links(&path, &roots)?;
        if outer.iter().any(|parent| path.starts_with(parent)) {
            continue;
        }
        let Some(metadata) = metadata(&path)? else {
            continue;
        };
        if metadata.is_dir() {
            tmpfs(&mut plan.args, "000", &path);
        } else if metadata.is_file() {
            files.push(path.clone());
        } else {
            return Err(SandboxError::InvalidPolicy(text::INVALID_DENIED_TYPE));
        }
        outer.push(path);
    }
    if !files.is_empty() {
        let cover = create_cover(scratch, &roots)?;
        for path in files {
            bind(&mut plan.args, "--ro-bind", &cover, &path);
        }
        plan.cleanup.cover = Some(cover);
    }
    plan.args.push("--chdir".into());
    plan.args.push(cwd.as_os_str().to_owned());
    Ok(plan)
}

fn metadata(path: &Path) -> Result<Option<fs::Metadata>, SandboxError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SandboxError::Io(error)),
    }
}

fn scan(
    policy: &SandboxPolicy,
    root: &Path,
    depth: u8,
    protected: &mut BTreeSet<PathBuf>,
) -> Result<(), SandboxError> {
    for entry in fs::read_dir(root).map_err(SandboxError::Io)? {
        let entry = entry.map_err(SandboxError::Io)?;
        let path = entry.path();
        let Some(metadata) = metadata(&path)? else {
            continue;
        };
        if policy
            .config()
            .protected_names
            .iter()
            .any(|name| entry.file_name().eq_ignore_ascii_case(name))
        {
            check_links(&path, &policy.config().writable)?;
            protected.insert(path);
        } else if depth < 3 && metadata.is_dir() {
            // lstat の結果で辿るため、リンクの先の木を探索しない。
            scan(policy, &path, depth.saturating_add(1), protected)?;
        }
    }
    Ok(())
}

fn check_links(path: &Path, writable: &[PathBuf]) -> Result<(), SandboxError> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component);
        if let Some(metadata) = metadata(&prefix)?
            && metadata.file_type().is_symlink()
            && writable.iter().any(|root| prefix.starts_with(root))
        {
            return Err(SandboxError::InvalidPolicy(text::WRITABLE_SYMLINK));
        }
    }
    Ok(())
}

fn bind(args: &mut Vec<OsString>, kind: &str, from: &Path, to: &Path) {
    args.extend([
        OsString::from(kind),
        from.as_os_str().to_owned(),
        to.as_os_str().to_owned(),
    ]);
}

fn tmpfs(args: &mut Vec<OsString>, mode: &str, path: &Path) {
    args.extend([
        OsString::from("--perms"),
        OsString::from(mode),
        OsString::from("--tmpfs"),
        path.as_os_str().to_owned(),
        OsString::from("--remount-ro"),
        path.as_os_str().to_owned(),
    ]);
}

fn create_cover(scratch: &Path, writable: &[PathBuf]) -> Result<PathBuf, SandboxError> {
    policy::canonical_path(scratch)?;
    policy::directory(scratch)?;
    if writable.iter().any(|root| scratch.starts_with(root)) {
        return Err(SandboxError::InvalidPolicy(text::WRITABLE_COVER));
    }
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|error| SandboxError::Io(io::Error::other(error)))?;
    let nonce = u128::from_ne_bytes(random);
    let path = scratch.join(format!(".benitoiteh-cover-{nonce:032x}"));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o000)
        .open(&path)
        .map_err(SandboxError::Io)?;
    let checked = file.metadata().and_then(|metadata| {
        if metadata.is_file() && metadata.len() == 0 && metadata.permissions().mode() & 0o7777 == 0
        {
            Ok(())
        } else {
            Err(io::Error::other(text::INVALID_COVER))
        }
    });
    if let Err(error) = checked {
        let _cleanup = fs::remove_file(&path);
        return Err(SandboxError::Io(error));
    }
    Ok(path)
}

#[cfg(test)]
mod tests;
