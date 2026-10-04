//! M-03: the capsule contract. Valid responses deserialize into `ResumeCapsule`
//! and survive a round trip; invalid ones are rejected by the schema.

use std::{fs, path::PathBuf};

use serde_json::{Value, json};
use threadback_core::{CapsuleSchema, ResumeCapsule, SCHEMA_VERSION, capsule_schema};

fn fixtures(folder: &str) -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/evaluation")
        .join(folder);
    let mut files: Vec<_> = fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("{}: {error}", root.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures in {folder}");
    files
        .into_iter()
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read_to_string(&path).unwrap(),
            )
        })
        .collect()
}

fn schema() -> CapsuleSchema {
    CapsuleSchema::new().unwrap()
}

fn valid_example() -> Value {
    serde_json::from_str(&fixtures("capsules/valid").remove(2).1).unwrap()
}

#[test]
fn the_schema_is_itself_a_valid_json_schema() {
    assert!(CapsuleSchema::new().is_ok());
    assert_eq!(
        capsule_schema()["properties"]["schema_version"]["const"],
        SCHEMA_VERSION
    );
}

#[test]
fn every_valid_fixture_matches_the_schema_and_round_trips_through_the_contract_type() {
    let schema = schema();
    for (name, text) in fixtures("capsules/valid") {
        let value: Value = serde_json::from_str(&text).unwrap();
        let issues = schema.check(&value);
        assert!(issues.is_empty(), "{name}: {issues:?}");

        let capsule: ResumeCapsule = serde_json::from_value(value.clone())
            .unwrap_or_else(|error| panic!("{name} does not deserialize: {error}"));
        // The wire names are exactly the spec's: nothing is lost or renamed.
        assert_eq!(serde_json::to_value(&capsule).unwrap(), value, "{name}");
    }
}

#[test]
fn the_example_in_the_spec_is_valid() {
    let spec = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec.md"))
        .unwrap();
    let section = spec
        .split("## 11. Resume capsule contract")
        .nth(1)
        .expect("spec section 11");
    let block = section
        .split("```json")
        .nth(1)
        .expect("a json block")
        .split("```")
        .next()
        .unwrap();
    let example: Value = serde_json::from_str(block).unwrap();

    assert!(
        schema().check(&example).is_empty(),
        "{:?}",
        schema().check(&example)
    );
    let capsule: ResumeCapsule = serde_json::from_value(example.clone()).unwrap();
    assert_eq!(serde_json::to_value(&capsule).unwrap(), example);
    assert_eq!(
        capsule.relevant_artifacts[0].artifact_type, "test",
        "`type` is the wire name"
    );
}

#[test]
fn every_invalid_fixture_is_rejected_with_a_located_reason() {
    let schema = schema();
    for (name, text) in fixtures("capsules/invalid") {
        let value: Value = serde_json::from_str(&text).unwrap();
        let issues = schema.check(&value);
        assert!(!issues.is_empty(), "{name} should be rejected");
        assert!(
            issues.iter().all(|issue| !issue.message.is_empty()),
            "{name}"
        );
    }
}

#[test]
fn rejections_point_at_the_field_that_is_wrong() {
    let schema = schema();
    let issues_for = |name: &str| {
        let text = fixtures("capsules/invalid")
            .into_iter()
            .find(|(file, _)| file == &format!("{name}.json"))
            .unwrap()
            .1;
        schema.check(&serde_json::from_str(&text).unwrap())
    };
    let any_at = |name: &str, path: &str| {
        let issues = issues_for(name);
        assert!(
            issues.iter().any(|issue| issue.path == path),
            "{name}: expected {path} in {issues:?}"
        );
    };
    any_at("fact-without-source", "/verified_facts/0/source_event_ids");
    any_at(
        "failed-attempt-without-source",
        "/failed_attempts/0/source_event_ids",
    );
    any_at("bad-confidence", "/hypotheses/0/confidence");
    any_at("wrong-schema-version", "/schema_version");
    any_at("questions-not-an-array", "/unresolved_questions");
    any_at("empty-claim", "/verified_facts/0/claim");
    any_at(
        "source-id-not-a-string",
        "/verified_facts/0/source_event_ids/0",
    );
    // Whole-object problems are reported at the object that is missing or has extras.
    any_at("missing-next-action", "");
    any_at("unknown-top-level-field", "");
    any_at("unknown-field-in-next-action", "/next_action");
    any_at("unknown-field-in-fact", "/verified_facts/0");
    any_at("unknown-field-in-artifact", "/relevant_artifacts/0");
    any_at("next-action-missing-success-check", "/next_action");
}

