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
threads.list() -> WorkThread[]
threads.delete(threadId) -> DeletionReceipt

events.list(threadId, cursor?) -> EventPage
events.addNote(threadId, note) -> ThreadEvent
events.captureGit(threadId) -> ThreadEvent

checkpoints.prepare(threadId) -> PayloadPreview
checkpoints.generate(checkpointId, approvedPayload) -> CapsuleDraft
checkpoints.update(checkpointId, patch) -> CapsuleDraft
checkpoints.approve(checkpointId) -> ApprovedCapsule
checkpoints.getResume(checkpointId) -> ResumeViewModel
checkpoints.preview(checkpointId, excludedEventIds) -> PayloadPreview
checkpoints.generate(checkpointId, excludedEventIds, reviewedHash) -> void
checkpoints.discard(checkpointId) -> WorkThread

exports.markdown(checkpointId) -> FileResult
exports.json(checkpointId) -> FileResult

configuration.storeTokenFactory(input) -> ConfigurationStatus
configuration.testTokenFactory(input) -> ConnectionTestResult
```

Shared contracts include `Project`, `ProjectPolicy`, `WorkThread`, `ThreadState`, the `ThreadEvent` discriminated union, `ArtifactRef`, `Checkpoint`, `Correction`, `PayloadPreview`, `ResumeCapsule`, `ResumeViewModel`, and `DeletionReceipt`.

Every command returns either its declared result or a serializable domain error with a stable code, user-safe message, retryability flag, and optional field details. Renderer input is validated before any service or filesystem operation. The API key is write-only from the renderer's perspective.

## 4. Implementation backlog and deliverables

### 4.0 Foundation

- [x] **F-01 — Scaffold Tauri 2, React, and TypeScript**
  - Dependencies: None.
  - Deliverable: Launchable macOS development application using Vite, npm, and Cargo scripts.
  - Accept when: `npm run tauri dev`, frontend type-checking, and Rust checking succeed.

- [x] **F-02 — Establish Rust workspace and module boundaries**
  - Dependencies: F-01.
  - Deliverable: Tauri application, reusable `threadback-core` crate, and `tb` CLI crate.
  - Accept when: Both binaries compile and depend on the same domain and storage library.

- [x] **F-03 — Define domain contracts**
  - Dependencies: F-02.
  - Deliverable: Rust types for projects, policies, threads, states, events, artifacts, checkpoints, corrections, payload previews, resume capsules, and receipts.
  - Accept when: TypeScript bindings are generated from Rust and contain no database- or UI-specific fields.

- [x] **F-04 — Define the Tauri command API and error envelope**
  - Dependencies: F-03.
  - Deliverable: Typed command declarations, generated frontend bindings, stable error codes, and user-safe messages.
  - Accept when: Invalid command input is rejected before service execution and the API key cannot be read back.

- [x] **F-05 — Add repository essentials**
  - Dependencies: F-01.
  - Deliverable: README skeleton, `.env.example` containing names only, license, ignore rules, and format, check, test, build, and demo-reset commands.
  - Accept when: A clean checkout can run all local validation commands documented by the repository.

### 4.1 Persistence and thread lifecycle

- [x] **P-01 — Create SQLite migrations**
  - Dependencies: F-02, F-03.
  - Deliverable: Versioned migrations and indexes for projects, policies, threads, events, artifacts, checkpoints, corrections, and metadata.
  - Accept when: A new database migrates successfully and repeated startup is idempotent.

- [x] **P-02 — Implement repositories**
  - Dependencies: P-01.
  - Deliverable: Transactional Rust repositories with append-only event insertion and cursor-based timeline reads.
  - Accept when: Project, thread, event, and checkpoint records survive application restart.

- [x] **P-03 — Implement lifecycle rules**
  - Dependencies: P-02.
  - Deliverable: Start, pause, resume, end, archive, and delete services enforcing the specification's state model.
  - Accept when: Table-driven tests cover every allowed and rejected transition.

- [x] **P-04 — Implement durable acknowledgement and recovery**
  - Dependencies: P-03.
  - Deliverable: Commit-before-success event handling and startup recovery for an interrupted active thread.
  - Accept when: A forced restart retains every acknowledged event and surfaces any unrecorded interval.

- [ ] **P-05 — Implement retention and transactional deletion**
  - Dependencies: P-02, P-03.
  - Deliverable: Fourteen-day default raw-event retention and complete deletion of related rows, payload caches, indexes, and exports.
  - Accept when: Deletion tests find no thread-associated local data after the transaction commits.

### 4.2 Project boundary and evidence collection

- [x] **E-01 — Implement project allowlisting**
  - Dependencies: F-03, P-02.
  - Deliverable: Canonical-root resolution, default exclusions, relative-path normalization, symlink enforcement, and readable rejection errors.
  - Accept when: Traversal, direct outside-root paths, and outside-root symlinks cannot create events.

- [x] **E-02 — Implement read-only Git snapshots**
  - Dependencies: E-01.
  - Deliverable: Branch, HEAD, porcelain status, changed paths, and staged and unstaged diff statistics collected with fixed `std::process::Command` arguments.
  - Accept when: Tests prove that no shell interpolation or mutating Git command is reachable.

- [x] **E-03 — Implement `tb run -- <command> [args...]`**
  - Dependencies: E-01, P-03.
  - Deliverable: Rust CLI that records start and finish times, canonical working directory, exit code, bounded stdout and stderr, and truncation details while forwarding the child exit code.
  - Accept when: It records only during an active thread, rejects invalid working directories, and never reads global shell history.

- [x] **E-04 — Implement output bounding**
  - Dependencies: E-03.
  - Deliverable: Configurable byte limits retaining head and tail excerpts with explicit original-size and truncation metadata.
  - Accept when: Large-output tests keep storage bounded without losing exit status or truncation visibility.

- [x] **E-05 — Implement notes and the evidence timeline**
  - Dependencies: F-04, P-03, E-02.
  - Deliverable: Fact, hypothesis, question, and next-step notes plus a chronological, paginated event timeline.
  - Accept when: User labels are preserved exactly and evidence survives restart.

### 4.3 Privacy and offline checkpoints

- [x] **C-01 — Implement deterministic redaction**
  - Dependencies: F-03.
  - Deliverable: Redaction for common tokens, private keys, credential URLs, absolute home paths, and configured patterns.
  - Accept when: Seeded secrets expose zero original secret characters in persisted or outbound content.

- [x] **C-02 — Implement outbound policy filtering**
  - Dependencies: C-01, E-01.
  - Deliverable: Included, excluded, blocked, and local-only classifications with per-entry and total sizes.
  - Accept when: A blocking match prevents transmission until the entry is removed or excluded.

- [x] **C-03 — Implement checkpoint preparation**
  - Dependencies: E-02, E-05, C-02, P-02.
  - Deliverable: Final Git snapshot, active evidence-range selection, deduplication, event and character budgets, and a durable local draft.
  - Accept when: **Save My Place** completes offline in under two seconds on the demo dataset and survives restart.

- [x] **C-04 — Build the payload-preview UI**
  - Dependencies: F-04, C-03.
  - Deliverable: Exact outbound-payload display, per-entry exclusion, redaction state, total size, cancel, and confirm controls.
  - Accept when: No network request can occur before explicit confirmation and blocking warnings disable generation.

### 4.4 Model integration and validation

- [x] **M-01 — Implement secure configuration**
  - Dependencies: F-04.
  - Deliverable: Endpoint and model configuration plus write-only API-key storage through macOS Keychain.
  - Accept when: The key is never rendered, stored in SQLite, included in exports, or written to diagnostics.

- [x] **M-02 — Implement the Token Factory client**
  - Dependencies: M-01, C-04.
  - Deliverable: OpenAI-compatible Nemotron client with cancellation, timeouts, and typed authentication, rate-limit, network, timeout, and server errors.
  - Accept when: Diagnostics contain model ID, latency, and token usage but no payload or authorization data.

- [x] **M-03 — Create the versioned prompt and capsule schema**
  - Dependencies: F-03, C-03.
  - Deliverable: Versioned prompt treating repository content as untrusted evidence and requiring every capsule section and required source ID.
  - Accept when: Valid fixture responses deserialize into the `ResumeCapsule` contract and prompt-injection fixtures do not alter the output contract.

- [x] **M-04 — Implement validation and repair**
  - Dependencies: M-03.
  - Deliverable: JSON-shape, provenance ownership, checkpoint-range, confidence, and artifact-path validation followed by one deterministic repair attempt.
  - Accept when: A second failure becomes a visible error and no unsupported verified fact can be approved.

- [x] **M-05 — Preserve draft and approved versions**
  - Dependencies: P-02, M-04.
  - Deliverable: Separate immutable model draft and user-approved capsule with corrections recorded by field path.
  - Accept when: Approval never overwrites the original model result and every correction remains inspectable.

### 4.5 Desktop workflow

- [x] **U-01 — Build the application shell and onboarding**
  - Dependencies: F-04, P-03, E-01, M-01.
  - Deliverable: Home, project onboarding, active-thread and privacy navigation, native folder selection, and connectivity and capture indicators.
  - Accept when: A project can be approved and a required-goal thread started using keyboard navigation.

- [x] **U-02 — Build the active-thread workflow**
  - Dependencies: E-05, C-03, U-01.
  - Deliverable: Goal, status, evidence timeline, labelled notes, pause, end, and **Save My Place**.
  - Accept when: Only `ACTIVE` threads record evidence and all acknowledged actions remain after restart.

- [x] **U-03 — Build capsule review**
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

### Command evidence (E-03)

- `tb run -- <command> [args...]` runs the command with literal arguments (no shell), streams stdout and stderr through unchanged, and forwards the exit code (128 plus the signal number when the command is killed; 127 for a missing program, 126 when it cannot be run).
- It records `command.started` and `command.finished` only when a thread is recording and the current folder is inside that thread's project and not excluded. Otherwise the command still runs, and `tb` prints a "not recorded" notice to stderr. Only an unusable working directory or an empty command is refused (exit code 2).
- `tb` never creates the database. If the app has not run yet, the command runs unrecorded.
- The working directory is stored relative to the project root, and the environment is never recorded.
- `command.finished` links to its start event, so a start without a finish is visible (for example, when the thread was paused mid-command or `tb` was killed).
- Output is passed to the terminal in full but stored as a bounded head and tail excerpt (4 KiB and 8 KiB per stream by default) with original and omitted byte counts. Binary output is counted but never stored. See the output limits below.
- The app and `tb` share one database path from `threadback_core::database_path()` (`THREADBACK_DB` overrides it).
- Known limits: the command sees pipes instead of a terminal, so tools may drop colour and progress bars, and commands are not redacted until C-01 lands, so stored command text and output are unredacted on this computer. Nothing leaves the computer before the payload preview exists.

### Redaction (C-01)

- `privacy::Redactor` is deterministic and idempotent. It replaces private-key blocks (including unterminated blocks and a lone END marker), authorization and bearer credentials, passwords embedded in URLs, token-like query parameters, well-known provider tokens (OpenAI-style, GitHub, GitLab, Slack, AWS, Google, npm, Hugging Face, SendGrid), JWTs, `<prefix>_<live|test|demo|prod|secret>_<body>` keys (a digit is required, so snake_case identifiers are left alone), and the value in `name = value` assignments whose name suggests a secret. It fails closed: only placeholders, literals, and references (`$VAR`, `process.env.X`, `true`) are left alone.
- Credentials become `[REDACTED:<kind>]`. Home-directory prefixes (`/Users/<name>`, `/home/<name>`, `C:\Users\<name>`, and the current `$HOME`) become `~`; this is not counted as a credential. User patterns (regular expressions, at most 50 of 200 characters, none that match empty text) are supported by `Redactor::new`; wiring them to per-project policy comes with U-05.
- Redaction is applied in three layers so no path can bypass it:
  1. `ThreadEvent::new` redacts the payload before hashing, so neither the stored text nor the content hash depends on a secret. An event that contained a credential is marked `Sensitive`.
  2. `Database::insert_event` redacts again as a backstop and recomputes the hash, so an event built any other way is still stored clean. Thread titles and goals are redacted too.
  3. Command output is redacted line by line as it streams, before any head/tail cut, so a cut cannot split a credential in two. The terminal still receives the raw output.
- `originalBytes` is what the command wrote; `omittedBytes` is counted against the redacted text, and `redactions` counts the credentials replaced in a stream.
- `Database::open` runs a one-time, versioned scrub (`REDACTION_VERSION`) that rewrites rows stored before redaction existed, then truncates the write-ahead log and vacuums so the old bytes leave the file. Bump the version whenever the rules change.
- Outbound policy (C-02): redaction at capture means a stored credential is already replaced. The check before sending scans again; any credential still found (a legacy row, or text the user edited into the preview) is **blocking** and disables generation until it is removed or excluded.
- Known limits: a credential written in free prose ("my password is hunter2"), a bare high-entropy string with no recognisable prefix or name, and a private-key body pasted without its BEGIN or END marker are not detected. A secret split across two output lines other than a PEM block is not joined.
- Performance: about 75 MB/s through the stream in an optimized build. The regex crates are built with optimizations even in dev and test builds, because unoptimized they slowed `tb` output to a few MB/s.

### Outbound policy (C-02)

- `OutboundService::prepare(checkpointId, excludedEventIds)` classifies every checkpoint entry:
  - **included**: sent exactly as shown.
  - **excluded**: you removed it. This wins over a block.
  - **blocked**: a credential would still be sent. The value shown is the redacted text, so a credential is never displayed again.
  - **local-only**: the kind is not on the allow-list (anything new stays local by default), or the event is marked `Blocked`. This wins over everything.
- The scan runs at preview time, with a redactor that can carry a project's custom patterns (`OutboundService::with_redactor`). Because built-in rules already redact at capture, a block is reached in practice by a custom pattern added after capture, or by a rules upgrade. Checkpoint drafts are also redacted when stored.
- Title, goal, and the snapshot note cannot be excluded, so a match there is replaced rather than blocked and listed in `notices`. Home paths are rewritten, not blocked.
- Every entry reports its size as sent, and the preview reports the total size and a `payloadHash` of the exact JSON document. `canGenerate` is false while anything is blocked or nothing is selected.
- `OutboundService::authorize(checkpointId, excludedEventIds, reviewedHash)` is the only way to obtain an `ApprovedPayload`. It rebuilds the payload on the server (it never trusts content sent back by the UI) and refuses with `PAYLOAD_BLOCKED` when anything is blocked or nothing is selected, and with `PAYLOAD_CHANGED` when the hash differs from what was reviewed. The model client (M-02) will accept only an `ApprovedPayload`, so there is no route to the network that skips the preview.
- Not done: editing entry text in the preview (the spec asks only for exclusion), and storing a project's custom patterns (U-05).

### Prompt and capsule schema (M-03)

- The capsule contract is `spec.md` §11 exactly: `ResumeCapsule` and its parts use the spec's `snake_case` wire names (the artifact's `type` included) and reject unknown fields. The same shape is the model's output, the stored draft, and the exported JSON. The `capsule_schema()` JSON Schema (draft 2020-12) requires every section, forbids extra properties at every level, requires at least one source id on a verified fact and a failed attempt, allows none on a hypothesis, artifact, or next action, restricts confidence to `low`, `medium`, and `high`, fixes `schema_version` at `1.0`, and bounds text (1,000 to 1,500 characters), list lengths, and source ids per item. A test extracts the example from `spec.md` §11 and validates it, so the schema cannot drift from the spec.
- `CapsuleSchema::check` reports every problem with its location (such as `/verified_facts/0/source_event_ids`), in a stable order, ready for the repair step in M-04.
- `build_prompt` accepts only an `ApprovedPayload`, so nothing can be prompted that the user did not review. The system message is a constant: nothing from a repository, terminal, or note is ever added to it. The user message is a fixed template plus the schema plus the reviewed document as one line of compact JSON between two marker lines. Compact JSON has no raw line breaks, so no line of evidence can equal a marker, and the markers also carry a nonce derived from the payload.
- The prompt tells the model to treat the evidence as untrusted data, not to follow instructions found in it, not to use tools, to reply with one JSON object only, to cite source ids copied exactly from the evidence, never to promote hypothesis, question, or next-step notes to facts, to give exactly one small next action, to use relative paths, and never to reconstruct a `[REDACTED:…]` value.
- `PROMPT_VERSION` (`capsule-prompt-v1`) and the schema version are stored with each draft. Bump the prompt version whenever its wording changes.
- Prompt-injection fixtures (`fixtures/evaluation/injection/`): hostile notes and command output (fake system messages, fake end markers, requests for tool calls, fake source ids, fenced JSON, right-to-left text, a long filler followed by an instruction) go through the real services into a real prompt. They arrive verbatim as JSON string values and never change the system message, the template, or the schema. Nine "the model obeyed the injection" responses (extra `tool_calls`, a `run_command` field, a dropped next action, an unsupported fact, a changed version, plain text, prose before the JSON, a fenced block, an empty object) are all rejected, while genuine capsules are still accepted.
- Decision left to M-04: a valid capsule wrapped in prose or a markdown fence is not accepted as it stands. Whether to extract it or send it through the one repair attempt is for the validation step to decide.
- Not yet: provenance (do the cited ids exist in this checkpoint's interval), artifact path checks, and the repair attempt (M-04); the model call (M-02).

### Model connection and Keychain (M-01)

- `ModelSettings` (endpoint, model ID, timeout, response format) is validated and stored in SQLite metadata. The endpoint must be `https`; plain `http` is accepted only for a loopback host (so a local server can stand in for tests), and credentials, a query, or a fragment in the URL are refused. Model IDs are limited to letters, digits, and `. _ : / -`; the timeout is 5 to 300 seconds. Defaults: `https://api.tokenfactory.nebius.com/v1`, `nvidia/nemotron-3-super-120b-a12b`, 60 seconds, strict JSON schema.
- The API key lives only in the macOS Keychain, through the `SecretStore` adapter (`KeychainSecretStore`; an in-memory store for tests; no fallback to a file on other platforms). The key is write-only from the interface: `configuration.store` accepts it, nothing returned contains it (`ConfigurationStatus` says only `hasApiKey`), `ApiKey` and `ModelConfigInput` hide it in `Debug`/`Display`, and tests scan the database files for the key and any 10-character piece of it. A bad value changes nothing, not even the key. Saving without a key keeps the stored one.
- Interface commands: `configuration.status`, `configuration.store`, `configuration.removeApiKey`, `configuration.test`. A connection test sends one tiny question that contains none of the user's evidence and saves nothing.
- Defaults come from the hackathon documentation: Token Factory is OpenAI-compatible at `https://api.tokenfactory.nebius.com/v1`, authenticated with a bearer token, and structured output is `response_format: {type: "json_schema", json_schema: {name, schema, strict}}`. Model IDs other than Super come from third-party listings and were not verified.

