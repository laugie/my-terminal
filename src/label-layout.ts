// 3D 拓扑名字标签的屏幕布局。纯函数：输入每颗星球在屏幕上的位置、半径和标签大小，
// 输出标签左上角坐标。近的星球先选位置，标签之间、标签与其他星球之间尽量不重叠。

export type LabelSlot =
  | "below"
  | "above"
  | "right"
  | "left"
  | "below-right"
  | "below-left"
  | "above-right"
  | "above-left"
  | "below-far"
  | "above-far";

/** Try order when nothing was chosen before: under the planet reads best. */
const SLOTS: readonly LabelSlot[] = [
  "below",
  "above",
  "right",
  "left",
  "below-right",
  "below-left",
  "above-right",
  "above-left",
  "below-far",
  "above-far",
];

/** One planet on screen, in CSS pixels. */
export interface LabelBox {
  id: string;
  /** Planet centre. */
  x: number;
  y: number;
  /** Planet radius on screen. */
  r: number;
  /** Label size. */
  w: number;
  h: number;
  /** Distance from the camera; nearer planets choose first. */
  depth: number;
}

export interface LabelPlacement {
  slot: LabelSlot;
  /** Label's top-left corner on screen. */
  left: number;
  top: number;
}

/** Gap between a planet and its label, and between two labels. */
const GAP = 4;
const PAD = 2;

interface Rect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export function placeLabels(
  boxes: readonly LabelBox[],
  view: { width: number; height: number; previous?: ReadonlyMap<string, LabelSlot> },
): Map<string, LabelPlacement> {
  const order = [...boxes].sort((a, b) => a.depth - b.depth || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  const discs = boxes.map((box) => ({
    id: box.id,
    rect: { left: box.x - box.r, top: box.y - box.r, right: box.x + box.r, bottom: box.y + box.r },
  }));
  const taken: Rect[] = [];
  const result = new Map<string, LabelPlacement>();
  for (const box of order) {
    const previous = view.previous?.get(box.id);
    const tries = previous ? [previous, ...SLOTS.filter((slot) => slot !== previous)] : SLOTS;
    let best: { placement: LabelPlacement; rect: Rect; cost: number } | null = null;
    for (const slot of tries) {
      const placement = slotAt(box, slot);
      const rect = {
        left: placement.left,
        top: placement.top,
        right: placement.left + box.w,
        bottom: placement.top + box.h,
      };
      let cost = outside(rect, view.width, view.height) * 4;
      for (const other of taken) {
        cost += overlap(rect, grow(other, PAD));
      }
      for (const disc of discs) {
        cost += overlap(rect, disc.rect) * (disc.id === box.id ? 2 : 1);
      }
      if (best === null || cost < best.cost) {
        best = { placement, rect, cost };
      }
      if (cost === 0) {
        break;
      }
    }
    if (best) {
      taken.push(best.rect);
      result.set(box.id, best.placement);
    }
  }
  return result;
}

function slotAt(box: LabelBox, slot: LabelSlot): LabelPlacement {
  const { x, y, r, w, h } = box;
  const d = r * Math.SQRT1_2;
  const centred = x - w / 2;
  switch (slot) {
    case "below":
      return { slot, left: centred, top: y + r + GAP };
    case "above":
      return { slot, left: centred, top: y - r - GAP - h };
    case "right":
      return { slot, left: x + r + GAP, top: y - h / 2 };
    case "left":
      return { slot, left: x - r - GAP - w, top: y - h / 2 };
    case "below-right":
      return { slot, left: x + d + GAP, top: y + d + GAP };
    case "below-left":
      return { slot, left: x - d - GAP - w, top: y + d + GAP };
    case "above-right":
      return { slot, left: x + d + GAP, top: y - d - GAP - h };
    case "above-left":
      return { slot, left: x - d - GAP - w, top: y - d - GAP - h };
    case "below-far":
      return { slot, left: centred, top: y + r + GAP * 2 + h };
    case "above-far":
      return { slot, left: centred, top: y - r - GAP * 2 - h * 2 };
  }
}

function overlap(a: Rect, b: Rect): number {
  const width = Math.min(a.right, b.right) - Math.max(a.left, b.left);
  const height = Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top);
  return width > 0 && height > 0 ? width * height : 0;
}

/** Area of `rect` that falls outside the view. */
function outside(rect: Rect, width: number, height: number): number {
  const area = (rect.right - rect.left) * (rect.bottom - rect.top);
  const inside = overlap(rect, { left: 0, top: 0, right: width, bottom: height });
  return area - inside;
}

function grow(rect: Rect, by: number): Rect {
  return { left: rect.left - by, top: rect.top - by, right: rect.right + by, bottom: rect.bottom + by };
}
