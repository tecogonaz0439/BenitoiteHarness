//! 再開の入口の試験が使う記録の準備（設計書 01-05）。
use super::*;
pub(super) fn paths(f: &Fixture) -> Result<DataPaths, Box<dyn Error>> {
    Ok(DataPaths::from_values(
        Some(&f.root.join("home")),
        Some(&f.root.join("config")),
        Some(&f.root.join("state")),
        Some(&f.root.join("tmp")),
    )?)
}

pub(super) fn rewrite(directory: &std::path::Path, records: &[Value]) -> TestResult {
    fs::write(
        directory.join("session.jsonl"),
        records
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n")
            + "\n",
    )?;
    Ok(())
}

pub(super) fn id(directory: &std::path::Path) -> Result<&str, Box<dyn Error>> {
    directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "missing ID".into())
}

pub(super) fn saved(
    f: &Fixture,
    template: &Value,
    session_id: &str,
    project: &std::path::Path,
) -> Result<PathBuf, Box<dyn Error>> {
    let session = Session::create(&paths(f)?, session_id.parse()?)?;
    let mut header = template.clone();
    *header.get_mut("session_id").ok_or("session_id")? = json!(session_id);
    *header.get_mut("project_dir").ok_or("project_dir")? = json!(project);
    *header.get_mut("started_at").ok_or("started_at")? = json!(session.id().started_at_utc());
    session.writer()?.append(&serde_json::from_value(header)?)?;
    Ok(session.directory().to_owned())
}

pub(super) fn sessions_command(f: &Fixture) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_benitoiteh"));
    command
        .arg("sessions")
        .current_dir(f.root.join("project"))
        .env("HOME", f.root.join("home"))
        .env("XDG_CONFIG_HOME", f.root.join("config"))
        .env("XDG_STATE_HOME", f.root.join("state"))
        .env("TMPDIR", f.root.join("tmp"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
