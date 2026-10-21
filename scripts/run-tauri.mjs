import { spawn } from "node:child_process";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const targetDir = path.join(projectRoot, "target");
const debugDir = path.join(targetDir, "debug");
const lockPath = path.join(targetDir, ".tauri-dev-cleanup.lock");
const tauriCli = path.join(projectRoot, "node_modules", ".bin", "tauri");
const args = process.argv.slice(2);
const isDev = args[0] === "dev";

function processIsRunning(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error.code === "EPERM";
  }
}

async function lockDevSession() {
  await mkdir(targetDir, { recursive: true });
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      await writeFile(lockPath, String(process.pid), { flag: "wx" });
      return;
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      const owner = Number(await readFile(lockPath, "utf8").catch(() => ""));
      if (processIsRunning(owner)) {
        throw new Error(`Another ThreadBack dev session is running (PID ${owner}). Stop it before restarting.`);
      }
      await rm(lockPath, { force: true });
    }
  }
  throw new Error("Could not acquire the ThreadBack dev session lock.");
}

async function removeDebugBuild() {
  await rm(debugDir, { recursive: true, force: true });
  console.log("Removed ThreadBack debug build files from target/debug.");
}

async function main() {
  if (isDev) {
    await lockDevSession();
    try {
      await removeDebugBuild();
    } catch (error) {
      await rm(lockPath, { force: true });
      throw error;
    }
  }

  const env = {
    ...process.env,
    PATH: `${path.join(homedir(), ".cargo", "bin")}${path.delimiter}${process.env.PATH ?? ""}`,
  };
  let requestedSignal = null;
  const child = spawn(tauriCli, args, {
    cwd: projectRoot,
    env,
    stdio: "inherit",
    detached: isDev,
  });

  if (isDev) {
    for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
      process.on(signal, () => {
        if (requestedSignal) return;
        requestedSignal = signal;
        try {
          process.kill(-child.pid, signal);
        } catch {
          child.kill(signal);
        }
      });
    }
  }

  try {
    const result = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("close", (code, signal) => resolve({ code, signal }));
    });
    process.exitCode = result.code ?? (requestedSignal === "SIGINT" || result.signal === "SIGINT" ? 130 : 1);
  } finally {
    if (isDev) {
      try {
        await removeDebugBuild();
      } finally {
        await rm(lockPath, { force: true });
      }
    }
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
