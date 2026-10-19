//! 会話、イベント、機能、誤りの共通の形（設計書 02-01「共通の形」「提供者ごとの機能の表」、01-01「やり直し」）。

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::text;

/// 推論の送り返しの可否を判定する送り先の組。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Destination {
    /// 設定ファイルの提供者の名前。
    pub provider: String,
    /// ベースの URL。認証の情報を含めない。
    pub base_url: String,
    /// モデルの名前。
    pub model: String,
}

/// 会社ではなく通信の形を区別する。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterKind {
    /// Anthropic Messages API。
    AnthropicMessages,
    /// OpenAI Chat Completions API。
    #[serde(rename = "openai-chat")]
    OpenAiChat,
    /// OpenAI Responses API。
    #[serde(rename = "openai-responses")]
    OpenAiResponses,
}

/// セッションに記録する会話。項目の順序を保持する。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    /// 利用者の発言、応答、道具の結果の並び。
    pub items: Vec<Item>,
}

/// 会話の一項目。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "content", rename_all = "snake_case")]
pub enum Item {
    /// 利用者の発言。
    UserMessage(UserMessage),
    /// 一回の要求への応答。
    AssistantTurn(AssistantTurn),
    /// 道具の実行結果。
    ToolResult(ToolResult),
}

/// 環境の情報と依頼を一つの発言に収める（設計書 01-04「文脈の構成」）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    /// アダプタはこの順で文の部分に変える。
    pub parts: Vec<UserPart>,
}

/// 利用者の発言の文の部分。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserPart {
    /// 文の用途。
    pub kind: UserPartKind,
    /// 提供者に渡す文。
    pub text: String,
}

/// 利用者の発言の部分の用途。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserPartKind {
    /// 環境の情報。
    Environment,
    /// 最初の依頼または後続の依頼。
    Request,
    /// 処理の途中の操縦。
    Steering,
    /// 圧縮で作った要約。利用者の依頼には数えない。
    Summary,
}

/// 応答の写しと、同じ送り先へ戻すための元の項目。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssistantTurn {
    /// 文、推論、道具の呼び出しが交互に現れる順序を保つ。
    pub parts: Vec<AssistantPart>,
    /// 順序や署名を変えずに保つ提供者の項目。
    pub raw_items: RawItems,
    /// 終わりのイベントで確かめた理由。未到着の切断は Disconnected。
    pub finish_reason: FinishReason,
    /// 報告されたトークンの量。
    pub usage: Usage,
}

/// 応答の中で順序を保つ内容。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "content", rename_all = "snake_case")]
pub enum AssistantPart {
    /// 応答の文。
    Text(String),
    /// 推論の表示用の文と不透明な値。
    Reasoning(Reasoning),
    /// 道具の呼び出し。
    ToolCall(ToolCall),
}

/// 元の項目と、それを作った送り先。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawItems {
    /// 元の項目を受け取った送り先。
    pub destination: Destination,
    /// 項目を解釈する通信の形。
    pub adapter: AdapterKind,
    /// 提供者が返したままの項目の並び。
    pub items: Vec<Value>,
}

/// 推論の内容。送り返す際の条件の検査は文脈の管理が行う（設計書 01-04「推論の内容の扱い」）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reasoning {
    /// 表示用の文。文の報告がなければ空。
    pub text: String,
    /// 署名や暗号化した内容など、解釈せず保持する値。
    pub opaque: Value,
}

/// 検証と実行に渡す道具の呼び出し。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// benitoiteh が付ける内部の ID。
    pub id: String,
    /// 提供者の呼び出しの ID。Responses では call_id。
    pub provider_id: Option<String>,
    /// Responses の項目の id。結果の対応付けには使わない。
    pub provider_item_id: Option<String>,
    /// 道具の名前。
    pub name: String,
    /// 解析した引数。解析に失敗した場合は Null とし、元の文字列を検証に使う。
    pub arguments: Value,
    /// 受け取った引数の文字列。検証の誤りを示すために保持する。
    pub raw_arguments: String,
}

