//! HTTP の診断に用いる文言（設計書 02-01「HTTP の通信」）。

pub const CONNECTION: &str = "HTTP connection failed";
pub const TIMEOUT: &str = "HTTP request timed out";
pub const TLS: &str = "TLS connection failed";
pub const CANCELLED: &str = "HTTP request was cancelled";
pub const BODY_LIMIT: &str = "HTTP response body exceeds the limit";
pub const STATUS: &str = "HTTP response has an unsuccessful status";
pub const INVALID_REQUEST: &str = "Invalid HTTP request";
pub const WORKER: &str = "HTTP worker failed";
