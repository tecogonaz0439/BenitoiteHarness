//! 道具の検証と実行をまとめ、範囲と結果の上限を守る（設計書 01-03「道具の定義の共通の規則」）。

mod bash;
mod benitoite_check;
mod benitoite_process;
mod benitoite_reference;
mod benitoite_run;
mod benitoite_scratch;
mod benitoite_test;
mod create_project;
mod definitions;
mod edit;
pub mod exclude;
mod paths;
pub(crate) mod process_output;
mod read;
pub(crate) use read::run_image_helper;
mod search;
mod search_notices;
pub(crate) mod text;
pub mod truncate;
mod web_fetch;
mod write;
mod write_path;

pub(crate) use read::inspect_image;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs::Metadata;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::approval::{ApprovalFacts, ApprovalKind};
use crate::config::{Approval, Mode, Sandbox};
use crate::provider::{ToolImage, ToolSpec};

/// 一枚の画像を読む上限（設計書 01-03「画像のファイルの読み取り」）。
pub const MAX_IMAGE_BYTES: u64 = 3_932_160;
/// 画像の各辺の上限。
pub const MAX_IMAGE_DIMENSION: u32 = 2_000;
use crate::safe_fs::{SafeFsError, ScopeRoot};
use crate::sandbox::SandboxPolicy;
use crate::session::{Session, SpillInfo};
use exclude::ExcludeRules;

/// セッション中に順序を変えない道具の識別子。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolId {
    /// 行番号を付けたテキストの読み取り。
    Read,
    /// パターンに合うパスの列挙。
    Glob,
    /// 正規表現による行の検索。
    Grep,
    /// ファイルの作成と置き換え。
    Write,
    /// 文字列の置き換え。
    Edit,
    /// 通常モードのシェルのコマンド。
    Bash,
    /// 診断のコードの説明。
    BenitoiteExplain,
    /// スクリプトとテストの静的検査。
    BenitoiteCheck,
    /// 確認したエフェクトでスクリプトを実行する。
    BenitoiteRun,
    /// 確認したエフェクトでテストを実行する。
    BenitoiteTest,
    BenitoiteScratch,
    /// 処理系の Skill の文書。
    BenitoiteReference,
    /// 新しいプロジェクトを作って切り替える。
    CreateProject,
    /// HTTPS の本文の取得。
    WebFetch,
}

impl ToolId {
    /// 提供者とプロンプトで共通に使う名前。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Glob => "glob",
            Self::Grep => "grep",
            Self::Write => "write",
            Self::Edit => "edit",
            Self::Bash => "bash",
            Self::BenitoiteCheck => "benitoite_check",
            Self::BenitoiteRun => "benitoite_run",
            Self::BenitoiteTest => "benitoite_test",
            Self::BenitoiteScratch => "benitoite_scratch",
            Self::BenitoiteExplain => "benitoite_explain",
            Self::BenitoiteReference => "benitoite_reference",
            Self::WebFetch => "web_fetch",
            Self::CreateProject => "create_project",
        }
    }
}

/// JSON の引数から独立した、ループが渡す確認の状態。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalStatus {
    /// まだ確認していない。
    Unapproved,
    /// ループでこの呼び出しの確認を受けた。
    Approved,
}

/// 一つの道具の定義と、ループとプロンプトが使う情報。
#[derive(Clone, Debug)]
pub struct ToolDefinition {
    pub id: ToolId,
    pub spec: ToolSpec,
    /// P1-08 が prompts/ の英文を選ぶための識別子。
    pub prompt_key: &'static str,
    pub summary: &'static str,
    pub rules: &'static [&'static str],
    pub parallel: bool,
}

/// 修復と検証を終えた呼び出し。未検証の引数では作れない。
#[derive(Clone, Debug)]
pub struct ValidatedCall {
    pub(crate) id: ToolId,
    pub(crate) arguments: Value,
}

impl ValidatedCall {
    /// 検証済みの道具の識別子。
    pub fn id(&self) -> ToolId {
        self.id
    }