/// 呼び出しに対応する道具の結果。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// 呼び出しの内部の ID。
    pub id: String,
    /// 提供者の呼び出しの ID。Responses では call_id。
    pub provider_id: Option<String>,
    /// LLM に渡す結果の文。
    pub text: String,
    /// 実行または検証の誤りか。
    pub is_error: bool,
    /// 画像の参照。中身は objects/ に保存し、記録には含めない。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ToolImage>,
}

/// 道具が読んだ画像の参照（設計書 02-01「画像の受け渡し」）。
#[derive(Clone, Serialize, Deserialize)]
pub struct ToolImage {
    /// MIME の型。
    pub media_type: String,
    /// 中身の SHA-256（小文字の十六進 64 字）。
    pub sha256: String,
    /// 横の画素数。
    pub width: u32,
    /// 縦の画素数。
    pub height: u32,
    /// 中身のバイト数。
    pub bytes: u64,
    /// 要求を送るときだけ使う中身。記録と診断には含めない。
    #[serde(skip)]
    pub data: Option<std::sync::Arc<[u8]>>,
}

impl fmt::Debug for ToolImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolImage")
            .field("media_type", &self.media_type)
            .field("sha256", &self.sha256)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl PartialEq for ToolImage {
    fn eq(&self, other: &Self) -> bool {
        self.media_type == other.media_type
            && self.sha256 == other.sha256
            && self.width == other.width
            && self.height == other.height
            && self.bytes == other.bytes
    }
}
impl Eq for ToolImage {}

/// 報告なしと 0 を区別するトークンの量。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// キャッシュの読み書きを含む入力全体。
    pub input_tokens: Option<u64>,
    /// 入力のうちキャッシュから読んだ量。
    pub cache_read_tokens: Option<u64>,
    /// 入力のうちキャッシュに書いた量。
    pub cache_write_tokens: Option<u64>,
    /// 出力全体。
    pub output_tokens: Option<u64>,
    /// 出力のうち推論の量。
    pub reasoning_tokens: Option<u64>,
    /// 提供者が返した値そのもの。報告がなければ Null。
    pub raw: Value,
}

/// 道具の実行の可否とやり直しの判断に使う終わりの理由。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "category", rename_all = "snake_case")]
pub enum FinishReason {
    /// 通常の終了。
    Completed,
    /// 道具の呼び出しを伴う終了。
    ToolCalls,
    /// 出力のトークンの上限。
    OutputLimit,
    /// 提供者が示した拒否の分類。
    Refusal(String),
    /// 応答の中の誤り。
    Error,
    /// 利用者の取り消し。
    Cancelled,
    /// 終わりのイベントの前の切断。
    Disconnected,
}

/// 応答の中の番号は提供者の block/item の番号であり、差分どうしを対応付ける。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    /// 応答の文の差分。
    TextDelta { index: usize, text: String },
    /// 推論の表示用の文の差分。
    ReasoningDelta { index: usize, text: String },
    /// 道具の呼び出しの始まり。
    ToolCallStart {
        index: usize,
        provider_id: Option<String>,
        provider_item_id: Option<String>,
        name: String,
    },
    /// 道具の引数の文字列の差分。
    ToolArgumentsDelta { index: usize, delta: String },
    /// 一つの道具の引数の受け取りの終わり。応答全体の正常終了とは別である。
    ToolCallEnd { index: usize },
    /// トークンの量。差分の合算や正規化はアダプタが行う。
    Usage { usage: Usage },
    /// 応答全体の終わり。
    Finished { reason: FinishReason },
}

/// 自動でやり直さない誤りの内訳。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NonRetryableKind {
    /// 認証の失敗。
    Authentication,
    /// 残高または利用の上限。
    Quota,
    /// 要求の形の誤り。
    InvalidRequest,
    /// ストリーム開始前の HTTP 400・413 による要求の拒否。
    RequestRejected,
    /// 送り返した thinking の検査の拒否。
    ThinkingRejected,
}

