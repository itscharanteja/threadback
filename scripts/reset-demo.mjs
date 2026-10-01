import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const template = fileURLToPath(new URL("../fixtures/demo-repo/template", import.meta.url));
const workspace = fileURLToPath(new URL("../fixtures/demo-repo/workspace", import.meta.url));

if (!existsSync(template)) {
  console.error("Demo template is missing.");
  process.exit(2);
}

rmSync(workspace, { recursive: true, force: true });
mkdirSync(workspace, { recursive: true });
cpSync(template, workspace, { recursive: true });

for (const args of [
  ["init", "--quiet"],
  ["config", "user.name", "ThreadBack Demo"],
  ["config", "user.email", "demo@threadback.local"],
  ["add", "."],
  ["commit", "--quiet", "-m", "Seed duplicate capture investigation"],
]) {
  const result = spawnSync("git", args, {
    cwd: workspace,
    encoding: "utf8",
    stdio: "pipe",
  });
  if (result.status !== 0) {
    console.error(result.stderr || `git ${args[0]} failed`);
    process.exit(result.status ?? 1);
  }
}

console.log(`Demo repository reset at ${workspace}`);