    /// 修復した後の引数。
    pub fn arguments(&self) -> &Value {
        &self.arguments
    }
}

/// セッションの開始時に固定する道具の一覧。
#[derive(Debug)]
pub struct ToolSet {
    mode: Mode,
    definitions: Vec<ToolDefinition>,
    direct_id: AtomicU64,
}

/// JSON から独立した呼び出しの識別子と取り消しの印。
pub struct Invocation<'a> {
    pub call_id: &'a str,
    pub cancelled: &'a AtomicBool,
    /// 信頼するループが単位ごとに確認したエフェクト。
    pub confirmed_effects: &'a BTreeMap<ConfirmKey, BTreeSet<crate::benitoite::Effect>>,
    /// 信頼するループがこのセッションで許したドメイン。
    pub session_domains: &'a BTreeSet<String>,
}

impl ToolSet {
    /// 評価用に read の要約と説明を行番号に触れない文へ替える。
    pub fn plain_read(mut self) -> Self {
        for tool in &mut self.definitions {
            if tool.id == ToolId::Read {
                tool.summary = text::READ_PLAIN.0;
                tool.spec.description = text::READ_PLAIN.0.into();
                tool.rules = &[text::READ_PLAIN.1];
            }
        }
        self
    }

    /// モードで許す道具を、決まった順序で用意する。
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            definitions: definitions::definitions(mode),
            direct_id: AtomicU64::new(0),
        }
    }

    /// 処理系を使えない通常モードで Benitoite の道具を外す。
    pub fn without_benitoite(mode: Mode) -> Self {
        let mut tools = Self::new(mode);
        tools.definitions.retain(|tool| {
            !matches!(
                tool.id,
                ToolId::BenitoiteExplain
                    | ToolId::BenitoiteReference
                    | ToolId::BenitoiteCheck
                    | ToolId::BenitoiteRun
                    | ToolId::BenitoiteTest
                    | ToolId::BenitoiteScratch
            )
        });
        tools
    }

    /// 道具と Skill の有無を揃えるための判定。
    pub fn has_benitoite(&self) -> bool {
        self.definitions
            .iter()
            .any(|tool| tool.id == ToolId::BenitoiteReference)
    }

    /// セッションの開始時に固定したモード。
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// プロンプトとスケジューラが共有する定義。
    pub fn definitions(&self) -> &[ToolDefinition] {
        &self.definitions
    }

    /// 提供者に渡す引数の定義を、同じ順序で返す。
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.definitions
            .iter()
            .map(|tool| tool.spec.clone())
            .collect()
    }

    /// 知らない道具を拒否し、引数を修復して型と必須を確かめる。
    pub fn validate(&self, name: &str, mut arguments: Value) -> Result<ValidatedCall, ToolError> {
        let tool = self
            .definitions
            .iter()
            .find(|tool| tool.spec.name == name)
            .ok_or_else(|| {
                ToolError::InvalidArguments(text::unknown_tool(name, &self.definitions))
            })?;
        repair_and_validate(&tool.spec, &mut arguments)?;
        Ok(ValidatedCall {
            id: tool.id,
            arguments,
        })
    }

    /// JSON の道具呼び出しを検証して実行する、持ち主の境界。
    pub fn execute(&self, context: &ToolContext, name: &str, arguments: Value) -> ToolOutput {
        match self.validate(name, arguments) {
            Ok(call) => self.execute_validated(
                context,
                &call,
                ApprovalStatus::Unapproved,
                Invocation {
                    call_id: &format!("direct-{}", self.direct_id.fetch_add(1, Ordering::Relaxed)),
                    cancelled: &AtomicBool::new(false),
                    session_domains: &BTreeSet::new(),
                    confirmed_effects: &BTreeMap::new(),
                },
            ),
            Err(error) => ToolOutput::error(error.to_string()),
        }
    }

    /// ループが検証済みの呼び出しを共有の文脈で実行する。
    pub fn execute_validated(
        &self,
        context: &ToolContext,
        call: &ValidatedCall,
        approval: ApprovalStatus,
        invocation: Invocation<'_>,
    ) -> ToolOutput {
        if self.mode != context.mode {
            return ToolOutput::error(text::MODE_MISMATCH);
        }
        let call_id = invocation.call_id;
        let result = match call.id {
            ToolId::Read => read::execute(context, &call.arguments, invocation.cancelled),
            ToolId::Glob => search::glob(context, &call.arguments, invocation.cancelled)
                .map(ToolOutput::success),
            ToolId::Grep => search::grep(context, &call.arguments, invocation.cancelled)
                .map(ToolOutput::success),
            ToolId::Write => write::execute(context, &call.arguments, approval, invocation),
            ToolId::Edit => edit::execute(context, &call.arguments, approval, invocation),
            ToolId::Bash => bash::execute(context, &call.arguments, approval, invocation),
            ToolId::BenitoiteRun => {
                benitoite_run::execute(context, &call.arguments, approval, invocation)
            }
            ToolId::BenitoiteTest => {
                benitoite_test::execute(context, &call.arguments, approval, invocation)
            }
            ToolId::BenitoiteScratch => {
                benitoite_scratch::execute(context, &call.arguments, approval, invocation)
            }
            ToolId::BenitoiteCheck => {
                benitoite_check::execute(context, &call.arguments, invocation)
            }
            ToolId::BenitoiteExplain => {
                benitoite_reference::explain(context, &call.arguments, invocation.cancelled)
            }
            ToolId::BenitoiteReference => {
                benitoite_reference::reference(context, &call.arguments, invocation.cancelled)
            }
            ToolId::CreateProject => {
                create_project::execute(context, &call.arguments, approval, invocation)
            }
            ToolId::WebFetch => web_fetch::execute(context, &call.arguments, approval, invocation),
        };
        let mut output = match result {
            Ok(output) => output,
            Err(error) => ToolOutput::error(error.to_string()),
        };
        if call.id == ToolId::BenitoiteScratch {
            context.scratch_result(call_id, &mut output);
        }
        output
    }
}

