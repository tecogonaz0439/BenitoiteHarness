//! Anthropic アダプタの診断文（設計書 02-01「認証の情報」）。

pub const PROVIDER: &str = "The provider rejected or could not complete the request.";
pub const INVALID_REQUEST: &str = "Could not build the provider request.";
pub const INVALID_RESPONSE: &str = "The provider returned an invalid stream.";
pub const CANCELLED: &str = "The provider request was cancelled.";
