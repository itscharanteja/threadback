import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";

// Usage: node scripts/demo-step.mjs <attempt-fix|revert-fix|drift> [workspace]
//
//   attempt-fix  the failed attempt: change the retry middleware's backoff.
//                The focused test still fails with the same two provider calls.
//   revert-fix   undo that attempt, leaving the working tree as committed.
//   drift        what happens while the user is away: switch to another
//                branch, commit a change, and leave an uncommitted edit in a
//                file the investigation had not touched, so the branch, HEAD,
//                and status all differ from the checkpoint.
const [step, target] = process.argv.slice(2);
const workspace = target
  ? resolve(target)
  : fileURLToPath(new URL("../fixtures/demo-repo/workspace", import.meta.url));
const middleware = join(workspace, "src/retry-middleware.mjs");

if (!step || !existsSync(join(workspace, ".git"))) {
  console.error("Usage: node scripts/demo-step.mjs <attempt-fix|revert-fix|drift> [workspace]");
  console.error("Run `npm run demo:reset` first if the demo repository does not exist.");
  process.exit(2);
}

function git(...args) {
  const result = spawnSync("git", args, {
    cwd: workspace,
    encoding: "utf8",
    stdio: "pipe",
    env: {
      ...process.env,
      GIT_AUTHOR_NAME: "Reviewer",
      GIT_AUTHOR_EMAIL: "reviewer@threadback.local",
      GIT_AUTHOR_DATE: "2026-01-15T11:30:00+00:00",
      GIT_COMMITTER_NAME: "Reviewer",
      GIT_COMMITTER_EMAIL: "reviewer@threadback.local",
      GIT_COMMITTER_DATE: "2026-01-15T11:30:00+00:00",
      GIT_CONFIG_GLOBAL: "/dev/null",
      GIT_CONFIG_SYSTEM: "/dev/null",
    },
  });
  if (result.status !== 0) {
    console.error(result.stderr || `git ${args[0]} failed`);
    process.exit(result.status ?? 1);
  }
  return result.stdout;
}

switch (step) {
  case "attempt-fix": {
    const source = readFileSync(middleware, "utf8");
    if (!source.includes("backoffMs: 0,")) {
      console.error("The retry middleware has already been changed.");
      process.exit(1);
    }
    writeFileSync(middleware, source.replace("backoffMs: 0,", "backoffMs: 250,"));
    console.log("Changed the retry middleware: backoffMs 0 -> 250.");
    break;
  }
  case "revert-fix":
    git("checkout", "--", "src/retry-middleware.mjs");
    console.log("Reverted the retry middleware change.");
    break;
  case "drift": {
    git("checkout", "--quiet", "-b", "hotfix/review-feedback");
    const readme = join(workspace, "README.md");
    writeFileSync(readme, `${readFileSync(readme, "utf8")}\nReviewer note: lock backends are configured per worker.\n`);
    git("add", "README.md");
    git("commit", "--quiet", "-m", "Address review feedback");
    const lock = join(workspace, "src/lock.mjs");
    writeFileSync(lock, `${readFileSync(lock, "utf8")}\n// TODO(reviewer): share one backend between workers.\n`);
    console.log("Moved to hotfix/review-feedback, committed a change, and left src/lock.mjs edited.");
    break;
  }
  default:
    console.error(`Unknown step "${step}".`);
    process.exit(2);
}
