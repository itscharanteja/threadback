# ThreadBack MVP Specification

**Status:** Implementation-ready MVP specification  
**Version:** 0.1  
**Primary track:** Personal AI  
**Target platform:** macOS desktop first  
**Target build window:** Seven days

## 1. Product summary

ThreadBack is a private interruption-recovery assistant. It captures selected evidence from a user-approved work session and turns that evidence into a structured **resume capsule**.

The capsule answers:

- What was the user trying to accomplish?
- What was verified?
- What remains a hypothesis?
- What was already attempted and failed?
- What questions remain unanswered?
- Which artifacts matter?
- What is the smallest justified next action?

ThreadBack does not score focus, block applications, record the screen, log keystrokes, or continue the work autonomously.

## 2. Product promise

> When work is interrupted, ThreadBack preserves the reasoning state—not just the files—so the user can return and continue with confidence.

## 3. Problem statement

After an interruption, the files still exist, but the user's mental model may be gone. Git can show what changed and a terminal can show what ran, but neither reliably explains why an approach was attempted, what a failure proved, which explanation is plausible, or what the user intended to try next.

Manual notes help only when the user writes a complete note at the correct moment. Activity trackers preserve history but generally do not reconstruct task state.

## 4. Target users

### 4.1 Maya — primary persona

Maya is a backend engineer maintaining payment services. Reviews and production incidents frequently interrupt her debugging work. She wants to return with the active hypothesis, failed attempts, relevant test, and next command intact.

**Success:** Maya resumes useful debugging within two minutes without rereading the complete Git and terminal history.

### 4.2 Arjun — secondary persona

Arjun is a solo founder switching between product development, support, deployment, and customer calls. He needs a clean continuation point whenever he leaves unfinished work.

### 4.3 Priya — expansion persona

Priya is a research engineer who runs experiments and changes assumptions frequently. She needs to preserve configurations, negative results, supporting evidence, and unresolved questions.

## 5. Jobs to be done

1. **Before starting:** Help me state the outcome and define a safe collection boundary.
2. **While working:** Preserve evidence that explains decisions without forcing constant note-taking.
3. **When interrupted:** Create a checkpoint I can inspect and approve quickly.
4. **When returning:** Restore verified facts, uncertainty, failed paths, and the next justified action.
5. **When the summary is wrong:** Let me correct it and make my correction authoritative.
6. **When I want out:** Let me pause, export, or delete the complete thread.

## 6. Product principles

1. **Recover, do not police.** Never classify the user as focused or distracted.
2. **Facts before fluency.** A verified claim must link to captured evidence.
3. **Least data.** Collect only inside an explicit project and send only approved context.
4. **Human checkpoints.** The user reviews what is sent and approves the capsule.
5. **Visible uncertainty.** Facts, hypotheses, questions, and missing evidence stay distinct.
6. **Graceful failure.** If inference fails, keep the local checkpoint and evidence timeline.
7. **One workflow first.** The MVP supports software debugging in one Git project.

## 7. Scope

### 7.1 Must ship

- macOS desktop application.
- One allowlisted Git project per work thread.
- Named work thread with a required goal.
- Start, pause, resume, end, archive, and delete lifecycle.
- Git evidence capture.
- Explicit command capture through `tb run -- <command>` or a controlled built-in wrapper.
- Timestamped manual notes labelled as fact, hypothesis, question, or next step.
- Manual **Save My Place** checkpoint.
- Optional inactivity suggestion that asks whether to save a checkpoint.
- Outbound payload preview and secret redaction.
- Runtime call to an NVIDIA Nemotron model through Nebius Token Factory.
- Strict resume-capsule JSON and provenance validation.
- Review, edit, relabel, regenerate, and approve workflow.
- Repository drift check on return.
- Resume screen with source links and one next action.
- Markdown and JSON export.
- Complete thread deletion.

### 7.2 Should ship if the core is stable

- Open a selected artifact after an explicit click.
- Copy the proposed command after an explicit click.
- Tray menu with current thread status and pause.
- Seeded demo repository and deterministic demonstration data.
- Local diagnostics without private payload contents.