pub use definitions::repair_and_validate;

/// ファイルの同一性と、read の後の変更を検知する版。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileVersion {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
}

impl FileVersion {
    /// 開いたファイルのメタデータから版を作る。
    pub fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        }
    }
}

/// 通常モードの子プロセスに渡す、起動時に固定した値（設計書 03-02）。
#[derive(Clone, Debug)]
pub struct ShellContext {
    /// なしはサンドボックスの外での起動。
    pub policy: Option<SandboxPolicy>,
    /// サンドボックスを掛けるときだけ方針の名前を持つ。
    pub policy_name: Option<&'static str>,
    /// Linux の補助として起動する benitoiteh の実のパス。
    pub helper: PathBuf,
    /// Linux の覆いのファイルと探査に使う、セッションの sandbox/。
    pub scratch: PathBuf,
    /// Linux の塞ぐ場所の錠を置く、データの placeholders/。
    pub placeholders: PathBuf,
    /// 秘密を除いた、子に渡す環境の全体。
    pub env: BTreeMap<OsString, OsString>,
    /// 選んだ方針の書ける場所。サンドボックスを掛けられなくても変えない。
    pub writable_roots: Vec<PathBuf>,
    /// 起動時に選んだサンドボックスの方針。
    pub sandbox: Sandbox,
    /// 実際に OS のサンドボックスを掛けるか。
    pub sandboxed: bool,
    /// 承認の方針。
    pub approval: Approval,
}

/// 起動時に固定したウェブの取得の設定（設計書 01-03「web_fetch」）。
#[derive(Clone, Debug, Default)]
pub struct WebFetchContext {
    /// 設定で検証し正規化したドメインの完全一致の集合。
    pub allowed_domains: BTreeSet<String>,
    /// 提供者の認証情報を含まない HTTP クライアント。
    pub client: crate::http::HttpClient,
}

