# ThreadBack MVP Solo Implementation Plan

**Purpose:** Provide one developer with a sequential, implementation-ready backlog whose tasks have explicit dependencies, deliverables, and acceptance checks.  
**Source of truth:** `spec.md`  
**Target:** A complete macOS-first MVP and deterministic hackathon demo in seven days.  
**Stack:** Tauri 2, React, TypeScript, Rust, and SQLite.

## 1. Working rules

1. Read `spec.md` before changing implementation files.
2. Treat privacy rules, product boundaries, the capsule contract, and non-goals as requirements.
3. Do not add screenshots, keystroke logging, global shell-history access, autonomous execution, or distraction scoring.
4. Use deterministic Rust code for collection, validation, redaction, path enforcement, retention, deletion, and every computer action.
5. The model may synthesize a capsule but may not execute tools or computer actions.
6. Never call a claim a verified fact without a valid local source event.
7. Prefer a complete vertical slice over broad infrastructure.
8. Complete tasks in dependency order and do not cross a delivery gate with failing required tests.
9. Add tests for boundaries, state transitions, persistence, validation, deletion, and the demo path.
10. Update this plan when a dependency, interface, deliverable, or acceptance condition changes.

## 2. Repository structure

```text
threadback/
├── spec.md
├── plan.md
├── README.md
├── LICENSE
├── package.json
├── src/
│   ├── app/
│   ├── components/
│   ├── screens/
│   ├── state/
│   ├── styles/
│   └── bindings/
├── src-tauri/
│   ├── capabilities/
│   ├── icons/
│   ├── migrations/
│   ├── src/
│   │   ├── commands/
│   │   ├── services/
│   │   └── main.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── crates/
│   ├── threadback-core/
│   │   └── src/
│   │       ├── domain/
│   │       ├── storage/
│   │       ├── threads/
│   │       ├── collectors/
│   │       ├── privacy/
│   │       ├── checkpoints/
│   │       ├── inference/
│   │       ├── validation/
│   │       └── export/
│   └── tb-cli/
│       └── src/
├── fixtures/
│   ├── demo-repo/
│   └── evaluation/
├── tests/
│   ├── integration/
│   ├── security/
│   └── evaluation/
└── scripts/
    ├── reset-demo.*
    └── run-evaluation.*
```

Rust is the source of truth for domain types. Generated TypeScript bindings are checked into `src/bindings/` and verified by a stale-binding check.

## 3. Public interfaces and contracts

Expose only the following service surface as typed Tauri commands:

```text
projects.create(input) -> Project
projects.list() -> Project[]
projects.updatePolicy(projectId, policy) -> ProjectPolicy

threads.start(input) -> WorkThread
threads.pause(threadId) -> WorkThread
threads.resume(threadId) -> WorkThread
threads.end(threadId) -> WorkThread
threads.archive(threadId) -> WorkThread
threads.delete(threadId) -> DeletionReceipt

events.list(threadId, cursor?) -> EventPage
events.addNote(threadId, note) -> ThreadEvent
events.captureGit(threadId) -> ThreadEvent

checkpoints.prepare(threadId) -> PayloadPreview
checkpoints.generate(checkpointId, approvedPayload) -> CapsuleDraft
checkpoints.update(checkpointId, patch) -> CapsuleDraft
checkpoints.approve(checkpointId) -> ApprovedCapsule
checkpoints.getResume(checkpointId) -> ResumeViewModel

exports.markdown(checkpointId) -> FileResult
exports.json(checkpointId) -> FileResult

configuration.storeTokenFactory(input) -> ConfigurationStatus
configuration.testTokenFactory(input) -> ConnectionTestResult
```

Shared contracts include `Project`, `ProjectPolicy`, `WorkThread`, `ThreadState`, the `ThreadEvent` discriminated union, `ArtifactRef`, `Checkpoint`, `Correction`, `PayloadPreview`, `ResumeCapsule`, `ResumeViewModel`, and `DeletionReceipt`.

Every command returns either its declared result or a serializable domain error with a stable code, user-safe message, retryability flag, and optional field details. Renderer input is validated before any service or filesystem operation. The API key is write-only from the renderer's perspective.

## 4. Implementation backlog and deliverables

### 4.0 Foundation

- [ ] **F-01 — Scaffold Tauri 2, React, and TypeScript**
  - Dependencies: None.
  - Deliverable: Launchable macOS development application using Vite, npm, and Cargo scripts.
  - Accept when: `npm run tauri dev`, frontend type-checking, and Rust checking succeed.

