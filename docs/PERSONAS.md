# ThreadBack User Personas & Real-World Usage Scenarios

ThreadBack is built for engineers and technical builders whose hardest debugging and investigation work is routinely interrupted—by production pages, code reviews, customer calls, context switches across branches, or overnight handoffs.

This document defines five core personas and walks through concrete scenarios showing how each uses ThreadBack from initial investigation to interruption, checkpointing, review, and safe return.

---

## Persona Summary Matrix

| Persona | Role | Typical Interruption Trigger | Primary Value from ThreadBack | Key Feature Exercised |
|---|---|---|---|---|
| **1. Maya** (Primary) | Senior Backend Engineer (Payments) | Urgent PR reviews, incident pages, teammates changing the repo while she is away | Returning to a subtle distributed bug with verified facts, ruled-out attempts, and repository drift already surfaced | `tb run`, secret blocking, provenance-linked facts, repository drift check |
| **2. Arjun** (Secondary) | Solo Founder & Full-Stack Builder | Customer support calls, investor demos, switching to an urgent hotfix branch | Context-switching across features and support without losing unfinished root-cause analysis or leaking live keys | Multi-day pause/resume, offline **Save My Place**, per-entry payload exclusion, Markdown export |
| **3. Priya** (Expansion) | Applied ML & Systems Research Engineer | Long training/benchmark runs, dead-end ablation experiments, noisy 1 MB+ logs | Preserving negative results (*what was tried and reverted*) and separating unproven hypotheses from measured facts | Output head/tail bounding, reverted-attempt preservation, fact-to-hypothesis relabelling |
| **4. Elena** | Staff Security & Platform Engineer | Compliance audits, strict data-handling policies, multi-repo monorepo boundaries | Auditing every byte before it leaves the machine, testing custom redaction patterns, and wiping threads after resolution | Privacy Center, live redaction tester, symlink/boundary rejection, complete deletion + `VACUUM` |
| **5. Marcus** | Site Reliability / On-Call Engineer | Shift handoffs, VPN/network drops during travel, sudden terminal/system crashes | Saving durable local checkpoints offline and recovering cleanly after an unexpected shutdown | Offline checkpointing, startup crash recovery banner, JSON/Markdown handoff export |

---

## 1. Maya — Senior Backend Engineer (Primary Persona)

### Profile
- **Role:** Backend engineer maintaining payment capture, webhook delivery, and ledger reconciliation services.
- **Work style:** Investigates concurrency and idempotency bugs using focused reproduction tests (`npm test`) and targeted diagnostic flags.
- **Pain point:** When pulled into a 45-minute incident review, she comes back to a clean working tree (because she reverted a failed fix before leaving) and cannot remember whether she already tested the retry middleware or whether a teammate merged a hotfix onto the branch.

### Scenario 1A — The Reverted Dead-End & Distributed Lock Investigation
1. **Start Thread:** Maya approves `payments-service` and starts **Duplicate capture investigation** with goal *"Find why retries create a second provider capture"*. ThreadBack records the initial read-only Git snapshot (`main` @ `a3be682`, clean).
2. **Capture Failing Test:** She runs `tb run -- npm test`. The test fails (`expected 1 provider capture, observed 2`), and its debug output includes a seeded test credential (`sk_live_threadback_demo_...`). ThreadBack bounds the output and redacts the secret before writing to SQLite.
3. **Rule Out a Dead End:** Maya edits `src/retry-middleware.mjs` to increase backoff, reruns `tb run -- npm test` (still fails), and reverts `src/retry-middleware.mjs` so her working tree is clean again.
4. **Record Reasoning State:** Before joining a Slack huddle, she adds:
   - **Fact:** *"Changing retry middleware backoff did not reduce the provider call count."*
   - **Hypothesis:** *"A second worker picks up the timed-out job and uses a separate lock backend."*
   - **Question:** *"Do both workers share the same lock backend instance?"*
5. **Save & Preview:** She clicks **Save My Place** (completing in `< 2s`), opens **Review what will be sent…**, unchecks the blocked command entry that printed a credential, and clicks **Generate capsule**.
6. **Review & Approve:** Nemotron returns a draft capsule listing the reverted middleware change under **Failed attempts** (even though `git status` is clean) and proposing `LOCK_DEBUG=1` as the **Next action**. Maya clicks `[evt-2]` to verify the source event inline and approves the capsule.

