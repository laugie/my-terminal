import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function run(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, args, { cwd: root, stdio: "inherit" });
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (code === 0) {
        resolve();
        return;
      }
      reject(new Error(`node exited with ${code ?? signal}`));
    });
  });
}

const outDir = await mkdtemp(path.join(tmpdir(), "my-terminal-frontend-"));
try {
  await run([
    path.join(root, "node_modules", "typescript", "lib", "tsc.js"),
    "-p",
    "tsconfig.test.json",
    "--outDir",
    outDir,
  ]);
  await run([
    "--test",
    "--test-reporter",
    "tap",
    path.join(outDir, "write-chunks.test.js"),
    path.join(outDir, "write-queue.test.js"),
  ]);
} finally {
  await rm(outDir, { recursive: true, force: true });
}
