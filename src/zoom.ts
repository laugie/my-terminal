// 界面缩放。档位与后端 config::ZOOM_LEVELS 相同，保存和真正的缩放都在后端。

export const ZOOM_LEVELS: readonly number[] = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200];

export type ZoomStep = "in" | "out" | "reset";

export interface ZoomView {
  percent: number;
  readOnly: boolean;
  message?: string | null;
}

/** Nearest level. Anything that is not a finite number is 100. */
export function normalizeZoom(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return 100;
  }
  let best = 100;
  for (const level of ZOOM_LEVELS) {
    if (Math.abs(level - value) < Math.abs(best - value)) {
      best = level;
    }
  }
  return best;
}

/** One level up or down, clamped at both ends. */
export function stepZoom(current: number, step: ZoomStep): number {
  if (step === "reset") {
    return 100;
  }
  const index = ZOOM_LEVELS.indexOf(normalizeZoom(current));
  const next = step === "in" ? index + 1 : index - 1;
  return ZOOM_LEVELS[Math.min(Math.max(next, 0), ZOOM_LEVELS.length - 1)] ?? 100;
}

export function zoomLabel(percent: number): string {
  return `${Math.round(percent)}%`;
}

/**
 * The one current level for shortcuts and the settings row. Each step updates
 * it at once and queues the save, so fast repeats reach the backend in order
 * and an older reply never moves the level back.
 */
export class ZoomController {
  percent = 100;
  readOnly = false;
  message = "";
  private chain: Promise<void> = Promise.resolve();

  constructor(
    private readonly deps: {
      save(percent: number): Promise<ZoomView>;
      onChange?(): void;
    },
  ) {}

  load(view: ZoomView): void {
    this.percent = normalizeZoom(view.percent);
    this.readOnly = view.readOnly;
    this.message = view.message ?? "";
    this.deps.onChange?.();
  }

  canStep(step: ZoomStep): boolean {
    return stepZoom(this.percent, step) !== this.percent;
  }

  step(step: ZoomStep): Promise<void> {
    const next = stepZoom(this.percent, step);
    if (next === this.percent) {
      return this.chain;
    }
    this.percent = next;
    this.deps.onChange?.();
    this.chain = this.chain.then(async () => {
      try {
        const view = await this.deps.save(next);
        this.readOnly = view.readOnly;
        this.message = view.message ?? "";
      } catch (err) {
        this.message = err instanceof Error ? err.message : String(err);
      }
      this.deps.onChange?.();
    });
    return this.chain;
  }
}
