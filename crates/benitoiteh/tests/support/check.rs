//! 道具の境界では本物の処理系をプロジェクトを根として起動する（実装プラン P2-07）。

use benitoiteh::benitoite::{BenitoiteError, CheckQuery, QueryOutput};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[allow(dead_code)]
pub fn checks(project: PathBuf) -> Result<Arc<CheckQuery>, Box<dyn std::error::Error>> {
    let processor = super::processor::benitoite()?;
    Ok(Arc::new(move |args, _, cancelled| {
        if cancelled.load(Ordering::Acquire) {
            return Err(BenitoiteError::Cancelled);
        }
        let output = std::process::Command::new(&processor)
            .args(args)
            .current_dir(&project)
            .output()?;
        if !output.status.success() {
            return Err(BenitoiteError::Exit {
                stdout: output.stdout,
                code: output.status.code(),
                stderr: output.stderr,
            });
        }
        Ok(QueryOutput {
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }))
}
