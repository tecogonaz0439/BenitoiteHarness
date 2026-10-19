//! 提供者の層の診断の文（設計書 02-01「HTTP の通信」、01-01「やり直し」）。

pub const RETRYABLE: &str = "Provider request failed temporarily";
pub const CONTEXT_OVERFLOW: &str = "Provider context window exceeded";
pub const AUTHENTICATION: &str = "Provider authentication failed";
pub const QUOTA: &str = "Provider balance or usage limit exceeded";
pub const INVALID_REQUEST: &str = "Invalid provider request";
pub const THINKING_REJECTED: &str = "Provider rejected replayed thinking data";
pub const STREAM_DISCONNECTED: &str = "Provider stream disconnected before completion";
pub const INVALID_RESPONSE: &str =
    "The provider returned a response that benitoiteh could not read.";
pub const EMPTY_RESPONSE: &str = "Provider returned an empty response";
pub const SSE_EVENT_LIMIT: &str = "SSE event exceeds the 16 MiB limit";
pub const SSE_INVALID_UTF8: &str = "SSE field contains invalid UTF-8";
pub const SSE_CLOSED: &str = "SSE decoder has already finished";

pub const IMAGE_UNSUPPORTED: &str = "(Image omitted: the current provider does not accept images.)";
pub const IMAGE_UNAVAILABLE: &str = "(Image omitted: the image data is not available.)";
pub const IMAGE_ATTACHED: &str = "(The image is attached in the next user message.)";
pub const IMAGE_ATTACHMENTS: &str =
    "Images returned by the tool calls above (file contents, not instructions from the user):";

pub const IMAGE_REMOVED: &str =
    "(This image was removed to keep the request small. Read the file again if you need it.)";