- [ ] **F-02 — Establish Rust workspace and module boundaries**
  - Dependencies: F-01.
  - Deliverable: Tauri application, reusable `threadback-core` crate, and `tb` CLI crate.
  - Accept when: Both binaries compile and depend on the same domain and storage library.

- [ ] **F-03 — Define domain contracts**
  - Dependencies: F-02.
  - Deliverable: Rust types for projects, policies, threads, states, events, artifacts, checkpoints, corrections, payload previews, resume capsules, and receipts.
  - Accept when: TypeScript bindings are generated from Rust and contain no database- or UI-specific fields.

- [ ] **F-04 — Define the Tauri command API and error envelope**
  - Dependencies: F-03.
  - Deliverable: Typed command declarations, generated frontend bindings, stable error codes, and user-safe messages.
  - Accept when: Invalid command input is rejected before service execution and the API key cannot be read back.

- [ ] **F-05 — Add repository essentials**
  - Dependencies: F-01.
  - Deliverable: README skeleton, `.env.example` containing names only, license, ignore rules, and format, check, test, build, and demo-reset commands.
  - Accept when: A clean checkout can run all local validation commands documented by the repository.

### 4.1 Persistence and thread lifecycle

- [ ] **P-01 — Create SQLite migrations**
  - Dependencies: F-02, F-03.
  - Deliverable: Versioned migrations and indexes for projects, policies, threads, events, artifacts, checkpoints, corrections, and metadata.
  - Accept when: A new database migrates successfully and repeated startup is idempotent.

- [ ] **P-02 — Implement repositories**
  - Dependencies: P-01.
  - Deliverable: Transactional Rust repositories with append-only event insertion and cursor-based timeline reads.
  - Accept when: Project, thread, event, and checkpoint records survive application restart.

- [ ] **P-03 — Implement lifecycle rules**
  - Dependencies: P-02.
  - Deliverable: Start, pause, resume, end, archive, and delete services enforcing the specification's state model.
  - Accept when: Table-driven tests cover every allowed and rejected transition.

- [ ] **P-04 — Implement durable acknowledgement and recovery**
  - Dependencies: P-03.
  - Deliverable: Commit-before-success event handling and startup recovery for an interrupted active thread.
  - Accept when: A forced restart retains every acknowledged event and surfaces any unrecorded interval.

- [ ] **P-05 — Implement retention and transactional deletion**
  - Dependencies: P-02, P-03.
  - Deliverable: Fourteen-day default raw-event retention and complete deletion of related rows, payload caches, indexes, and exports.
  - Accept when: Deletion tests find no thread-associated local data after the transaction commits.

### 4.2 Project boundary and evidence collection

- [ ] **E-01 — Implement project allowlisting**
  - Dependencies: F-03, P-02.
  - Deliverable: Canonical-root resolution, default exclusions, relative-path normalization, symlink enforcement, and readable rejection errors.
  - Accept when: Traversal, direct outside-root paths, and outside-root symlinks cannot create events.

- [ ] **E-02 — Implement read-only Git snapshots**
  - Dependencies: E-01.
  - Deliverable: Branch, HEAD, porcelain status, changed paths, and staged and unstaged diff statistics collected with fixed `std::process::Command` arguments.
  - Accept when: Tests prove that no shell interpolation or mutating Git command is reachable.

- [ ] **E-03 — Implement `tb run -- <command> [args...]`**
  - Dependencies: E-01, P-03.
  - Deliverable: Rust CLI that records start and finish times, canonical working directory, exit code, bounded stdout and stderr, and truncation details while forwarding the child exit code.
  - Accept when: It records only during an active thread, rejects invalid working directories, and never reads global shell history.

- [ ] **E-04 — Implement output bounding**
  - Dependencies: E-03.
  - Deliverable: Configurable byte limits retaining head and tail excerpts with explicit original-size and truncation metadata.
  - Accept when: Large-output tests keep storage bounded without losing exit status or truncation visibility.

- [ ] **E-05 — Implement notes and the evidence timeline**
  - Dependencies: F-04, P-03, E-02.
  - Deliverable: Fact, hypothesis, question, and next-step notes plus a chronological, paginated event timeline.
  - Accept when: User labels are preserved exactly and evidence survives restart.

### 4.3 Privacy and offline checkpoints

- [ ] **C-01 — Implement deterministic redaction**
  - Dependencies: F-03.
  - Deliverable: Redaction for common tokens, private keys, credential URLs, absolute home paths, and configured patterns.
  - Accept when: Seeded secrets expose zero original secret characters in persisted or outbound content.

