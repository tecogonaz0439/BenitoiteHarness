//! bubblewrap の見え方に、読み書きと実行の権利を重ねる（設計書 03-02、実装プラン P0-07）。

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use landlock::{
    ABI, Access, AccessFs, BitFlags, CompatLevel, Compatible, LandlockStatus, PathBeneath, Ruleset,
    RulesetAttr, RulesetCreated, RulesetCreatedAttr, RulesetStatus, Scope,
};
use rustix::fs::{CWD, Mode, OFlags, ResolveFlags, openat2};

use super::{SandboxError, SandboxPolicy, failure, text};
use crate::sandbox::{ProgramAccess, ReadAccess};

type Result<T> = std::result::Result<T, SandboxError>;

pub(super) fn restrict(policy: &SandboxPolicy, required: u64) -> Result<()> {
    let config = policy.config();
    let mut rules = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V3))
        .map_err(error)?
        .create()
        .map_err(error)?;
    let readable = match &config.readable {
        ReadAccess::All => vec![PathBuf::from("/")],
        ReadAccess::Roots(roots) => roots.clone(),
    };
    for root in readable {
        grant(
            &mut rules,
            &root,
            // from_read(V3) は Execute も含む。実行は programs の規則だけで許す。
            // 同じ層の許可は足し合わされるため、読み取りに混ぜると一覧を迂回できる
            // （設計書 03-02「方針の要素」「制限モードで実行を許すプログラム」）。
            AccessFs::ReadFile | AccessFs::ReadDir,
        )?;
    }
    for path in &config.readable_data_roots {
        grant(&mut rules, path, AccessFs::ReadFile | AccessFs::ReadDir)?;
    }
    for path in &config.readable_files {
        grant(&mut rules, path, AccessFs::ReadFile.into())?;
    }
    match &config.programs {
        ProgramAccess::Unrestricted => {
            let excluded = if config.deny_exec_in_writable {
                config.writable.as_slice()
            } else {
                &[]
            };
            grant_tree(
                &mut rules,
                Path::new("/"),
                excluded,
                AccessFs::Execute.into(),
            )?;
        }
        ProgramAccess::Only(programs) => {
            for program in programs {
                if !config.deny_exec_in_writable
                    || !config.writable.iter().any(|root| program.starts_with(root))
                {
                    let file = open_path(program).map_err(error)?;
                    if !file.metadata().map_err(error)?.is_file() {
                        return Err(error(text::INVALID_PROGRAM));
                    }
                    (&mut rules)
                        .add_rule(PathBeneath::new(&file, AccessFs::Execute))
                        .map_err(error)?;
                }
            }
        }
    }
    for root in &config.writable {
        grant(&mut rules, root, AccessFs::from_write(ABI::V3))?;
    }
    // Stdio::null() とシェルの出力の破棄を、書ける場所の指定によらず許す。
    // bubblewrap が作った /dev/null を補助のプロセスで開き、その対象にだけ
    // 書き込みを許す（設計書 03-02「方針の要素」）。
    let null = open_path(Path::new("/dev/null")).map_err(error)?;
    (&mut rules)
        .add_rule(PathBeneath::new(&null, AccessFs::WriteFile))
        .map_err(error)?;
    let status = rules.restrict_self().map_err(error)?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err(error(text::NOT_ENFORCED));
    }
    // 必須の規則を弱めず、より新しい ABI を要求した場合にだけ失敗させる。
    let abi = match status.landlock {
        LandlockStatus::Available {
            effective_abi,
            kernel_abi,
        } => u64::try_from(kernel_abi.unwrap_or(effective_abi as i32)).unwrap_or(0),
        LandlockStatus::NotEnabled | LandlockStatus::NotImplemented => 0,
    };
    if abi < required {
        return Err(error(text::ABI_REQUIRED));
    }
    // 必須 V3 は別の規則として適用済み。追加の権利がないカーネルでも V3 を維持する。
    // REFER は、扱うと指定しない層でも既定で拒否されるため、この層でも書ける場所に与える。
    // V2 からある権利なので、必須 V3 を満たす環境では BestEffort でも省略されない。
    // https://docs.kernel.org/userspace-api/landlock.html の LANDLOCK_ACCESS_FS_REFER と、
    // 設計書 03-02「Linux のファイルシステムの見え方」の「書ける場所」に従う。
    let mut extra_rules = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::Refer | AccessFs::IoctlDev | AccessFs::ResolveUnix)
        .map_err(error)?
        .scope(Scope::AbstractUnixSocket)
        .map_err(error)?
        .create()
        .map_err(error)?;
    for root in &config.writable {
        grant(&mut extra_rules, root, AccessFs::Refer.into())?;
    }
    extra_rules.restrict_self().map_err(error)?;
    Ok(())
}

fn error(error: impl std::fmt::Display) -> SandboxError {
    failure(text::LANDLOCK, error)
}

fn open_path(path: &Path) -> std::io::Result<File> {
    // 検査したパスの途中が置き換わってもシンボリックリンクへ権利を渡さない。
    // V3 のカーネルより前からある openat2 を使い、使えなければ適用に失敗させる。
    let fd = openat2(
        CWD,
        path,
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )?;
    Ok(File::from(fd))
}

fn grant(rules: &mut RulesetCreated, path: &Path, access: BitFlags<AccessFs>) -> Result<()> {
    let file = match open_path(path) {
        Ok(file) => file,
        // 探索中に消えた項目には権利を与えない（実装プラン P0-07「辿る途中で消えた項目」）。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(failure) => return Err(error(failure)),
    };
    let metadata = file.metadata().map_err(error)?;
    // シンボリックリンクを経た別名に許可を与えない。
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    let access = if metadata.is_dir() {
        access
    } else {
        access & !AccessFs::ReadDir
    };
    if !access.is_empty() {
        rules
            .add_rule(PathBeneath::new(&file, access))
            .map_err(error)?;
    }
    Ok(())
}

fn grant_tree(
    rules: &mut RulesetCreated,
    root: &Path,
    excluded: &[PathBuf],
    access: BitFlags<AccessFs>,
) -> Result<()> {
    if excluded.iter().any(|path| root.starts_with(path)) {
        return Ok(());
    }
    if !excluded.iter().any(|path| path.starts_with(root)) {
        return grant(rules, root, access);
    }
    // 実行の権利だけを、書ける場所を除いた部分木に与える。読み取りには使わない。
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(failure) => return Err(error(failure)),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => continue,
            Err(failure) => return Err(error(failure)),
        };
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => continue,
            Err(failure) => return Err(error(failure)),
        };
        if !kind.is_symlink() {
            grant_tree(rules, &entry.path(), excluded, access)?;
        }
    }
    Ok(())
}
