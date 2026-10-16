//! 提供者に依らない会話と SSE の読み取りを扱う（設計書 02-01「層の分け方」）。

pub mod anthropic;
pub mod ids;
pub mod sse;
mod text;
pub mod tool_schema;
pub mod types;

pub use tool_schema::{ArgumentSpec, ArgumentType, SchemaKind, ToolSpec};
pub use types::{
    AdapterKind, AssistantPart, AssistantTurn, Capabilities, Conversation, Destination,
    FinishReason, Item, NonRetryableKind, ProviderError, RawItems, Reasoning, ReasoningReplay,
    StreamEvent, ToolCall, ToolResult, Usage, UserMessage, UserPart, UserPartKind,
};