### Token Factory client (M-02)

- `TokenFactoryClient` implements `CapsuleModel`. It posts to `{endpoint}/chat/completions` with the key as a bearer header marked sensitive, `temperature` 0, `max_tokens` 6,000, and the configured response format. Redirects are never followed. The response is read in chunks up to 2 MB. A leading `<think>…</think>` block in a reply is dropped.
- Typed errors the interface can act on: `MODEL_AUTHENTICATION` (401, 403), `MODEL_RATE_LIMITED` (429, with the `Retry-After` seconds when given), `MODEL_NETWORK`, `MODEL_TIMEOUT` (also 408), `MODEL_SERVER` (5xx, a redirect, or an unreadable reply), `MODEL_REJECTED` (400, 404, and other 4xx, with the service's own explanation shortened and passed through redaction), `CANCELLED`, and `NOT_CONFIGURED`. No error message contains the key or any evidence.
- If a 400 or 422 complains about the response format, the client steps down (schema, then JSON object, then none) and remembers it for the session, at most twice. The same messages are sent each time; only how JSON is asked for changes. Nothing else is retried.
- Cancellation: a `CancelToken` aborts a request in flight (tested), and a request cancelled before it starts is never sent. The model commands are `async` and run their blocking work on a worker thread, because Tauri runs plain commands on the main thread and a long request would freeze the window. The database lock is not held while waiting for the model.
- Diagnostics (`ModelCallRecord`) have only a model name, a host, a status label, an HTTP status, latency, token counts, a finish reason, the response format used, and the attempt number. They have no field that could hold the payload, the reply, or the authorization header. Each call is stored in the `model_calls` table (migration 0002), including failures and cancellations, which also gives the p50 and p95 latency for the evaluation report.
- The network code and the Keychain adapter sit behind the `model-client` and `keychain` features, which `tb` does not enable: `tb`'s dependency tree contains no HTTP or TLS crates.

### Generation and the stored draft

- `plan_generation` (database), `run_generation` (network, no database), and `finish_generation` (database) split generation so the lock is never held during a request. `generate_draft` runs all three. The payload is authorized first, so a blocked, empty, or changed payload never reaches the model, and a missing key is reported only after the payload checks.
- A valid capsule is stored once as the checkpoint's `draft` (status `generated`, `model_id`, `prompt_version`). The draft can never be overwritten: the database refuses a second write, and a second generation is refused before any request. A `checkpoint.generated` event records the model, prompt version, payload fingerprint, attempts, latency, and token counts, and none of the content.
- A failed generation (two invalid replies, a service error, a cancellation) stores no draft and keeps the checkpoint as it was, so it can be retried. The calls are recorded either way.
- A generated draft can still be discarded until it is approved (M-05).
- Interface: the preview names the destination before anything is sent, locks the selection while a request is in flight, shows progress with a cancel button, and sends the user to the model settings when the key is missing or rejected. The generated capsule is shown read-only with facts, hypotheses, attempts, open questions, files, and limits in separate labelled sections.
- Still open for Gate G3: a real call to Nemotron. Everything up to the network boundary is tested against a local server; the live call needs an API key.

### Validation and the single correction (M-04)

- `evaluate(reply, context)` judges a reply in order: it must be one JSON object (a reply wrapped in prose or a code fence is rejected with a specific hint, not silently unwrapped), then match the schema, then deserialize into the strict `ResumeCapsule`, then pass the evidence checks below. Problems come back with a location, a kind, and a message, in a stable order, capped at 20.
- Evidence checks (`validate_capsule`, also meant to run when a person edits a capsule, so an edit cannot create an unsupported fact):
  - **Provenance and interval ownership**: every cited id, in every section, must be one of the entries the user approved. An id that is in the checkpoint but was excluded, blocked, or kept local gets its own message ("was not part of the evidence you approved"); anything else is "not an event in this checkpoint".
  - **Unsupported facts**: a verified fact whose only sources are notes the developer labelled hypothesis, question, or next step is rejected. A fact needs at least one source that can support it.
  - **Confidence**: only `low`, `medium`, `high` (schema).
  - **Artifact paths**: relative, forward slashes, no `..`, no stray whitespace, not the project folder itself, inside the project (symlinks resolved) and not a path ThreadBack never captures (`.env`, keys, `node_modules`, `.git`, binaries).
  - **Repeated ids** within verified facts or hypotheses.
  - **Credentials**: any text that looks like a credential anywhere in the capsule. The message never repeats the credential.
- `build_repair_prompt` is a pure function of the original request, the reply, and the problems, so the correction is deterministic. It carries over the system message and the original evidence message unchanged, returns the reply being corrected as the model's turn, and adds a fixed instruction with the numbered problems (one line each, at most 20, each at most 300 characters). The returned reply is cut at 20,000 characters.
- `generate_capsule(model, request, context)` makes at most two model calls: the reply, and one correction if the reply failed. A second failure returns `InvalidOutput` with both sets of problems and the last reply, converted to a visible `INVALID_MODEL_OUTPUT` error ("even after one correction… your checkpoint is kept and nothing was approved… you can try again"). A failure to reach the model passes through unchanged and is never treated as a bad reply. A capsule is returned only when it passed every check.
- `CapsuleModel` is the seam for the model call. M-02 implements it for Token Factory; tests script it.
- `OutboundService::validation_context` builds the context from the approved payload and the checkpoint, so withheld entries are known.
- Prompt-injection replies from the M-03 fixtures are corrected when the second reply is genuine and rejected when they are obeyed twice.
- Still open: the "Needs Review" presentation of unsupported items (U-03). Persisting a failed generation's problems was left out of M-05 (see below).

### Draft, approved capsule, and corrections (M-05)

- A checkpoint now holds two separate capsules. `draft` is what the model returned and is written once. `approved` is what the user signed off on and is also written once. Neither write can replace an earlier one: each is a database update that only matches while its column is empty, so a second attempt changes nothing and is reported. Approving never touches the draft column; a test compares its stored text before and after.
- `ApprovalService::approve(checkpointId, capsule)` takes the **final capsule**, not a list of edits. The interface holds the user's working copy and sends it whole; the server decides what changed. Approving the draft as it stands is the same call with the draft, and records no corrections. Edits that are only in the window are lost if the app closes before approval, which is accepted for the MVP.
- The capsule is judged by the same checks as the model's reply (`check_capsule`: the schema's limits, then provenance, unsupported facts, repeated ids, artifact paths, and credentials), so an edit cannot create an unsupported verified fact, cite an event outside the checkpoint, or carry a credential. A failure is `INVALID_CAPSULE` with the path of the first problem in `field` and nothing is stored.
- **Ownership differs from generation on purpose.** The model could cite only entries the user sent it. An edit may cite any entry in the checkpoint, including ones kept from the model, because that evidence is on this machine and the person editing is the authority. The unsupported-fact rule still applies to them.
- Corrections are **derived by comparing** the two capsules, never reported by the interface, so nothing can be missing from or invented in the record. Each is a field path (a JSON Pointer into the capsule's wire form), the value before, and the value after. A removal has `after: null` and an addition has `before: null`. Lists are aligned by their unchanged items, so deleting one fact does not make the rest look edited; a removed and an added item in the same place are one edit unless their ids differ. Removed items are located by their position in the draft, everything else by position in the approved capsule. Relabelling a fact as a hypothesis is therefore a removal under `/verified_facts` plus an addition under `/hypotheses`. The `reason` column exists and stays empty until the review screen can ask for one.
- The approved capsule and both sides of each correction are redacted before they are stored, like everything else persisted. The capsule is redacted before it is compared, so what is diffed is what is kept.
- One transaction commits the approved capsule, the corrections, the `checkpoint.approved` event, and the thread's move to `READY`. If any step fails none of them happens (tested by making the event insert fail). The event records the checkpoint, the number of corrections, and a SHA-256 fingerprint of the approved capsule, and none of its content; exports can use the fingerprint to show they match what was approved.
- Statuses: `draft`, `generated`, `approved`, `discarded`. Only a `generated` checkpoint can be approved. An approved one cannot be approved again, discarded, or generated again.
- Interface commands: `checkpoints.approve(checkpointId, capsule)` returns the checkpoint with both versions, the thread, and the corrections; `checkpoints.corrections(checkpointId)` lists them again later. The review screen that uses them is U-03. Because approval re-runs the checks, unsupported facts cannot be approved silently even after editing, not only when the model drafts them.
- I did not use database triggers to forbid updating the two capsule columns: the one-time redaction scrub must be able to rewrite stored rows when its rules change.
- Not done here: keeping the problems of a failed generation on the checkpoint. Failed calls are already recorded in `model_calls`, and the visible error carries the reasons; showing them again after a restart can wait for U-03 if it proves useful. **Regenerate** (U-03) cannot replace a draft, so it will discard the unapproved draft and save the place again.

### Payload preview and discarding a draft (C-04)

- From a checkpoint draft, **Review what will be sent…** opens the preview. It states that nothing has been sent, lists every entry with a per-entry checkbox, shows each entry's size, state (in words and a glyph), reason, and redaction tag, and offers the exact content of each entry and the complete payload as one document with its fingerprint.
- Looking, excluding, and cancelling send nothing. Each exclusion asks the server to recompute the payload; the screen never builds the payload itself.
- **Generate capsule** stays focusable with `aria-disabled` and a visible reason (a blocked entry, nothing selected, or the payload still updating), so keyboard and screen-reader users can find out why. It is also refused on the server: `checkpoints.generate` authorizes the payload first (`PAYLOAD_BLOCKED`, `PAYLOAD_CHANGED`) and then, until the model connection exists (M-01 and M-02), ends in `NOT_CONFIGURED: Nothing was sent`.
- Blocked entries show only their replaced text; entries that can never be sent are shown but cannot be selected.
- **Discard draft and keep working** exists on both the draft and the preview, behind a confirmation. It marks the checkpoint `discarded`, records `checkpoint.discarded`, clears `ended_at`, and returns the thread to `ACTIVE`. It is refused if another thread is recording. A discarded draft consumes nothing: the next checkpoint's interval starts where the discarded one would have, and a discarded draft cannot be previewed or generated. This closes the earlier open question about leaving a draft.
- `FAILED` also allows discarding, ready for when generation exists.

### Demo fixture (R-02b)

- `npm run demo:reset` recreates `fixtures/demo-repo/workspace` (or a folder passed as an argument) with fixed author, committer, and dates, so the seeded commit hash is identical every time.
- The defect: a capture that times out after the provider accepted it is redelivered to a second worker, and the two workers are configured with separate lock backends. The retry middleware looks like the cause but never retries a timeout, so changing it does not change the result.
- Terminal output from the focused test contains the fake credential, which exercises the secret-blocking path once redaction exists. `LOCK_DEBUG=1` logs which backend each worker uses (the lock-owner diagnostic), and `npm run test:lock` is the success check.
- `npm run demo:attempt-fix`, `demo:revert-fix`, and `demo:drift` apply the failed attempt, undo it, and change the branch, HEAD, and working tree while the user is away. Drift edits a file the investigation did not touch, so branch, HEAD, and status all differ in the demo flow.
- `npm run test:noisy` prints about 1 MB and fails on the last line, for the large-output case.
- `crates/tb-cli/tests/demo_path.rs` runs this path through the real `tb` binary and the checkpoint service. `fixtures/demo-repo/DEMO.md` is the human script.

### Active-thread workflow (U-02)

- The thread screen shows the goal, state in words, the controls that apply to that state, a labelled-note form, and the evidence timeline.
  - Recording: Save My Place, Pause, Take Git snapshot, End thread.
  - Paused: Resume, End thread.
  - Checkpoint draft: the saved draft is shown and nothing can be added.
- Notes: no label is pre-selected, so every note's kind is an explicit choice; text is sent exactly as typed. The form is disabled, with an explanation, whenever the thread is not recording. Ctrl or ⌘ + Enter adds the note.
- End thread asks for confirmation first, because it stops recording and saves a checkpoint.
- While a thread is recording, the evidence refreshes every 2 seconds, so commands run with `tb` appear on screen. Nothing is polled when the thread is not recording.
- The checkpoint draft view shows what was saved with no model or network: trigger, evidence count, whether the final Git snapshot was taken (and why not), what was left out, and the condensed evidence. It states that nothing has been sent anywhere.
- Actions announce their result in a polite live region, and focus moves to the checkpoint heading after Save My Place or End.
- UI tests (`npm run test:ui`, part of `npm run check`) run the screens against an in-memory fake of the Rust commands. They verify the screen's behavior, not the Rust rules themselves, which have their own tests.
- Still open: a saved draft cannot yet be discarded to return to work.

### Output limits (E-04)

- Limits are set per run with `tb run --head-bytes <n> --tail-bytes <n> -- <command>` or the environment variables `THREADBACK_HEAD_BYTES` and `THREADBACK_TAIL_BYTES`; flags override the environment. Options are read only before the command, so a command's own `--head-bytes` is left alone. An invalid value stops `tb` with exit code 2 before the command runs.
- Each limit is at most 64 KiB (`MAX_CAPTURE_BYTES`). The ceiling is enforced inside the capture itself, so no caller can make stored evidence unbounded. Zero is allowed, which keeps only sizes.
- Each stored excerpt records `originalBytes`, `omittedBytes`, `truncated`, and `binary`, and the head and tail always add up to the original size minus the omitted bytes. The exit status is recorded independently of the output.
- The queue between the pipe readers and the writer is bounded, so a slow terminal slows the command down instead of growing memory. The terminal still receives all output.
- Verified with 100 MB on stdout and 20 MB on stderr at once: all bytes reached the terminal, stored evidence stayed under 40 KB, and the exit code and the final lines of each stream were kept.
- Per-project defaults wait for policy storage (U-05).

### Offline checkpoint (C-03)

- `CheckpointService::save` is Save My Place. It needs no network: it takes a final Git snapshot when the thread is recording (if Git fails, or the thread is paused, the checkpoint is still saved and records why there is no final snapshot), selects the evidence since the previous checkpoint, stores a local `draft`, records `checkpoint.requested`, and moves the thread to `CHECKPOINT_DRAFT`, all in one transaction.
- The interval starts after the previous checkpoint's last event (or at the thread start) and ends at the final snapshot, so a resumed thread's second checkpoint covers only new evidence.
- Selection is deterministic: identical Git snapshots collapse to the latest and identical notes to the first; the budget is 150 entries and 48,000 characters. When over budget it drops other snapshots first, then routine commands, then failures and gaps. The thread start, every note, and the final snapshot are never dropped, and a command's start and finish are dropped together. `omissions` counts everything left out. Notes are shortened only if the never-dropped entries alone exceed the budget.
- Each entry keeps its source `eventId`, so later claims stay traceable. Command output is condensed to the first 1,000 and last 3,000 characters.
- Redaction and the included/excluded/blocked classification are not applied here; they come with C-01 and C-02 when the outbound payload is built from these entries.
- The desktop "End thread" command now saves a checkpoint (trigger `thread_ended`) instead of only changing state, so a closed thread always has a draft to review.

### Project approval (U-01)

- Approval fails during onboarding, with the error shown on the folder field, if the folder is missing, is already approved (compared by canonical path, so symlinks cannot register it twice), or is not a readable Git repository.
- The native folder picker uses `tauri-plugin-dialog` with only the `dialog:allow-open` permission.
- Onboarding shows the default exclusions, the 14-day retention default, and the outbound-data rule, and requires an explicit consent checkbox. The API-key step is not part of onboarding; it belongs to M-01.
- `projects.defaultExclusions()` was added so the UI shows the exclusions the backend actually applies.

### Evidence timeline and notes (E-05)

- `threads.list()` was added to the public surface so the Home screen and a restart can find existing threads.
- `threads.start` records the first Git snapshot in the same transaction as the thread, so a project that is not a readable Git repository fails without leaving a thread behind.
- Notes are stored exactly as typed and are rejected only when blank or longer than 8,000 characters. Redaction runs later (C-01).
- The timeline is oldest-first, 100 events per page from the desktop commands, with a `timestamp|id` cursor that is stable across events sharing a timestamp.
- Desktop commands hold the database lock while Git runs (bounded by a 10-second timeout).

### Git snapshot behavior (E-02)

- The collector runs a closed set of read-only Git commands with no caller-supplied arguments, a rebuilt environment, `--no-optional-locks`, and `core.fsmonitor=false`.
- The project folder may be a subfolder of a repository; only paths under it are reported.
- Changed paths that are excluded or resolve outside the project are counted in `excludedChanges`, not listed, but still make the snapshot `dirty`.
- Renames are reported as a delete plus an add (`--no-renames`).
- The per-project `capture_git` policy flag is not enforced yet because policy storage does not exist; that arrives with the privacy controls (U-05).

### Lifecycle interpretation (P-03)

`spec.md` leaves a few transitions open. The implementation in `threads/lifecycle.rs` resolves them as follows:

- `RESUMED` is the evidence interval that follows a resume, so it records evidence like `ACTIVE`. At most one thread may be `ACTIVE` or `RESUMED` at a time.
- `pause` moves `ACTIVE` or `RESUMED` to `PAUSED`; `resume` moves `PAUSED` to `ACTIVE` and `READY` to `RESUMED`.
- `end` moves `ACTIVE`, `RESUMED`, or `PAUSED` to `CHECKPOINT_DRAFT` and sets `ended_at`. **Save My Place** (`begin_checkpoint`) makes the same move from `ACTIVE` or `RESUMED` without setting `ended_at`.
- `FAILED` returns to `CHECKPOINT_DRAFT` through an explicit retry (FR-20).
- `archive` is allowed from `READY`, `RESUMED`, `PAUSED`, and `FAILED`.
- `delete` is allowed from every state except `DELETED`; the transactional row removal itself belongs to P-05.
- Startup recovery (P-04) pauses any thread still `ACTIVE` or `RESUMED` with the system reason `interrupted` and reports the gap from its last acknowledged event. Capture never resumes silently after a restart.
- Open question: there is no way to abandon a `CHECKPOINT_DRAFT` and return to work.

### Capsule review, editing, and approval (U-03)

- `CapsuleReview` holds a deep copy of `checkpoint.draft` as the user's working copy, so editing never mutates the model's draft.
- Every cited event ID renders as a `SourceList` button that expands the corresponding checkpoint entry inline (`entryItems`/`Timeline`), or warns if the ID is not in the checkpoint.
- Verified facts with no resolvable source or resting only on `hypothesis`/`question`/`next_step` notes are flagged `Needs review` (in words and a glyph), and **Approve capsule** stays focusable with `aria-disabled="true"` and an explanation until fixed.
- Facts can be relabelled as hypotheses (`low` confidence by default, editable via select), and hypotheses can be promoted to verified facts once they cite supporting evidence. Claims, summaries, goals, attempts, questions, artifact reasons, and next-action fields can be edited or removed; open questions and limitations can be added.
- **Show the original model draft** discloses the read-only `CapsuleView` of `checkpoint.draft` alongside the working copy, and **Discard my edits** resets the working copy.
- **Approve capsule** sends the whole working copy to `checkpoints.approve(checkpointId, capsule)`. On `INVALID_CAPSULE`, focus moves to the field identified by the error's JSON Pointer. On success, `ApprovedCapsule` renders the approved version read-only, lists the recorded corrections (`before` and `after` by field path), and keeps the original model draft inspectable.
- **Regenerate…** confirms, discards the unapproved draft (`checkpoints.discard`), saves the place again (`checkpoints.savePlace`), and opens the payload preview.

### Retention, complete deletion, exports, drift return, and privacy controls (P-05, R-01, U-04, U-05)

- `Database::delete_thread` transactionally removes `corrections`, `model_calls`, `checkpoints`, `artifact_refs`, `events`, and `work_threads` in FK-safe order, deletes any files in `thread_exports_directory(thread_id)`, and runs `PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);` so deleted bytes leave the database file.
- `Database::expire_raw_events` deletes raw `events` older than each project's `retention_days` (default 14) while protecting any event referenced by a checkpoint (`start_event_id`, `end_event_id`, or cited inside `draft_json` / `approved_json`). It runs on app initialization, when saving a project policy, and via `privacy.expireEvents`.
- `ExportService::export_json` and `ExportService::export_markdown` write only `checkpoint.approved` (refusing unapproved checkpoints) into `<data-dir>/exports/<thread_id>/<checkpoint_id>.json` and `.md` alongside a SHA-256 fingerprint.
- `CheckpointService::get_resume` compares the checkpoint's final Git snapshot against a fresh read-only Git snapshot of the project root, reporting `branch_changed`, `head_changed`, `working_tree_changed`, or `unavailable` drift signals in `ApprovedCapsule` before resume actions.
- `CheckpointService::open_artifact` validates that the path belongs to the approved capsule's `artifacts_to_reopen` and resolves inside the project boundary, returning a bounded 32 KiB text preview on explicit click. Next-action instructions are copied only on explicit click (`Copy next action`), and no capsule action ever executes a command.
- `PrivacyScreen` exposes per-project retention days, collector toggles (`capture_git`, `capture_commands`, `capture_file_events`), custom excluded globs, custom redaction regexes, a live non-persisting redaction tester (`privacy.testRedaction`), a single-click **Pause all active capture** action (`threads.pauseAll`), and destructive thread deletion with confirmation.

## 10. Final acceptance checklist

- [x] An approved project cannot escape its canonical boundary.
- [x] Thread lifecycle and acknowledged events persist across restart.
- [x] Git and explicitly wrapped-command events are durable.
- [x] User-authored note labels remain authoritative.
- [x] **Save My Place** works offline.
- [ ] The outbound payload is fully visible and editable before transmission.
- [x] The seeded fake secret never reaches the model payload.
- [x] A real Nemotron call produces the capsule draft.
- [x] Unsupported facts cannot be approved silently.
- [x] The user can correct and approve the capsule without overwriting the model draft.
- [x] Repository drift appears before resume actions.
- [x] No command executes automatically from a capsule.
- [x] Approved JSON and Markdown exports work.
- [x] Retention and complete deletion work.
- [ ] The ten-scenario evaluation report exists.
- [ ] The demo resets in one command.
- [ ] README, license, setup, tests, privacy notes, and limitations are complete.
- [ ] The recorded demo matches the distributable build.