/// 最大八つの道具が共有する文脈。変わる版の記録だけを Mutex で守る。
pub struct ToolContext {
    pub(crate) image_input: bool,
    pub(crate) after_write_check: bool,
    pub(crate) read_line_numbers: bool,
    pub(crate) noninteractive: bool,
    pub(crate) project_parent: Option<PathBuf>,
    pub mode: Mode,
    pub project: ScopeRoot,
    pub temporary: ScopeRoot,
    pub spill: ScopeRoot,
    skill_roots: Vec<ScopeRoot>,
    pub exclude: ExcludeRules,
    pub benitoite: Option<Arc<crate::benitoite::Processor>>,
    pub skill: Option<Arc<crate::benitoite::skill::SkillDocuments>>,
    pub check_query: Option<Arc<crate::benitoite::CheckQuery>>,
    pub run: Option<crate::benitoite::RunContext>,
    pub shell: Option<ShellContext>,
    pub(crate) image_helper: Option<ImageHelperContext>,
    pub web_fetch: WebFetchContext,
    pub session: Option<Session>,
    read_versions: Mutex<BTreeMap<PathBuf, FileVersion>>,
    pub checkpoints: Option<crate::rewind::CheckpointRecorder>,
    search_limits: SearchLimits,
    scratch_state: Mutex<benitoite_scratch::ScratchState>,
    project_creation: Option<(crate::config::paths::DataPaths, PathBuf)>,
    pub(crate) project_candidates: Mutex<BTreeMap<String, create_project::Candidate>>,
    pub(crate) created_projects: Mutex<BTreeMap<String, ScopeRoot>>,
}

/// 画像の補助専用の起動情報。通常モードの権限の設定から独立して持つ。
#[derive(Clone, Debug)]
pub struct ImageHelperContext {
    /// benitoiteh 自身の正規の実行ファイルのパス。
    pub helper: PathBuf,
    /// Linux の補助を準備する非公開の場所。
    pub scratch: PathBuf,
    /// Linux の守る名前の錠を置く場所。
    pub placeholders: PathBuf,
    #[cfg(test)]
    pub(crate) test_command: Option<(String, std::time::Duration)>,
}

/// 現在の根に触れずに準備した、道具のプロジェクトの状態。
pub(crate) struct PreparedProjectSwitch {
    pub(crate) project: ScopeRoot,
    pub(crate) exclude: ExcludeRules,
    run: Option<crate::benitoite::RunContext>,
}

impl fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolContext")
            .field("mode", &self.mode)
            .field("project", &self.project)
            .finish_non_exhaustive()
    }
}

/// 検索が主のプロセスで使える走査と読み取りの上限（設計書 01-03「glob」「grep」）。
#[derive(Clone, Copy, Debug)]
pub struct SearchLimits {
    pub max_entries: usize,
    pub max_duration: std::time::Duration,
    pub max_file_bytes: u64,
}

impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_entries: 200_000,
            max_duration: std::time::Duration::from_secs(10),
            max_file_bytes: read::MAX_FILE_BYTES,
        }
    }
}

impl ToolContext {
    /// 起動時に開いた範囲と、除外の規則を受け取る。
    pub fn new(
        mode: Mode,
        project: ScopeRoot,
        temporary: ScopeRoot,
        spill: ScopeRoot,
        exclude: ExcludeRules,
    ) -> Self {
        Self {
            image_input: false,
            noninteractive: false,
            after_write_check: true,
            read_line_numbers: true,
            project_parent: None,
            project_creation: None,
            project_candidates: Mutex::new(BTreeMap::new()),
            created_projects: Mutex::new(BTreeMap::new()),
            mode,
            project,
            temporary,
            spill,
            skill_roots: Vec::new(),
            exclude,
            benitoite: None,
            skill: None,
            check_query: None,
            shell: None,
            image_helper: None,
            run: None,
            web_fetch: WebFetchContext::default(),
            session: None,
            read_versions: Mutex::new(BTreeMap::new()),
            checkpoints: None,
            search_limits: SearchLimits::default(),
            scratch_state: Mutex::new(benitoite_scratch::ScratchState::default()),
        }
    }

