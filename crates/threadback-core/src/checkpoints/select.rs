use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::{
    CommandFinishedPayload, CommandStartedPayload, EventId, EventPayload, GitSnapshot,
    OutputExcerpt, ThreadEvent, WorkThread,
    checkpoints::{
        CheckpointEntry, CheckpointPayload, CheckpointTrigger, Omissions, SnapshotStatus,
    },
    storage::event_type,
};

pub const DEFAULT_MAX_ENTRIES: usize = 150;
pub const DEFAULT_MAX_CHARS: usize = 48_000;
const GIT_PATHS_CAP: usize = 60;
const GIT_FILES_CAP: usize = 30;
const OUTPUT_HEAD_CHARS: usize = 1_000;
const OUTPUT_TAIL_CHARS: usize = 3_000;
const NOTE_SHORT_CHARS: usize = 1_500;

#[derive(Debug, Clone, Copy)]
pub struct SelectionLimits {
    pub max_entries: usize,
    pub max_chars: usize,
}

impl Default for SelectionLimits {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            max_chars: DEFAULT_MAX_CHARS,
        }
    }
}

/// Lower numbers are kept first. Priority 0 is never dropped.
const PINNED: u8 = 0;
const FAILURE_OR_GAP: u8 = 1;
const ROUTINE_COMMAND: u8 = 2;
const OTHER: u8 = 3;

struct Candidate {
    entry: CheckpointEntry,
    priority: u8,
    /// Entries in a group are dropped together, so a command's start never
    /// outlives its finish.
    group: Option<EventId>,
    is_note: bool,
}

/// Turns the events of one interval into a bounded, deduplicated checkpoint
/// payload. `interval` must be non-empty and in chronological order.
///
/// - Repeated identical Git snapshots collapse to the latest one, and
///   repeated identical notes to the first.
/// - If the result is over budget, the least useful evidence goes first:
///   other snapshots, then routine commands, then failures and gaps.
///   The thread start, every note, and the final snapshot are never dropped.
pub fn build_payload(
    thread: &WorkThread,
    interval: &[ThreadEvent],
    trigger: CheckpointTrigger,
    final_snapshot: SnapshotStatus,
    limits: &SelectionLimits,
) -> CheckpointPayload {
    let mut omissions = Omissions::default();
    let kept = deduplicate(interval, &mut omissions);

    let command_priority = command_priorities(&kept);
    let last_snapshot = kept
        .iter()
        .rposition(|event| matches!(event.payload, EventPayload::GitSnapshot(_)));

    let mut candidates = Vec::new();
    for (index, event) in kept.iter().enumerate() {
        let Some(content) = condense(event) else {
            if !matches!(
                event.payload,
                EventPayload::CheckpointRequested(_)
                    | EventPayload::CheckpointGenerated(_)
                    | EventPayload::CheckpointApproved(_)
            ) {
                omissions.unreadable += 1;
            }
            continue;
        };
        let (priority, group) = match &event.payload {
            EventPayload::ThreadStarted { .. } | EventPayload::NoteAdded { .. } => (PINNED, None),
            EventPayload::ThreadPaused { .. } | EventPayload::ThreadResumed { .. } => {
                (FAILURE_OR_GAP, None)
            }
            EventPayload::GitSnapshot(_) if Some(index) == last_snapshot => (PINNED, None),
            EventPayload::CommandStarted(_) => (
                command_priority
                    .get(&event.id)
                    .copied()
                    .unwrap_or(FAILURE_OR_GAP),
                Some(event.id.clone()),
            ),
            EventPayload::CommandFinished(value) => {
                let started = value
                    .get("startedEventId")
                    .and_then(Value::as_str)
                    .map(|id| EventId(id.to_owned()));
                let priority = started
                    .as_ref()
                    .and_then(|id| command_priority.get(id).copied())
                    .unwrap_or(FAILURE_OR_GAP);
                (priority, started)
            }
            _ => (OTHER, None),
        };
        candidates.push(Candidate {
            entry: CheckpointEntry {
                event_id: event.id.clone(),
                timestamp: event.timestamp,
                kind: event_type(&event.payload).to_owned(),
                content,
            },
            priority,
            group,
            is_note: matches!(event.payload, EventPayload::NoteAdded { .. }),
        });
    }

    let mut alive = vec![true; candidates.len()];
    apply_budget(&mut candidates, &mut alive, limits, &mut omissions);

    let entries: Vec<CheckpointEntry> = candidates
        .into_iter()
        .zip(alive)
        .filter_map(|(candidate, alive)| alive.then_some(candidate.entry))
        .collect();
    let total_chars = entries.iter().map(entry_chars).sum::<usize>();

    CheckpointPayload {
        version: 1,
        thread_title: thread.title.clone(),
        goal: thread.goal.clone(),
        trigger,
        start_event_id: interval[0].id.clone(),
        end_event_id: interval[interval.len() - 1].id.clone(),
        interval_event_count: interval.len() as u32,
        final_snapshot,
        entries,
        omissions,
        total_chars: total_chars as u32,
    }
}

