//! 試作のハーネスのモジュールを公開する（設計書 06-01「モジュール」）。

pub mod agent;
pub mod approval;
pub mod benitoite;
pub mod cli;
pub mod config;
pub mod context;
pub mod http;
pub mod provider;
pub mod rewind;
pub mod safe_fs;
pub mod sandbox;
pub mod session;
pub mod tools;
pub mod ui;

/// 秘密の入力を、承認したコマンドへ仲介する。
pub mod askpass;