    /// 利用可能性を確かめた画像の補助だけを追加する。
    pub fn with_image_helper(mut self, helper: ImageHelperContext) -> Self {
        self.image_helper = Some(helper);
        self
    }

    /// 提供者が画像を受け付けるかを設定する。
    pub fn with_image_input(mut self, enabled: bool) -> Self {
        self.image_input = enabled;
        self
    }

    /// 利用者の設定だけから、評価で比べる振る舞いを固定する（設計書 01-03）。
    pub fn with_eval_switches(mut self, after_write_check: bool, read_line_numbers: bool) -> Self {
        self.after_write_check = after_write_check;
        self.read_line_numbers = read_line_numbers;
        self
    }

    /// 作る前の書ける場所の検査に使う、起動時のデータの場所と実行ファイル。
    pub fn with_project_creation(
        mut self,
        paths: crate::config::paths::DataPaths,
        executable: PathBuf,
    ) -> Self {
        self.project_creation = Some((paths, executable));
        self
    }

    pub(crate) fn take_created_project(
        &self,
        call_id: &str,
    ) -> Result<Option<ScopeRoot>, ToolError> {
        self.project_candidates
            .lock()
            .map_err(|_| ToolError::State)?
            .remove(call_id);
        Ok(self
            .created_projects
            .lock()
            .map_err(|_| ToolError::State)?
            .remove(call_id))
    }

    pub(crate) fn prepare_project_switch(
        &self,
        root: ScopeRoot,
    ) -> Result<PreparedProjectSwitch, ToolError> {
        // 版の状態の誤りも、出来事の記録より先に検知する。
        drop(self.read_versions.lock().map_err(|_| ToolError::State)?);
        let exclude = self.exclude.rebased(root.real_path());
        let mut run = self.run.clone();
        if let Some(run) = &mut run {
            run.locations.project = root.real_path().into();
        }
        Ok(PreparedProjectSwitch {
            project: root,
            exclude,
            run,
        })
    }

    pub(crate) fn switch_project(&mut self, prepared: PreparedProjectSwitch) {
        self.project = prepared.project;
        self.exclude = prepared.exclude;
        self.read_versions = Mutex::new(BTreeMap::new());
        self.run = prepared.run;
        if let Some(processor) = &self.benitoite {
            processor.set_project(self.project.real_path());
        }
    }

    /// 起動時に確かめた処理系を、ヘッダと Benitoite の道具で共有する。
    pub fn with_benitoite(mut self, processor: Option<Arc<crate::benitoite::Processor>>) -> Self {
        self.benitoite = processor;
        self
    }

    /// セッションの間共有する Skill の問い合わせとキャッシュを追加する。
    pub fn with_skill(mut self, skill: Arc<crate::benitoite::skill::SkillDocuments>) -> Self {
        self.skill = Some(skill);
        self
    }

    /// 探索時に開いた Skill の根を保持し、読み取りの範囲だけに加える。
    pub fn with_skill_roots(mut self, roots: Vec<ScopeRoot>) -> Self {
        self.skill_roots = roots;
        self
    }

    /// 起動時の処理系の検査の口を共有する。
    pub fn with_check_query(mut self, query: Arc<crate::benitoite::CheckQuery>) -> Self {
        self.check_query = Some(query);
        self
    }

    /// 制限モードの実行に要る、起動時に固定した値を渡す。
    pub fn with_run_context(mut self, run: crate::benitoite::RunContext) -> Self {
        self.run = Some(run);
        self
    }

    /// 通常モードの起動時に検査した子プロセスの情報を追加する。
    pub fn with_shell(mut self, shell: ShellContext) -> Self {
        self.shell = Some(shell);
        self
    }

    /// 利用者の設定で検査したドメインと HTTP クライアントを渡す。
    pub fn with_web_fetch(mut self, web_fetch: WebFetchContext) -> Self {
        self.web_fetch = web_fetch;
        self
    }

