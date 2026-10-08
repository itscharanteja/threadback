use threadback_core::{
    load_scenario_specs, render_evaluation_markdown, run_evaluation_suite, run_workflow_evaluations,
};

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
    assert_eq!(report.summary.workflows_total, 5);
    assert_eq!(
        report.summary.workflows_passed,
        5,
        "failed workflows: {:?}",
        report
            .workflows
            .iter()
            .filter(|w| !w.passed)
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
    assert!(markdown.contains("## 3. End-to-End Persona Workflow Evaluations (W01–W05)"));
    assert!(markdown.contains("## 4. Known Limitations"));
}

#[test]
fn persona_workflows_w01_through_w05_complete_all_stages_without_secret_leaks() {
    let workflows = run_workflow_evaluations();
    assert_eq!(workflows.len(), 5);
    let ids: Vec<&str> = workflows.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, vec!["W01", "W02", "W03", "W04", "W05"]);
    for wf in &workflows {
        assert!(wf.passed, "workflow {} failed: {}", wf.id, wf.details);
        assert_eq!(wf.stages_completed, wf.stages_total);
        assert_eq!(wf.secret_leaks_found, 0);
    }
}
