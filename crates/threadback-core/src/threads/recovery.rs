use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
    Database, DomainError, ThreadId,
    threads::service::{PauseReason, ThreadService},
};

/// A thread that was still recording when the application last stopped.
///
/// Nothing is known about the time between `lastEventAt` and `recoveredAt`:
/// the application was not running, so no evidence was captured in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub thread_id: ThreadId,
    pub thread_title: String,
    /// Timestamp of the last acknowledged event, which starts the gap.
    pub last_event_at: Option<DateTime<Utc>>,
    pub recovered_at: DateTime<Utc>,
}

/// Run once at startup. Any thread still `ACTIVE` or `RESUMED` was interrupted,
/// so it is paused with an `interrupted` reason rather than silently resuming
/// capture. Every event acknowledged before the stop is already committed;
/// this only marks the gap. Calling it again finds nothing to do.
pub fn recover_interrupted_threads(
    database: &Database,
) -> Result<Vec<RecoveryReport>, DomainError> {
    let service = ThreadService::new(database);
    let mut reports = Vec::new();
    for thread in database.list_live_threads()? {
        let last_event_at = database.latest_event_timestamp(&thread.id)?;
        let paused = service.pause(&thread.id, PauseReason::Interrupted)?;
        reports.push(RecoveryReport {
            thread_id: thread.id,
            thread_title: thread.title,
            last_event_at,
            recovered_at: paused.updated_at,
        });
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EventPayload, NoteLabel, Project, ProjectId, Sensitivity, StartThreadInput, ThreadState,
    };
    use std::{path::Path, process::Command};

    const CRASH_DB: &str = "THREADBACK_CRASH_TEST_DB";
    const ACKNOWLEDGED_BEFORE_CRASH: usize = 25;

    fn start_thread(db: &Database) -> ThreadId {
        let now = Utc::now();
        let project = Project {
            id: ProjectId::new(),
            display_name: "Payments".into(),
            canonical_root: "/tmp/payments".into(),
            created_at: now,
            updated_at: now,
        };
        db.insert_project(&project).unwrap();
        ThreadService::new(db)
            .start(StartThreadInput {
                project_id: project.id,
                title: "Duplicate capture".into(),
                goal: "Find the second provider call".into(),
                hypothesis: None,
            })
            .unwrap()
            .id
    }

    fn note(text: &str) -> EventPayload {
        EventPayload::NoteAdded {
            text: text.into(),
            label: NoteLabel::Fact,
        }
    }

    #[test]
    fn interrupted_thread_is_paused_with_the_gap_reported() {
        let db = Database::open_in_memory().unwrap();
        let id = start_thread(&db);
        let service = ThreadService::new(&db);
        let last = service
            .record(&id, note("test fails"), Sensitivity::Normal)
            .unwrap();

        let reports = recover_interrupted_threads(&db).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].thread_id, id);
        assert_eq!(reports[0].last_event_at, Some(last.timestamp));
        assert!(reports[0].recovered_at >= last.timestamp);

        assert_eq!(
            db.find_thread(&id).unwrap().unwrap().state,
            ThreadState::Paused
        );
        let events = db.list_events(&id, None, 10).unwrap().events;
        assert!(matches!(
            &events.last().unwrap().payload,
            EventPayload::ThreadPaused { reason } if reason == "interrupted"
        ));
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn recovery_is_idempotent_and_leaves_other_threads_alone() {
        let db = Database::open_in_memory().unwrap();
        let drafting = start_thread(&db);
        ThreadService::new(&db).begin_checkpoint(&drafting).unwrap();

        assert!(recover_interrupted_threads(&db).unwrap().is_empty());
        assert_eq!(
            db.find_thread(&drafting).unwrap().unwrap().state,
            ThreadState::CheckpointDraft
        );

        let live = {
            let project = db.list_projects().unwrap().remove(0);
            ThreadService::new(&db)
                .start(StartThreadInput {
                    project_id: project.id,
                    title: "Second".into(),
                    goal: "Goal".into(),
                    hypothesis: None,
                })
                .unwrap()
                .id
        };
        assert_eq!(recover_interrupted_threads(&db).unwrap().len(), 1);
        assert!(recover_interrupted_threads(&db).unwrap().is_empty());
        assert_eq!(
            db.find_thread(&live).unwrap().unwrap().state,
            ThreadState::Paused
        );
    }

    #[test]
    fn recovered_thread_does_not_record_until_resumed() {
        let db = Database::open_in_memory().unwrap();
        let id = start_thread(&db);
        recover_interrupted_threads(&db).unwrap();
        let service = ThreadService::new(&db);
        assert!(
            service
                .record(&id, note("late"), Sensitivity::Normal)
                .is_err()
        );
        service.resume(&id).unwrap();
        assert!(
            service
                .record(&id, note("after resume"), Sensitivity::Normal)
                .is_ok()
        );
    }

    #[test]
    fn events_are_rejected_for_a_thread_that_is_not_recording() {
        let db = Database::open_in_memory().unwrap();
        let id = start_thread(&db);
        let service = ThreadService::new(&db);
        service.pause(&id, PauseReason::User).unwrap();
        let before = db.list_events(&id, None, 100).unwrap().events.len();
        assert!(
            service
                .record(&id, note("nope"), Sensitivity::Normal)
                .is_err()
        );
        assert_eq!(db.list_events(&id, None, 100).unwrap().events.len(), before);
    }

    /// Re-run by `acknowledged_events_survive_a_killed_process` as a child
    /// process. It records events, prints each acknowledgement, then aborts
    /// without running any destructors, as a crash would.
    #[test]
    fn crash_child() {
        let Ok(path) = std::env::var(CRASH_DB) else {
            return;
        };
        let db = Database::open(Path::new(&path)).unwrap();
        let id = start_thread(&db);
        println!("\nTHREAD {}", id.0);
        let service = ThreadService::new(&db);
        for index in 0..ACKNOWLEDGED_BEFORE_CRASH {
            service
                .record(&id, note(&format!("note {index}")), Sensitivity::Normal)
                .unwrap();
            println!("\nACK {index}");
        }
        std::process::abort();
    }

    #[test]
    fn acknowledged_events_survive_a_killed_process() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("crash.db");
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "threads::recovery::tests::crash_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CRASH_DB, &path)
            .output()
            .unwrap();
        assert!(!output.status.success(), "child should have crashed");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let acknowledged = stdout
            .lines()
            .filter(|line| line.starts_with("ACK "))
            .count();
        assert_eq!(acknowledged, ACKNOWLEDGED_BEFORE_CRASH, "{stdout}");
        let thread_id = ThreadId(
            stdout
                .lines()
                .find_map(|line| line.strip_prefix("THREAD "))
                .expect("child printed its thread id")
                .to_owned(),
        );

        let db = Database::open(&path).unwrap();
        let events = db.list_events(&thread_id, None, 200).unwrap().events;
        // thread.started plus every acknowledged note.
        assert_eq!(events.len(), 1 + acknowledged);
        for index in 0..acknowledged {
            assert!(events.iter().any(|event| matches!(
                &event.payload,
                EventPayload::NoteAdded { text, .. } if *text == format!("note {index}")
            )));
        }

        let reports = recover_interrupted_threads(&db).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].thread_id, thread_id);
        assert_eq!(
            reports[0].last_event_at,
            Some(events.last().unwrap().timestamp)
        );
    }
}
