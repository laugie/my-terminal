// #35 前端失败测试：连接成功后自动美化远端 Shell，每个目标每次运行只执行一次。
import assert from "node:assert/strict";
import { test } from "node:test";
import { AutoBeautify, reportMessage, targetKey, type ShellReport, type ShellTarget } from "./remote-shell.js";

const device: ShellTarget = { kind: "device", deviceName: "app-server" };
const relay: ShellTarget = { kind: "device", deviceName: "app-server", connectionName: "via-nas" };
const host: ShellTarget = { kind: "host", host: "192.0.2.7", user: "me", port: 22 };

const report = (outcome: ShellReport["outcome"], extra: Partial<ShellReport> = {}): ShellReport => ({
  outcome,
  files: [],
  backups: [],
  notes: [],
  ...extra,
});

test("target keys tell routes and hosts apart", () => {
  assert.equal(targetKey(device), targetKey({ kind: "device", deviceName: "app-server" }));
  assert.ok(targetKey(device) !== targetKey(relay));
  assert.ok(targetKey(host) !== targetKey({ ...host, user: "other" }));
});

test("auto runs once per target per app run, only when enabled", async () => {
  const runs: string[] = [];
  let enabled = true;
  const auto = new AutoBeautify({
    run: async (target, mode) => {
      runs.push(`${targetKey(target)}:${mode}`);
      return report("applied");
    },
    enabled: () => enabled,
    onReport: () => {},
  });
  await auto.connected(device, "app-server");
  await auto.connected(device, "app-server");
  await auto.connected(relay, "app-server · via-nas");
  enabled = false;
  await auto.connected(host, "box");
  assert.deepEqual(runs, [`${targetKey(device)}:auto`, `${targetKey(relay)}:auto`]);
});

test("a second connect while the first run is in flight does not start another", async () => {
  let calls = 0;
  let release!: () => void;
  const auto = new AutoBeautify({
    run: () => {
      calls += 1;
      return new Promise<ShellReport>((resolve) => {
        release = () => resolve(report("unchanged"));
      });
    },
    enabled: () => true,
    onReport: () => {},
  });
  const first = auto.connected(device, "app-server");
  const second = auto.connected(device, "app-server");
  release();
  await Promise.all([first, second]);
  assert.equal(calls, 1);
});

test("a failed auto run is reported and not retried this run; manual apply still runs", async () => {
  const seen: string[] = [];
  let calls = 0;
  const auto = new AutoBeautify({
    run: async (_target, mode) => {
      calls += 1;
      if (mode === "auto") {
        throw new Error("无法免交互登录");
      }
      return report("applied");
    },
    enabled: () => true,
    onReport: (_label, result, mode) => {
      seen.push(`${mode}:${result instanceof Error ? result.message : result.outcome}`);
    },
  });
  await auto.connected(host, "box");
  await auto.connected(host, "box");
  await auto.run(host, "box", "apply");
  assert.equal(calls, 2);
  assert.deepEqual(seen, ["auto:无法免交互登录", "apply:applied"]);
});

test("messages say what changed, where the backup is, and when it takes effect", () => {
  const applied = reportMessage(
    report("applied", { files: ["~/.zshrc"], backups: ["~/.my-terminal/backups/.zshrc.1"] }),
    "mac-mini",
  );
  assert.match(applied, /mac-mini/);
  assert.match(applied, /~\/\.zshrc/);
  assert.match(applied, /备份/);
  assert.match(applied, /重新连接/);
  assert.match(reportMessage(report("restored", { files: ["~/.bashrc"] }), "box"), /恢复/);
  assert.match(reportMessage(report("unsupported", { notes: ["登录 shell 是 fish"] }), "box"), /fish/);
  assert.match(reportMessage(report("skipped"), "box"), /立即美化/);
});

test("only an applied auto run is worth a notice", () => {
  assert.equal(AutoBeautify.notable(report("applied"), "auto"), true);
  assert.equal(AutoBeautify.notable(report("unchanged"), "auto"), false);
  assert.equal(AutoBeautify.notable(report("skipped"), "auto"), false);
  assert.equal(AutoBeautify.notable(new Error("x"), "auto"), false);
  assert.equal(AutoBeautify.notable(new Error("x"), "apply"), true);
  assert.equal(AutoBeautify.notable(report("unchanged"), "apply"), true);
});