    /// 検索の上限を小さくして渡す。固定の安全の上限を超えて広げない。
    pub fn with_search_limits(mut self, limits: SearchLimits) -> Self {
        let defaults = SearchLimits::default();
        self.search_limits = SearchLimits {
            max_entries: limits.max_entries.min(defaults.max_entries),
            max_duration: limits.max_duration.min(defaults.max_duration),
            max_file_bytes: limits.max_file_bytes.min(defaults.max_file_bytes),
        };
        self
    }

    /// 子プロセスの出力を保存するセッションを設定する。
    pub fn with_session(mut self, session: Session) -> Self {
        self.session = Some(session);
        self
    }

    /// 書き戻した場所の読んだ版を消し、次の edit で読み直させる。
    pub fn clear_read_version(&self, path: &Path) -> Result<(), ToolError> {
        self.read_versions
            .lock()
            .map_err(|_| ToolError::State)?
            .remove(path);
        Ok(())
    }

    /// 枝を移った後、すべての edit に読み直しを求める。
    pub fn clear_read_versions(&self) -> Result<(), ToolError> {
        self.read_versions
            .lock()
            .map_err(|_| ToolError::State)?
            .clear();
        Ok(())
    }

    /// 実行する側だけが呼ぶ。保存の失敗で道具は止めず、取り消しでは実行しない。
    pub fn checkpoint_before(
        &self,
        reason: crate::rewind::CheckpointReason,
        path: Option<&Path>,
        invocation: &Invocation<'_>,
    ) -> Result<(), ToolError> {
        if let Some(recorder) = &self.checkpoints
            && path.is_none_or(|path| recorder.contains(path))
        {
            let _recorded =
                recorder.capture(reason, Some(invocation.call_id), invocation.cancelled);
        }
        if invocation.cancelled.load(Ordering::Acquire) {
            return Err(ToolError::InvalidArguments(
                text::CHECKPOINT_CANCELLED.into(),
            ));
        }
        if let (Some(session), Some(real_path)) = (&self.session, path) {
            // 記録を保存できなくても道具を止めない。不足した行は巻き戻しの判定で
            // 保守的に「戻せない」と扱う（設計書 01-05「不完全な時点」）。
            let _recorded = session.writer().and_then(|mut writer| {
                writer.append(&crate::session::Record::WriteTarget {
                    call_id: invocation.call_id.into(),
                    real_path: real_path.into(),
                })
            });
        }
        Ok(())
    }

    /// read または書き込みを終えた実のパスの版を記録する。
    pub fn record_read_version(
        &self,
        path: PathBuf,
        version: FileVersion,
    ) -> Result<(), ToolError> {
        self.read_versions
            .lock()
            .map_err(|_| ToolError::State)?
            .insert(path, version);
        Ok(())
    }

    /// 書く道具が、read の記録と現在の版を突き合わせる。
    pub fn read_version(&self, path: &Path) -> Result<Option<FileVersion>, ToolError> {
        Ok(self
            .read_versions
            .lock()
            .map_err(|_| ToolError::State)?
            .get(path)
            .cloned())
    }
}

/// 確認を受ける側へ渡す、実行前の操作の要約。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalRequest {
    pub kind: ApprovalKind,
    pub facts: ApprovalFacts,
    pub justification: Option<String>,
    pub summary: String,
}

/// 検査の起動を、LLM の文によらず数えるための区分。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckLaunch {
    Tool,
    AfterWrite,
}

/// 同じ実のパスで続けて失敗した回数を数える結果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckOutcome {
    pub key: PathBuf,
    pub failed: bool,
    pub diagnostics: String,
}

/// LLM の結果、画面用の文、実行前の確認の要求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolOutput {
    /// 結果と一緒に送る画像。
    pub images: Vec<ToolImage>,
    /// 信頼する道具側で、存在の検査より先に方針によって拒否した。
    pub policy_denied: bool,
    pub text: String,
    pub is_error: bool,
    pub display: Option<String>,
    /// 道具が数えた画面用の一行の要約。
    pub screen: Option<String>,
    pub approval: Option<ApprovalRequest>,
    pub spill: Option<SpillInfo>,
    pub check_launch: Option<CheckLaunch>,
    pub check: Option<CheckOutcome>,
    pub processor_launches: u64,
    pub stderr_spill: Option<SpillInfo>,
    pub stop_run: Option<String>,
}