### 7.3 Later

- Calendar awareness.
- Browser and research-document evidence.
- IDE extension.
- Multi-project threads.
- Cross-device encrypted sync.
- Team handoff capsules.
- Reusable personal skills.
- NemoClaw or OpenShell integration.
- Nebius Serverless background processing.

### 7.4 Non-goals

- Screenshots, keystroke logging, or microphone capture.
- Global terminal-history or browser-history scraping.
- Distraction scoring or application blocking.
- Employee-monitoring dashboards.
- Automatic code changes or command execution.
- Automatic purchases, messages, reminders, or calendar edits.
- Treating model output as correct without evidence.

## 8. Primary workflow

### 8.1 Onboard a project

1. User selects a project folder.
2. Application resolves its canonical path.
3. Application displays default exclusions.
4. User reviews retention and outbound-data settings. 
5. Application stores the Token Factory API key in the OS keychain.

### 8.2 Start a thread

1. User chooses an approved project.
2. User enters a title and intended outcome.
3. User may enter a starting hypothesis.
4. User presses **Start thread**.
5. Application creates the thread and captures the first Git snapshot.

### 8.3 Collect evidence

ThreadBack records only supported events:

- Git branch, HEAD, status, changed paths, and compact diff statistics.
- Explicitly wrapped commands with timestamps, exit codes, and bounded output.
- File-change metadata inside the project.
- User-authored notes.

### 8.4 Save a checkpoint

1. User presses **Save My Place**, or accepts an inactivity suggestion.
2. Application captures a final Git snapshot.
3. Application selects events from the active thread.
4. Policy engine applies path rules, size limits, and redaction.
5. User sees the complete outbound payload.
6. User excludes fields, cancels, or confirms generation.
7. Nemotron returns a structured capsule draft.
8. Local validator checks schema and provenance.
9. User edits and approves the capsule.

### 8.5 Return and resume

1. User opens the ready capsule.
2. Application compares current Git state with the checkpoint.
3. If drift exists, application explains what changed.
4. User reviews the next action and supporting evidence.
5. User may open an artifact or copy a command.
6. User presses **Resume thread**.

## 9. Thread state model

```text
IDLE -> ACTIVE -> CHECKPOINT_DRAFT -> READY -> RESUMED -> ARCHIVED
           |              |             |
           +-> PAUSED     +-> FAILED    +-> DELETED
```

- `ACTIVE` requires a project, title, and goal.
- Only `ACTIVE` records new evidence.
- `CHECKPOINT_DRAFT` may exist without network access.
- `READY` requires a valid, user-approved capsule.
- `RESUMED` begins a new evidence interval in the same thread.
- `DELETED` is terminal and removes all related local data.

## 10. Functional requirements

### FR-01 — Project boundary

Accept one canonical project root and reject captured paths outside it.

**Acceptance:** Direct paths, `..` traversal, and symlinks resolving outside the root cannot create events.

### FR-02 — Exclusions

Exclude `.git` objects, `node_modules`, dependency caches, build output, binaries, environment files, private keys, and configured patterns.

### FR-03 — Thread lifecycle

The user can start, pause, resume, end, archive, and delete a thread. State survives restart.

### FR-04 — Goal capture

An active thread requires an intended outcome. The original goal remains visible unless the user edits it.

### FR-05 — Git evidence

Capture branch, HEAD, clean/dirty status, changed relative paths, staged and unstaged diff statistics, and timestamp. Git collection is read-only.

### FR-06 — Command evidence

Capture only commands launched through the approved wrapper. Store redacted command text, canonical working directory, timestamps, exit code, bounded stdout/stderr, and truncation metadata. Never read global shell history.

### FR-07 — Manual notes

The user can create notes labelled fact, hypothesis, question, or next step. Preserve the user's label.

### FR-08 — Checkpoint

**Save My Place** must work offline and create a durable local draft.

### FR-09 — Payload preview

Before any model request, show the exact filtered payload. The user can exclude entries, cancel, or approve.

