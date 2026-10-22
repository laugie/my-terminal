import { spawn } from "node:child_process";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const outDir = path.join(root, "dist-test", "diagram-preview-build");

await rm(outDir, { recursive: true, force: true });
await mkdir(outDir, { recursive: true });

await new Promise((resolve, reject) => {
  const child = spawn(
    process.execPath,
    [
      path.join(root, "node_modules", "typescript", "lib", "tsc.js"),
      "-p",
      "tsconfig.test.json",
      "--outDir",
      outDir,
    ],
    { cwd: root, stdio: "inherit" },
  );
  child.on("error", reject);
  child.on("exit", (code, signal) => {
    if (code === 0) {
      resolve();
      return;
    }
    reject(new Error(`tsc exited with ${code ?? signal}`));
  });
});

const { buildDiagram, renderDiagramSvg } = await import(
  pathToFileURL(path.join(outDir, "diagram.js")).href
);
const example = JSON.parse(await readFile(path.join(root, "topology.example.json"), "utf8"));
const view = {
  configPath: "topology.example.json",
  state: "ok",
  regions: example.regions.map((region) => ({
    name: region.name,
    devices: region.devices.map((device) => ({
      name: device.name,
      host: device.host,
      port: device.port ?? 22,
      user: device.user,
      mobile: device.mobile === true,
      presence: "none",
      ...(device.type !== undefined ? { type: device.type } : {}),
      ...(device.jump !== undefined ? { jump: device.jump } : {}),
    })),
  })),
};

const svg = renderDiagramSvg(buildDiagram(view));
const destDir = path.join(root, "dist-test");
await mkdir(destDir, { recursive: true });
const svgPath = path.join(destDir, "diagram-preview.svg");
const htmlPath = path.join(destDir, "diagram-preview.html");
await writeFile(svgPath, svg);
await writeFile(
  htmlPath,
  `<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <title>my-terminal</title>
  <style>
    html, body { margin: 0; background: #0e1116; color: #e6edf3; font: 14px/1.45 ui-sans-serif, system-ui, sans-serif; }
    svg text { font-family: ui-sans-serif, system-ui, "WenQuanYi Micro Hei", "Droid Sans Fallback", sans-serif; }
    main { box-sizing: border-box; padding: 24px 0 40px; }
    h1 { margin: 0 24px 16px; font-size: 20px; font-weight: 600; }
    .scroll { overflow-x: auto; }
  </style>
</head>
<body>
  <main>
    <h1>my-terminal</h1>
    <div class="scroll">${svg}</div>
  </main>
</body>
</html>
`,
);
console.log(htmlPath);
