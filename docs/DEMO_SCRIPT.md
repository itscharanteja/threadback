# ThreadBack Product Video & Step-by-Step Demo Guide

- **Format:** 16:9 Screen recording with voiceover
- **Target Duration:** 2:40 – 2:50 (Strictly under the Devpost 3:00 limit)
- **Target Build:** `target/release/bundle/macos/ThreadBack.app` + `target/release/tb`
- **Demo Workspace Fixture:** `fixtures/demo-repo/workspace`
- **Core Message:** *Leave the problem. Keep the thread.*

---

## 1. Pre-Recording Setup Checklist

Run these commands from the `threadback` repository root to ensure a completely clean, reproducible environment:

```bash
# 1. Build the production release desktop application and CLI wrapper
npm run tauri:build

# 2. Reset the deterministic payments repository fixture
npm run demo:reset

# 3. Add the release CLI wrapper to PATH
export PATH="$PWD/target/release:$PATH"

# 4. Launch the release application
open target/release/bundle/macos/ThreadBack.app
```

> [!IMPORTANT]
> **API Key Setup (Off-Camera):**  
> Before recording, click the menu bar item, navigate to **Privacy & Model Settings**, and enter your Nebius Token Factory API key (`https://api.tokenfactory.nebius.com/v1`). It is stored securely in the macOS Keychain and will remain cached during the demo.

---

## 2. Presenter Step-by-Step Operator Guide

Follow these sequential operator steps during recording. Keep mouse clicks deliberate and terminal windows clean.

### Phase 1: Project Setup & Starting a Thread (0:00 – 0:39)

| Step | Location | Action | Inputs / Details |
|---|---|---|---|
| **1.1** | Terminal | Show failing terminal test | `cd fixtures/demo-repo/workspace && npm run test:retry` (shows 2 captures instead of 1) |
| **1.2** | macOS Menu Bar | Open popover | Click the **EvidenceList** menu bar icon to reveal the ThreadBack popover |
| **1.3** | ThreadBack UI | Choose project folder | Click **Choose a folder…** and select `fixtures/demo-repo/workspace` (`Payments`) |
| **1.4** | ThreadBack UI | Fill in thread details | **Title:** `Duplicate capture investigation`<br>**Intended outcome:** `Find why retries produce a second provider capture`<br>**Initial idea:** `A second worker may bypass the same lock` |
| **1.5** | ThreadBack UI | Start recording | Click **Start recording** → Popover shifts to compact **Recording** view |

---

### Phase 2: Evidence Capture & Secret Redaction (0:39 – 1:04)

| Step | Location | Action | Inputs / Details |
|---|---|---|---|
| **2.1** | Terminal | Run test via wrapper | `tb run -- npm run test:retry` |
| **2.2** | Terminal | Observe failure & key | Note test failure (`assertion failed: 1 === 2`) and simulated API key output |
| **2.3** | ThreadBack UI | Inspect timeline | Open popover → Notice new command event in **Activity · newest first** with bounded output and masked credential (`[REDACTED:secret-key]`) |

---

### Phase 3: Notes & Instant Offline Checkpoint (1:04 – 1:27)

| Step | Location | Action | Inputs / Details |
|---|---|---|---|
| **3.1** | ThreadBack UI | Add Verified Fact note | Type: `Fact`<br>Content: `The retry test made two provider calls.` → Click **Add note** |
| **3.2** | ThreadBack UI | Add Hypothesis note | Type: `Idea`<br>Content: `A second worker may use a different lock backend.` → Click **Add note** |
| **3.3** | ThreadBack UI | Save local checkpoint | Click **Save my place** |
| **3.4** | ThreadBack UI | Verify local status | Hold on **Checkpoint saved locally** and **Nothing has been sent to a model** badges |

---

### Phase 4: Privacy Gate & Nemotron Capsule Generation (1:27 – 1:55)

| Step | Location | Action | Inputs / Details |
|---|---|---|---|
| **4.1** | ThreadBack UI | Review AI sharing | Click **Review AI sharing** |
| **4.2** | ThreadBack UI | Inspect privacy gate | Point out that the command entry with detected credentials has a red **Blocked** badge and cannot be sent |
| **4.3** | ThreadBack UI | Inspect SHA-256 payload | Click **Inspect exact payload and fingerprint** to preview the raw sanitized JSON and hash |
| **4.4** | ThreadBack UI | Generate draft | Click **Generate draft summary** (sends payload to NVIDIA Nemotron on Nebius Token Factory) |

---

### Phase 5: Capsule Review, Citations & Human Approval (1:55 – 2:18)

| Step | Location | Action | Inputs / Details |
|---|---|---|---|
| **5.1** | ThreadBack UI | Review draft structure | Show separate sections: **Verified facts**, **Idea to check**, **Failed attempts**, and **Suggested next action** |
| **5.2** | ThreadBack UI | Click citation source | Click a `[evt-...]` pill next to a fact to expand and inspect the underlying local terminal output |
| **5.3** | ThreadBack UI | Adjust & Approve | (Optional) Click **Edit next action** to tweak wording, then click **Approve summary** |

---

### Phase 6: Drift Detection & Safe Return (2:18 – 2:48)