fn deduplicate<'a>(interval: &'a [ThreadEvent], omissions: &mut Omissions) -> Vec<&'a ThreadEvent> {
    let mut last_snapshot_with_hash: HashMap<&str, usize> = HashMap::new();
    for (index, event) in interval.iter().enumerate() {
        if matches!(event.payload, EventPayload::GitSnapshot(_)) {
            last_snapshot_with_hash.insert(&event.content_hash, index);
        }
    }
    let mut seen_notes: HashSet<&str> = HashSet::new();

    let mut kept = Vec::new();
    for (index, event) in interval.iter().enumerate() {
        let keep = match &event.payload {
            EventPayload::GitSnapshot(_) => {
                last_snapshot_with_hash.get(event.content_hash.as_str()) == Some(&index)
            }
            EventPayload::NoteAdded { .. } => seen_notes.insert(&event.content_hash),
            _ => true,
        };
        if keep {
            kept.push(event);
        } else {
            omissions.duplicates_removed += 1;
        }
    }
    kept
}

/// A command that failed, or never finished, is worth more than one that passed.
fn command_priorities(events: &[&ThreadEvent]) -> HashMap<EventId, u8> {
    let mut priorities = HashMap::new();
    for event in events {
        if let EventPayload::CommandStarted(_) = event.payload {
            priorities.insert(event.id.clone(), FAILURE_OR_GAP);
        }
    }
    for event in events {
        if let EventPayload::CommandFinished(value) = &event.payload {
            if let Ok(finished) = serde_json::from_value::<CommandFinishedPayload>(value.clone()) {
                if finished.exit_code == Some(0) {
                    priorities.insert(finished.started_event_id, ROUTINE_COMMAND);
                }
            }
        }
    }
    priorities
}

fn apply_budget(
    candidates: &mut [Candidate],
    alive: &mut [bool],
    limits: &SelectionLimits,
    omissions: &mut Omissions,
) {
    let mut sizes: Vec<usize> = candidates.iter().map(|c| entry_chars(&c.entry)).collect();
    let over = |alive: &[bool], sizes: &[usize]| {
        let count = alive.iter().filter(|alive| **alive).count();
        let chars: usize = sizes
            .iter()
            .zip(alive)
            .filter(|(_, a)| **a)
            .map(|(s, _)| *s)
            .sum();
        count > limits.max_entries || chars > limits.max_chars
    };

    while over(alive, &sizes) {
        // Least valuable first; among equals, the oldest.
        let victim = (0..candidates.len())
            .filter(|&i| alive[i] && candidates[i].priority > PINNED)
            .max_by_key(|&i| (candidates[i].priority, std::cmp::Reverse(i)));
        let Some(victim) = victim else { break };
        let group = candidates[victim].group.clone();
        for i in 0..candidates.len() {
            if alive[i] && (i == victim || (group.is_some() && candidates[i].group == group)) {
                alive[i] = false;
                omissions.over_budget_removed += 1;
            }
        }
    }

    if over(alive, &sizes) {
        // Only never-dropped entries remain. Shorten long notes rather than
        // exceed the budget, and say so.
        for i in 0..candidates.len() {
            if alive[i] && candidates[i].is_note && shorten_note(&mut candidates[i].entry) {
                omissions.notes_shortened += 1;
                sizes[i] = entry_chars(&candidates[i].entry);
            }
        }
    }
}

