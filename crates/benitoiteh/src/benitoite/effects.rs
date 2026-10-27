//! 処理系の原子エフェクトを確認と OS の方針へ変換する（設計書 04-01、03-02）。

use crate::sandbox::{ReadAccess, RestrictedAccess};
use std::collections::BTreeSet;
use std::path::Path;

/// `--allow-effects` と検査の報告が共有する十四の名前。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Effect {
    State,
    Log,
    ConsoleRead,
    ConsoleWrite,
    FileRead,
    FileWrite,
    ProcessRun,
    ProcessExit,
    ProcessEnvironment,
    ClockTime,
    RandomGenerate,
    HttpConnect,
    HttpListen,
    AssertCheck,
}

impl Effect {
    /// 処理系の綴り。列挙の順序を表示の順序に使わない。
    pub const fn name(self) -> &'static str {
        match self {
            Self::State => "State",
            Self::Log => "Log",
            Self::ConsoleRead => "Console.Read",
            Self::ConsoleWrite => "Console.Write",
            Self::FileRead => "File.Read",
            Self::FileWrite => "File.Write",
            Self::ProcessRun => "Process.Run",
            Self::ProcessExit => "Process.Exit",
            Self::ProcessEnvironment => "Process.Environment",
            Self::ClockTime => "Clock.Time",
            Self::RandomGenerate => "Random.Generate",
            Self::HttpConnect => "Http.Connect",
            Self::HttpListen => "Http.Listen",
            Self::AssertCheck => "Assert.Check",
        }
    }
    /// 知らない名前は、空の集合へ読み替えずに拒む。
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "State" => Self::State,
            "Log" => Self::Log,
            "Console.Read" => Self::ConsoleRead,
            "Console.Write" => Self::ConsoleWrite,
            "File.Read" => Self::FileRead,
            "File.Write" => Self::FileWrite,
            "Process.Run" => Self::ProcessRun,
            "Process.Exit" => Self::ProcessExit,
            "Process.Environment" => Self::ProcessEnvironment,
            "Clock.Time" => Self::ClockTime,
            "Random.Generate" => Self::RandomGenerate,
            "Http.Connect" => Self::HttpConnect,
            "Http.Listen" => Self::HttpListen,
            "Assert.Check" => Self::AssertCheck,
            _ => return None,
        })
    }
}

/// 綴りの順に揃える。表明は表示と確認だけで省く。
pub fn names(effects: &BTreeSet<Effect>, include_assert: bool) -> Vec<String> {
    let mut names: Vec<_> = effects
        .iter()
        .filter(|e| include_assert || **e != Effect::AssertCheck)
        .map(|e| e.name().to_owned())
        .collect();
    names.sort();
    names
}

/// 今回の報告だけから起動の権限を作る。過去の広い許可は渡さない。
pub fn restricted_access(
    effects: &BTreeSet<Effect>,
    processor: &Path,
    allowed: &[std::path::PathBuf],
) -> RestrictedAccess {
    let mut programs = vec![processor.to_owned()];
    if effects.contains(&Effect::ProcessRun) {
        programs.extend_from_slice(allowed);
    }
    RestrictedAccess {
        write_project: effects.contains(&Effect::FileWrite),
        allow_ip: effects.contains(&Effect::HttpConnect) || effects.contains(&Effect::HttpListen),
        programs,
        readable: ReadAccess::All,
    }
}

/// 検証済みの利用者の設定を、既定の確認なしの範囲と合わせる（設計書 03-03）。
pub fn scratch_auto_effects(configured: &[String]) -> BTreeSet<Effect> {
    [
        Effect::ConsoleWrite,
        Effect::Log,
        Effect::State,
        Effect::ProcessExit,
    ]
    .into_iter()
    .chain(configured.iter().filter_map(|name| Effect::from_name(name)))
    .collect()
}