### FR-10 — Capsule generation

Use an NVIDIA Nemotron model through Token Factory's OpenAI-compatible API. Endpoint and model ID are configuration.

### FR-11 — Certainty separation

Separate verified facts, hypotheses, failed attempts, unresolved questions, relevant artifacts, and the next action.

### FR-12 — Provenance

Every verified fact has at least one valid local `source_event_id`.

**Acceptance:** A fact without a valid source cannot be approved as verified.

### FR-13 — Failed attempts

Each failed attempt contains action, observed outcome, learning, and source IDs.

### FR-14 — Next action

Return one small action with instruction, justification, success check, and source IDs where applicable.

### FR-15 — Review and approval

The user can edit text, relabel certainty, remove a claim, inspect sources, regenerate, and approve. Preserve model draft and approved version separately.

### FR-16 — Repository drift

On return, compare current branch, HEAD, and status with the checkpoint. Show changed signals before resume actions.

### FR-17 — Resume tools

Opening a file or copying a command requires an explicit click. Never execute a command automatically.

### FR-18 — Export

Export the approved capsule as JSON and readable Markdown.

### FR-19 — Delete

Delete events, drafts, approved capsules, corrections, cached payloads, and local indexes.

### FR-20 — Offline degradation

If Token Factory is unavailable, retain the checkpoint, show the local timeline, and allow manual completion or explicit retry.

## 11. Resume capsule contract

```json
{
  "schema_version": "1.0",
  "goal": "Find why retries create a duplicate provider capture",
  "checkpoint_summary": "The focused retry test still produces two provider calls.",
  "verified_facts": [
    {
      "id": "fact_1",
      "claim": "The focused retry test made two provider calls.",
      "source_event_ids": ["evt_184"]
    }
  ],
  "hypotheses": [
    {
      "id": "hyp_1",
      "claim": "A second worker bypasses the distributed lock.",
      "confidence": "medium",
      "source_event_ids": ["note_21"]
    }
  ],
  "failed_attempts": [
    {
      "action": "Changed retry middleware.",
      "outcome": "The focused test still failed.",
      "learning": "Middleware alone does not prevent the second provider call.",
      "source_event_ids": ["evt_182", "evt_184"]
    }
  ],
  "unresolved_questions": [
    "Do both workers share the same lock backend?"
  ],
  "relevant_artifacts": [
    {
      "type": "test",
      "relative_path": "tests/payment_retry.spec.ts",
      "reason": "Reproduces the duplicate provider call.",
      "source_event_ids": ["evt_184"]
    }
  ],
  "next_action": {
    "instruction": "Log the lock owner in both workers and rerun the focused test.",
    "justification": "Current evidence suggests the workers do not observe the same lock state.",
    "success_check": "Both retries observe the same lock owner.",
    "source_event_ids": ["evt_184", "note_21"]
  },
  "limitations": [
    "ThreadBack did not inspect the production lock backend."
  ]
}
```

### Validation rules

- Reject unsupported top-level fields unless the schema version permits them.
- A verified fact needs a nonempty claim and valid source ID.
- Hypothesis confidence is `low`, `medium`, or `high`.
- Source IDs belong to the current thread and checkpoint interval.
- Artifact paths are relative and resolve inside the project.
- The next action contains an instruction and success check.
- Model output cannot request tool execution.
- Invalid output receives one deterministic repair attempt; a second failure becomes a visible error.

## 12. Data model

### Project

`id`, `display_name`, `canonical_root`, `exclusions_json`, `retention_days`, `created_at`, `updated_at`

### WorkThread

`id`, `project_id`, `title`, `goal`, `state`, `started_at`, `ended_at`, `created_at`, `updated_at`

### Event

`id`, `thread_id`, `type`, `timestamp`, `payload_json`, `sensitivity`, `content_hash`

### ArtifactRef

`id`, `event_id`, `type`, `relative_path`, `content_hash`, `line_hint`

### Checkpoint

`id`, `thread_id`, `start_event_id`, `end_event_id`, `status`, `model_id`, `prompt_version`, `payload_json`, `draft_json`, `approved_json`, `created_at`, `approved_at`

