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
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected");
    for (mode, name) in [(Mode::Restricted, "restricted"), (Mode::Normal, "normal")] {
        let context = SystemContext::new(&ToolSet::new(mode))?;
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
