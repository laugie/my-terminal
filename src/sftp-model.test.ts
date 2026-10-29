// 文件传输的纯逻辑：格式化、测速、队列状态、选择和拖放方向。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  SpeedMeter,
  TransferQueue,
  baseName,
  conflictingNames,
  cssPoint,
  dropDirection,
  formatBytes,
  formatModified,
  formatSpeed,
  nextSelection,
  overwriteMessage,
  progressFraction,
  progressText,
} from "./sftp-model.js";

test("bytes are shown with 1024-based units", () => {
  assert.equal(formatBytes(0), "0 B");
  assert.equal(formatBytes(1023), "1023 B");
  assert.equal(formatBytes(1536), "1.5 KB");
  assert.equal(formatBytes(12.3 * 1024 * 1024), "12.3 MB");
  assert.equal(formatBytes(1.2 * 1024 ** 3), "1.20 GB");
  assert.equal(formatBytes(-5), "0 B");
  assert.equal(formatSpeed(null), "—");
  assert.equal(formatSpeed(2 * 1024 * 1024), "2.0 MB/s");
});

test("speed follows the last few seconds and restarts when the count goes back", () => {
  const meter = new SpeedMeter(3000, 400);
  meter.add(0, 0);
  assert.equal(meter.rate(), null, "too short to measure");
  meter.add(1_000_000, 1000);
  assert.equal(meter.rate(), 1_000_000);
  // A stall: nothing for four seconds drops the old samples.
  meter.add(1_000_000, 5000);
  meter.add(1_000_000, 5500);
  assert.equal(meter.rate(), 0);
  meter.add(10, 6000);
  assert.equal(meter.rate(), null);
});

test("the queue runs at most two transfers and starts the next when one ends", () => {
  const queue = new TransferQueue(2);
  const a = queue.add({ direction: "upload", source: "/Users/me/a.txt", destDir: "/home/me" });
  const b = queue.add({ direction: "upload", source: "C:\\Users\\me\\b", destDir: "/home/me" });
  const c = queue.add({ direction: "download", source: "/home/me/c/", destDir: "/Users/me" });
  assert.equal(a.name, "a.txt");
  assert.equal(b.name, "b");
  assert.equal(c.name, "c");
  assert.deepEqual(
    queue.startable().map((item) => item.key),
    [a.key, b.key],
  );
  assert.deepEqual(queue.startable(), []);
  assert.equal(progressText(c), "等待中");

  queue.progress(a.key, 512, 2048, 0);
  queue.progress(a.key, 1024, 2048, 1000);
  assert.equal(a.speed, 512);
  assert.equal(progressText(a), "1.0 KB / 2.0 KB · 512 B/s");
  assert.equal(progressFraction(a), 0.5);
  queue.finish(a.key, 2048, 2048);
  assert.equal(a.status, "done");
  assert.equal(progressText(a), "2.0 KB · 完成");
  assert.deepEqual(
    queue.startable().map((item) => item.key),
    [c.key],
  );
  assert.equal(queue.active(), 2);
});

test("a failure keeps its reason, retry requeues it, cancel is not a failure", () => {
  const queue = new TransferQueue(1);
  const a = queue.add({ direction: "download", source: "/srv/big.iso", destDir: "/tmp" });
  queue.startable();
  queue.progress(a.key, 100, 1000, 0);
  queue.fail(a.key, "connection", "连接已断开", 100, 1000);
  assert.equal(a.status, "failed");
  assert.equal(a.reason, "connection");
  assert.equal(progressText(a), "连接已断开");
  assert.equal(progressFraction(a), 0.1);

  assert.equal(queue.retry(a.key), true);
  assert.equal(a.status, "queued");
  assert.equal(a.message, undefined);
  assert.equal(a.done, 0);
  assert.deepEqual(queue.startable(), [a]);
  queue.fail(a.key, "cancelled", "已取消", 300, 1000);
  assert.equal(a.status, "cancelled");
  assert.equal(a.reason, undefined);
  assert.equal(progressText(a), "300 B / 1000 B · 已取消");
  assert.equal(queue.retry(a.key), true);

  const b = queue.add({ direction: "download", source: "/srv/b", destDir: "/tmp" });
  assert.equal(queue.cancelQueued(b.key), true);
  assert.equal(b.status, "cancelled");
  // Events for an item that is no longer running are ignored.
  queue.finish(b.key, 1, 1);
  assert.equal(b.status, "cancelled");
});

