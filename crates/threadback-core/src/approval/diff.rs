use serde_json::Value;

use crate::{DomainError, ResumeCapsule};

/// One difference between the model's draft and the capsule the user approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    /// A JSON Pointer into the capsule's wire form, such as
    /// `/verified_facts/0/claim`. A removed item is located by its position in
    /// the draft; a changed or added one by its position in the approved capsule.
    pub field_path: String,
    /// `null` when the user added the item.
    pub before: Value,
    /// `null` when the user removed the item.
    pub after: Value,
}

/// Every change the user made, found by comparing the two capsules. The result
/// is derived, not reported: nothing the interface says about an edit can make
/// a change disappear from the record or appear in it without being made.
///
/// Lists are aligned by their unchanged items, so deleting one fact does not
/// make every fact after it look edited. A changed item is reported field by
/// field; an item that was removed and one that was added in the same place are
/// treated as one edit unless both carry different ids, which makes them two
/// separate items.
pub fn diff_capsules(
    draft: &ResumeCapsule,
    approved: &ResumeCapsule,
) -> Result<Vec<FieldChange>, DomainError> {
    let to_value = |capsule: &ResumeCapsule| {
        serde_json::to_value(capsule).map_err(|error| {
            DomainError::internal(format!("Capsule serialization failed: {error}"))
        })
    };
    let mut changes = Vec::new();
    diff_value("", &to_value(draft)?, &to_value(approved)?, &mut changes);
    Ok(changes)
}

fn diff_value(path: &str, before: &Value, after: &Value, out: &mut Vec<FieldChange>) {
    match (before, after) {
        (Value::Object(old), Value::Object(new)) => {
            for (key, old_value) in old {
                let child = format!("{path}/{}", escape(key));
                match new.get(key) {
                    Some(new_value) => diff_value(&child, old_value, new_value, out),
                    None => out.push(change(child, old_value.clone(), Value::Null)),
                }
            }
            for (key, new_value) in new {
                if !old.contains_key(key) {
                    out.push(change(
                        format!("{path}/{}", escape(key)),
                        Value::Null,
                        new_value.clone(),
                    ));
                }
            }
        }
        (Value::Array(old), Value::Array(new)) => diff_array(path, old, new, out),
        _ if before != after => out.push(change(path.to_owned(), before.clone(), after.clone())),
        _ => {}
    }
}

fn diff_array(path: &str, before: &[Value], after: &[Value], out: &mut Vec<FieldChange>) {
    let (old_len, new_len) = (before.len(), after.len());

    // lengths[i][j]: how many items before[i..] and after[j..] have in common, in order.
    let mut lengths = vec![vec![0usize; new_len + 1]; old_len + 1];
    for i in (0..old_len).rev() {
        for j in (0..new_len).rev() {
            lengths[i][j] = if before[i] == after[j] {
                lengths[i + 1][j + 1] + 1
            } else {
                lengths[i + 1][j].max(lengths[i][j + 1])
            };
        }
    }

    // Walk both lists; each run of items between two unchanged ones is a gap.
    let (mut i, mut j) = (0, 0);
    let mut removed: Vec<usize> = Vec::new();
    let mut added: Vec<usize> = Vec::new();
    while i < old_len || j < new_len {
        if i < old_len && j < new_len && before[i] == after[j] {
            settle_gap(path, before, after, &mut removed, &mut added, out);
            i += 1;
            j += 1;
        } else if j < new_len && (i == old_len || lengths[i][j + 1] >= lengths[i + 1][j]) {
            added.push(j);
            j += 1;
        } else {
            removed.push(i);
            i += 1;
        }
    }
    settle_gap(path, before, after, &mut removed, &mut added, out);
}

/// Pairs what was removed with what was added in the same gap, so an edited
/// item is reported as an edit, and reports the rest as removals and additions.
fn settle_gap(
    path: &str,
    before: &[Value],
    after: &[Value],
    removed: &mut Vec<usize>,
    added: &mut Vec<usize>,
    out: &mut Vec<FieldChange>,
) {
    let removed_items = std::mem::take(removed);
    let added_items = std::mem::take(added);

    let mut next_added = 0;
    let mut paired_added = vec![false; added_items.len()];
    let mut unpaired_removed = Vec::new();
    for &old_index in &removed_items {
        let partner = (next_added..added_items.len())
            .find(|&k| can_pair(&before[old_index], &after[added_items[k]]));
        match partner {
            Some(k) => {
                paired_added[k] = true;
                next_added = k + 1;
                diff_value(
                    &format!("{path}/{}", added_items[k]),
                    &before[old_index],
                    &after[added_items[k]],
                    out,
                );
            }
            None => unpaired_removed.push(old_index),
        }
    }
    for old_index in unpaired_removed {
        out.push(change(
            format!("{path}/{old_index}"),
            before[old_index].clone(),
            Value::Null,
        ));
    }
    for (k, &new_index) in added_items.iter().enumerate() {
        if !paired_added[k] {
            out.push(change(
                format!("{path}/{new_index}"),
                Value::Null,
                after[new_index].clone(),
            ));
        }
    }
}

