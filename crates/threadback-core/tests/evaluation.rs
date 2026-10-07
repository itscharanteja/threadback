use threadback_core::{load_scenario_specs, render_evaluation_markdown, run_evaluation_suite};

#[test]
fn all_ten_scenarios_and_day_six_verification_gates_pass() {
    let specs = load_scenario_specs();
    assert_eq!(specs.len(), 10);

    let report = run_evaluation_suite();
    assert_eq!(report.summary.scenarios_total, 10);
    assert_eq!(
        report.summary.scenarios_passed,
        10,
        "failed scenarios: {:?}",
        report
            .scenarios
            .iter()
            .filter(|s| !s.passed)
            .collect::<Vec<_>>()
    );

    // Day 6 verification gates
    assert_eq!(
        report.summary.unsupported_verified_facts_after_validation, 0,
        "Unsupported verified facts after validation must be zero"
    );
    assert_eq!(
        report.summary.seeded_secret_leakage, 0,
        "Seeded secret leakage must be zero"
    );
    assert_eq!(
        report.summary.outside_root_access_successes, 0,
        "Outside-root access successes must be zero"
    );
    assert_eq!(
        report.summary.acknowledged_events_lost_on_restart, 0,
        "Acknowledged events lost during restart test must be zero"
    );
    assert!(
        report.summary.fact_evidence_precision >= 0.95,
        "Fact evidence precision must be at least 95%"
    );
    assert!(
        report.summary.next_action_agreement >= 0.80,
        "Next-action agreement must be at least 80%"
    );
    assert!(
        report.summary.material_correction_rate < 0.30,
        "Material correction rate must be below 30%"
    );
    assert!(
        report.summary.max_checkpoint_prep_ms < 2_000,
        "Checkpoint preparation must be under two seconds"
    );
    assert!(
        report.comparisons.iter().all(|c| c.passed),
        "Every target-vs-observed metric comparison must pass: {:?}",
        report.comparisons
    );
    assert!(
        !report.known_limitations.is_empty(),
        "Known limitations must be documented explicitly"
    );

    let markdown = render_evaluation_markdown(&report);
    assert!(markdown.contains("## 1. Target vs. Observed Metrics"));
    assert!(markdown.contains("## 2. Prepared Scenario Results (S01–S10)"));
    assert!(markdown.contains("## 3. Known Limitations"));
}