test("clearing keeps failed rows so they can still be retried", () => {
  const queue = new TransferQueue(3);
  const done = queue.add({ direction: "upload", source: "/a", destDir: "/r" });
  const failed = queue.add({ direction: "upload", source: "/b", destDir: "/r" });
  const running = queue.add({ direction: "upload", source: "/c", destDir: "/r" });
  queue.startable();
  queue.finish(done.key, 1, 1);
  queue.fail(failed.key, "permission", "远端权限不足：/r/b", 0, 1);
  queue.clearFinished();
  assert.deepEqual(
    queue.items.map((item) => item.key),
    [failed.key, running.key],
  );
  queue.remove(failed.key);
  assert.deepEqual(
    queue.items.map((item) => item.key),
    [running.key],
  );
  assert.equal(progressFraction({ ...done, total: 0, status: "done" }), 1);
});

test("drops between panes pick the direction; same pane does nothing", () => {
  assert.equal(dropDirection("local", "remote"), "upload");
  assert.equal(dropDirection("remote", "local"), "download");
  assert.equal(dropDirection("local", "local"), null);
  assert.equal(dropDirection("remote", "remote"), null);
});

test("selection follows click, toggle-click and shift-click", () => {
  const order = ["a", "b", "c", "d", "e"];
  let state = nextSelection(order, new Set(), null, "b", { toggle: false, range: false });
  assert.deepEqual([...state.selected], ["b"]);
  state = nextSelection(order, state.selected, state.anchor, "d", { toggle: false, range: true });
  assert.deepEqual([...state.selected], ["b", "c", "d"]);
  assert.equal(state.anchor, "b");
  state = nextSelection(order, state.selected, state.anchor, "c", { toggle: true, range: false });
  assert.deepEqual([...state.selected].sort(), ["b", "d"]);
  state = nextSelection(order, state.selected, state.anchor, "a", { toggle: true, range: true });
  assert.deepEqual([...state.selected].sort(), ["a", "b", "c", "d"]);
  state = nextSelection(order, state.selected, "gone", "e", { toggle: false, range: true });
  assert.deepEqual([...state.selected], ["e"]);
});

test("existing names are found for the overwrite question", () => {
  const existing = [{ name: "Report.txt" }, { name: "docs" }];
  assert.deepEqual(conflictingNames(["report.txt", "docs", "new"], existing, false), ["docs"]);
  assert.deepEqual(conflictingNames(["report.txt", "docs", "new"], existing, true), [
    "report.txt",
    "docs",
  ]);
  assert.equal(
    overwriteMessage(["a", "b"]),
    "目标文件夹已有 a、b，继续会覆盖同名文件，同名文件夹会合并。",
  );
  assert.match(overwriteMessage(["a", "b", "c", "d"]), /a、b、c 等 4 项/);
});

test("base names, drop points and times", () => {
  assert.equal(baseName("/home/me/file.txt"), "file.txt");
  assert.equal(baseName("C:\\Users\\me\\Downloads\\"), "Downloads");
  assert.equal(baseName("/"), "/");
  assert.deepEqual(cssPoint({ x: 300, y: 150 }, 2), { x: 150, y: 75 });
  assert.deepEqual(cssPoint({ x: 300, y: 150 }, 0), { x: 300, y: 150 });
  assert.equal(formatModified(null), "");
  assert.match(formatModified(1_700_000_000), /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
});