#[test]
fn unknown_fields_are_rejected_by_the_contract_type_as_well() {
    // The schema and the Rust type are two independent gates.
    for name in [
        "unknown-top-level-field",
        "unknown-field-in-next-action",
        "unknown-field-in-fact",
        "unknown-field-in-artifact",
        "camel-case-field-names",
    ] {
        let text = fixtures("capsules/invalid")
            .into_iter()
            .find(|(file, _)| file == &format!("{name}.json"))
            .unwrap()
            .1;
        assert!(
            serde_json::from_str::<ResumeCapsule>(&text).is_err(),
            "{name}"
        );
    }
}

#[test]
fn every_section_is_required() {
    let schema = schema();
    let example = valid_example();
    for section in example.as_object().unwrap().keys() {
        let mut without = example.clone();
        without.as_object_mut().unwrap().remove(section);
        assert!(
            !schema.is_valid(&without),
            "removing {section} should be rejected"
        );
    }
}

#[test]
fn only_the_three_confidence_levels_are_accepted() {
    let schema = schema();
    let mut example = valid_example();
    for level in ["low", "medium", "high"] {
        example["hypotheses"][0]["confidence"] = json!(level);
        assert!(schema.is_valid(&example), "{level}");
    }
    for level in ["certain", "Low", "HIGH", "", "none"] {
        example["hypotheses"][0]["confidence"] = json!(level);
        assert!(!schema.is_valid(&example), "{level:?}");
    }
}

#[test]
fn a_verified_fact_needs_a_source_but_a_hypothesis_and_next_action_need_not_have_one() {
    let schema = schema();
    let mut example = valid_example();
    example["hypotheses"][0]["source_event_ids"] = json!([]);
    example["next_action"]["source_event_ids"] = json!([]);
    example["relevant_artifacts"][0]["source_event_ids"] = json!([]);
    assert!(schema.is_valid(&example));
    example["verified_facts"][0]["source_event_ids"] = json!([]);
    assert!(!schema.is_valid(&example));
}

#[test]
fn sizes_are_bounded() {
    let schema = schema();
    let mut example = valid_example();
    example["verified_facts"][0]["claim"] = json!("x".repeat(1_000));
    assert!(schema.is_valid(&example));
    example["verified_facts"][0]["claim"] = json!("x".repeat(1_001));
    assert!(!schema.is_valid(&example), "an oversized claim");

    let mut example = valid_example();
    let fact = example["verified_facts"][0].clone();
    example["verified_facts"] = json!(vec![fact.clone(); 20]);
    assert!(schema.is_valid(&example));
    example["verified_facts"] = json!(vec![fact; 21]);
    assert!(!schema.is_valid(&example), "too many facts");

    let mut example = valid_example();
    example["next_action"]["source_event_ids"] =
        json!((0..21).map(|i| format!("evt_{i}")).collect::<Vec<_>>());
    assert!(!schema.is_valid(&example), "too many source ids");
}

#[test]
fn scalars_and_non_objects_are_rejected() {
    let schema = schema();
    for candidate in [json!(null), json!("{}"), json!([]), json!(1), json!({})] {
        assert!(!schema.is_valid(&candidate), "{candidate}");
    }
}

#[test]
fn issues_come_back_in_a_stable_order() {
    let schema = schema();
    let mut example = valid_example();
    example["hypotheses"][0]["confidence"] = json!("certain");
    example["verified_facts"][0]["claim"] = json!("");
    example["extra"] = json!(true);
    let first = schema.check(&example);
    assert_eq!(first, schema.check(&example));
    assert!(first.len() >= 3);
    let paths: Vec<_> = first.iter().map(|issue| issue.path.clone()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
}
