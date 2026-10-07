# ThreadBack — Under-3-Minute Demo Recording Script

**Target Build:** `target/release/bundle/macos/ThreadBack.app` + `target/release/tb`  
**Demo Repository:** `fixtures/demo-repo/workspace`  
**Duration Target:** ~2 minutes 35 seconds

---

## Pre-Recording One-Command Reset

```bash
npm run demo:reset
export PATH="$PWD/target/release:$PATH"
open target/release/bundle/macos/ThreadBack.app
```

---

## Timestamped Walkthrough (`spec.md` §21)

### 0:00 – 0:25 — Project Approval & Starting the Thread (Steps 1–2)
1. In **ThreadBack.app**, click **Approve a project folder** and select `fixtures/demo-repo/workspace` (`Payments`).
   - Point out the default exclusions (`.env*`, `*.pem`, `*.key`, `fixtures/sensitive/**`) and 14-day raw event retention.
2. Start a new thread:
   - **Title:** `Duplicate capture investigation`
   - **Goal:** `Find why retries create a second provider capture`
   - **Initial Hypothesis:** `Retry middleware retries after provider timeout`
3. Show the initial read-only Git snapshot (`main` @ `a3be682`) in the timeline.

### 0:25 – 0:55 — Explicit Command Capture & Failed Attempt (Steps 3–4)
1. In the terminal inside `fixtures/demo-repo/workspace`, run the focused test via `tb`:
   ```bash
   tb run -- npm test
   ```
   - Point out that the test fails (`expected 1 provider capture, observed 2`) and prints a fake credential (`sk_live_threadback_demo_99f18a4c2d8e4b60b5c7a1d2e3f40506`).
2. Apply the retry-middleware change, rerun the test, and revert it:
   ```bash
   npm run demo:attempt-fix
   tb run -- npm test
   npm run demo:revert-fix
   ```
3. In **ThreadBack.app**, show the command events in the timeline with bounded output and `[REDACTED:secret-key]`.

### 0:55 – 1:25 — Labelled Notes & Offline Save My Place (Steps 5–6)
1. Add three labelled notes in **ThreadBack.app**:
   - **Fact:** `The focused retry test made two provider calls, and changing retry middleware backoff did not change the result.`
   - **Hypothesis:** `A second worker picks up the redelivered job and uses a separate lock backend.`
   - **Question:** `Do both workers share the same lock backend?`
2. Click **Save My Place**.
   - Highlight that the checkpoint draft is saved locally in under 2 seconds with zero network calls.

### 1:25 – 2:00 — Outbound Payload Preview & Nemotron Capsule Generation (Steps 7–9)
1. Click **Review what will be sent…**.
   - Show the **Blocked** badge on the command output that contained the fake `sk_live_...` credential and note that **Generate capsule** is disabled until the blocked entry is unchecked.
   - Uncheck the blocked command entry, inspect the exact outbound JSON and SHA-256 payload fingerprint, and click **Generate capsule**.
2. On the **Resume capsule (draft)** review screen:
   - Show **Next action** at the top, followed by **Verified facts**, **Hypotheses**, and **Failed attempts**.
   - Click a cited `[evt-...]` source button on a verified fact to expand the underlying local evidence inline.
   - Click **Approve capsule** (preserving the immutable model draft alongside your approved capsule).

### 2:00 – 2:35 — Repository Drift Detection & Safe Resume (Steps 10–11)
1. In the terminal, simulate repository changes while away:
   ```bash
   npm run demo:drift
   ```
2. Reopen the thread in **ThreadBack.app**:
   - Point out the **Repository state since checkpoint** panel before any resume actions, listing `branch_changed` (`main` -> `hotfix/review-feedback`), `head_changed`, and `working_tree_changed` (`src/capture-service.mjs`).
   - Click **Open file tests/payment-retry.test.mjs** to preview the artifact read-only, and click **Copy next action** (noting that ThreadBack never executes commands automatically).
3. Click **Resume thread** and run the lock-owner check in the terminal:
   ```bash
   tb run -- npm run test:lock
   ```
   - Show `workerA` (`memory-a`) vs `workerB` (`memory-b`) confirming the hypothesis and passing the lock check.
