//! CLI の入口に終了状態の決定を委ねる（設計書 05-01「終了状態」）。

fn main() -> std::process::ExitCode {
    benitoiteh::cli::main()
}
