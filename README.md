# ThreadBack

**ThreadBack** is a private, local-first interruption-recovery assistant for software debugging on macOS. It captures an explicit, auditable trail of Git snapshots, opt-in wrapped commands (`tb run -- <command>`), and user-labelled notes inside one approved project folder, saves offline checkpoints in under two seconds, previews and redacts the exact outbound payload before any network request, and calls **NVIDIA Nemotron** (`nvidia/nemotron-3-super-120b-a12b`) on **Nebius Token Factory** to draft a source-linked **Resume Capsule** that separates verified facts from hypotheses.

When you return after an interruption, ThreadBack compares the repository's current branch, `HEAD`, and working tree against the checkpoint snapshot before presenting any resume actions, and never executes commands automatically.

---

## Table of Contents

1. [Prerequisites & Setup](#1-prerequisites--setup)
2. [Architecture](#2-architecture)
3. [Privacy & Security Model](#3-privacy--security-model)
4. [NVIDIA Nemotron on Nebius Token Factory](#4-nvidia-nemotron-on-nebius-token-factory)
5. [Deterministic Demo Scenario (Under 3 Minutes)](#5-deterministic-demo-scenario-under-3-minutes)
6. [Automated Tests & Ten-Scenario Evaluation](#6-automated-tests--ten-scenario-evaluation)
7. [Known Limitations](#7-known-limitations)
8. [License & Attribution](#8-license--attribution)

---

## 1. Prerequisites & Setup

### System Requirements

- **OS:** macOS (macOS Keychain is used for write-only API-key storage)
- **Node.js:** 22.12 or newer (`npm` 10+)
- **Rust:** Stable toolchain (`cargo`, `rustc`)
- **Xcode Command Line Tools:** `xcode-select --install`
- **Git:** 2.30+

### Clean-Environment Installation

```bash
# 1. Install frontend dependencies
npm install

# 2. Seed and reset the deterministic demo repository
npm run demo:reset

# 3. Run type generation check, TypeScript check, Rust workspace check, and UI tests
npm run check

# 4. Run the full Rust unit, integration, security, and evaluation test suites
npm test
```

### Running in Development Mode

```bash
npm run tauri dev
```

To use the `tb` command wrapper from your terminal during an active thread:

```bash
cargo build -p tb-cli
export PATH="$PWD/target/debug:$PATH"
```

### Building the Distributable macOS Application

```bash
npm run tauri:build
```

This produces:
- **macOS Application Bundle:** `target/release/bundle/macos/ThreadBack.app`
- **Release CLI Wrapper:** `target/release/tb`

---

## 2. Architecture

ThreadBack is organized as a Cargo workspace paired with a React 19 + TypeScript frontend inside a Tauri 2 desktop shell:

```
threadback/
├── crates/
│   ├── threadback-core/       # Core Rust library: domain types, SQLite WAL storage,
│   │                          # project boundary enforcement, Git & command collectors,
│   │                          # redaction engine, checkpoint selection, Token Factory
│   │                          # client, capsule validation/repair, exports, & evaluation
│   └── tb-cli/                # Explicit command-wrapper binary (`tb run -- <cmd>`)
│                              # compiled WITHOUT network or Keychain dependencies
├── src-tauri/                 # Tauri 2 desktop shell & typed IPC command handlers
├── src/                       # React 19 + TypeScript UI (Home, Thread, Payload Preview,
│                              # Capsule Review, Approved Resume, Privacy, Model Settings)
├── fixtures/
│   ├── demo-repo/             # Deterministic payments service fixture & reset scripts
│   └── evaluation/            # 10-scenario evaluation specifications & latest JSON report
└── docs/
    ├── DEMO_SCRIPT.md         # Under-3-minute demo recording script
    ├── EVALUATION.md          # Generated 10-scenario evaluation report (observed vs target)
    └── PERSONAS.md            # User personas (Maya, Arjun, Priya, Elena, Marcus) & scenarios
```

### Key Architectural Invariants

- **Shared Rust-to-TypeScript Contracts (`ts-rs`):** All wire types in `crates/threadback-core/src/domain/mod.rs` export directly to `src/bindings/contracts.ts` (`npm run bindings` / `npm run check:bindings`).
- **Commit-Before-Acknowledge Durability:** SQLite runs in `WAL` mode with `synchronous = NORMAL` and `foreign_keys = ON`. Every event is committed to disk before its command returns. On startup, any thread left in `ACTIVE` or `RESUMED` after an unexpected termination is paused with reason `interrupted` and surfaces a recovery banner showing the unrecorded interval.
- **Air-Gapped CLI (`tb`):** The `tb-cli` binary shares the SQLite database and redaction/bounding rules with `threadback-core`, but does not link the HTTP client or Keychain adapter.

---

## 3. Privacy & Security Model

### Strict Project Boundaries
- **Canonical Root Allowlisting:** Project folders are canonicalized on registration (`std::fs::canonicalize`). Every file path, artifact reference, or `tb run` working directory is verified to resolve inside the canonical root.
- **Traversal & Symlink Escape Rejection:** Relative paths containing `..`, absolute paths outside the project root, and symlinks inside the project that resolve to targets outside the project root are deterministically rejected (`OUTSIDE_PROJECT_BOUNDARY`).
- **Default Exclusions:** `.git/**`, `.env`, `.env.*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*secret*`, `*credentials*`, `id_rsa*`, `id_ed25519*`, `node_modules/**`, `target/**`, and `dist/**` are excluded by default and customizable per project.

### Explicit Opt-In Evidence Collection
- **Never Captured:** Screenshots, keystrokes, window titles, clipboard history, background shell history, environment variable values, or files outside the approved project root.
- **Read-Only Git Snapshots:** Collected using fixed `std::process::Command` argument arrays (`--no-optional-locks`, `core.fsmonitor=false`) with zero shell interpolation.
- **Explicit Command Wrapping (`tb run -- <cmd>`):** Records commands only when an approved project has an `ACTIVE` or `RESUMED` thread. Output streams are bounded (default 16 KiB head / 16 KiB tail, ceiling 64 KiB per stream) with explicit `originalBytes`, `omittedBytes`, and `truncated` metadata. Binary streams are counted by byte length and never stored.

### Deterministic Redaction & Exact Outbound Payload Preview
- **Two-Stage Redaction:** Runs before persistence on command output and again when building the outbound model payload and persisting capsules/corrections.
- **Built-In Rules:** Bearer tokens, OpenAI/Stripe/GitHub/AWS-style secret keys (`sk_live_...`, `ghp_...`, `AKIA...`), PEM private-key blocks, credential-bearing URLs (`https://user:pass@host`), absolute home-directory paths (`/Users/<name>` -> `~`), and user-defined per-project regex patterns.
- **Blocking Gate:** If a secret finding is detected in an entry, that entry is marked `blocked` (`hasBlockingFindings = true`) and **Generate capsule** is disabled both in the UI (`aria-disabled="true"`) and on the server (`PAYLOAD_BLOCKED`) until the user excludes the entry.
- **Exact Payload Hash Authorization:** `checkpoints.generate` requires the SHA-256 `reviewedHash` of the exact `payloadJson` shown in the preview (`PAYLOAD_CHANGED` if anything changed).

### Retention & Complete Deletion
- **Raw Event Retention:** Raw timeline events older than the project's retention window (default 14 days) are expired while preserving events referenced by saved checkpoints.
- **Transactional Complete Deletion:** Deleting a thread removes all rows across `corrections`, `model_calls`, `checkpoints`, `artifact_refs`, `events`, and `work_threads` in a single transaction, deletes any exported `.json`/`.md` files in `exports/<thread_id>/`, and runs `PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);`.

---

## 4. NVIDIA Nemotron on Nebius Token Factory

ThreadBack uses **NVIDIA Nemotron** (`nvidia/nemotron-3-super-120b-a12b`) hosted on **Nebius Token Factory** (`https://api.tokenfactory.nebius.com/v1`) exclusively to synthesize the structured **Resume Capsule** from the user-approved evidence payload.

1. **Write-Only Keychain Storage:** Your Nebius Token Factory API key is stored in the macOS Keychain (`security` framework / service `com.threadback.desktop.tokenfactory`). It is never written to SQLite, logs, diagnostics, or exports, and cannot be read back by the renderer.
2. **Untrusted-Evidence Prompt (`resume-capsule-v1`):** Repository text, command output, and notes are wrapped as untrusted data inside `<untrusted_checkpoint_evidence_json>` with explicit anti-prompt-injection instructions and no tool/function-calling access.
3. **Deterministic Validation & Single Repair Pass:**
   - Every response is validated against the `ResumeCapsule` contract (`schema_version: "1.0"`, `goal`, `checkpoint_summary`, `verified_facts`, `hypotheses`, `failed_attempts`, `unresolved_questions`, `relevant_artifacts`, `next_action`, `limitations`).
   - Every `verified_fact` must cite at least one valid `source_event_id` from the sent checkpoint interval and cannot rest solely on `hypothesis`, `question`, or `next_step` notes.
   - Every `relevant_artifact.relative_path` must stay within the canonical project boundary and outside excluded globs.
   - If the first model response fails schema or provenance validation, ThreadBack performs **at most one** deterministic repair call feeding back the exact validation errors. If the second response still fails, the checkpoint remains safely inspectable locally and the error is surfaced.
4. **Immutable Model Draft vs. User-Approved Capsule:** The model's response is stored once in `checkpoints.draft_json`. User edits, deletions, and fact-to-hypothesis relabellings are compared against `draft_json` to record field-path `Correction` entries (`before` and `after`) and stored separately in `checkpoints.approved_json`.

---

## 5. Deterministic Demo Scenario (Under 3 Minutes)

The repository includes a deterministic payments-service debugging scenario in `fixtures/demo-repo/` (see [fixtures/demo-repo/DEMO.md](file:///Users/charansriteja/Documents/threadback/fixtures/demo-repo/DEMO.md) for full details).

### One-Command Reset

```bash
npm run demo:reset
```

This recreates `fixtures/demo-repo/workspace` at fixed commit `a3be682c81f5df53f64f11aa8663169870aa9701` on branch `main`.

### 11-Step Walkthrough (`spec.md` §21)

| Step | Action | Command / UI Action |
|---|---|---|
| **1–2** | Approve `fixtures/demo-repo/workspace` and start thread **Duplicate capture investigation** with goal *"Find why retries produce a second provider capture"* | Home -> Approve Project -> Start Thread |
| **3** | Run the focused retry test (fails with `2` provider captures and prints a fake live credential `sk_live_threadback_demo_...`, which is redacted before persistence and blocked from outbound payloads) | `tb run -- npm test` |
| **4** | Apply the retry-middleware change, rerun the failing test, and revert the change | `npm run demo:attempt-fix` -> `tb run -- npm test` -> `npm run demo:revert-fix` |
| **5** | Record a `fact` note (*"The focused retry test made two provider calls"*), a `hypothesis` note (*"A second worker bypasses the lock"*), and a `question` note (*"Do both workers share the same lock backend?"*) | Thread Screen -> Add Note |
| **6** | Save checkpoint locally (completes offline in `< 2s`) | Press **Save My Place** |
| **7** | Open **Review what will be sent…**, inspect the exact outbound JSON and SHA-256 hash, and verify that blocked secret entries cannot be transmitted | Press **Review what will be sent…** -> uncheck blocked command output |
| **8** | Generate the Nemotron Resume Capsule | Press **Generate capsule** |
| **9** | Inspect source events inline for each verified fact, optionally edit or relabel claims, and approve the capsule | Press cited `[evt-...]` buttons -> Press **Approve capsule** |
| **10** | Simulate repository drift while away (switches to `hotfix/review-feedback`, commits a change, and dirties `src/capture-service.mjs`) | `npm run demo:drift` |
| **11** | Reopen the approved capsule, review the branch/HEAD/working-tree drift warnings before resume actions, click **Copy next action** or **Open file**, resume the thread, and run the lock-owner check | `tb run -- npm run test:lock` |

You can also run the entire 11-step scenario headlessly through the automated integration test:

```bash
cargo test -p tb-cli --test demo_path
```

---

## 6. Automated Tests & Ten-Scenario Evaluation

### Validation Commands

| Command | Description |
|---|---|
| `npm run check` | Verifies Rust->TS contract bindings (`check:bindings`), TypeScript compilation (`typecheck`), Rust workspace compilation (`check:rust`), and all 90 React UI tests (`test:ui`). |
| `npm test` | Runs all Rust unit, integration, security, CLI demo-path, and 10-scenario evaluation tests across `threadback-core`, `tb-cli`, and `threadback-desktop`. |
| `npm run eval` | Executes the 10-scenario evaluation harness (`S01`–`S10`) and writes [docs/EVALUATION.md](file:///Users/charansriteja/Documents/threadback/docs/EVALUATION.md) and [fixtures/evaluation/latest-report.json](file:///Users/charansriteja/Documents/threadback/fixtures/evaluation/latest-report.json). |
| `npm run format` | Checks Rust formatting across the workspace (`cargo fmt --all -- --check`). |

### Ten-Scenario Evaluation Summary (`docs/EVALUATION.md`)

All 10 prepared scenarios (`S01` through `S10`) execute repeatably via `npm run eval`, reporting observed values separately from targets:

| Metric | Target | Observed | Status |
|---|---|---|---|
| Unsupported verified facts after validation | `0` | `0` | PASS |
| Provenance validity of verified facts | `100%` | `100.0% (21/21)` | PASS |
| Fact evidence precision on prepared scenarios | `>= 95%` | `100.0%` | PASS |
| Seeded secret leakage (persisted, logs, outbound) | `0` | `0 characters` | PASS |
| Outside-root access attempts succeeded | `0` | `0` | PASS |
| Acknowledged events lost during forced restart test | `0` | `0` | PASS |
| Next-action agreement across 10 scenarios | `>= 8/10 (80%)` | `10/10 (100.0%)` | PASS |
| Uncorrected capsules requiring no material correction | `>= 7/10 (<= 30% correction rate)` | `8/10 (20.0% material correction rate)` | PASS |

See [docs/EVALUATION.md](file:///Users/charansriteja/Documents/threadback/docs/EVALUATION.md) for the per-scenario breakdown.

---

## 7. Known Limitations

1. **macOS-First Platform Adapters:** API-key storage uses the macOS Keychain (`security` CLI / Keychain Services). Linux Secret Service and Windows Credential Manager adapters are out of scope for this MVP.
2. **Explicit Command Wrapping (`tb run --`):** Terminal commands are captured only when invoked explicitly through `tb run -- <command> [args...]`. ThreadBack intentionally avoids shell hooks or pty interception so it never captures global shell history or ambient terminal activity.
3. **No Background File Watcher:** File changes are captured through read-only Git snapshots (initial snapshot, manual **Take Git snapshot**, and final checkpoint snapshot) rather than a background filesystem daemon.
4. **Window-Local Draft Edits Before Approval:** Working edits made in the Capsule Review screen prior to clicking **Approve capsule** live in window state; closing the window before approving preserves the untouched model draft (`checkpoint.draft`) so review can be restarted cleanly.
5. **Evaluation Harness Transport:** By default, `npm run eval` runs against a deterministic local loopback OpenAI-compatible server so CI and offline reviewers can reproduce all 10 scenarios (including simulated `503` outages and malformed-JSON repair) without spending Token Factory credits. Live Token Factory calls are exercised through the desktop **Model** settings and **Generate capsule** workflow.

---

## 8. License & Attribution

### License

Released under the [MIT License](file:///Users/charansriteja/Documents/threadback/LICENSE).

### Attribution

- **Model:** [NVIDIA Nemotron (`nvidia/nemotron-3-super-120b-a12b`)](https://build.nvidia.com) served via [Nebius Token Factory](https://tokenfactory.nebius.com).
- **Desktop Framework:** [Tauri 2](https://v2.tauri.app) (Apache-2.0 / MIT).
- **Frontend:** [React 19](https://react.dev) and [Vite](https://vite.dev) (MIT).
- **Storage & Serialization:** [SQLite](https://www.sqlite.org) via [`rusqlite`](https://github.com/rusqlite/rusqlite), [`serde`](https://serde.rs), and [`ts-rs`](https://github.com/Aleph-Alpha/ts-rs) (MIT / Apache-2.0).