- [ ] **C-02 — Implement outbound policy filtering**
  - Dependencies: C-01, E-01.
  - Deliverable: Included, excluded, blocked, and local-only classifications with per-entry and total sizes.
  - Accept when: A blocking match prevents transmission until the entry is removed or excluded.

- [ ] **C-03 — Implement checkpoint preparation**
  - Dependencies: E-02, E-05, C-02, P-02.
  - Deliverable: Final Git snapshot, active evidence-range selection, deduplication, event and character budgets, and a durable local draft.
  - Accept when: **Save My Place** completes offline in under two seconds on the demo dataset and survives restart.

- [ ] **C-04 — Build the payload-preview UI**
  - Dependencies: F-04, C-03.
  - Deliverable: Exact outbound-payload display, per-entry exclusion, redaction state, total size, cancel, and confirm controls.
  - Accept when: No network request can occur before explicit confirmation and blocking warnings disable generation.

### 4.4 Model integration and validation

- [ ] **M-01 — Implement secure configuration**
  - Dependencies: F-04.
  - Deliverable: Endpoint and model configuration plus write-only API-key storage through macOS Keychain.
  - Accept when: The key is never rendered, stored in SQLite, included in exports, or written to diagnostics.

- [ ] **M-02 — Implement the Token Factory client**
  - Dependencies: M-01, C-04.
  - Deliverable: OpenAI-compatible Nemotron client with cancellation, timeouts, and typed authentication, rate-limit, network, timeout, and server errors.
  - Accept when: Diagnostics contain model ID, latency, and token usage but no payload or authorization data.

- [ ] **M-03 — Create the versioned prompt and capsule schema**
  - Dependencies: F-03, C-03.
  - Deliverable: Versioned prompt treating repository content as untrusted evidence and requiring every capsule section and required source ID.
  - Accept when: Valid fixture responses deserialize into the `ResumeCapsule` contract and prompt-injection fixtures do not alter the output contract.

- [ ] **M-04 — Implement validation and repair**
  - Dependencies: M-03.
  - Deliverable: JSON-shape, provenance ownership, checkpoint-range, confidence, and artifact-path validation followed by one deterministic repair attempt.
  - Accept when: A second failure becomes a visible error and no unsupported verified fact can be approved.

- [ ] **M-05 — Preserve draft and approved versions**
  - Dependencies: P-02, M-04.
  - Deliverable: Separate immutable model draft and user-approved capsule with corrections recorded by field path.
  - Accept when: Approval never overwrites the original model result and every correction remains inspectable.

### 4.5 Desktop workflow

- [ ] **U-01 — Build the application shell and onboarding**
  - Dependencies: F-04, P-03, E-01, M-01.
  - Deliverable: Home, project onboarding, active-thread and privacy navigation, native folder selection, and connectivity and capture indicators.
  - Accept when: A project can be approved and a required-goal thread started using keyboard navigation.

- [ ] **U-02 — Build the active-thread workflow**
  - Dependencies: E-05, C-03, U-01.
  - Deliverable: Goal, status, evidence timeline, labelled notes, pause, end, and **Save My Place**.
  - Accept when: Only `ACTIVE` threads record evidence and all acknowledged actions remain after restart.

- [ ] **U-03 — Build capsule review**
  - Dependencies: M-04, M-05, C-04.
  - Deliverable: Next action first, source-linked facts, hypotheses, failed attempts, questions, artifacts, editing, relabelling, regeneration, approval, and original-draft inspection.
  - Accept when: Every verified fact opens valid local evidence and user corrections are authoritative.

- [ ] **U-04 — Build the resume workflow**
  - Dependencies: U-03, E-02, P-03.
  - Deliverable: Branch, HEAD, and status drift comparison; explicit artifact opening; command copying; resume; archive; export; and delete actions.
  - Accept when: Drift appears before resume actions and commands are copied but never automatically executed.

- [ ] **U-05 — Build privacy controls**
  - Dependencies: C-01, C-02, P-05, U-01.
  - Deliverable: Project roots, exclusions, retention, capture toggles, redaction testing, pause-all, and destructive deletion confirmation.
  - Accept when: Privacy state remains visible and no status relies on color alone.

### 4.6 Export, demo, evaluation, and release

- [ ] **R-01 — Implement exports**
  - Dependencies: M-05, P-05.
  - Deliverable: Approved-capsule export as schema-valid JSON and readable Markdown.
  - Accept when: Both exports reflect the approved version and contain no excluded evidence.