/// Whether two items in the same gap are one item before and after an edit.
/// Items that carry an id are the same item only if the ids match.
fn can_pair(old: &Value, new: &Value) -> bool {
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => match (a.get("id"), b.get("id")) {
            (Some(x), Some(y)) => x == y,
            _ => true,
        },
        (Value::Object(_), _) | (_, Value::Object(_)) => false,
        _ => true,
    }
}

fn change(field_path: String, before: Value, after: Value) -> FieldChange {
    FieldChange {
        field_path,
        before,
        after,
    }
}

/// RFC 6901: `~` and `/` inside a key would otherwise be read as structure.
fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn diff(before: Value, after: Value) -> Vec<(String, Value, Value)> {
        let mut out = Vec::new();
        diff_value("", &before, &after, &mut out);
        out.into_iter()
            .map(|c| (c.field_path, c.before, c.after))
            .collect()
    }

    #[test]
    fn identical_values_have_no_changes() {
        let value = json!({"a": [1, 2], "b": {"c": "x"}});
        assert!(diff(value.clone(), value).is_empty());
    }

    #[test]
    fn an_edited_field_is_reported_at_its_own_path() {
        let changes = diff(
            json!({"verified_facts": [{"id": "f1", "claim": "old"}]}),
            json!({"verified_facts": [{"id": "f1", "claim": "new"}]}),
        );
        assert_eq!(
            changes,
            vec![("/verified_facts/0/claim".into(), json!("old"), json!("new"))]
        );
    }

    #[test]
    fn deleting_the_first_item_does_not_make_the_rest_look_edited() {
        let changes = diff(json!({"q": ["a", "b", "c"]}), json!({"q": ["b", "c"]}));
        assert_eq!(changes, vec![("/q/0".into(), json!("a"), Value::Null)]);
    }

    #[test]
    fn an_added_item_is_located_in_the_approved_list() {
        let changes = diff(json!({"q": ["a", "c"]}), json!({"q": ["a", "b", "c"]}));
        assert_eq!(changes, vec![("/q/1".into(), Value::Null, json!("b"))]);
    }

    #[test]
    fn a_reworded_string_in_a_list_is_one_edit() {
        let changes = diff(json!({"q": ["a", "b", "c"]}), json!({"q": ["a", "B", "c"]}));
        assert_eq!(changes, vec![("/q/1".into(), json!("b"), json!("B"))]);
    }

    #[test]
    fn items_with_different_ids_are_a_removal_and_an_addition() {
        let changes = diff(
            json!({"f": [{"id": "f1", "claim": "x"}]}),
            json!({"f": [{"id": "f2", "claim": "x"}]}),
        );
        assert_eq!(
            changes,
            vec![
                (
                    "/f/0".into(),
                    json!({"id": "f1", "claim": "x"}),
                    Value::Null
                ),
                (
                    "/f/0".into(),
                    Value::Null,
                    json!({"id": "f2", "claim": "x"})
                ),
            ]
        );
    }

    #[test]
    fn an_edit_and_a_deletion_in_the_same_gap_are_both_reported() {
        let changes = diff(
            json!({"f": [{"id": "f1", "c": "a"}, {"id": "f2", "c": "b"}, {"id": "f3", "c": "z"}]}),
            json!({"f": [{"id": "f2", "c": "B"}, {"id": "f3", "c": "z"}]}),
        );
        assert_eq!(
            changes,
            vec![
                ("/f/0/c".into(), json!("b"), json!("B")),
                ("/f/0".into(), json!({"id": "f1", "c": "a"}), Value::Null),
            ]
        );
    }

    #[test]
    fn a_reordered_list_is_reported_not_ignored() {
        let changes = diff(json!({"q": ["a", "b"]}), json!({"q": ["b", "a"]}));
        assert!(!changes.is_empty());
    }

    #[test]
    fn keys_are_escaped_so_a_slash_cannot_pose_as_structure() {
        let changes = diff(json!({"a/b": 1}), json!({"a/b": 2}));
        assert_eq!(changes[0].0, "/a~1b");
    }

    #[test]
    fn the_diff_is_the_same_every_time() {
        let before = json!({"q": ["a", "b", "c", "d"], "g": "x"});
        let after = json!({"q": ["a", "d", "e"], "g": "y"});
        assert_eq!(diff(before.clone(), after.clone()), diff(before, after));
    }
}
