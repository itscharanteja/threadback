# Demo script

The seeded demonstration from `spec.md` §21, with the exact commands. Every
step below is also run, headlessly, by `crates/tb-cli/tests/demo_path.rs`.

## Prepare

```bash
npm run demo:reset          # recreates fixtures/demo-repo/workspace (same commit hash every time)
cargo build -p tb-cli       # builds target/debug/tb
npm run tauri dev
```

In ThreadBack: approve `fixtures/demo-repo/workspace`, then start a thread.

- Title: `Duplicate capture investigation`
- Goal: `Find why retries produce a second provider capture`

Run the commands below **from inside the workspace folder**, using
`../../../target/debug/tb` (or put `target/debug` on your `PATH`).

## Steps

| # | Do | Expect |
|---|----|--------|
| 1 | `tb run -- npm run test:retry` | Fails: `2 !== 1`, "expected exactly one provider call". The output also contains the fake credential `nbf_demo_7F4A91C2_DO_NOT_SEND`. |
| 2 | `npm run demo:attempt-fix` (from the repository root), then step 1 again | The middleware backoff changed, and the test fails exactly the same way. |
| 3 | Add notes: **Fact** "The focused retry test made two provider calls."; **Hypothesis** "A second worker bypasses the distributed lock."; **Question** "Do both workers share the same lock backend?" | Each appears in the timeline with its label. |
| 4 | **Save My Place** | A checkpoint draft appears in well under two seconds. |
| 5 | (Later) preview the outbound payload | The fake credential must be blocked. |
| 6 | `npm run demo:drift` (from the repository root) | While you were away: another branch, a new commit, and an edited file. |
| 7 | Resume from the lock-owner check: `LOCK_DEBUG=1 tb run -- npm run test:retry` | The log shows `worker-a` on `lock-backend-1` and `worker-b` on `lock-backend-2`: the workers do not share a lock. |

## Other cases

- `npm run demo:revert-fix` undoes the middleware attempt (the "failed attempt whose code was reverted" case). The evidence of the attempt stays in the thread.
- `tb run -- npm run test:noisy` prints about 1 MB, then fails on the last line (the large-output case). Only a head and tail excerpt is stored, and the timeline says output was truncated.
- `npm run test:lock` (inside the workspace) is the success check for the next action: it passes once both workers share one lock backend.

## Expected results

- `git rev-parse HEAD` after `npm run demo:reset` is always `a138e4604cffa629e62920eb722f9c62c9de3bb0`.
- The retry middleware never retries a timeout, so changing its policy cannot fix the defect. The workers are given separate lock backends in `src/payment-retry.mjs`.