impl ToolOutput {
    /// 成功の結果にも共通の 64 KiB の上限を掛ける。
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            images: Vec::new(),
            policy_denied: false,
            text: truncate::cap_middle(&text.into()),
            is_error: false,
            display: None,
            screen: None,
            approval: None,
            spill: None,
            check_launch: None,
            check: None,
            processor_launches: 0,
            stderr_spill: None,
            stop_run: None,
        }
    }

    /// 誤りもセッションを止めずに LLM へ返す。
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            is_error: true,
            ..Self::success(text)
        }
    }

    /// 実行せずに確認の要求を返す。許可後に同じ呼び出しを再実行する。
    pub fn needs_approval(request: ApprovalRequest) -> Self {
        Self {
            text: truncate::cap_middle(&request.summary),
            images: Vec::new(),
            policy_denied: false,
            approval: Some(request),
            is_error: false,
            display: None,
            screen: None,
            spill: None,
            check_launch: None,
            check: None,
            processor_launches: 0,
            stderr_spill: None,
            stop_run: None,
        }
    }
}

/// 道具の失敗。LLM に示す文は text の関数で組み立てる。
#[derive(Debug)]
pub enum ToolError {
    InvalidArguments(String),
    SafeFs(SafeFsError),
    Io(io::Error),
    Excluded(String),
    OutsideScope,
    NotText,
    Offset { offset: u64, total: usize },
    Pattern(String),
    State,
    Changed,
    NotRead,
    StaleRead,
    RestrictedExtension,
    ProtectedWrite,
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments(message) | Self::Pattern(message) => f.write_str(message),
            Self::SafeFs(error) => write!(f, "{} {error}", text::SAFE_FS),
            Self::Io(error) => write!(f, "{} {error}", text::IO),
            Self::Excluded(rule) => f.write_str(&text::excluded(rule)),
            Self::OutsideScope => f.write_str(text::OUTSIDE_SCOPE),
            Self::NotText => f.write_str(text::NOT_TEXT),
            Self::Offset { offset, total } => f.write_str(&text::offset(*offset, *total)),
            Self::State => f.write_str(text::STATE),
            Self::Changed => f.write_str(text::CHANGED),
            Self::NotRead => f.write_str(text::NOT_READ),
            Self::StaleRead => f.write_str(text::STALE_READ),
            Self::RestrictedExtension => f.write_str(text::RESTRICTED_EXTENSION),
            Self::ProtectedWrite => f.write_str(text::PROTECTED_WRITE),
        }
    }
}

impl std::error::Error for ToolError {}

impl From<SafeFsError> for ToolError {
    fn from(error: SafeFsError) -> Self {
        Self::SafeFs(error)
    }
}

impl From<io::Error> for ToolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// 会話と記録の再開から独立した、確認の単位。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConfirmKey {
    /// スクリプトの絶対の実のパス。
    Run(PathBuf),
    /// テストの対象の並び（P2-12）。
    Test(Vec<PathBuf>),
}
impl ConfirmKey {
    /// 道具とループで同じ事実から単位を得る。
    pub fn from_facts(kind: ApprovalKind, facts: &ApprovalFacts) -> Option<Self> {
        match kind {
            ApprovalKind::Effects => facts.targets.first().cloned().map(Self::Run),
            ApprovalKind::TestEffects if !facts.targets.is_empty() => {
                let mut targets = facts.targets.clone();
                targets.sort();
                targets.dedup();
                Some(Self::Test(targets))
            }
            ApprovalKind::TestEffects
            | ApprovalKind::CreateProject
            | ApprovalKind::WriteScope
            | ApprovalKind::Command
            | ApprovalKind::Escalate
            | ApprovalKind::WebFetch => None,
        }
    }
}