fn shorten_note(entry: &mut CheckpointEntry) -> bool {
    let Some(text) = entry.content.get("text").and_then(Value::as_str) else {
        return false;
    };
    if text.chars().count() <= NOTE_SHORT_CHARS {
        return false;
    }
    let shortened = format!("{}… [note shortened]", take_chars(text, NOTE_SHORT_CHARS));
    entry.content["text"] = Value::String(shortened);
    entry.content["shortened"] = Value::Bool(true);
    true
}

fn entry_chars(entry: &CheckpointEntry) -> usize {
    serde_json::to_string(entry).map_or(0, |text| text.chars().count())
}

/// The compact form of one event, or `None` when it is not evidence or cannot
/// be read.
fn condense(event: &ThreadEvent) -> Option<Value> {
    match &event.payload {
        EventPayload::ThreadStarted { goal, hypothesis } => {
            Some(json!({ "goal": goal, "hypothesis": hypothesis }))
        }
        EventPayload::ThreadPaused { reason } => Some(json!({ "reason": reason })),
        EventPayload::ThreadResumed {
            prior_checkpoint_id,
        } => Some(json!({ "priorCheckpointId": prior_checkpoint_id })),
        EventPayload::NoteAdded { text, label } => Some(json!({ "label": label, "text": text })),
        EventPayload::GitSnapshot(value) => {
            let snapshot: GitSnapshot = serde_json::from_value(value.clone()).ok()?;
            Some(condense_snapshot(snapshot))
        }
        EventPayload::CommandStarted(value) => {
            let started: CommandStartedPayload = serde_json::from_value(value.clone()).ok()?;
            Some(json!({ "command": started.command, "cwd": started.cwd }))
        }
        EventPayload::CommandFinished(value) => {
            let finished: CommandFinishedPayload = serde_json::from_value(value.clone()).ok()?;
            Some(json!({
                "startedEventId": finished.started_event_id,
                "exitCode": finished.exit_code,
                "signal": finished.signal,
                "durationMs": finished.duration_ms,
                "stdout": condense_output(&finished.stdout),
                "stderr": condense_output(&finished.stderr),
            }))
        }
        EventPayload::FileChanged(value) => Some(value.clone()),
        EventPayload::CheckpointRequested(_)
        | EventPayload::CheckpointGenerated(_)
        | EventPayload::CheckpointApproved(_) => None,
    }
}

fn condense_snapshot(mut snapshot: GitSnapshot) -> Value {
    let omitted_paths = snapshot.changed_paths.len().saturating_sub(GIT_PATHS_CAP)
        + snapshot.staged.files.len().saturating_sub(GIT_FILES_CAP)
        + snapshot.unstaged.files.len().saturating_sub(GIT_FILES_CAP);
    snapshot.changed_paths.truncate(GIT_PATHS_CAP);
    snapshot.staged.files.truncate(GIT_FILES_CAP);
    snapshot.unstaged.files.truncate(GIT_FILES_CAP);
    let mut value = serde_json::to_value(&snapshot).unwrap_or(Value::Null);
    value["omittedPaths"] = json!(omitted_paths);
    value
}

fn condense_output(excerpt: &OutputExcerpt) -> Value {
    if excerpt.binary {
        return json!({ "binary": true, "originalBytes": excerpt.original_bytes });
    }
    let combined = if excerpt.omitted_bytes > 0 {
        format!(
            "{}\n… {} bytes omitted …\n{}",
            excerpt.head, excerpt.omitted_bytes, excerpt.tail
        )
    } else {
        format!("{}{}", excerpt.head, excerpt.tail)
    };
    let length = combined.chars().count();
    if length <= OUTPUT_HEAD_CHARS + OUTPUT_TAIL_CHARS {
        return json!({
            "text": combined,
            "originalBytes": excerpt.original_bytes,
            "truncated": excerpt.truncated,
        });
    }
    let hidden = length - OUTPUT_HEAD_CHARS - OUTPUT_TAIL_CHARS;
    json!({
        "text": format!(
            "{}\n… {hidden} characters omitted …\n{}",
            take_chars(&combined, OUTPUT_HEAD_CHARS),
            last_chars(&combined, OUTPUT_TAIL_CHARS)
        ),
        "originalBytes": excerpt.original_bytes,
        "truncated": true,
    })
}

