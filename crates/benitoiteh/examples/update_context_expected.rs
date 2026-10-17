//! 文脈の期待ファイルを明示的に更新する。生成後に全文を読み直す（設計書 07-01「LLM に見せる文の固定」）。

use std::error::Error;
use std::fs;
use std::path::Path;

use benitoiteh::config::Mode;
use benitoiteh::context::{SystemContext, tool_definitions_json};
use benitoiteh::tools::ToolSet;

// cargo run --offline -p benitoiteh --example update_context_expected で更新する。
// テストの既定の実行からは呼ばず、差分と全文を確認してから採用する。
fn main() -> Result<(), Box<dyn Error>> {
    let processor =
        std::env::var_os("BENITOITEH_TEST_BENITOITE").ok_or(text::PROCESSOR_REQUIRED)?;
    let output = std::process::Command::new(processor)
        .args(["skill", "show", "--format=json", "SKILL.md"])
        .output()?;
    if !output.status.success() {
        return Err(text::QUERY_FAILED.into());
    }
    let skill = benitoiteh::benitoite::skill::SkillDocument::from_json("SKILL.md", &output.stdout)?;
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected");
    for (mode, name, enabled) in [
        (Mode::Restricted, "restricted", true),
        (Mode::Normal, "normal", true),
        (Mode::Normal, "normal-without-benitoite", false),
    ] {
        let tools = if enabled {
            ToolSet::new(mode)
        } else {
            ToolSet::without_benitoite(mode)
        };
        let context = SystemContext::new(&tools, enabled.then_some(&skill))?;
        fs::write(
            directory.join(format!("system-{name}.txt")),
            context.system_prompt,
        )?;
        fs::write(
            directory.join(format!("tools-{name}.json")),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&tool_definitions_json(&context.tools))?
            ),
        )?;
    }
    Ok(())
}

mod text {
    //! 期待ファイルを更新できない理由を示す英文。
    pub const PROCESSOR_REQUIRED: &str =
        "BENITOITEH_TEST_BENITOITE is required; supply benitoite v0.0.7";
    pub const QUERY_FAILED: &str = "benitoite skill show failed";
}