### Correction

`id`, `checkpoint_id`, `field_path`, `before_json`, `after_json`, `reason`, `created_at`

### ProjectPolicy

`project_id`, capture flags, `retention_days`, and `redaction_patterns_json`

## 13. Event types

| Event | Required payload |
|---|---|
| `thread.started` | goal and optional hypothesis |
| `thread.paused` | user or system reason |
| `thread.resumed` | prior checkpoint ID when available |
| `git.snapshot` | branch, HEAD, status, and diff statistics |
| `file.changed` | relative path, timestamp, hash, and line counts |
| `command.started` | redacted command and working directory |
| `command.finished` | exit code, duration, bounded output, and truncation |
| `note.added` | text and user-selected label |
| `checkpoint.requested` | trigger and selected event range |
| `checkpoint.generated` | model ID and prompt version |
| `checkpoint.approved` | approved capsule version |

## 14. Architecture

### Recommended stack

- Tauri 2 desktop shell.
- React and TypeScript UI.
- Rust application core exposed through typed Tauri commands and events.
- SQLite through `rusqlite` with bundled SQLite and explicit migrations.
- JSON Schema validation in Rust through `jsonschema` or equivalent.
- Git through `std::process::Command` with a fixed read-only allowlist and no shell interpolation.
- `tb run` wrapper or controlled built-in terminal.
- macOS Keychain for secrets.
- Token Factory OpenAI-compatible API.
- Tauri's bundler and updater tooling for packaging.

This stack assumes at least one team member can handle basic Rust implementation and debugging.

### Components

```text
React renderer
  -> typed Tauri command/event boundary
Rust application core
  -> project policy gate
  -> Git collector
  -> command collector
  -> event normalizer
  -> SQLite repository
  -> redaction service
  -> checkpoint builder
  -> Token Factory client
  -> capsule validator
  -> export/delete services
```

### Model boundary

Use the model for grouping evidence, separating certainty, explaining failed attempts, identifying open questions, and proposing the next justified action.

Use deterministic code for collection, timestamps, exit codes, path enforcement, redaction, retention, validation, provenance, deletion, and every computer action.

## 15. Internal service interfaces

These may be typed Tauri commands rather than HTTP routes.

```text
projects.create(input) -> Project
projects.list() -> Project[]
projects.updatePolicy(projectId, policy) -> ProjectPolicy

threads.start(input) -> WorkThread
threads.pause(threadId) -> WorkThread
threads.resume(threadId) -> WorkThread
threads.end(threadId) -> WorkThread
threads.delete(threadId) -> DeletionReceipt

events.list(threadId, cursor?) -> EventPage
events.addNote(threadId, note) -> Event
events.captureGit(threadId) -> Event

checkpoints.prepare(threadId) -> PayloadPreview
checkpoints.generate(checkpointId, approvedPayload) -> CapsuleDraft
checkpoints.update(checkpointId, patch) -> CapsuleDraft
checkpoints.approve(checkpointId) -> ApprovedCapsule
checkpoints.getResume(checkpointId) -> ResumeViewModel

exports.markdown(checkpointId) -> FileResult
exports.json(checkpointId) -> FileResult
```

## 16. UI surfaces

### Home

Approved projects, recent threads, ready capsules, connectivity status, and start-thread action.

### New thread

Project, title, intended outcome, optional hypothesis, and project-policy summary.

### Active thread

Goal, status, evidence timeline, labelled notes, pause, end, and Save My Place.

### Payload preview

Every outbound field, redaction state, exclusion controls, total size, cancel, and generate.

### Capsule review

Summary, source-linked facts, hypotheses, attempts, questions, artifacts, next action, edit, relabel, regenerate, and approve.

### Resume

Repository drift, next action first, checkpoint context, evidence, open/copy actions, resume, archive, export, and delete.

### Privacy center

Project roots, exclusions, capture toggles, retention, redaction test, pause all, export, and delete.

## 17. Privacy and security

### Never send

