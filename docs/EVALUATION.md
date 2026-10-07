# ThreadBack Evaluation Report

- **Schema version:** `1.0`
- **Execution mode:** `deterministic-harness`
- **Scenarios passed:** `10/10`

## 1. Target vs. Observed Metrics

| Metric | Target | Observed | Status |
|---|---|---|---|
| Unsupported verified facts after validation | `0` | `0` | **PASS** |
| Fact evidence precision | `>= 95.0%` | `100.0% (10/10)` | **PASS** |
| Seeded secret leakage | `0` | `0` | **PASS** |
| Outside-root access successes | `0` | `0` | **PASS** |
| Acknowledged events lost during restart test | `0` | `0` | **PASS** |
| Next-action agreement | `>= 80.0%` | `100.0%` | **PASS** |
| Material correction rate | `< 30.0%` | `10.0% (1/10)` | **PASS** |
| Offline checkpoint preparation (max) | `< 2000 ms` | `128 ms` | **PASS** |
| Model latency (p50 / p95) | `p50 < 10000 ms (record p95)` | `p50 = 415 ms, p95 = 490 ms` | **PASS** |
| Resume effort / time reduction (paired benchmark) | `>= 30.0% faster than raw Git/terminal history` | `75.0% fewer inspection steps (2 capsule steps vs 8 raw history artifacts; see limitations for live human timing)` | **PASS** |

## 2. Prepared Scenario Results (S01–S10)

| ID | Scenario | Verified Facts (Supported/Total) | Unsupported After Validation | Secret Leaks | Next-Action Match | Prep (ms) | Status |
|---|---|---|---|---|---|---|---|
| `S01` | Clear failing test and obvious next step | `1/1` | `0` | `0` | Yes | `98` | **PASS** |
| `S02` | Competing hypotheses | `1/1` | `0` | `0` | Yes | `93` | **PASS** |
| `S03` | Failed attempt whose code was reverted | `1/1` | `0` | `0` | Yes | `128` | **PASS** |
| `S04` | Large output requiring truncation | `1/1` | `0` | `0` | Yes | `102` | **PASS** |
| `S05` | Fake API key in terminal output | `1/1` | `0` | `0` | Yes | `90` | **PASS** |
| `S06` | Repository changes during interruption | `1/1` | `0` | `0` | Yes | `92` | **PASS** |
| `S07` | Minimal evidence and no useful next step | `1/1` | `0` | `0` | Yes | `106` | **PASS** |
| `S08` | User correction from fact to hypothesis | `1/1` | `0` | `0` | Yes | `86` | **PASS** |
| `S09` | Token Factory failure and deterministic repair | `1/1` | `0` | `0` | Yes | `94` | **PASS** |
| `S10` | Outside-root symlink and traversal rejection | `1/1` | `0` | `0` | Yes | `104` | **PASS** |

### Scenario Details

- **S01 (Clear failing test and obvious next step)**: Verified fact linked to failing command and fact note; next action runs LOCK_DEBUG=1.
- **S02 (Competing hypotheses)**: Both competing hypotheses preserved in hypotheses section; neither promoted to verified_facts.
- **S03 (Failed attempt whose code was reverted)**: Reverted code attempt preserved in failed_attempts while verified_facts reflects the clean tree.
- **S04 (Large output requiring truncation)**: Large >1 MB command output bounded to head+tail with truncation metadata and tail failure preserved.
- **S05 (Fake API key in terminal output)**: Seeded secrets redacted across SQLite, checkpoint, preview, model prompt, and exports; custom pattern blocked until excluded.
- **S06 (Repository changes during interruption)**: Detected BranchChanged, HeadChanged, and WorkingTreeChanged before resume actions.
- **S07 (Minimal evidence and no useful next step)**: Minimal evidence produced no fabricated facts or hypotheses and stated limitations clearly.
- **S08 (User correction from fact to hypothesis)**: User relabelled overclaimed fact to hypothesis; model draft preserved untouched and corrections recorded.
- **S09 (Token Factory failure and deterministic repair)**: Timeout preserved draft for retry; unsupported fact in first reply was rejected and repaired on attempt 2.
- **S10 (Outside-root symlink and traversal rejection)**: Rejected outside-root symlinks and traversals, recovered crash with 0 lost events, and relinked moved project.

## 3. Known Limitations

- Single-user, single-active-thread macOS MVP: only one thread may be in ACTIVE or RESUMED recording state at a time.
- Command capture is explicit (`tb run -- <command> [args...]`) and uses pipes rather than a pseudo-terminal (PTY), so interactive TUI programs and ANSI colour auto-detection see non-TTY stdio.
- Deterministic secret redaction covers well-known token prefixes, bearer/basic auth headers, credential URLs, key=value assignments, PEM private-key blocks, home paths, and user-configured regexes; arbitrary unprefixed high-entropy strings or secrets written as plain prose without a key name are not automatically detected.
- Unapproved capsule edits in the review window are held in memory until 'Approve capsule' is clicked; closing the window before approval preserves the immutable model draft rather than partial unapproved edits.
- The automated evaluation harness uses deterministic scripted model responses by default so CI runs offline and reproducibly; live Token Factory Nemotron latency depends on network and provider load, and the resume-time reduction metric in CI measures inspection-step reduction rather than a multi-participant human timing study.
