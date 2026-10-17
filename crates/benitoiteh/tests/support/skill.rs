//! 本物の処理系の Skill を統合テストで共有する（設計書 07-01「処理系の用意」）。

use super::processor::benitoite;

/// 本物の処理系を直接起動する Skill の共有部品。
#[allow(dead_code)]
pub fn skills()
-> Result<std::sync::Arc<benitoiteh::benitoite::skill::SkillDocuments>, Box<dyn std::error::Error>>
{
    let processor = benitoite()?;
    Ok(std::sync::Arc::new(
        benitoiteh::benitoite::skill::SkillDocuments::new(move |args, cancelled| {
            use benitoiteh::benitoite::{BenitoiteError, QueryOutput};
            if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                return Err(BenitoiteError::Cancelled);
            }
            let output = std::process::Command::new(&processor).args(args).output()?;
            if !output.status.success() {
                return Err(BenitoiteError::Exit {
                    stdout: Vec::new(),
                    code: output.status.code(),
                    stderr: output.stderr,
                });
            }
            Ok(QueryOutput {
                stdout: output.stdout,
                stderr: output.stderr,
            })
        }),
    ))
}
