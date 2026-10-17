//! 検索の走査時間を同じ入力で比較する（設計書 06-02「測り方」）。

use benitoiteh::config::{Mode, ReadConfig, paths::DataPaths};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::tools::{ToolContext, ToolSet, exclude::ExcludeRules};
use serde_json::json;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

// 作成時の関門: FD を開き直す回数の増加を、実装内のカウンタではなく、
// 公開の道具の完了時間と結果で測る。時間の閾値で通常の検査を不安定にしない。
#[test]
#[ignore = "manual search traversal measurement; run release with --ignored --nocapture"]
fn measure_safe_search_traversal() -> TestResult {
    let mut random = [0; 16];
    getrandom::fill(&mut random)?;
    let root = std::env::temp_dir().join(format!(
        "benitoiteh-walk-measure-{:032x}",
        u128::from_ne_bytes(random)
    ));
    fs::create_dir(&root)?;
    let fixture = Fixture(fs::canonicalize(root)?);
    let project = fixture.0.join("project");
    let temporary = fixture.0.join("temporary");
    let paths = DataPaths::from_values(Some(&fixture.0), None, None, Some(&fixture.0))?;
    let spill = paths.data_dir.join("sessions/current/spill");
    for directory in [&project, &temporary, &spill] {
        fs::create_dir_all(directory)?;
    }
    let mut deep = project.join("deep");
    fs::create_dir(&deep)?;
    for _ in 0..240 {
        deep.push("d");
        fs::create_dir(&deep)?;
        fs::write(deep.join("s.txt"), "hay\n")?;
    }
    fs::write(deep.join("end.txt"), "needle\n")?;
    let wide = project.join("wide");
    fs::create_dir(&wide)?;
    for index in 0..4096 {
        fs::write(wide.join(format!("file-{index:04}.txt")), "hay\n")?;
    }
    fs::write(wide.join("end.txt"), "needle\n")?;
    let context = ToolContext::new(
        Mode::Restricted,
        ScopeRoot::open(&project)?,
        ScopeRoot::open(&temporary)?,
        ScopeRoot::open(&spill)?,
        ExcludeRules::new(
            &project,
            &paths,
            &spill,
            &ReadConfig::default(),
            &ReadConfig::default(),
        )?,
    );
    let tools = ToolSet::new(Mode::Restricted);
    for path in ["deep", "wide"] {
        for name in ["glob", "grep"] {
            let pattern = if name == "glob" { "*.txt" } else { "needle" };
            let args = json!({"path":path,"pattern":pattern});
            let warm = tools.execute(&context, name, args.clone());
            assert!(!warm.is_error, "{}", warm.text);
            let mut samples = Vec::new();
            for _ in 0..7 {
                let start = Instant::now();
                let result = tools.execute(&context, name, args.clone());
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                assert!(!result.is_error, "{}", result.text);
                assert!(result.text.contains("end.txt"), "{}", result.text);
                assert!(!result.text.contains("Search stopped"), "{}", result.text);
                samples.push(elapsed);
            }
            samples.sort_by(f64::total_cmp);
            let median = samples.get(3).ok_or("missing median")?;
            println!("{path},{name},median_ms={median:.3},samples_ms={samples:?}");
        }
    }
    Ok(())
}
