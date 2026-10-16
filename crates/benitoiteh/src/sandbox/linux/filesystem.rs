//! 読ませない部分木を除き、既存ファイルだけに書く権利を与える（設計書 03-02、BH-049、実装プラン P0-05）。

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
        grant_tree(
            &mut rules,
            &root,
            &config.denied_read,
            // from_read(V3) は Execute も含む。実行は programs の規則だけで許す。
            // 同じ層の許可は足し合わされるため、読み取りに混ぜると一覧を迂回できる
            // （設計書 03-02「方針の要素」「制限モードで実行を許すプログラム」）。
            AccessFs::ReadFile | AccessFs::ReadDir,
        )?;
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
                    let file = open_path(program)?;
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
        grant_existing_files(&mut rules, root, &config.protected_names)?;
    }
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
    Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::IoctlDev | AccessFs::ResolveUnix)
        .map_err(error)?
        .scope(Scope::AbstractUnixSocket)
        .map_err(error)?
        .create()
        .map_err(error)?
        .restrict_self()
        .map_err(error)?;
    Ok(())
}

fn error(error: impl std::fmt::Display) -> SandboxError {
    failure(text::LANDLOCK, error)
}

fn open_path(path: &Path) -> Result<File> {
    // 検査したパスの途中が置き換わってもシンボリックリンクへ権利を渡さない。
    // V3 のカーネルより前からある openat2 を使い、使えなければ適用に失敗させる。
    let fd = openat2(
        CWD,
        path,
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )
    .map_err(error)?;
    Ok(File::from(fd))
}

fn grant(rules: &mut RulesetCreated, path: &Path, access: BitFlags<AccessFs>) -> Result<()> {
    let file = open_path(path)?;
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
    // ReadDir も子孫へ継承されるので、除外先の祖先には一切の権利を与えない。
    // このため、その祖先の一覧も読めないが、許した兄弟のパスを直接開くことはできる。
    // 内容も一覧も秘密から除くための制約である（設計書 03-02「サンドボックスの掛け方」）。
    for entry in fs::read_dir(root).map_err(error)? {
        let entry = entry.map_err(error)?;
        if !entry.file_type().map_err(error)?.is_symlink() {
            grant_tree(rules, &entry.path(), excluded, access)?;
        }
    }
    Ok(())
}

fn grant_existing_files(
    rules: &mut RulesetCreated,
    root: &Path,
    protected: &[String],
) -> Result<()> {
    // ディレクトリに MakeReg/MakeDir/Remove*/Refer を与えると、まだない保護名を
    // 作ることも許してしまう。試作では全階層で作成・削除・rename を拒み、既存の
    // 通常ファイルの更新に限る。新しいディレクトリの中の保護名も許す形にはしない。
    // この制約と、名前を付け替えて書く道具との不一致を P0-05 の結果として報告する。
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        if directory.components().any(|part| {
            protected
                .iter()
                .any(|name| part.as_os_str().eq_ignore_ascii_case(name))
        }) {
            continue;
        }
        for entry in fs::read_dir(directory).map_err(error)? {
            let entry = entry.map_err(error)?;
            if protected
                .iter()
                .any(|name| entry.file_name().eq_ignore_ascii_case(name))
            {
                continue;
            }
            let path = entry.path();
            let file = open_path(&path)?;
            let metadata = file.metadata().map_err(error)?;
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                rules
                    .add_rule(PathBeneath::new(
                        &file,
                        AccessFs::WriteFile | AccessFs::Truncate,
                    ))
                    .map_err(error)?;
            }
        }
    }
    Ok(())
}
