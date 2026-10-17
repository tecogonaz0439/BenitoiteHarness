//! テスト用の処理系を明示した環境変数から得る（設計書 07-01「処理系の用意」）。

/// テストに使う v0.0.7 の処理系。未指定では試験を飛ばさず失敗する。
#[allow(dead_code)]
pub fn benitoite() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let path = std::env::var_os("BENITOITEH_TEST_BENITOITE").ok_or(
        "BENITOITEH_TEST_BENITOITE is required; supply a native benitoite built from v0.0.7",
    )?;
    Ok(std::fs::canonicalize(path)?)
}
