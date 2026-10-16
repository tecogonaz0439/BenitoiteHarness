//! CLI に表示する英文をまとめる（設計書 06-01「lint と実装の規約」）。

/// 利用者向けのコマンドラインの一覧。
pub const HELP: &str = "Usage:
    benitoiteh [options] [request]
    benitoiteh exec [options] <request>
    benitoiteh doctor
    benitoiteh sessions
    benitoiteh --help | --version | --licenses
";

/// 使い方の誤りに共通の案内。
pub const INVALID_ARGUMENTS: &str = "benitoiteh: invalid arguments; use --help for usage";

/// 重複した指定を示す。
pub fn duplicate_option(option: &str) -> String {
    format!("benitoiteh: duplicate option {option}; use --help for usage")
}

/// 後の段階で使える指定を示す。
pub fn unavailable_option(option: &str) -> String {
    format!("benitoiteh: option {option} is not available yet")
}

/// ビルドしたパッケージの版を一行に整える。
pub fn version() -> String {
    format!("benitoiteh {}\n", env!("CARGO_PKG_VERSION"))
}

/// 出力に失敗した理由を案内に添える。
pub fn output_error(error: &std::io::Error) -> String {
    format!("benitoiteh: could not write output: {error}")
}
