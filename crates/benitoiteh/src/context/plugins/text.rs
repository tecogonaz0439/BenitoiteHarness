//! プラグインの拒否と通知の文（設計書 01-04「Agent Plugins のプラグイン」）。

pub const SCHEMA_URL: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
pub const SCHEMA: &str = "plugin.json must set $schema to https://agent-plugins.org/schemas/1.0.0/plugin.schema.json; other Agent Plugins versions are not supported.";
pub const JSON: &str = "plugin.json is not a valid JSON object.";
pub const NAME: &str = "Plugin name must contain 1 to 64 lowercase ASCII letters, digits, hyphens, or dots, start and end with a letter or digit, and not contain -- or ...";
pub const DIRECTORY_NAME: &str =
    "Plugin name must match both the entry name and the real directory name.";
pub const EXTENSIONS: &str = "Ignored plugin.json extensions because it is not an object.";
pub const MCP: &str = "mcp.json is present; MCP servers from this plugin are not used.";
pub const DUPLICATE_NAME: &str = "Another plugin with this name takes precedence.";
pub const HOME_UNAVAILABLE: &str = "Could not resolve the home directory for user plugins.";

pub fn invalid_type(field: &str) -> String {
    format!("plugin.json field {field} has an invalid type.")
}

pub const VERSION: &str = "Ignored plugin.json version because it is not 1 to 64 ASCII letters, digits, dots, plus signs, underscores, or hyphens.";
pub const UNREADABLE: &str = "Could not open or read this plugin.";
pub const OUTSIDE_SCOPE: &str = "Plugin path is outside the search scope.";
pub const BROAD_DIRECTORY: &str =
    "Plugin directory is the project, search root, home, or an ancestor of one of them.";
pub const NOT_REGULAR: &str = "plugin.json is not a regular file.";
pub const HARD_LINKS: &str = "plugin.json has more than one hard link.";
pub const DUPLICATE_FILE: &str = "This plugin file has already been read.";
pub const FILE_LIMIT: &str = "plugin.json exceeds the 256 KiB limit.";
pub const NOT_UTF8: &str = "plugin.json is not valid UTF-8.";
pub const PATH_NOT_UTF8: &str = "Plugin directory or plugin.json path is not valid UTF-8.";

pub fn excluded(reason: &str) -> String {
    format!("Plugin is excluded by the read rules: {reason}")
}

pub fn plugin_limit(count: usize) -> String {
    format!("Skipped {count} entries beyond the 100-plugin limit for this source.")
}

/// 一つの名前を 64 文字で切り、10 個までを示す（設計書 01-04 の「長さと数の上限」）。
pub fn unknown_fields(names: &[&String]) -> String {
    let shown = names
        .iter()
        .take(10)
        .map(|name| {
            if name.chars().count() > 64 {
                format!("{}...", name.chars().take(64).collect::<String>())
            } else {
                name.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let more = names.len().saturating_sub(10);
    if more == 0 {
        format!("Ignored unknown plugin.json fields: {shown}.")
    } else {
        format!("Ignored unknown plugin.json fields: {shown} (and {more} more).")
    }
}