- API keys, credentials, or environment-variable values.
- Private-key blocks.
- Ignored or binary files.
- Absolute home-directory paths.
- Keystrokes, screenshots, or global history.

### Send only after confirmation

- Goal and user notes.
- Git metadata.
- Selected redacted command-output excerpts.
- Selected diff hunks or file snippets when enabled.

### Required controls

- Canonicalize paths before allowlist checks.
- Reject symlink targets outside the project.
- Use fixed Git arguments without shell interpolation.
- Treat repository text as untrusted model data.
- Give the model no tools or execution capability.
- Redact before persistence when feasible and again before transmission.
- Never log API headers or complete private payloads.
- Store credentials only in the OS keychain.
- Delete rows, cached payloads, exports, and indexes together.

## 18. Nonfunctional requirements

| Area | Requirement |
|---|---|
| Privacy | No capture outside an active thread and approved project. |
| Provenance | 100% of verified facts have valid source IDs. |
| Responsiveness | Local UI actions should complete within 200 ms. |
| Checkpoint preparation | Under two seconds for the demo dataset. |
| Model latency | Target under ten seconds at p50; record actual p95. |
| Durability | Persist an event before acknowledging it. |
| Bounded storage | Limit output and snippet size; never ingest binary content. |
| Accessibility | Keyboard navigation, visible focus, semantic labels, and no color-only status. |
| Diagnostics | Record errors and timings without private payload text. |
| Compatibility | macOS MVP with isolated platform adapters. |

## 19. Required failure behavior

| Failure | Response |
|---|---|
| No network or API failure | Keep the checkpoint and allow explicit retry. |
| Invalid model JSON | Attempt one repair, then show validation errors. |
| Secret detected | Block transmission until removed or excluded. |
| Project moved | Keep the capsule and offer explicit relinking. |
| Artifact missing | Show the last known relative path. |
| Conflicting evidence | Preserve both observations and add an open question. |
| Repository drift | List changed branch, HEAD, and status signals. |
| Application crash | Recover durable events and acknowledge the missing interval. |
| Huge output | Store bounded head/tail excerpts with truncation metadata. |
| Unsupported fact | Move it to Needs Review and prevent fact approval. |

## 20. Evaluation

### Prepared scenarios

1. Clear failing test and obvious next step.
2. Competing hypotheses.
3. Failed attempt whose code was reverted.
4. Large output requiring truncation.
5. Fake API key in terminal output.
6. Repository changes during interruption.
7. Minimal evidence and no useful next step.
8. User correction from fact to hypothesis.
9. Token Factory failure.
10. Outside-root symlink.

### Metrics

| Metric | Target |
|---|---|
| Unsupported verified facts | 0 after validation |
| Fact evidence precision | At least 95% on prepared scenarios |
| Seeded secret leakage | 0 |
| Next-action agreement | At least 80% |
| Material correction rate | Below 30% after prompt iteration |
| Resume time | At least 30% faster than raw Git/terminal history in a small paired study |

Report targets separately from observed results.

## 21. Demo scenario

1. Start **Duplicate capture investigation** in a seeded payments repository.
2. State the goal: find why retries produce a second provider capture.
3. Run the focused test and record two provider calls.
4. Change retry middleware and rerun; the test still fails.
5. Add a hypothesis that a second worker bypasses the lock.
6. Press **Save My Place**.
7. Show the payload preview and block a seeded fake secret.
8. Generate the Nemotron capsule.
9. Inspect the verified fact and source event.
10. Simulate a later repository change.
11. Reopen the capsule, show drift, and resume from the lock-owner check.

## 22. Definition of done

- A new user completes the seeded demo without builder assistance.
- The application makes a real Token Factory runtime call using an NVIDIA open-source model.
- Model output passes schema and provenance validation.
- Allowlisting, preview, redaction, correction, pause, export, and deletion work in one build.
- The ten prepared scenarios run through a repeatable evaluation harness.
- The repository contains setup, license, architecture, demo data, tests, and limitations.
- The distributable build matches the recorded demonstration.
