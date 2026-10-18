//! 新しい一時スクリプトと確認の呼び直しを管理する（設計書 01-03、03-03）。

use super::{
    ApprovalStatus, Invocation, ToolContext, ToolError, ToolOutput, benitoite_run, read, text,
};
use crate::benitoite::Effect;
use crate::safe_fs::{SafeFsError, ScopeRelPath};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct ScratchState {
    counter: u64,
    pending: Option<PendingScratch>,
}

struct PendingScratch {
    call_id: String,
    path: PathBuf,
    automatic_effects: BTreeSet<Effect>,
}

impl ToolContext {
    // 自動の範囲はパスの確認に昇格させず、同じ呼び出しの再検査にだけ使う。
    // （設計書 03-03「非対話での扱い」）。
    pub(crate) fn remember_scratch_automatic(
        &self,
        call_id: &str,
        reported: &BTreeSet<Effect>,
        allowed: &BTreeSet<Effect>,
    ) -> Result<(), ToolError> {
        let mut state = self.scratch_state.lock().map_err(|_| ToolError::State)?;
        let pending = state
            .pending
            .as_mut()
            .filter(|pending| pending.call_id == call_id)
            .ok_or(ToolError::State)?;
        pending.automatic_effects = reported.difference(allowed).copied().collect();
        Ok(())
    }

    pub(super) fn take_scratch_automatic(
        &self,
        call_id: &str,
    ) -> Result<BTreeSet<Effect>, ToolError> {
        let mut state = self.scratch_state.lock().map_err(|_| ToolError::State)?;
        let pending = state
            .pending
            .as_mut()
            .filter(|pending| pending.call_id == call_id)
            .ok_or(ToolError::State)?;
        Ok(std::mem::take(&mut pending.automatic_effects))
    }

    // 確認の拒否はループが作るため、道具の結果と同じ入口でパスを添える。
    // 確認の要約は変更せず、結果の先頭だけを固定する（設計書 01-03）。
    pub(crate) fn scratch_result(&self, call_id: &str, output: &mut ToolOutput) {
        if let Ok(state) = self.scratch_state.lock()
            && let Some(pending) = &state.pending
            && pending.call_id == call_id
        {
            let prefix = format!("{}\n", pending.path.display());
            if !output.text.starts_with(&prefix) {
                output.text = super::truncate::cap_middle(&format!("{prefix}{}", output.text));
            }
        }
    }
}

pub(super) fn execute(
    context: &ToolContext,
    arguments: &Value,
    approval: ApprovalStatus,
    invocation: Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let path = if approval == ApprovalStatus::Approved {
        context
            .scratch_state
            .lock()
            .map_err(|_| ToolError::State)?
            .pending
            .as_ref()
            .filter(|pending| pending.call_id == invocation.call_id)
            .map(|pending| pending.path.clone())
            .ok_or(ToolError::State)?
    } else {
        let mut state = context.scratch_state.lock().map_err(|_| ToolError::State)?;
        state.pending = None;
        let code = read::string(arguments, "code")?;
        if code.len() > read::MAX_FILE_BYTES as usize
            || code.as_bytes().iter().take(8 * 1024).any(|byte| *byte == 0)
        {
            return Err(ToolError::InvalidArguments(text::SCRATCH_CODE.into()));
        }
        let mut created = None;
        for _attempt in 0..128 {
            state.counter = state
                .counter
                .checked_add(1)
                .ok_or_else(|| ToolError::InvalidArguments(text::SCRATCH_COUNTER.into()))?;
            let name = ScopeRelPath::new(format!("scratch-{}.bnt", state.counter))?;
            match context.temporary.create_new(&name) {
                Ok(mut file) => {
                    let path = context.temporary.real_path().join(name.as_path());
                    state.pending = Some(PendingScratch {
                        call_id: invocation.call_id.into(),
                        path: path.clone(),
                        automatic_effects: BTreeSet::new(),
                    });
                    file.write_all(code.as_bytes())?;
                    created = Some(path);
                    break;
                }
                Err(SafeFsError::AlreadyExists(_)) => {}
                Err(error) => return Err(error.into()),
            }
        }
        created.ok_or_else(|| ToolError::InvalidArguments(text::SCRATCH_NAMES.into()))?
    };
    // 呼び直しは同じ read の範囲検査を通す。初回は今作った実パスを使い、
    // 除外した一時ディレクトリでも unread_imports による確認を省略しない。
    let path = if approval == ApprovalStatus::Approved {
        context.open_file(&path, false)?.real_path
    } else {
        path
    };
    let display = path.to_string_lossy();
    benitoite_run::execute_script(
        context,
        arguments,
        benitoite_run::ScriptPath {
            real: &path,
            display: &display,
        },
        approval,
        invocation,
        120,
        true,
    )
}
