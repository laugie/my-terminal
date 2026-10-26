// #33 前端失败测试：界面缩放档位、步进，以及连续按键时按顺序逐个保存。
import assert from "node:assert/strict";
import { test } from "node:test";
import { ZOOM_LEVELS, ZoomController, normalizeZoom, stepZoom, zoomLabel } from "./zoom.js";

test("levels match the browser ladder and include 100", () => {
  assert.deepEqual(ZOOM_LEVELS, [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200]);
});

test("step moves one level and stops at both ends; reset goes to 100", () => {
  assert.equal(stepZoom(100, "in"), 110);
  assert.equal(stepZoom(100, "out"), 90);
  assert.equal(stepZoom(200, "in"), 200);
  assert.equal(stepZoom(50, "out"), 50);
  assert.equal(stepZoom(150, "reset"), 100);
});

test("an off-ladder value snaps to the nearest level; junk is 100", () => {
  assert.equal(normalizeZoom(112), 110);
  assert.equal(normalizeZoom(10), 50);
  assert.equal(normalizeZoom(900), 200);
  assert.equal(normalizeZoom(Number.NaN), 100);
  assert.equal(normalizeZoom("125"), 100);
  assert.equal(normalizeZoom(undefined), 100);
});

test("label is a whole percent", () => {
  assert.equal(zoomLabel(67), "67%");
  assert.equal(zoomLabel(100), "100%");
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

test("rapid steps update at once and save one at a time, in order", async () => {
  const saved: number[] = [];
  const pending: Array<{ percent: number; done: (v: { percent: number; readOnly: boolean }) => void }> = [];
  let inFlight = 0;
  let maxInFlight = 0;
  let changes = 0;
  const zoom = new ZoomController({
    save(percent) {
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      saved.push(percent);
      const d = deferred<{ percent: number; readOnly: boolean }>();
      pending.push({
        percent,
        done: (v) => {
          inFlight -= 1;
          d.resolve(v);
        },
      });
      return d.promise;
    },
    onChange() {
      changes += 1;
    },
  });
  zoom.load({ percent: 100, readOnly: false });
  void zoom.step("in");
  void zoom.step("in");
  const last = zoom.step("in");
  assert.equal(zoom.percent, 150, "界面立即反映");
  assert.equal(changes, 4);
  const tick = () => new Promise((r) => setTimeout(r, 0));
  for (let i = 0; i < 3; i += 1) {
    await tick();
    const next = pending.shift();
    if (!next) {
      throw new Error(`第 ${i + 1} 次保存没有发出`);
    }
    next.done({ percent: next.percent, readOnly: false });
  }
  await last;
  assert.deepEqual(saved, [110, 125, 150]);
  assert.equal(maxInFlight, 1);
  assert.equal(zoom.percent, 150, "旧的保存结果不能把档位改回去");
});

test("a step at the end does not save", async () => {
  const saved: number[] = [];
  const zoom = new ZoomController({
    save: async (percent) => {
      saved.push(percent);
      return { percent, readOnly: false };
    },
  });
  zoom.load({ percent: 200, readOnly: false });
  await zoom.step("in");
  assert.deepEqual(saved, []);
});

test("read-only config keeps the zoom and shows the message", async () => {
  const zoom = new ZoomController({
    save: async (percent) => ({ percent, readOnly: true, message: "配置来自更新版本，无法保存修改" }),
  });
  zoom.load({ percent: 100, readOnly: true, message: "配置来自更新版本，无法保存修改" });
  await zoom.step("out");
  assert.equal(zoom.percent, 90);
  assert.equal(zoom.message, "配置来自更新版本，无法保存修改");
});

test("a failed save keeps the level and reports the error", async () => {
  const zoom = new ZoomController({
    save: async () => {
      throw new Error("无法保存配置");
    },
  });
  zoom.load({ percent: 100, readOnly: false });
  await zoom.step("in");
  assert.equal(zoom.percent, 110);
  assert.equal(zoom.message, "无法保存配置");
  await zoom.step("reset");
  assert.equal(zoom.percent, 100);
});
