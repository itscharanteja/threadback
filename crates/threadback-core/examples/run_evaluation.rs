use std::{fs, path::PathBuf};

use threadback_core::{render_evaluation_markdown, run_evaluation_suite};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report = run_evaluation_suite();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let docs_dir = root.join("docs");
    fs::create_dir_all(&docs_dir)?;

    let markdown = render_evaluation_markdown(&report);
    let md_path = docs_dir.join("EVALUATION.md");
    fs::write(&md_path, &markdown)?;

    let json_path = root.join("fixtures/evaluation/latest-report.json");
    fs::write(&json_path, serde_json::to_string_pretty(&report)?)?;

    println!(
        "Evaluation complete: {}/{} scenarios passed.",
        report.summary.scenarios_passed, report.summary.scenarios_total
    );
    println!("Wrote {} and {}", md_path.display(), json_path.display());

    if report.summary.scenarios_passed != report.summary.scenarios_total
        || report.comparisons.iter().any(|c| !c.passed)
    {
        return Err("One or more evaluation gates failed.".into());
    }
    Ok(())
}
