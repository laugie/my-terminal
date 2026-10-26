// #37 前端失败测试：3D 拓扑的名字标签在屏幕上避让，互不遮挡，也不压住别的星球。
import assert from "node:assert/strict";
import { test } from "node:test";
import { placeLabels, type LabelBox, type LabelPlacement } from "./label-layout.js";

const VIEWPORT = { width: 800, height: 600 };

const box = (id: string, x: number, y: number, extra: Partial<LabelBox> = {}): LabelBox => ({
  id,
  x,
  y,
  r: 14,
  w: 96,
  h: 20,
  depth: 0,
  ...extra,
});

interface Rect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

const rectOf = (b: LabelBox, p: LabelPlacement): Rect => ({
  left: p.left,
  top: p.top,
  right: p.left + b.w,
  bottom: p.top + b.h,
});

const overlaps = (a: Rect, b: Rect) =>
  a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;

const discRect = (b: LabelBox): Rect => ({ left: b.x - b.r, top: b.y - b.r, right: b.x + b.r, bottom: b.y + b.r });

function assertClean(boxes: LabelBox[], placed: Map<string, LabelPlacement>): void {
  for (const a of boxes) {
    const ra = rectOf(a, placed.get(a.id)!);
    assert.ok(ra.left >= 0 && ra.top >= 0 && ra.right <= VIEWPORT.width && ra.bottom <= VIEWPORT.height, `${a.id} 在画面内`);
    for (const b of boxes) {
      if (a === b) {
        continue;
      }
      assert.ok(!overlaps(ra, rectOf(b, placed.get(b.id)!)), `${a.id} 与 ${b.id} 的标签重叠`);
      assert.ok(!overlaps(ra, discRect(b)), `${a.id} 的标签压住了 ${b.id}`);
    }
    assert.ok(!overlaps(ra, discRect(a)), `${a.id} 的标签压住了自己的星球`);
  }
}

test("a lone label sits centred below its planet", () => {
  const a = box("a", 400, 300);
  const placed = placeLabels([a], VIEWPORT).get("a")!;
  assert.equal(placed.slot, "below");
  assert.equal(placed.left, 400 - 48);
  assert.ok(placed.top >= 300 + 14);
  assert.ok(placed.top <= 300 + 14 + 8, "紧贴星球");
});

test("a crowded cluster gets labels that cover nothing", () => {
  const boxes = [
    box("a", 360, 280),
    box("b", 420, 285, { depth: 1 }),
    box("c", 390, 320, { depth: 2 }),
    box("d", 450, 330, { depth: 3 }),
    box("e", 340, 330, { depth: 4 }),
    box("f", 400, 250, { depth: 5 }),
  ];
  assertClean(boxes, placeLabels(boxes, VIEWPORT));
});

test("planets lined up in a row still get readable labels", () => {
  const boxes = Array.from({ length: 6 }, (_, i) => box(`p${i}`, 250 + i * 60, 300, { depth: i }));
  assertClean(boxes, placeLabels(boxes, VIEWPORT));
});

test("the nearer planet keeps the best spot", () => {
  const near = box("near", 400, 300, { depth: 1 });
  const far = box("far", 404, 306, { depth: 5, r: 6 });
  const placed = placeLabels([far, near], VIEWPORT);
  assert.equal(placed.get("near")!.slot, "below");
});

test("a free previous slot is kept, so labels do not jump while rotating", () => {
  const a = box("a", 400, 300);
  const placed = placeLabels([a], { ...VIEWPORT, previous: new Map([["a", "above"]]) });
  assert.equal(placed.get("a")!.slot, "above");
});

test("a label near the bottom edge moves inside the view", () => {
  const a = box("a", 400, 590, { r: 8 });
  const placed = placeLabels([a], VIEWPORT).get("a")!;
  assert.ok(placed.top + a.h <= VIEWPORT.height);
  assert.notEqual(placed.slot, "below");
});

test("placement is deterministic", () => {
  const boxes = [box("a", 380, 300), box("b", 410, 305, { depth: 1 }), box("c", 395, 330, { depth: 2 })];
  assert.deepEqual([...placeLabels(boxes, VIEWPORT)], [...placeLabels(boxes, VIEWPORT)]);
});
