//! 提供者に依らない会話と SSE の読み取りを扱う（設計書 02-01「層の分け方」）。

pub mod anthropic;
pub mod ids;
mod images;
pub mod openai_chat;
pub mod openai_responses;
pub mod sse;
mod text;
pub mod tool_schema;
pub mod types;

pub(crate) use images::{plan as image_plan, remove_images};

pub use tool_schema::{ArgumentSpec, ArgumentType, SchemaKind, ToolSpec};
pub use types::{
    AdapterKind, AssistantPart, AssistantTurn, Capabilities, Conversation, Destination,
    FinishReason, Item, NonRetryableKind, ProviderError, RawItems, Reasoning, ReasoningReplay,
    StreamEvent, ToolCall, ToolImage, ToolResult, Usage, UserMessage, UserPart, UserPartKind,
};
