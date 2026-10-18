//! BH-029 の表示パスと読める囮が衝突する、本物の処理系向けの準備。

use std::{fs, path::Path};

pub fn prepare(project: &Path, nested_package: bool) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(project.join("t5/Secret"))?;
    fs::create_dir_all(project.join("t5/Lib/Secret"))?;
    fs::write(
        project.join("t5/Secret/Keys.bnt"),
        "public function token() -> String\n return \"SECRET_HASH_COLLISION\"\nend function\n",
    )?;
    if nested_package {
        fs::write(project.join("t5/Secret/package.bnt"), "")?;
    }
    fs::write(
        project.join("t5/Lib/Secret/Keys.bnt"),
        "public function token() -> String\n return \"readable decoy\"\nend function\n",
    )?;
    fs::write(
        project.join("t5/Lib/T.bnt"),
        "import Secret.Keys\nimport Benitoite.Assert\n@test\nfunction example() -> Unit uses Assert.Check\n Assert.equal(actual: Keys.token(), expected: \"\")\nend function\n",
    )?;
    Ok(())
}