- [ ] **R-02 — Create the deterministic payments fixture**
  - Dependencies: F-05.
  - Deliverable: Duplicate-capture defect, focused retry test, failed middleware path, lock-owner diagnostic, fake secret, drift script, and reset command.
  - Accept when: One command restores the complete scenario to its documented starting state.

- [ ] **R-03 — Encode all ten evaluation scenarios**
  - Dependencies: R-02, M-04.
  - Deliverable: Expected facts, forbidden claims, seeded secrets, acceptable next actions, and deterministic local fixtures.
  - Accept when: The harness reports provenance, leakage, next-action agreement, latency, and correction metrics separately from their targets.

- [ ] **R-04 — Harden failures and accessibility**
  - Dependencies: U-02, U-03, U-04, R-03.
  - Deliverable: Offline, invalid-JSON, missing-artifact, moved-project, conflicting-evidence, huge-output, crash-recovery, and keyboard-only flows.
  - Accept when: Failures preserve local evidence and clearly present retry or recovery actions.

- [ ] **R-05 — Package and document**
  - Dependencies: R-01, R-04.
  - Deliverable: macOS development build, setup guide, architecture, privacy model, Nemotron usage, demo instructions, tests, limitations, license, and attribution.
  - Accept when: A clean environment completes setup, demo, tests, and packaging without undocumented steps.

- [ ] **R-06 — Produce release evidence**
  - Dependencies: R-03, R-05.
  - Deliverable: Evaluation report, distributable build, and an under-three-minute recorded demo.
  - Accept when: The recording uses the exact packaged build and every definition-of-done item is checked.

## 5. Test and delivery gates

### Gate G1 — Local thread

**Flow:** Project onboarding -> thread creation -> initial Git snapshot -> persistence -> restart recovery.  
**Required tasks:** F-01 through F-05, P-01 through P-04, E-01, E-02, U-01.  
**Pass condition:** The application can create and reopen a real local thread without losing acknowledged data.

### Gate G2 — Explicit evidence

**Flow:** `tb run` -> command events -> labelled notes -> final Git snapshot -> offline checkpoint.  
**Required tasks:** E-03 through E-05, C-03, U-02.  
**Pass condition:** A complete evidence interval is durable and inspectable without a model or network.

### Gate G3 — Private model call

**Flow:** Redaction -> payload preview -> confirmation -> Token Factory -> validated draft.  
**Required tasks:** C-01, C-02, C-04, M-01 through M-04.  
**Pass condition:** A real Nemotron call creates a valid capsule without leaking the seeded secret.

### Gate G4 — Human review

**Flow:** Source inspection -> editing -> relabelling -> approval -> correction history.  
**Required tasks:** M-05, U-03.  
**Pass condition:** Approved output is distinguishable from the immutable model draft, and unsupported facts cannot be approved silently.

### Gate G5 — Return

**Flow:** Repository drift -> resume view -> explicit open or copy -> resumed evidence interval.  
**Required tasks:** U-04, U-05, R-01, P-05.  
**Pass condition:** The complete Maya scenario reaches a useful, evidence-backed resume point without automatic execution.

### Gate G6 — Release

**Flow:** Deterministic reset -> ten scenarios -> hardening -> package -> documentation -> demo.  
**Required tasks:** R-02 through R-06.  
**Pass condition:** Every definition-of-done condition in `spec.md` passes against the distributable build.

## 6. Seven-day schedule

### Day 1 — Foundation and persistence

- Complete F-01 through F-05.
- Complete P-01 and P-02.
- Begin R-02 so the vertical slices use real demo data.

**Daily deliverable:** The shell runs, Rust and TypeScript contracts agree, SQLite migrates, and a project and thread can be inserted and read.

### Day 2 — Local thread and evidence

- Complete P-03 and P-04.
- Complete E-01, E-02, and E-05.
- Complete U-01.
- Pass Gate G1.

**Daily deliverable:** A project-bound thread captures Git evidence, persists it, and recovers after restart.

### Day 3 — Command evidence and offline checkpoint

- Complete E-03 and E-04.
- Complete C-03 and U-02.
- Finish the resettable base of R-02.
- Pass Gate G2.

**Daily deliverable:** Wrapped commands, notes, and Git evidence form a durable offline checkpoint.

### Day 4 — Privacy and Nemotron

- Complete C-01, C-02, and C-04.
- Complete M-01 through M-04.
- Pass Gate G3.

**Daily deliverable:** An approved, redacted payload produces a source-valid Nemotron capsule draft without leaking the seeded secret.

### Day 5 — Human review and return

- Complete M-05 and U-03.
- Complete U-04 and U-05.
- Complete P-05 and R-01.
- Pass Gates G4 and G5.