/// アダプタが分類してループに返す誤り。提供者の本文やキーを診断に含めない。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderError {
    /// 一時的な通信や提供者の失敗。
    Retryable { retry_after_secs: Option<u64> },
    /// 文脈の長さの超過。
    ContextOverflow,
    /// 自動でやり直さない失敗。
    NonRetryable { reason: NonRetryableKind },
    /// 応答の途中の切断。受け取った内容を捨ててやり直す。
    StreamDisconnected,
    /// 応答の形や SSE を読み取れない。
    InvalidResponse,
    /// 内容が空の応答。やり直す。
    EmptyResponse,
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Retryable { .. } => text::RETRYABLE,
            Self::ContextOverflow => text::CONTEXT_OVERFLOW,
            Self::NonRetryable { reason } => match reason {
                NonRetryableKind::Authentication => text::AUTHENTICATION,
                NonRetryableKind::Quota => text::QUOTA,
                NonRetryableKind::InvalidRequest | NonRetryableKind::RequestRejected => {
                    text::INVALID_REQUEST
                }
                NonRetryableKind::ThinkingRejected => text::THINKING_REJECTED,
            },
            Self::StreamDisconnected => text::STREAM_DISCONNECTED,
            Self::InvalidResponse => text::INVALID_RESPONSE,
            Self::EmptyResponse => text::EMPTY_RESPONSE,
        })
    }
}

impl std::error::Error for ProviderError {}

/// 推論の内容を送り返す規則。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningReplay {
    /// 送らない。
    #[default]
    None,
    /// 同じ送り先に送り、圧縮の後は残した番から除く。
    SameDestination,
    /// 圧縮の後も残したすべての番の文を送る。
    AllTurns,
}

/// 互換の口は既定では追加の機能を使わない。
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    /// 道具の結果の画像を送るか。
    #[serde(default)]
    pub image_input: bool,
    /// Anthropic のキャッシュの区切り。
    pub cache_control: bool,
    /// 厳密な道具の引数の生成。
    pub strict_tools: bool,
    /// 道具の引数の細かいストリーミング。
    pub eager_input_streaming: bool,
    /// 道具を呼ばせない指定。
    pub tool_choice_none: bool,
    /// 推論の送り返しの規則。
    pub reasoning_replay: ReasoningReplay,
    /// 一つの応答で複数の道具を呼ぶ指定。
    pub parallel_tool_calls: bool,
    /// Chat Completions の要求に推論の深さを付けるか。
    pub reasoning_effort: bool,
    /// モデルの文脈の上限。不明なら None。
    pub context_window: Option<u64>,
    /// モデルの出力の上限。不明なら None。
    pub max_output_tokens: Option<u64>,
}

impl AdapterKind {
    /// 設定とオプションで省略したときの推論の深さ（設計書 02-01、05-01「オプション」）。
    /// Chat Completions は reasoning_effort の機能が真のときだけ要求に付ける。
    pub fn default_effort(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "low",
            Self::OpenAiChat | Self::OpenAiResponses => "high",
        }
    }

    /// 表にない互換の口に用いる、機能を使わない側の値。
    pub fn default_capabilities(self) -> Capabilities {
        Capabilities::default()
    }

    /// 本家の口で対応する機能の値。
    pub fn official_capabilities(self) -> Option<Capabilities> {
        match self {
            Self::AnthropicMessages => Some(Capabilities {
                image_input: true,
                cache_control: true,
                strict_tools: true,
                eager_input_streaming: true,
                tool_choice_none: true,
                reasoning_replay: ReasoningReplay::SameDestination,
                parallel_tool_calls: true,
                ..Capabilities::default()
            }),
            Self::OpenAiChat => Some(Capabilities {
                strict_tools: true,
                tool_choice_none: true,
                parallel_tool_calls: true,
                ..Capabilities::default()
            }),
            Self::OpenAiResponses => Some(Capabilities {
                image_input: true,
                strict_tools: true,
                tool_choice_none: true,
                parallel_tool_calls: true,
                reasoning_replay: ReasoningReplay::SameDestination,
                ..Capabilities::default()
            }),
        }
    }
}
