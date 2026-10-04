use crate::{DomainError, ThreadState};

/// Every way a stored thread can change state. Creation is not an action
/// because a thread does not exist before `ThreadService::start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
    Pause,
    /// From `PAUSED` returns to `ACTIVE`; from `READY` opens a new evidence
    /// interval as `RESUMED`.
    Resume,
    /// Closes the thread and moves it to checkpoint drafting.
    End,
    /// Save My Place: the thread stays open but a checkpoint is being drafted.
    BeginCheckpoint,
    /// A user-approved, valid capsule exists.
    MarkReady,
    /// Capsule generation failed; the local checkpoint is kept for a retry.
    MarkFailed,
    Retry,
    /// The user threw away a checkpoint draft and keeps working.
    Discard,
    Archive,
    Delete,
}

impl ThreadState {
    /// Only these states record new evidence. `RESUMED` is the evidence
    /// interval that follows a resume, so it records like `ACTIVE`.
    pub fn accepts_evidence(&self) -> bool {
        matches!(self, Self::Active | Self::Resumed)
    }
}

/// The complete transition table. Anything not listed is rejected.
pub fn next_state(
    current: &ThreadState,
    action: LifecycleAction,
) -> Result<ThreadState, DomainError> {
    use LifecycleAction as A;
    use ThreadState as S;

    let next = match (action, current) {
        (A::Pause, S::Active | S::Resumed) => S::Paused,
        (A::Resume, S::Paused) => S::Active,
        (A::Resume, S::Ready) => S::Resumed,
        (A::End, S::Active | S::Resumed | S::Paused) => S::CheckpointDraft,
        (A::BeginCheckpoint, S::Active | S::Resumed) => S::CheckpointDraft,
        (A::MarkReady, S::CheckpointDraft) => S::Ready,
        (A::MarkFailed, S::CheckpointDraft) => S::Failed,
        (A::Retry, S::Failed) => S::CheckpointDraft,
        (A::Discard, S::CheckpointDraft | S::Failed) => S::Active,
        (A::Archive, S::Ready | S::Resumed | S::Paused | S::Failed) => S::Archived,
        (A::Delete, state) if *state != S::Deleted => S::Deleted,
        _ => {
            return Err(DomainError::invalid_state(format!(
                "This thread is {} and cannot {}.",
                state_label(current),
                action_label(action)
            )));
        }
    };
    Ok(next)
}

fn state_label(state: &ThreadState) -> &'static str {
    match state {
        ThreadState::Active => "active",
        ThreadState::Paused => "paused",
        ThreadState::CheckpointDraft => "checkpoint-draft",
        ThreadState::Ready => "ready",
        ThreadState::Resumed => "resumed",
        ThreadState::Archived => "archived",
        ThreadState::Failed => "failed",
        ThreadState::Deleted => "deleted",
    }
}

fn action_label(action: LifecycleAction) -> &'static str {
    match action {
        LifecycleAction::Pause => "be paused",
        LifecycleAction::Resume => "be resumed",
        LifecycleAction::End => "be ended",
        LifecycleAction::BeginCheckpoint => "start a checkpoint",
        LifecycleAction::MarkReady => "be marked ready",
        LifecycleAction::MarkFailed => "be marked failed",
        LifecycleAction::Retry => "retry its checkpoint",
        LifecycleAction::Discard => "discard a checkpoint",
        LifecycleAction::Archive => "be archived",
        LifecycleAction::Delete => "be deleted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;
    use LifecycleAction as A;
    use ThreadState as S;

    const ALL_STATES: [ThreadState; 8] = [
        S::Active,
        S::Paused,
        S::CheckpointDraft,
        S::Ready,
        S::Resumed,
        S::Archived,
        S::Failed,
        S::Deleted,
    ];
    const ALL_ACTIONS: [LifecycleAction; 10] = [
        A::Pause,
        A::Resume,
        A::End,
        A::BeginCheckpoint,
        A::MarkReady,
        A::MarkFailed,
        A::Retry,
        A::Discard,
        A::Archive,
        A::Delete,
    ];

    /// Independent statement of every legal move, so the test does not just
    /// replay the implementation's match arms.
    fn allowed() -> Vec<(ThreadState, LifecycleAction, ThreadState)> {
        vec![
            (S::Active, A::Pause, S::Paused),
            (S::Resumed, A::Pause, S::Paused),
            (S::Paused, A::Resume, S::Active),
            (S::Ready, A::Resume, S::Resumed),
            (S::Active, A::End, S::CheckpointDraft),
            (S::Resumed, A::End, S::CheckpointDraft),
            (S::Paused, A::End, S::CheckpointDraft),
            (S::Active, A::BeginCheckpoint, S::CheckpointDraft),
            (S::Resumed, A::BeginCheckpoint, S::CheckpointDraft),
            (S::CheckpointDraft, A::MarkReady, S::Ready),
            (S::CheckpointDraft, A::MarkFailed, S::Failed),
            (S::Failed, A::Retry, S::CheckpointDraft),
            (S::CheckpointDraft, A::Discard, S::Active),
            (S::Failed, A::Discard, S::Active),
            (S::Ready, A::Archive, S::Archived),
            (S::Resumed, A::Archive, S::Archived),
            (S::Paused, A::Archive, S::Archived),
            (S::Failed, A::Archive, S::Archived),
            (S::Active, A::Delete, S::Deleted),
            (S::Paused, A::Delete, S::Deleted),
            (S::CheckpointDraft, A::Delete, S::Deleted),
            (S::Ready, A::Delete, S::Deleted),
            (S::Resumed, A::Delete, S::Deleted),
            (S::Archived, A::Delete, S::Deleted),
            (S::Failed, A::Delete, S::Deleted),
        ]
    }

    #[test]
    fn every_state_action_pair_is_either_listed_or_rejected() {
        let allowed = allowed();
        for state in &ALL_STATES {
            for action in ALL_ACTIONS {
                let expected = allowed
                    .iter()
                    .find(|(from, act, _)| from == state && *act == action)
                    .map(|(_, _, to)| to);
                match (expected, next_state(state, action)) {
                    (Some(to), Ok(actual)) => {
                        assert_eq!(&actual, to, "{state:?} + {action:?}")
                    }
                    (None, Err(error)) => {
                        assert_eq!(
                            error.code,
                            ErrorCode::InvalidState,
                            "{state:?} + {action:?}"
                        )
                    }
                    (Some(_), Err(error)) => {
                        panic!("{state:?} + {action:?} should be allowed: {error}")
                    }
                    (None, Ok(actual)) => {
                        panic!("{state:?} + {action:?} should be rejected, got {actual:?}")
                    }
                }
            }
        }
    }

    #[test]
    fn deleted_is_terminal() {
        for action in ALL_ACTIONS {
            assert!(next_state(&S::Deleted, action).is_err(), "{action:?}");
        }
    }

    #[test]
    fn archived_only_allows_deletion() {
        for action in ALL_ACTIONS {
            let result = next_state(&S::Archived, action);
            assert_eq!(result.is_ok(), action == A::Delete, "{action:?}");
        }
    }

    #[test]
    fn only_active_and_resumed_accept_evidence() {
        for state in &ALL_STATES {
            assert_eq!(
                state.accepts_evidence(),
                matches!(state, S::Active | S::Resumed),
                "{state:?}"
            );
        }
    }

    #[test]
    fn rejection_message_is_readable() {
        let error = next_state(&S::Archived, A::Pause).unwrap_err();
        assert_eq!(
            error.message,
            "This thread is archived and cannot be paused."
        );
    }
}