**Daily deliverable:** The user can correct, approve, export, resume, archive, or delete a capsule after reviewing repository drift.

### Day 6 — Evaluation and hardening

- Complete R-02 and R-03.
- Complete R-04.
- Fix secret leakage, unsupported facts, boundary failures, and state loss before lower-severity defects.

**Daily deliverable:** All ten scenarios execute repeatably, security and provenance gates pass, and known limitations are documented.

### Day 7 — Packaging and release evidence

- Complete R-05 and R-06.
- Verify setup and the demo in a clean environment.
- Pass Gate G6.

**Daily deliverable:** A distributable macOS build, evaluation report, public-facing documentation, and matching under-three-minute demo are ready.

## 7. Required automated coverage

### Unit tests

- Legal and illegal lifecycle transitions.
- Canonicalization, traversal rejection, and symlink escape.
- Git command allowlist and parser behavior.
- Command argument handling and exit-code forwarding.
- Output head/tail truncation.
- Built-in and user-defined redaction.
- Capsule schema, provenance, and artifact-path validation.
- Markdown and JSON export.

### Integration tests

- Create project -> start thread -> restart -> recover.
- Run wrapped command -> persist exit code and bounded output.
- Save and reopen a checkpoint offline.
- Preview -> confirm -> generate -> validate -> edit -> approve.
- Detect branch, HEAD, and status drift.
- Retain approved capsule while expiring eligible raw events.
- Delete thread and verify related rows, caches, indexes, and exports are gone.

### Security tests

- Direct outside-root path, traversal, and outside-root symlink.
- Shell metacharacters passed as literal command arguments.
- Fake API keys, credential URLs, and private-key blocks.
- Prompt-injection text inside repository evidence.
- Unsupported verified facts and cross-thread source IDs.
- Artifact paths that resolve outside the approved project.
- API-key absence from SQLite, logs, diagnostics, and exports.

### Manual experience checks

- Complete the demo using only the keyboard.
- Recover clearly from offline and model failures.
- Distinguish facts from hypotheses without relying on color.
- Reach evidence from every verified fact.
- Find pause and delete controls quickly.
- Confirm no wording judges focus, motivation, or distraction.

## 8. Scope-cut order

If time runs out, cut optional features in this order:

1. Idle checkpoint suggestion.
2. Tray-menu polish.
3. Open-file convenience action.
4. Markdown export.
5. User-defined redaction patterns.
6. File-change watcher; retain Git snapshots.

Do not cut:

- Project boundary enforcement.
- **Save My Place** and offline checkpointing.
- Exact payload preview.
- Secret redaction.
- A real Nemotron runtime call.
- Certainty separation.
- Provenance validation.
- User correction and approval.
- Repository drift warning.
- Pause and complete deletion.
- The deterministic end-to-end demo.

Auto-update, tray polish, idle suggestions, file watching, and open-file convenience are optional. Packaging a working macOS build is required; implementing an updater is not.

## 9. Assumptions and defaults

- One developer owns all implementation and integration work.
- The project uses npm for frontend commands and Cargo for Rust commands.
- SQLite uses `rusqlite` with bundled SQLite and explicit migrations.
- JSON Schema and provenance validation execute in Rust.
- macOS Keychain is the only API-key persistence location.
- The MVP supports one macOS user, one project per thread, and one locally active thread at a time.
- The command wrapper and desktop app share the domain and storage implementation through `threadback-core`.
- Repository and command content is untrusted data, never executable model instruction.
- The application sends data only after the user reviews and confirms the exact filtered payload.

## 10. Final acceptance checklist

- [ ] An approved project cannot escape its canonical boundary.
- [ ] Thread lifecycle and acknowledged events persist across restart.
- [ ] Git and explicitly wrapped-command events are durable.
- [ ] User-authored note labels remain authoritative.
- [ ] **Save My Place** works offline.
- [ ] The outbound payload is fully visible and editable before transmission.
- [ ] The seeded fake secret never reaches the model payload.
- [ ] A real Nemotron call produces the capsule draft.
- [ ] Unsupported facts cannot be approved silently.
- [ ] The user can correct and approve the capsule without overwriting the model draft.
- [ ] Repository drift appears before resume actions.
- [ ] No command executes automatically from a capsule.
- [ ] Approved JSON and Markdown exports work.
- [ ] Retention and complete deletion work.
- [ ] The ten-scenario evaluation report exists.
- [ ] The demo resets in one command.
- [ ] README, license, setup, tests, privacy notes, and limitations are complete.
- [ ] The recorded demo matches the distributable build.
