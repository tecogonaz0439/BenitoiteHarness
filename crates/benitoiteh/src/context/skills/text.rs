//! Skill を採らなかった理由（設計書 01-04「利用者の Agent Skill」）。

pub const HOME_UNAVAILABLE: &str = "Could not resolve the home directory for user skills.";
pub const UNREADABLE: &str = "Could not open or read this skill.";
pub const OUTSIDE_SCOPE: &str = "Skill path is outside the search scope.";
pub const BROAD_DIRECTORY: &str =
    "Skill directory is the project, search root, home, or an ancestor of one of them.";
pub const NOT_REGULAR: &str = "SKILL.md is not a regular file.";
pub const HARD_LINKS: &str = "SKILL.md has more than one hard link.";
pub const DUPLICATE_FILE: &str = "This skill file has already been read.";
pub const FILE_LIMIT: &str = "SKILL.md exceeds the 256 KiB limit.";
pub const NOT_UTF8: &str = "SKILL.md is not valid UTF-8.";
pub const PATH_NOT_UTF8: &str = "Skill directory or SKILL.md path is not valid UTF-8.";
pub const FRONTMATTER: &str =
    "SKILL.md has no valid frontmatter with a nonempty name and description.";
pub const MULTILINE: &str = "Skill name and description must be single-line values.";
pub const NAME: &str =
    "Skill name must contain 1 to 64 lowercase ASCII letters, digits, or hyphens.";
pub const DESCRIPTION: &str =
    "Skill description must contain 1 to 1024 Unicode scalar values without newlines.";
pub const DIRECTORY_NAME: &str =
    "Skill name must match both the entry name and the real directory name.";
pub const RESERVED_NAME: &str = "The skill name benitoite is reserved for the processor skill.";
pub const DUPLICATE_NAME: &str = "Another skill with this name takes precedence.";
pub const SKILL_LIMIT: &str = "Skill exceeds the 100-skill limit; skills are counted in order: user skills, user plugin skills, project skills, project plugin skills; each stage is ordered by plugin name then skill name.";
pub const INDEX_LIMIT: &str = "Skill exceeds the 16 KiB index limit; skills are counted in order: user skills, user plugin skills, project skills, project plugin skills; each stage is ordered by plugin name then skill name.";

pub fn entry_limit(count: usize) -> String {
    format!("Skipped {count} entries beyond the first 1000 in name order.")
}

pub fn excluded(reason: &str) -> String {
    format!("Skill is excluded by the read rules: {reason}")
}

pub fn skipped_beyond_limits(count: usize) -> String {
    format!("Skipped {count} skills beyond the skill or index limit without reading them.")
}

pub fn plugin_candidate_limit(count: usize) -> String {
    format!(
        "Skipped {count} plugin skill entries beyond the first 1000 for this source without opening them."
    )
}

pub fn skipped_hidden(count: usize) -> String {
    format!(
        "{count} more skipped skills and plugins for this source are not listed; only the first 100 are listed."
    )
}