### Scenario 1B — Returning After Teammate Drift
1. **External Change During Interruption:** While Maya is in her meeting, a teammate checks out `hotfix/review-feedback`, commits a ledger tweak, and leaves `src/capture-service.mjs` modified in the shared dev checkout.
2. **Drift Surfaced First:** When Maya reopens her approved capsule, ThreadBack immediately displays **Repository state since checkpoint** *above* the resume controls:
   - `branch_changed`: `main` → `hotfix/review-feedback`
   - `head_changed`: `a3be682` → `3d6f3be`
   - `working_tree_changed`: `clean` → `1 changed: src/capture-service.mjs`
3. **Safe Continuation:** Because she sees the drift warning before running anything, Maya stashes/switches back to her expected state, clicks **Copy next action**, and runs `tb run -- npm run test:lock` to confirm that `workerA` (`memory-a`) and `workerB` (`memory-b`) were wired to different lock backends.

---

## 2. Arjun — Solo Founder & Full-Stack Builder (Secondary Persona)

### Profile
- **Role:** Solo technical founder juggling backend API development, frontend polish, Stripe billing webhooks, and live customer support calls.
- **Work style:** Switches tasks 6–10 times a day; often leaves a debugging session for 48 hours to handle a customer onboarding issue.
- **Pain point:** His terminal scrollback is gone by the next day, and he is terrified of accidentally sending production webhook secrets or customer URLs to an external LLM.

### Scenario 2A — Sudden Customer Call Mid-Investigation
1. **Context Capture:** Arjun is debugging why a webhook signature check fails only on chunked payloads. He has run two `tb run -- cargo test webhook_chunked` commands and recorded a **Question** note (*"Does the reverse proxy strip the raw body newline?"*).
2. **Instant Offline Save:** A customer calls 10 minutes early. Arjun does not have time to wait for an LLM call or review a payload—he clicks **Save My Place** and closes his laptop in 1 second.
3. **Later Synthesis:** That evening, Arjun reopens ThreadBack, reviews the saved checkpoint draft, previews the outbound payload, and generates the Nemotron capsule when he has 60 seconds to review and approve it.

### Scenario 2B — Relinking a Reorganized Repository
1. **Folder Move:** Over the weekend, Arjun reorganizes his disk from `~/code/billing-api` to `~/projects/saas/billing-api`.
2. **Graceful Moved-Project Handling:** On Monday morning, he opens his approved capsule. ThreadBack keeps the entire approved capsule readable and shows `repository_unavailable` in the drift panel with an inline **Relink moved project folder** control.
3. **Relink & Resume:** Arjun selects `~/projects/saas/billing-api` and clicks **Relink project folder**. ThreadBack canonicalizes the new path, verifies it is a readable Git repo, recomputes the drift check (`No drift`), and lets him open the referenced test file with one click.

---

## 3. Priya — Applied ML & Systems Research Engineer (Expansion Persona)

### Profile
- **Role:** Research engineer tuning quantization kernels and evaluation pipelines.
- **Work style:** Runs noisy benchmark scripts that print hundreds of megabytes of progress bars before failing on the final assertion, and tests multiple competing hypotheses in parallel.
- **Pain point:** Standard AI assistants hallucinate certainty—turning an unverified intuition (*"I think FP8 accumulation overflows in layer 12"*) into a stated fact—or choke on 50 MB of stdout.

### Scenario 3A — Bounded Capture of Noisy 1 MB+ Benchmark Output
1. **Noisy Command Execution:** Priya runs `tb run -- npm run test:noisy` (or a verbose benchmark script). The command streams 1 MB+ of log lines to her terminal in real time and exits with code `1` on the final line.
2. **Head/Tail Preservation:** ThreadBack forwards every byte to Priya's terminal without buffering everything in memory, and stores only the bounded head and tail excerpts (`originalBytes`, `omittedBytes`, `truncated: true`), preserving the exact failure message at the end of the stream.
3. **Checkpoint Budget Discipline:** When Priya clicks **Save My Place**, ThreadBack condenses the output to fit the checkpoint character budget while retaining the source `eventId` and truncation metadata.

### Scenario 3B — Enforcing Epistemic Humility (Fact vs. Hypothesis)
1. **Competing Hypotheses:** Priya records two **Hypothesis** notes (*"Hypothesis A: KV-cache stride alignment"* and *"Hypothesis B: tokenizer padding mask"*) and only one **Fact** note (*"Batch size 1 passes; batch size 4 fails on token 129"*).
2. **Validation & Human Relabelling:**
   - If the model attempts to output a `verified_fact` that cites only a `hypothesis` note, ThreadBack's deterministic validator rejects it and triggers a single repair pass.
   - During **Capsule Review**, Priya notices a verified fact that she considers only partially supported by the log. With one click on **Make hypothesis**, she demotes it to a `medium`-confidence hypothesis.
