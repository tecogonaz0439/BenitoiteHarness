//! Agent Plugins 1.0.0 のマニフェストを検査する（設計書 01-04「Agent Plugins のプラグイン」）。

pub(super) mod text;

use serde_json::{Map, Value};

/// 記録して示す版の最大の長さ（設計書 01-04「Agent Plugins のプラグイン」の「長さと数の上限」）。
const VERSION_LIMIT: usize = 64;

/// 検査を通過した識別情報と、無視する欄の通知。
pub(super) struct Manifest {
    pub name: String,
    pub version: Option<String>,
    // 決まった文の通知（extensions、version）。
    pub notices: Vec<String>,
    // 知らない欄の通知。名前をリポジトリが選べるので、まとめた行の最後に置く。
    pub unknown: Option<String>,
}

/// マニフェスト全体を断る理由。
pub(super) enum PluginError {
    Invalid(&'static str),
    Field(String),
}

impl PluginError {
    /// 利用者に示す、入力の値を含まない拒否の文。
    pub fn reason(self) -> String {
        match self {
            Self::Invalid(reason) => reason.into(),
            Self::Field(field) => text::invalid_type(&field),
        }
    }
}

/// 生の JSON を検査し、部品を読む前に名前とディレクトリの一致も確かめる。
pub(super) fn parse(
    bytes: &[u8],
    entry_name: Option<&str>,
    real_name: Option<&str>,
) -> Result<Manifest, PluginError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| PluginError::Invalid(text::JSON))?;
    let fields = value.as_object().ok_or(PluginError::Invalid(text::JSON))?;
    if fields.get("$schema").and_then(Value::as_str) != Some(text::SCHEMA_URL) {
        return Err(PluginError::Invalid(text::SCHEMA));
    }
    let name = fields
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| valid_name(name))
        .ok_or(PluginError::Invalid(text::NAME))?;
    if entry_name != Some(name) || real_name != Some(name) {
        return Err(PluginError::Invalid(text::DIRECTORY_NAME));
    }
    for field in [
        "version",
        "description",
        "homepage",
        "repository",
        "license",
    ] {
        if fields.get(field).is_some_and(|value| !value.is_string()) {
            return Err(PluginError::Field(field.into()));
        }
    }
    if let Some(author) = fields.get("author") {
        let author = author
            .as_object()
            .ok_or_else(|| PluginError::Field("author".into()))?;
        if author.iter().any(|(key, value)| {
            !matches!(key.as_str(), "name" | "email" | "url") || !value.is_string()
        }) {
            return Err(PluginError::Field("author".into()));
        }
    }
    if let Some(keywords) = fields.get("keywords") {
        let keywords = keywords
            .as_array()
            .ok_or_else(|| PluginError::Field("keywords".into()))?;
        if keywords.iter().any(|value| !value.is_string()) {
            return Err(PluginError::Field("keywords".into()));
        }
    }
    let mut notices = Vec::new();
    // 使わない拡張の値にはクライアント固有の検査を掛けない（同節、仕様 §8.1）。
    if fields
        .get("extensions")
        .is_some_and(|value| !value.is_object())
    {
        notices.push(text::EXTENSIONS.into());
    }
    // 信頼しないリポジトリの長い版で記録のヘッダを膨らませず、表示で
    // パスや SHA-256 を装わせない（設計書 01-04「Agent Plugins のプラグイン」の「長さと数の上限」）。
    let version = match fields.get("version").and_then(Value::as_str) {
        Some(version) if valid_version(version) => Some(version.to_owned()),
        Some(_) => {
            notices.push(text::VERSION.into());
            None
        }
        None => None,
    };
    Ok(Manifest {
        name: name.into(),
        version,
        notices,
        unknown: unknown_fields(fields),
    })
}

fn valid_version(version: &str) -> bool {
    (1..=VERSION_LIMIT).contains(&version.len())
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'_' | b'-'))
}

fn valid_name(name: &str) -> bool {
    let alphanumeric = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    (1..=64).contains(&name.len())
        && name.as_bytes().first().is_some_and(alphanumeric)
        && name.as_bytes().last().is_some_and(alphanumeric)
        && name
            .bytes()
            .all(|byte| alphanumeric(&byte) || matches!(byte, b'-' | b'.'))
        && !name.contains("--")
        && !name.contains("..")
}

fn unknown_fields(fields: &Map<String, Value>) -> Option<String> {
    let mut names = fields
        .keys()
        .filter(|key| {
            !matches!(
                key.as_str(),
                "$schema"
                    | "name"
                    | "version"
                    | "description"
                    | "homepage"
                    | "repository"
                    | "license"
                    | "author"
                    | "keywords"
                    | "extensions"
            )
        })
        .collect::<Vec<_>>();
    names.sort();
    (!names.is_empty()).then(|| text::unknown_fields(&names))
}