fn take_chars(text: &str, count: usize) -> &str {
    match text.char_indices().nth(count) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

fn last_chars(text: &str, count: usize) -> &str {
    let total = text.chars().count();
    if total <= count {
        return text;
    }
    match text.char_indices().nth(total - count) {
        Some((index, _)) => &text[index..],
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CommandFinishedPayload, DiffStats, NoteLabel, ProjectId, Sensitivity, ThreadId, ThreadState,
    };
    use chrono::{Duration, Utc};

    fn thread() -> WorkThread {
        let now = Utc::now();
        WorkThread {
            id: ThreadId::new(),
            project_id: ProjectId::new(),
            title: "Duplicate capture".into(),
            goal: "Find the second provider call".into(),
            state: ThreadState::Active,
            started_at: now,
            ended_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Builds events one second apart, in order.
    struct Events {
        thread_id: ThreadId,
        items: Vec<ThreadEvent>,
    }

    impl Events {
        fn new() -> Self {
            Self {
                thread_id: ThreadId::new(),
                items: Vec::new(),
            }
        }

        fn push(&mut self, payload: EventPayload) -> EventId {
            let mut event =
                ThreadEvent::new(self.thread_id.clone(), payload, Sensitivity::Normal).unwrap();
            event.timestamp = Utc::now() + Duration::seconds(self.items.len() as i64);
            let id = event.id.clone();
            self.items.push(event);
            id
        }

        fn started(&mut self) -> EventId {
            self.push(EventPayload::ThreadStarted {
                goal: "Find the second provider call".into(),
                hypothesis: None,
            })
        }

        fn note(&mut self, text: &str, label: NoteLabel) -> EventId {
            self.push(EventPayload::NoteAdded {
                text: text.into(),
                label,
            })
        }

        fn snapshot(&mut self, head: &str) -> EventId {
            let snapshot = GitSnapshot {
                branch: Some("main".into()),
                head: Some(head.into()),
                dirty: true,
                changed_paths: vec![],
                staged: DiffStats::default(),
                unstaged: DiffStats::default(),
                excluded_changes: 0,
                truncated: false,
            };
            self.push(EventPayload::GitSnapshot(
                serde_json::to_value(snapshot).unwrap(),
            ))
        }

        fn command(&mut self, command: &str, exit_code: i32, stdout: &str) -> (EventId, EventId) {
            let started = self.push(EventPayload::CommandStarted(
                serde_json::to_value(CommandStartedPayload {
                    command: command.into(),
                    cwd: ".".into(),
                })
                .unwrap(),
            ));
            let finished = CommandFinishedPayload {
                started_event_id: started.clone(),
                exit_code: Some(exit_code),
                signal: None,
                duration_ms: 1200,
                stdout: excerpt(stdout),
                stderr: excerpt(""),
            };
            let finished = self.push(EventPayload::CommandFinished(
                serde_json::to_value(finished).unwrap(),
            ));
            (started, finished)
        }
    }

    fn excerpt(text: &str) -> OutputExcerpt {
        OutputExcerpt {
            head: text.into(),
            tail: String::new(),
            original_bytes: text.len() as u64,
            omitted_bytes: 0,
            truncated: false,
            binary: false,
        }
    }

    fn build(events: &Events, limits: SelectionLimits) -> CheckpointPayload {
        build_payload(
            &thread(),
            &events.items,
            CheckpointTrigger::Manual,
            SnapshotStatus {
                captured: true,
                event_id: None,
                reason: None,
            },
            &limits,
        )
    }

    fn ids(payload: &CheckpointPayload) -> Vec<&EventId> {
        payload
            .entries
            .iter()
            .map(|entry| &entry.event_id)
            .collect()
    }

    fn roomy() -> SelectionLimits {
        SelectionLimits {
            max_entries: 1_000,
            max_chars: 1_000_000,
        }
    }

    #[test]
    fn selects_every_kind_of_evidence_in_order_with_the_interval_range() {
        let mut events = Events::new();
        let started = events.started();
        let note = events.note("the focused test fails", NoteLabel::Fact);
        let (command_started, command_finished) = events.command("npm test", 1, "2 provider calls");
        let snapshot = events.snapshot("abc1234");

        let payload = build(&events, roomy());
        assert_eq!(
            ids(&payload),
            [
                &started,
                &note,
                &command_started,
                &command_finished,
                &snapshot
            ]
        );
        let kinds: Vec<_> = payload
            .entries
            .iter()
            .map(|entry| entry.kind.as_str())
            .collect();
        assert_eq!(
            kinds,
            [
                "thread.started",
                "note.added",
                "command.started",
                "command.finished",
                "git.snapshot"
            ]
        );
        assert_eq!(payload.start_event_id, started);
        assert_eq!(payload.end_event_id, snapshot);
        assert_eq!(payload.interval_event_count, 5);
        assert_eq!(payload.omissions, Omissions::default());
        assert_eq!(payload.version, 1);
    }

    #[test]
    fn user_labels_and_text_pass_through_exactly() {
        let mut events = Events::new();
        events.started();
        events.note("  It is definitely the middleware\n", NoteLabel::Hypothesis);
        let payload = build(&events, roomy());
        let note = &payload.entries[1].content;
        assert_eq!(note["label"], "hypothesis");
        assert_eq!(note["text"], "  It is definitely the middleware\n");
    }

    #[test]
    fn identical_snapshots_collapse_to_the_latest_and_identical_notes_to_the_first() {
        let mut events = Events::new();
        events.started();
        let first_note = events.note("same", NoteLabel::Fact);
        events.snapshot("aaa");
        events.note("same", NoteLabel::Fact);
        events.snapshot("aaa");
        let different_label = events.note("same", NoteLabel::Question);
        let last_snapshot = events.snapshot("aaa");

        let payload = build(&events, roomy());
        let snapshots: Vec<_> = payload
            .entries
            .iter()
            .filter(|e| e.kind == "git.snapshot")
            .collect();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].event_id, last_snapshot);
        let notes: Vec<_> = payload
            .entries
            .iter()
            .filter(|e| e.kind == "note.added")
            .map(|e| &e.event_id)
            .collect();
        assert_eq!(notes, [&first_note, &different_label]);
        assert_eq!(payload.omissions.duplicates_removed, 2 + 1);
    }

    #[test]
    fn changed_snapshots_are_all_kept_when_the_budget_allows() {
        let mut events = Events::new();
        events.started();
        events.snapshot("aaa");
        events.snapshot("bbb");
        let payload = build(&events, roomy());
        assert_eq!(
            payload
                .entries
                .iter()
                .filter(|e| e.kind == "git.snapshot")
                .count(),
            2
        );
    }

    #[test]
    fn repeated_commands_are_not_deduplicated() {
        let mut events = Events::new();
        events.started();
        events.command("npm test", 1, "fail");
        events.command("npm test", 1, "fail");
        let payload = build(&events, roomy());
        assert_eq!(
            payload
                .entries
                .iter()
                .filter(|e| e.kind == "command.finished")
                .count(),
            2
        );
    }

    #[test]
    fn the_entry_budget_drops_old_snapshots_then_passing_commands_before_failures() {
        let mut events = Events::new();
        events.started(); // pinned
        events.snapshot("s1"); // other, oldest
        let (ok_started, ok_finished) = events.command("ls", 0, "files"); // routine
        let (fail_started, fail_finished) = events.command("npm test", 1, "boom"); // failure
        events.snapshot("s2"); // other
        let final_snapshot = events.snapshot("s3"); // pinned: the last snapshot

        // Eight entries, room for five. Dropped in value order: s1, s2, then
        // the passing command as a pair, leaving four.
        let payload = build(
            &events,
            SelectionLimits {
                max_entries: 5,
                max_chars: 1_000_000,
            },
        );
        let kept = ids(&payload);
        assert_eq!(kept.len(), 4);
        assert!(kept.contains(&&fail_started) && kept.contains(&&fail_finished));
        assert!(kept.contains(&&final_snapshot));
        assert!(!kept.contains(&&ok_started) && !kept.contains(&&ok_finished));
        assert_eq!(payload.omissions.over_budget_removed, 4);
    }

    #[test]
    fn a_commands_start_and_finish_are_always_dropped_together() {
        let mut events = Events::new();
        events.started();
        for index in 0..10 {
            events.command(&format!("cmd {index}"), 0, "ok");
        }
        for max in 1..=21 {
            let payload = build(
                &events,
                SelectionLimits {
                    max_entries: max,
                    max_chars: 1_000_000,
                },
            );
            let started = payload
                .entries
                .iter()
                .filter(|e| e.kind == "command.started")
                .count();
            let finished = payload
                .entries
                .iter()
                .filter(|e| e.kind == "command.finished")
                .count();
            assert_eq!(started, finished, "budget {max}");
        }
    }

    #[test]
    fn a_command_that_never_finished_is_treated_as_worth_keeping() {
        let mut events = Events::new();
        events.started();
        let interrupted = events.push(EventPayload::CommandStarted(
            serde_json::to_value(CommandStartedPayload {
                command: "make".into(),
                cwd: ".".into(),
            })
            .unwrap(),
        ));
        events.command("ls", 0, "ok");
        let payload = build(
            &events,
            SelectionLimits {
                max_entries: 2,
                max_chars: 1_000_000,
            },
        );
        assert!(ids(&payload).contains(&&interrupted));
    }

    #[test]
    fn the_start_notes_and_final_snapshot_are_never_dropped() {
        let mut events = Events::new();
        let started = events.started();
        let notes: Vec<_> = (0..5)
            .map(|i| events.note(&format!("note {i}"), NoteLabel::Fact))
            .collect();
        events.command("ls", 0, "ok");
        let final_snapshot = events.snapshot("zzz");

        let payload = build(
            &events,
            SelectionLimits {
                max_entries: 1,
                max_chars: 1,
            },
        );
        let kept = ids(&payload);
        assert!(kept.contains(&&started) && kept.contains(&&final_snapshot));
        for note in &notes {
            assert!(kept.contains(&note));
        }
        assert_eq!(kept.len(), 1 + 5 + 1);
    }

    #[test]
    fn the_character_budget_is_respected_when_droppable_evidence_exists() {
        let mut events = Events::new();
        events.started();
        events.note("keep me", NoteLabel::Fact);
        for index in 0..20 {
            events.command(&format!("cmd {index}"), 0, &"x".repeat(2_000));
        }
        let payload = build(
            &events,
            SelectionLimits {
                max_entries: 1_000,
                max_chars: 5_000,
            },
        );
        assert!(
            payload.total_chars as usize <= 5_000,
            "{}",
            payload.total_chars
        );
        assert!(payload.entries.iter().any(|e| e.kind == "note.added"));
        assert!(payload.omissions.over_budget_removed > 0);
    }

    #[test]
    fn oversized_notes_are_shortened_only_as_a_last_resort() {
        let mut events = Events::new();
        events.started();
        events.note(&"long ".repeat(2_000), NoteLabel::Hypothesis);
        let roomy_payload = build(&events, roomy());
        assert_eq!(roomy_payload.omissions.notes_shortened, 0);
        assert_eq!(
            roomy_payload.entries[1].content["text"]
                .as_str()
                .unwrap()
                .len(),
            10_000
        );

        let tight = build(
            &events,
            SelectionLimits {
                max_entries: 10,
                max_chars: 3_000,
            },
        );
        assert_eq!(tight.omissions.notes_shortened, 1);
        assert_eq!(tight.entries[1].content["shortened"], true);
        assert_eq!(tight.entries[1].content["label"], "hypothesis");
        assert!(
            tight.entries[1].content["text"]
                .as_str()
                .unwrap()
                .ends_with("[note shortened]")
        );
    }

    #[test]
    fn command_output_keeps_the_failure_at_the_end() {
        let mut events = Events::new();
        events.started();
        let output = format!(
            "{}FAILED: expected 1 provider call, got 2",
            "log line\n".repeat(2_000)
        );
        events.command("npm test", 1, &output);
        let payload = build(&events, roomy());
        let text = payload.entries[2].content["stdout"]["text"]
            .as_str()
            .unwrap();
        assert!(text.chars().count() < 4_100);
        assert!(text.ends_with("got 2"));
        assert!(text.contains("characters omitted"));
        assert_eq!(payload.entries[2].content["stdout"]["truncated"], true);
        assert_eq!(payload.entries[2].content["exitCode"], 1);
    }

    #[test]
    fn capture_level_truncation_is_carried_into_the_entry() {
        let mut events = Events::new();
        events.started();
        let started = events.push(EventPayload::CommandStarted(
            serde_json::to_value(CommandStartedPayload {
                command: "x".into(),
                cwd: ".".into(),
            })
            .unwrap(),
        ));
        let finished = CommandFinishedPayload {
            started_event_id: started,
            exit_code: Some(1),
            signal: None,
            duration_ms: 5,
            stdout: OutputExcerpt {
                head: "start".into(),
                tail: "end".into(),
                original_bytes: 5_000_000,
                omitted_bytes: 4_999_992,
                truncated: true,
                binary: false,
            },
            stderr: OutputExcerpt {
                head: String::new(),
                tail: String::new(),
                original_bytes: 9,
                omitted_bytes: 9,
                truncated: true,
                binary: true,
            },
        };
        events.push(EventPayload::CommandFinished(
            serde_json::to_value(finished).unwrap(),
        ));
        let payload = build(&events, roomy());
        let content = &payload.entries[2].content;
        assert_eq!(
            content["stdout"]["text"],
            "start\n… 4999992 bytes omitted …\nend"
        );
        assert_eq!(content["stdout"]["truncated"], true);
        assert_eq!(content["stdout"]["originalBytes"], 5_000_000);
        assert_eq!(content["stderr"]["binary"], true);
        assert!(content["stderr"].get("text").is_none());
    }

    #[test]
    fn large_snapshots_are_capped_and_the_omission_is_counted() {
        use crate::{ChangedPath, FileDiffStat};
        let mut events = Events::new();
        events.started();
        let snapshot = GitSnapshot {
            branch: Some("main".into()),
            head: Some("abc".into()),
            dirty: true,
            changed_paths: (0..100)
                .map(|i| ChangedPath {
                    path: format!("f{i}.rs"),
                    index_status: " ".into(),
                    worktree_status: "M".into(),
                })
                .collect(),
            staged: DiffStats::default(),
            unstaged: DiffStats {
                files: (0..50)
                    .map(|i| FileDiffStat {
                        path: format!("f{i}.rs"),
                        additions: 1,
                        deletions: 0,
                        binary: false,
                    })
                    .collect(),
                file_count: 50,
                additions: 50,
                deletions: 0,
            },
            excluded_changes: 0,
            truncated: false,
        };
        events.push(EventPayload::GitSnapshot(
            serde_json::to_value(snapshot).unwrap(),
        ));
        let payload = build(&events, roomy());
        let content = &payload.entries[1].content;
        assert_eq!(
            content["changedPaths"].as_array().unwrap().len(),
            GIT_PATHS_CAP
        );
        assert_eq!(
            content["unstaged"]["files"].as_array().unwrap().len(),
            GIT_FILES_CAP
        );
        assert_eq!(content["unstaged"]["fileCount"], 50, "totals stay exact");
        assert_eq!(content["omittedPaths"], 40 + 20);
    }

    #[test]
    fn earlier_checkpoint_events_and_unreadable_events_are_handled() {
        let mut events = Events::new();
        events.started();
        events.push(EventPayload::CheckpointRequested(json!({})));
        events.push(EventPayload::GitSnapshot(json!("not a snapshot")));
        let payload = build(&events, roomy());
        assert_eq!(payload.entries.len(), 1);
        assert_eq!(
            payload.omissions.unreadable, 1,
            "meta events are skipped silently"
        );
    }

    #[test]
    fn character_helpers_respect_multibyte_boundaries() {
        assert_eq!(take_chars("héllo", 2), "hé");
        assert_eq!(last_chars("héllo", 3), "llo");
        assert_eq!(take_chars("ab", 5), "ab");
        assert_eq!(last_chars("ab", 5), "ab");
    }
}
