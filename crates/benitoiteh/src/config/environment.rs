//! 起動の形によらず、利用者の規則を当てて秘密を除いた子プロセスの環境を作る
//! （設計書 03-02「子プロセスの環境」、04-01「処理系の起動」）。

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::{PROVIDER_KEY_ENV_VARS, text};

/// 環境の規則の一行（設計書 03-02「環境の規則」）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnvironmentRule {
    /// 親の環境のうちパターンに合うものを、親の値で入れる。
    Allow(String),
    /// 今の環境からパターンに合うものを除く。
    Deny(String),
    /// 名前に値を入れる。値は展開しない。
    Set(String, String),
}

/// 上から順に当てる環境の規則の並び。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentRules {
    rules: Vec<EnvironmentRule>,
    written: bool,
}

/// 書かなければ `allow *` の一行と同じに扱う（設計書 03-02「環境の規則」）。
impl Default for EnvironmentRules {
    fn default() -> Self {
        Self {
            rules: vec![EnvironmentRule::Allow("*".to_owned())],
            written: false,
        }
    }
}

/// 規則の行の誤り。値を含めないよう、行の番号と理由だけを持つ。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleError {
    /// 0 から数えた行の位置。
    pub index: usize,
    pub reason: &'static str,
}

/// `benitoiteh` が規則の後に必ず設定する名前。`BENITOITEH_` で始まる名前も含む
/// （設計書 05-01「設定ファイル」）。
const FIXED_NAMES: &[&str] = &["TMPDIR", "NO_COLOR", "TERM", "PAGER", "GIT_PAGER"];

impl EnvironmentRules {
    /// 利用者が書いた行を読む。`provider_keys` は設定で参照した提供者のキーの変数
    /// （固定の一覧は別に確かめる）。
    pub fn parse(lines: &[String], provider_keys: &BTreeSet<String>) -> Result<Self, RuleError> {
        let provider_key = |name: &str| {
            PROVIDER_KEY_ENV_VARS
                .iter()
                .copied()
                .chain(provider_keys.iter().map(String::as_str))
                .any(|key| key.eq_ignore_ascii_case(name))
        };
        let rules = lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                parse_line(line, &provider_key).map_err(|reason| RuleError { index, reason })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            rules,
            written: true,
        })
    }

    /// 当てる行の並び。
    pub fn rules(&self) -> &[EnvironmentRule] {
        &self.rules
    }

    /// 利用者が書いた規則か（書かなければ既定の `allow *`）。
    pub fn written(&self) -> bool {
        self.written
    }
}

impl std::fmt::Display for EnvironmentRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allow(pattern) => write!(f, "allow {pattern}"),
            Self::Deny(pattern) => write!(f, "deny {pattern}"),
            // 値は秘密でありうるので示さない。
            Self::Set(name, _) => write!(f, "set {name}=(value omitted)"),
        }
    }
}

fn parse_line(
    line: &str,
    provider_key: &impl Fn(&str) -> bool,
) -> Result<EnvironmentRule, &'static str> {
    // 語と引数の間は空白一つ。二つ目の空白は引数の誤りとして断る（設計書 05-01「設定ファイル」）。
    let (word, argument) = line.split_once(' ').unwrap_or((line, ""));
    match word {
        "allow" => {
            let pattern = pattern(argument)?;
            // `*` のない allow は一つの名前を指す。提供者のキーなら、利用者は通したつもりになる。
            if !pattern.contains('*') && provider_key(&pattern) {
                return Err(text::RULE_ALLOW_PROVIDER_KEY);
            }
            Ok(EnvironmentRule::Allow(pattern))
        }
        "deny" => Ok(EnvironmentRule::Deny(pattern(argument)?)),
        "set" => {
            let (name, value) = argument.split_once('=').ok_or(text::RULE_MALFORMED)?;
            let mut bytes = name.bytes();
            if !bytes
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
                || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || value.contains('\0')
            {
                return Err(text::RULE_MALFORMED);
            }
            // 規則の後に benitoiteh が打ち消す名前は、黙って打ち消さずに断る。
            if provider_key(name)
                || FIXED_NAMES
                    .iter()
                    .any(|fixed| fixed.eq_ignore_ascii_case(name))
                || name
                    .get(..BENITOITEH_PREFIX.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(BENITOITEH_PREFIX))
                || crate::askpass::ENVIRONMENT_NAMES
                    .iter()
                    .any(|fixed| fixed.eq_ignore_ascii_case(name))
            {
                return Err(text::RULE_SET_FIXED);
            }
            Ok(EnvironmentRule::Set(name.to_owned(), value.to_owned()))
        }
        _ => Err(text::RULE_UNKNOWN),
    }
}

const BENITOITEH_PREFIX: &str = "BENITOITEH_";

fn pattern(argument: &str) -> Result<String, &'static str> {
    if argument.is_empty() {
        return Err(text::RULE_EMPTY_PATTERN);
    }
    if argument.contains(|c: char| c.is_whitespace() || c == '=' || c == '\0') {
        return Err(text::RULE_MALFORMED);
    }
    Ok(argument.to_owned())
}