| Step | Location | Action | Inputs / Details |
|---|---|---|---|
| **6.1** | Terminal | Simulate repository drift | In repository root: `npm run demo:drift`<br>*(Switches branch to `hotfix/review-feedback`, makes a commit, and dirties working tree)* |
| **6.2** | ThreadBack UI | Reopen thread | Open ThreadBack popover → Navigate to **Threads** → Click **Duplicate capture investigation** |
| **6.3** | ThreadBack UI | Inspect drift warning | Highlight the **Since the checkpoint** panel showing: `branch_changed`, `head_changed`, and `working_tree_changed` |
| **6.4** | ThreadBack UI | Copy Next Action | Click **Copy next action** (ThreadBack never executes commands without consent) |
| **6.5** | ThreadBack UI | Resume | Click **Resume thread** to return to active tracking |

---

## 3. Synchronized Timeline & Voiceover Script (0:00 – 2:48)

### 0:00 – 0:16 — The Interruption
* **Visual:** Open on terminal window showing a focused unit test failure. Pan/cut to the macOS menu bar. Click the ThreadBack icon; the popover slides down cleanly.
* **On-Screen Callout:** `Where was I?`
* **Voiceover:**
  > *"Debugging rarely ends at a clean stopping point. A test fails, an idea emerges, and then a meeting pulls you away. Coming back means rebuilding your entire mental model from scratch."*

### 0:16 – 0:39 — Give the Work a Thread
* **Visual:** In **Start a thread**, select `fixtures/demo-repo/workspace`. Fill in Title (`Duplicate capture investigation`), Intended Outcome, and Initial Idea. Click **Start recording**. Show the compact **Recording** view.
* **Voiceover:**
  > *"ThreadBack lives quietly in the Mac menu bar. Choose your Git project once, start a thread around one problem, and keep your investigation close without opening a heavy full-screen workspace."*

### 0:39 – 1:04 — Explicit Evidence & Automatic Redaction
* **Visual:** In terminal, execute `tb run -- npm run test:retry`. Show the failure assertion in terminal, then open ThreadBack's timeline showing the bounded output with `[REDACTED:secret-key]`.
* **Voiceover:**
  > *"Run tests through `tb run`, and ThreadBack captures the command, result, and Git context. Sensitive credentials and high-volume noise are automatically redacted. Recording is strictly explicit—ThreadBack never records keystrokes or screens in the background."*

### 1:04 – 1:27 — Instant Offline Checkpoint
* **Visual:** Add a **Fact** note and an **Idea** note. Click **Save my place**. Hold on the green badge: **Checkpoint saved locally** and **Nothing has been sent to a model**.
* **On-Screen Callout:** `Saved locally in 99ms. Ready when you return.`
* **Voiceover:**
  > *"Capture what you know and what you only suspect. Then save your place. Checkpoints save to local SQLite in under a hundred milliseconds with zero network requests—allowing you to close your laptop and leave immediately."*

### 1:27 – 1:55 — Review Before AI Sharing
* **Visual:** Click **Review AI sharing**. Highlight the blocked credential entry. Expand **Inspect exact payload and fingerprint** showing the audited JSON payload and SHA-256 hash. Click **Generate draft summary**.
* **Voiceover:**
  > *"When you're ready for synthesis, you stay in complete control. ThreadBack displays the exact JSON payload and SHA-256 fingerprint before sending. Entries with sensitive findings are blocked at the gate. Only approved evidence is sent to NVIDIA Nemotron on Nebius Token Factory."*

### 1:55 – 2:18 — Verifiable Human Approval
* **Visual:** Showcase the returned Resume Capsule draft: **Verified facts**, **Idea to check**, and **Suggested next action**. Click a cited source badge `[evt-1]` to reveal the local evidence snippet. Click **Approve summary**.
* **Voiceover:**
  > *"Nemotron synthesizes a structured Resume Capsule, strictly separating verified facts from hypotheses. Every fact links directly to verified local evidence. You can edit the next action and approve it, preserving your final decision alongside the raw model draft."*

### 2:18 – 2:48 — Drift Detection & Safe Return
* **Visual:** Run `npm run demo:drift` in the terminal. Reopen ThreadBack. Show the **Since the checkpoint** warning flagging branch, HEAD, and unstaged changes. Click **Copy next action**, then **Resume thread**.
* **End Card:** `ThreadBack` · *Leave the problem. Keep the thread.*
* **Voiceover:**
  > *"When you return after lunch or the next morning, the repo may have drifted. ThreadBack detects changed branches, commits, and modified working trees before you run anything. Copy your next action, resume your thread, and dive back in without missing a beat. ThreadBack: Leave the problem. Keep the thread."*

---

## 4. Post-Recording Quality & Submission Checklist

- [ ] **Total Runtime:** Verify the edited cut is between **2:30 and 2:55** (Must not exceed 3:00:00).
- [ ] **Resolution:** 1080p or 4K at 60fps (16:9 aspect ratio).
- [ ] **Audio Balance:** Clear narration centered at -14 LUFS to -16 LUFS with no background hiss or fan noise.
- [ ] **Privacy Check:** Ensure no real API keys, email addresses, or internal paths appear in terminal prompts.
- [ ] **Upload:** Upload to YouTube as **Unlisted** (or Vimeo/Loom) and verify playback in an incognito window before pasting into Devpost.
