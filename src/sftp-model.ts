// 文件传输的纯逻辑：字节和速度格式化、传输队列、选择、拖放方向。不碰 DOM。

export type PaneSide = "local" | "remote";

export type Direction = "upload" | "download";

export type TransferStatus = "queued" | "running" | "done" | "failed" | "cancelled";

/** Why a transfer failed, as sent by the backend. */
export type FailReason =
  | "permission"
  | "diskFull"
  | "connection"
  | "notFound"
  | "cancelled"
  | "other";

export interface TransferSpec {
  direction: Direction;
  /** Full path of the file or folder being copied. */
  source: string;
  /** Folder it is copied into, on the other side. */
  destDir: string;
}

export interface TransferItem extends TransferSpec {
  key: string;
  name: string;
  status: TransferStatus;
  done: number;
  total: number;
  /** Bytes per second, or null before there is enough to measure. */
  speed: number | null;
  reason?: FailReason;
  message?: string;
  /** Backend transfer id while running. */
  transferId?: string;
}

const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** 1024-based, one decimal from KB up (two under 10 GB and above). */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) {
    return "0 B";
  }
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit === 0) {
    return `${Math.round(value)} B`;
  }
  const digits = unit >= 3 && value < 10 ? 2 : 1;
  return `${value.toFixed(digits)} ${UNITS[unit]}`;
}

export function formatSpeed(bytesPerSecond: number | null): string {
  if (bytesPerSecond === null) {
    return "—";
  }
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** Text under a progress bar: "12.3 MB / 45.6 MB · 3.2 MB/s". */
export function progressText(item: TransferItem): string {
  const amount = `${formatBytes(item.done)} / ${formatBytes(item.total)}`;
  switch (item.status) {
    case "queued":
      return "等待中";
    case "running":
      return `${amount} · ${formatSpeed(item.speed)}`;
    case "done":
      return `${formatBytes(item.total)} · 完成`;
    case "cancelled":
      return `${amount} · 已取消`;
    case "failed":
      return item.message ?? "传输失败";
  }
}

/** Fraction for the bar, 0 to 1. An empty transfer counts as full when done. */
export function progressFraction(item: TransferItem): number {
  if (item.total <= 0) {
    return item.status === "done" ? 1 : 0;
  }
  return Math.min(1, Math.max(0, item.done / item.total));
}

/**
 * Current speed from the last few seconds of byte counts. Older samples are
 * dropped so the number follows a link that speeds up or stalls.
 */
export class SpeedMeter {
  private samples: { at: number; bytes: number }[] = [];

  constructor(
    private readonly windowMs = 3000,
    private readonly minSpanMs = 400,
  ) {}

  add(bytes: number, at: number): void {
    const last = this.samples[this.samples.length - 1];
    if (last && bytes < last.bytes) {
      // Counter went back (a retry); start over.
      this.samples = [];
    }
    this.samples.push({ at, bytes });
    while (this.samples.length > 2 && at - (this.samples[0]?.at ?? at) > this.windowMs) {
      this.samples.shift();
    }
  }

  /** Bytes per second, or null when the samples span too short a time. */
  rate(): number | null {
    const first = this.samples[0];
    const last = this.samples[this.samples.length - 1];
    if (!first || !last || last.at - first.at < this.minSpanMs) {
      return null;
    }
    return ((last.bytes - first.bytes) * 1000) / (last.at - first.at);
  }

  reset(): void {
    this.samples = [];
  }
}

/** Last path component for either separator. */
export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const index = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  const name = index >= 0 ? trimmed.slice(index + 1) : trimmed;
  return name === "" ? path : name;
}

/**
 * Transfer list. Holds every item until cleared; `startable` says which
 * queued items may run now so at most `concurrency` run together.
 */
export class TransferQueue {
  private list: TransferItem[] = [];
  private meters = new Map<string, SpeedMeter>();
  private nextKey = 1;

  constructor(private readonly concurrency = 2) {}

  get items(): readonly TransferItem[] {
    return this.list;
  }

  add(spec: TransferSpec): TransferItem {
    const item: TransferItem = {
      ...spec,
      key: `t${this.nextKey++}`,
      name: baseName(spec.source),
      status: "queued",
      done: 0,
      total: 0,
      speed: null,
    };
    this.list.push(item);
    return item;
  }

  find(key: string): TransferItem | undefined {
    return this.list.find((item) => item.key === key);
  }

  /** Queued items to start now, oldest first. Marks them running. */
  startable(): TransferItem[] {
    const running = this.list.filter((item) => item.status === "running").length;
    const room = Math.max(0, this.concurrency - running);
    const next = this.list.filter((item) => item.status === "queued").slice(0, room);
    for (const item of next) {
      item.status = "running";
      item.done = 0;
      item.total = 0;
      item.speed = null;
      delete item.reason;
      delete item.message;
      delete item.transferId;
      this.meter(item.key).reset();
    }
    return next;
  }

