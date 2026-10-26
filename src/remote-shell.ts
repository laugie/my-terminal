// 远端 Shell 美化：连接成功后自动执行一次，设置里可手动美化或恢复备份。
// 真正的改动由后端经 ssh 执行固定脚本，这里只决定何时执行以及怎么提示。

export type ShellMode = "auto" | "apply" | "restore";

export type ShellTarget =
  | { kind: "device"; deviceName: string; connectionName?: string }
  | { kind: "host"; host: string; user: string | null; port: number | null };

export interface ShellReport {
  outcome: "applied" | "unchanged" | "skipped" | "unsupported" | "restored" | "nothing" | "failed";
  files: string[];
  backups: string[];
  notes: string[];
}

export function targetKey(target: ShellTarget): string {
  if (target.kind === "device") {
    return JSON.stringify(["device", target.deviceName, target.connectionName ?? ""]);
  }
  return JSON.stringify(["host", target.host, target.user ?? "", target.port ?? ""]);
}

export function reportMessage(report: ShellReport, label: string): string {
  const files = report.files.join("、");
  const notes = report.notes.length > 0 ? ` ${report.notes.join("；")}` : "";
  switch (report.outcome) {
    case "applied": {
      const backups = report.backups.length > 0 ? `，原文件已备份到 ${report.backups.join("、")}` : "";
      return `已为 ${label} 开启远端 Shell 颜色：改了 ${files}${backups}。重新连接后生效。${notes}`.trim();
    }
    case "unchanged":
      return `${label} 的远端 Shell 已经美化过，颜色设置已更新。${notes}`.trim();
    case "skipped":
      return `${label} 恢复过备份，不再自动美化。需要时点「立即美化」。`;
    case "unsupported":
      return `${label} 没有美化：${report.notes.join("；") || "只支持 bash 和 zsh"}`;
    case "restored":
      return `已恢复 ${label} 的 ${files}，之后不再自动美化。${notes}`.trim();
    case "nothing":
      return `${label} 没有需要恢复的改动。`;
    case "failed":
      return `${label} 的远端 Shell 美化失败：${report.notes.join("；") || "未知原因"}`;
  }
}

/**
 * Runs the auto mode once per target per app run. A failure is reported and
 * not retried until the next launch; the settings buttons always run.
 */
export class AutoBeautify {
  private readonly started = new Map<string, Promise<void>>();

  constructor(
    private readonly deps: {
      run(target: ShellTarget, mode: ShellMode): Promise<ShellReport>;
      enabled(): boolean;
      onReport(label: string, result: ShellReport | Error, mode: ShellMode): void;
    },
  ) {}

  connected(target: ShellTarget, label: string): Promise<void> {
    if (!this.deps.enabled()) {
      return Promise.resolve();
    }
    const key = targetKey(target);
    const existing = this.started.get(key);
    if (existing) {
      return existing;
    }
    const pending = this.run(target, label, "auto");
    this.started.set(key, pending);
    return pending;
  }

  async run(target: ShellTarget, label: string, mode: ShellMode): Promise<void> {
    if (mode !== "auto") {
      const key = targetKey(target);
      // Two runs at once could both append a block; let a running one finish.
      const running = this.started.get(key);
      // After a manual run this launch has nothing left for auto to do.
      this.started.set(key, Promise.resolve());
      if (running) {
        await running;
      }
    }
    let result: ShellReport | Error;
    try {
      result = await this.deps.run(target, mode);
    } catch (err) {
      result = err instanceof Error ? err : new Error(String(err));
    }
    this.deps.onReport(label, result, mode);
  }

  /** Auto runs only speak up when they changed a file. */
  static notable(result: ShellReport | Error, mode: ShellMode): boolean {
    if (mode !== "auto") {
      return true;
    }
    return !(result instanceof Error) && result.outcome === "applied";
  }
}
