# ThreadBack MVP Execution Checklist

**Source:** `plan.md`  
**Owner:** Solo developer  
**Target:** Seven-day macOS-first MVP  
**Final deliverable:** A packaged Tauri 2 macOS application that completes the seeded interruption-and-resume demo, makes a real Nemotron request without leaking the seeded secret, validates provenance, supports user approval, detects repository drift, and passes the ten-scenario evaluation suite.

## Delivery rules

- Work in task-ID order unless all listed dependencies are already complete.
- Mark a task complete only after its `plan.md` acceptance condition passes.
- Do not advance past a gate while its required automated checks are failing.
- Fix boundary escape, secret leakage, unsupported facts, state loss, and automatic execution before lower-severity defects.
- Update `plan.md` first if scope, interfaces, dependencies, or acceptance conditions change.

## Day 1 — Foundation and persistence

**Deliverable:** The Tauri shell runs, Rust and TypeScript contracts agree, SQLite migrates, and project/thread records can be written and read.

- [x] F-01 Scaffold Tauri 2, React, and TypeScript.
- [x] F-02 Establish the Rust workspace, core crate, and CLI crate.
- [x] F-03 Define Rust domain contracts and generated TypeScript bindings.
- [x] F-04 Define typed Tauri commands and the stable error envelope.
- [x] F-05 Add repository scripts, environment example, README skeleton, and license.
- [x] P-01 Create idempotent SQLite migrations and indexes.
- [x] P-02 Implement transactional repositories and timeline pagination.
- [x] R-02a Create the base deterministic payments fixture and reset command.

### Day 1 verification

- [x] `npm` install and documented setup complete successfully.
- [x] Frontend type-check passes.
- [x] Rust workspace check passes.
- [x] Tauri development shell opens.
- [x] A fresh database migrates successfully twice.
- [x] Project and thread persistence smoke test passes.

## Day 2 — Local thread and Git evidence

**Deliverable:** A project-bound work thread captures a read-only Git snapshot, persists its state, and recovers after application restart.

- [x] P-03 Implement and test all legal lifecycle transitions.
- [x] P-04 Commit events before acknowledgement and recover interrupted active threads.
- [x] E-01 Enforce canonical project roots, exclusions, traversal checks, and symlink checks.
- [x] E-02 Capture branch, HEAD, porcelain status, paths, and diff statistics read-only.
- [x] E-05 Implement labelled notes and the chronological evidence timeline.
- [x] U-01 Build the application shell, onboarding, native folder selection, and status indicators.

### Gate G1 — Local thread

- [x] A project can be approved through the desktop UI.
- [x] A thread cannot start without a goal.
- [x] The initial Git snapshot appears in the timeline.
- [x] Restarting the application restores the acknowledged thread and evidence.
- [x] Traversal and outside-root symlink tests pass.

## Day 3 — Explicit command evidence and offline checkpoint

**Deliverable:** Wrapped commands, labelled notes, and Git evidence form a durable, inspectable checkpoint without network access.

- [x] E-03 Implement `tb run -- <command> [args...]` with exit-code forwarding.
- [x] E-04 Bound stdout/stderr and retain head/tail truncation metadata.
- [x] C-03 Select, deduplicate, budget, and persist the checkpoint evidence interval.
- [ ] U-02 Build the active-thread timeline, notes, pause, end, and **Save My Place** workflow.
- [ ] R-02b Complete the resettable command, failure, hypothesis, and drift paths in the fixture.

### Gate G2 — Explicit evidence

- [x] `tb run` records only for an active thread inside its approved project.
- [x] The child process receives literal arguments and its exit code is preserved.
- [ ] Large output remains bounded and visibly marked as truncated.
- [ ] **Save My Place** finishes offline in under two seconds on demo data.
- [ ] The checkpoint survives restart and remains inspectable without a model.

## Day 4 — Privacy and Nemotron

**Deliverable:** The user reviews and approves a redacted payload, and a real Nemotron call returns a source-valid capsule draft without receiving the seeded secret.

- [ ] C-01 Implement deterministic secret, private-key, credential-URL, and home-path redaction.
- [ ] C-02 Classify preview entries as included, excluded, blocked, or local-only.
- [ ] C-04 Build the exact outbound-payload preview and confirmation workflow.
- [ ] M-01 Store the API key only in macOS Keychain and configure endpoint/model ID.
- [ ] M-02 Implement the Token Factory client, cancellation, timeouts, and typed errors.
- [ ] M-03 Create the versioned prompt and complete capsule schema.
- [ ] M-04 Validate shape, provenance, interval ownership, confidence, and artifact paths; allow one deterministic repair attempt.