/// 空の環境から行を上から順に当てる。
fn apply_rules(
    rules: &[EnvironmentRule],
    inherited: &BTreeMap<OsString, OsString>,
) -> BTreeMap<OsString, OsString> {
    let mut environment = BTreeMap::new();
    for rule in rules {
        match rule {
            EnvironmentRule::Allow(pattern) => environment.extend(
                inherited
                    .iter()
                    .filter(|(name, _)| matches(pattern, name))
                    .map(|(name, value)| (name.clone(), value.clone())),
            ),
            EnvironmentRule::Deny(pattern) => environment.retain(|name, _| !matches(pattern, name)),
            EnvironmentRule::Set(name, value) => {
                environment.insert(name.into(), value.into());
            }
        }
    }
    environment
}

/// `*` だけを長さ 0 以上の任意の並びとし、ASCII の大文字と小文字を区別せずに名前の全体と比べる。
/// UTF-8 でない名前も、バイト列として比べる（設計書 03-02「環境の規則」）。
fn matches(pattern: &str, name: &OsStr) -> bool {
    let name = name.as_encoded_bytes();
    let parts: Vec<&[u8]> = pattern.as_bytes().split(|byte| *byte == b'*').collect();
    let [first, middle @ .., last] = parts.as_slice() else {
        return name.eq_ignore_ascii_case(pattern.as_bytes());
    };
    let Some(rest) = name
        .get(..first.len())
        .filter(|head| head.eq_ignore_ascii_case(first))
        .and_then(|_| name.get(first.len()..))
    else {
        return false;
    };
    // 先頭の部分と重ならないよう、残りの末尾から最後の部分を取る。
    let Some(mut rest) = rest
        .len()
        .checked_sub(last.len())
        .filter(|start| {
            rest.get(*start..)
                .is_some_and(|tail| tail.eq_ignore_ascii_case(last))
        })
        .and_then(|start| rest.get(..start))
    else {
        return false;
    };
    // 途中の部分は、左から最初に合う所を取れば足りる。
    for part in middle.iter().filter(|part| !part.is_empty()) {
        let Some(position) = rest
            .windows(part.len())
            .position(|window| window.eq_ignore_ascii_case(part))
        else {
            return false;
        };
        rest = rest
            .get(position.saturating_add(part.len())..)
            .unwrap_or_default();
    }
    true
}

/// 親から採った環境に利用者の規則を当て、提供者のキー（設定で参照したものを含む）と古い印を除き、
/// 今回の起動の値を設定する。規則の後の変更は規則で崩せない（設計書 03-02「規則の後に必ず当てる変更」）。
/// `sandbox` は方針の名前と IP 通信の可否。外で動かすときは `None` とする。
pub fn child_environment(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
    referenced_keys: &BTreeSet<String>,
    rules: &EnvironmentRules,
    temporary: &Path,
    sandbox: Option<(&str, bool)>,
) -> BTreeMap<OsString, OsString> {
    let mut environment = apply_rules(&rules.rules, &inherited.into_iter().collect());
    environment.retain(|name, _| !referenced_keys.iter().any(|key| name == key.as_str()));
    remark_child_environment(environment, temporary, sandbox)
}

/// 除いても [`child_environment`] の結果（名前と値）が変わらない行の位置。
/// 規則の後に必ず当てる変更で打ち消される行も含む（設計書 05-01「doctor」）。
pub fn ineffective_rules(
    inherited: &BTreeMap<OsString, OsString>,
    referenced_keys: &BTreeSet<String>,
    rules: &EnvironmentRules,
    temporary: &Path,
) -> Vec<usize> {
    let result = child_environment(inherited.clone(), referenced_keys, rules, temporary, None);
    (0..rules.rules.len())
        .filter(|index| {
            let rest = EnvironmentRules {
                rules: rules
                    .rules
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| other != index)
                    .map(|(_, rule)| rule.clone())
                    .collect(),
                written: rules.written,
            };
            child_environment(inherited.clone(), referenced_keys, &rest, temporary, None) == result
        })
        .collect()
}

/// すでに [`child_environment`] で作った環境の、今回の起動の印と値だけを付け直す。
/// 提供者のキーは、念のため改めて除く。
pub fn remark_child_environment(
    mut environment: BTreeMap<OsString, OsString>,
    temporary: &Path,
    sandbox: Option<(&str, bool)>,
) -> BTreeMap<OsString, OsString> {
    environment.retain(|name, _| !PROVIDER_KEY_ENV_VARS.iter().any(|key| name == key));
    // 外側の benitoiteh の印を引き継ぐと、今回の制限を誤って伝える。
    environment.remove(OsStr::new(crate::askpass::SOCKET_ENV));
    environment.remove(OsStr::new(crate::askpass::TOKEN_ENV));
    environment.remove(OsStr::new("BENITOITEH_SANDBOX"));
    environment.remove(OsStr::new("BENITOITEH_SANDBOX_NETWORK_DISABLED"));
    environment.insert("TMPDIR".into(), temporary.as_os_str().to_owned());
    for (name, value) in [
        ("NO_COLOR", "1"),
        ("TERM", "dumb"),
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
    ] {
        environment.insert(name.into(), value.into());
    }
    if let Some((name, allow_ip)) = sandbox {
        environment.insert("BENITOITEH_SANDBOX".into(), name.into());
        if !allow_ip {
            environment.insert("BENITOITEH_SANDBOX_NETWORK_DISABLED".into(), "1".into());
        }
    }
    environment
}
