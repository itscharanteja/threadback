import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

// Usage: node scripts/reset-demo.mjs [target-folder]
// The default target is fixtures/demo-repo/workspace.
const template = fileURLToPath(new URL("../fixtures/demo-repo/template", import.meta.url));
const defaultWorkspace = fileURLToPath(new URL("../fixtures/demo-repo/workspace", import.meta.url));
const workspace = process.argv[2] ? resolve(process.argv[2]) : defaultWorkspace;

if (!existsSync(template)) {
  console.error("Demo template is missing.");
  process.exit(2);
}

rmSync(workspace, { recursive: true, force: true });
mkdirSync(workspace, { recursive: true });
cpSync(template, workspace, { recursive: true });

// Fixed identity and dates make the commit hash identical on every reset.
const FIXED_DATE = "2026-01-15T09:00:00+00:00";
const environment = {
  ...process.env,
  GIT_AUTHOR_NAME: "ThreadBack Demo",
  GIT_AUTHOR_EMAIL: "demo@threadback.local",
  GIT_AUTHOR_DATE: FIXED_DATE,
  GIT_COMMITTER_NAME: "ThreadBack Demo",
  GIT_COMMITTER_EMAIL: "demo@threadback.local",
  GIT_COMMITTER_DATE: FIXED_DATE,
  GIT_CONFIG_GLOBAL: "/dev/null",
  GIT_CONFIG_SYSTEM: "/dev/null",
};

for (const args of [
  ["init", "--quiet", "--initial-branch=main"],
  ["config", "user.name", "ThreadBack Demo"],
  ["config", "user.email", "demo@threadback.local"],
  ["add", "."],
  ["commit", "--quiet", "-m", "Seed duplicate capture investigation"],
]) {
  const result = spawnSync("git", args, { cwd: workspace, encoding: "utf8", stdio: "pipe", env: environment });
  if (result.status !== 0) {
    console.error(result.stderr || `git ${args[0]} failed`);
    process.exit(result.status ?? 1);
  }
}

console.log(`Demo repository reset at ${workspace}`);