### Gate G3 — Private model call

- [ ] The seeded fake secret is absent from persistence, diagnostics, and the outbound request.
- [ ] Blocking redaction findings disable generation.
- [ ] No request occurs before explicit payload confirmation.
- [ ] A real Nemotron response validates against the capsule contract.
- [ ] Invalid output receives one repair attempt and then a visible error.
- [ ] Unsupported facts cannot proceed as verified facts.

## Day 5 — Human review, return, and deletion

**Deliverable:** The user can inspect sources, correct and approve a capsule, see repository drift, resume safely, export the result, or completely delete the thread.

- [ ] M-05 Preserve the immutable model draft, approved version, and correction history.
- [ ] U-03 Build source-linked review, editing, relabelling, regeneration, and approval.
- [ ] U-04 Build drift comparison, resume, archive, explicit open/copy, export, and delete actions.
- [ ] U-05 Build privacy, retention, redaction-test, pause-all, and destructive-confirmation controls.
- [ ] P-05 Implement raw-event retention and transactional complete deletion.
- [ ] R-01 Implement approved JSON and Markdown exports.

### Gate G4 — Human review

- [ ] Every verified fact resolves to valid evidence in the checkpoint interval.
- [ ] Relabelling and edits are authoritative and recorded as corrections.
- [ ] Approval does not overwrite the original model draft.

### Gate G5 — Return

- [ ] Branch, HEAD, and status drift appear before resume actions.
- [ ] Artifact opening and command copying require explicit clicks.
- [ ] No capsule action executes a command automatically.
- [ ] JSON and Markdown exports contain only the approved result.
- [ ] Complete deletion leaves no related rows, caches, indexes, or exports.

## Day 6 — Evaluation and hardening

**Deliverable:** All ten prepared scenarios run repeatably; security, provenance, durability, and accessibility failures are resolved or explicitly documented.

- [ ] R-02 Complete the deterministic payments fixture and one-command reset.
- [ ] R-03a Encode all ten scenarios, expectations, forbidden claims, secrets, and acceptable next actions.
- [ ] R-03b Produce metrics for provenance, leakage, next-action agreement, latency, and corrections.
- [ ] R-04a Implement offline, invalid-JSON, missing-artifact, moved-project, conflicting-evidence, huge-output, and crash-recovery states.
- [ ] R-04b Complete keyboard, focus, semantic-label, and non-color-only accessibility checks.

### Day 6 verification

- [ ] Unsupported verified facts after validation: zero.
- [ ] Seeded secret leakage: zero.
- [ ] Outside-root access: zero successful attempts.
- [ ] Acknowledged events lost during restart test: zero.
- [ ] Evaluation results report observed values separately from targets.
- [ ] Known limitations are written down rather than hidden.

## Day 7 — Packaging and release evidence

**Deliverable:** A clean macOS build, evaluation report, complete documentation, and matching under-three-minute demonstration are ready to share.

- [ ] R-05 Produce the macOS development/distributable build.
- [ ] R-05 Complete setup, architecture, privacy, Nemotron, demo, test, limitation, license, and attribution documentation.
- [ ] R-05 Verify installation and setup in a clean environment.
- [ ] R-06 Generate the final evaluation report.
- [ ] R-06 Record the end-to-end demonstration using the packaged build.

### Gate G6 — Release

- [ ] All required automated tests pass from a clean checkout.
- [ ] The demo resets with one documented command.
- [ ] The packaged application completes the Maya resume scenario.
- [ ] The recorded demonstration matches the distributable build.
- [ ] Every non-optional item in the final `plan.md` acceptance checklist is complete.

## Optional cuts, in order

Only cut these when a required gate is at risk:

1. Idle checkpoint suggestion.
2. Tray-menu polish.
3. Open-file convenience action.
4. Markdown export.
5. User-defined redaction patterns.
6. File-change watcher; retain Git snapshots.

Never cut project boundary enforcement, offline checkpointing, exact payload preview, secret redaction, the real Nemotron call, certainty separation, provenance validation, user review and approval, drift detection, pause, complete deletion, or the deterministic demo.