  progress(key: string, done: number, total: number, at: number): void {
    const item = this.find(key);
    if (!item || item.status !== "running") {
      return;
    }
    item.done = done;
    item.total = total;
    const meter = this.meter(key);
    meter.add(done, at);
    item.speed = meter.rate();
  }

  finish(key: string, done: number, total: number): void {
    const item = this.find(key);
    if (!item || item.status !== "running") {
      return;
    }
    item.status = "done";
    item.done = done;
    item.total = total;
    item.speed = null;
    delete item.transferId;
  }

  fail(key: string, reason: FailReason, message: string, done: number, total: number): void {
    const item = this.find(key);
    if (!item || (item.status !== "running" && item.status !== "queued")) {
      return;
    }
    item.done = done;
    item.total = total;
    item.speed = null;
    delete item.transferId;
    if (reason === "cancelled") {
      item.status = "cancelled";
      delete item.reason;
      delete item.message;
      return;
    }
    item.status = "failed";
    item.reason = reason;
    item.message = message;
  }

  /** Failed or cancelled items go back in the queue. */
  retry(key: string): boolean {
    const item = this.find(key);
    if (!item || (item.status !== "failed" && item.status !== "cancelled")) {
      return false;
    }
    item.status = "queued";
    item.done = 0;
    item.total = 0;
    item.speed = null;
    delete item.reason;
    delete item.message;
    return true;
  }

  /** Queued items are dropped at once; running ones need the backend to stop. */
  cancelQueued(key: string): boolean {
    const item = this.find(key);
    if (!item || item.status !== "queued") {
      return false;
    }
    item.status = "cancelled";
    return true;
  }

  remove(key: string): void {
    this.list = this.list.filter((item) => item.key !== key);
    this.meters.delete(key);
  }

  /** Remove finished rows (done or cancelled). Failed rows stay for Retry. */
  clearFinished(): void {
    for (const item of this.list) {
      if (item.status === "done" || item.status === "cancelled") {
        this.meters.delete(item.key);
      }
    }
    this.list = this.list.filter((item) => item.status !== "done" && item.status !== "cancelled");
  }

  active(): number {
    return this.list.filter((item) => item.status === "running" || item.status === "queued")
      .length;
  }

  private meter(key: string): SpeedMeter {
    let meter = this.meters.get(key);
    if (!meter) {
      meter = new SpeedMeter();
      this.meters.set(key, meter);
    }
    return meter;
  }
}

/** Dropping items from `from` onto `to`. Same pane does nothing. */
export function dropDirection(from: PaneSide, to: PaneSide): Direction | null {
  if (from === to) {
    return null;
  }
  return from === "local" ? "upload" : "download";
}

export interface ClickMods {
  /** Cmd on macOS, Ctrl on Windows. */
  toggle: boolean;
  range: boolean;
}

/** File manager selection: click, toggle-click, and shift-click ranges. */
export function nextSelection(
  order: readonly string[],
  current: ReadonlySet<string>,
  anchor: string | null,
  clicked: string,
  mods: ClickMods,
): { selected: Set<string>; anchor: string } {
  if (mods.range && anchor !== null && order.includes(anchor)) {
    const from = order.indexOf(anchor);
    const to = order.indexOf(clicked);
    const [low, high] = from <= to ? [from, to] : [to, from];
    const range = order.slice(low, high + 1);
    const selected = mods.toggle ? new Set([...current, ...range]) : new Set(range);
    return { selected, anchor };
  }
  if (mods.toggle) {
    const selected = new Set(current);
    if (selected.has(clicked)) {
      selected.delete(clicked);
    } else {
      selected.add(clicked);
    }
    return { selected, anchor: clicked };
  }
  return { selected: new Set([clicked]), anchor: clicked };
}

/** Names that already exist in the destination listing. */
export function conflictingNames(
  names: readonly string[],
  existing: readonly { name: string }[],
  caseInsensitive: boolean,
): string[] {
  const fold = (name: string) => (caseInsensitive ? name.toLowerCase() : name);
  const taken = new Set(existing.map((entry) => fold(entry.name)));
  return names.filter((name) => taken.has(fold(name)));
}

/** Message for the overwrite question. */
export function overwriteMessage(names: readonly string[]): string {
  const shown = names.slice(0, 3).join("、");
  const more = names.length > 3 ? ` 等 ${names.length} 项` : "";
  return `目标文件夹已有 ${shown}${more}，继续会覆盖同名文件，同名文件夹会合并。`;
}

/** Physical drop position to CSS pixels, given physical pixels per CSS pixel. */
export function cssPoint(
  position: { x: number; y: number },
  ratio: number,
): { x: number; y: number } {
  const scale = Number.isFinite(ratio) && ratio > 0 ? ratio : 1;
  return { x: position.x / scale, y: position.y / scale };
}

/** Modified time for a row: "2026-10-29 09:41", or "" when unknown. */
export function formatModified(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) {
    return "";
  }
  const date = new Date(seconds * 1000);
  const pad = (value: number) => String(value).padStart(2, "0");
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}`
  );
}