3. **Authoritative Correction Log:** When Priya clicks **Approve capsule**, ThreadBack keeps the model's original draft immutable and records the exact JSON Pointer diff (`/verified_facts/0` removed, `/hypotheses/1` added) in the checkpoint's correction history.

---

## 4. Elena — Staff Security & Platform Engineer

### Profile
- **Role:** Security-focused infrastructure engineer debugging authentication middleware and certificate rotation tooling.
- **Work style:** Works in repositories containing test fixtures with PEM blocks, internal URLs, and symlinks; operates under strict zero-trust rules for third-party model APIs.
- **Pain point:** She needs verifiable guarantees that no symlink can escape the repository boundary, custom internal token formats are redacted before leaving the machine, and sensitive debugging threads can be permanently purged after an incident closes.

### Scenario 4A — Custom Redaction Rules & Live Redaction Testing
1. **Policy Configuration:** Before starting a thread, Elena opens the **Privacy** screen for her project, adds `internal-ca/**` to the excluded globs, and adds a custom regex pattern `ACME_TOKEN_[A-Z0-9]{16}` to the project's redaction rules.
2. **Non-Persisting Verification:** In the **Live redaction tester**, she pastes a sample log containing `Authorization: Bearer eyJ...` and `ACME_TOKEN_98F12A34B56C78D9` and clicks **Run redaction test**. ThreadBack shows the exact redacted output (`[REDACTED:bearer-token]` and `[REDACTED:custom-1]`) without writing the test string to SQLite.

### Scenario 4B — Symlink Escape Rejection & Complete Deletion
1. **Boundary Enforcement:** A test script creates a symlink `fixtures/external-link` pointing to `/etc/hosts`. When ThreadBack takes a Git snapshot or validates capsule artifact paths, `boundary.rs` canonicalizes the target, detects that it resolves outside the canonical project root, and withholds/rejects it (`OUTSIDE_PROJECT_BOUNDARY`).
2. **Complete Deletion After Resolution:** Once the auth bug is fixed and merged, Elena opens **Privacy -> Thread data deletion**, clicks **Delete** on the investigation thread, and confirms. ThreadBack transactionally deletes all events, checkpoints, corrections, artifact refs, model call diagnostics, and exported files, then executes `PRAGMA wal_checkpoint(TRUNCATE); VACUUM;` so zero bytes of the investigation remain on disk.

---

## 5. Marcus — Site Reliability / On-Call Engineer

### Profile
- **Role:** On-call SRE diagnosing intermittent database connection-pool exhaustion across shifts.
- **Work style:** Often works over flaky tethering while commuting, experiences sudden laptop sleep/restarts during heavy local load tests, and hands off unfinished investigations to the next on-call engineer.
- **Pain point:** Tooling that depends on cloud connectivity loses his place when the network drops, and crashes leave him unsure which commands were actually recorded.

### Scenario 5A — Offline Checkpointing & Token Factory Outage Recovery
1. **Working Offline:** While commuting without Wi-Fi, Marcus reproduces a connection-pool timeout with `tb run`, records his observations, and clicks **Save My Place**. The status bar clearly states `✕ Offline — checkpoints still work locally`, and his full checkpoint draft is saved to SQLite.
2. **Graceful Retry on Outage:** When he first tries **Generate capsule** over a flaky connection (or during a `503` service blip), ThreadBack preserves the checkpoint draft, records no partial state loss, and lets him retry generation once connectivity returns.

### Scenario 5B — Crash Recovery & Shift Handoff Export
1. **Unexpected Termination:** While Marcus's thread is `ACTIVE`, his machine reboots during a kernel panic.
2. **Honest Gap Reporting:** When Marcus reopens ThreadBack, startup recovery (`P-04`) automatically pauses the interrupted thread (`reason: interrupted`) and displays the **Interrupted threads** recovery banner:
   > *"⚠ **Pool exhaustion investigation** was still recording when ThreadBack last stopped, so it was paused. Evidence is complete up to 10:15 PM; nothing was captured between then and 10:20 PM."*
    Capture never resumes silently in the background.
3. **Shift Handoff Export:** After approving his capsule, Marcus clicks **Export Markdown** and **Export JSON**, sharing the clean, redacted, SHA-256-fingerprinted Markdown summary (`<data-dir>/exports/<thread_id>/<checkpoint_id>.md`) with the incoming on-call engineer.
