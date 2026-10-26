// 标签状态机。不碰 DOM，会话的打开和关闭由调用方注入。

import { deviceTabTitle } from "./connection-menu.js";
import type { ZoomStep } from "./zoom.js";

export type TabStatus = "connecting" | "connected" | "ended" | "failed";

export type TabKind = "home" | "session";

export type TabTrack = "device" | "host";

export interface TabModel {
  key: string;
  kind: TabKind;
  deviceName?: string;
  /** Set when this tab is a command relay. Absent for the default method. */
  connectionName?: string;
  hostId?: string;
  track?: TabTrack;
  sessionId?: string;
  status: TabStatus;
  title: string;
  detail?: string;
}

export interface TabStripDeps {
  openSession(name: string, connectionName?: string): Promise<{ id: string }>;
  closeSession(id: string): Promise<void>;
  limit?: number;
  onChange?(): void;
}

export type ShortcutAction =
  | { type: "close" }
  | { type: "jump"; index: number | "last" }
  | { type: "zoom"; step: ZoomStep };

export interface ShortcutKey {
  key: string;
  /** Physical key. Lets zoom work where Shift or the layout changes `key`. */
  code?: string;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
  isComposing?: boolean;
}

const HOME_KEY = "home";
const HOME_TITLE = "拓扑";

export const TAB_LIMIT_MESSAGE = "最多 8 个标签";

export function sessionLimitMessage(limit: number): string {
  if (limit === 8) {
    return TAB_LIMIT_MESSAGE;
  }
  return `最多 ${limit} 个标签`;
}

export function statusLabel(status: TabStatus): string {
  switch (status) {
    case "connecting":
      return "连接中";
    case "connected":
      return "已连接";
    case "ended":
      return "已断开";
    case "failed":
      return "连接失败";
  }
}

export function shortcutAction(
  event: ShortcutKey,
  platform: "mac" | "windows",
): ShortcutAction | null {
  if (event.isComposing || event.altKey) {
    return null;
  }
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  if (platform === "mac") {
    if (!event.metaKey || event.ctrlKey) {
      return null;
    }
    const zoom = zoomStep(event);
    if (zoom) {
      return { type: "zoom", step: zoom };
    }
    if (key === "w") {
      return { type: "close" };
    }
    return jumpIndex(key);
  }
  if (event.metaKey || !event.ctrlKey) {
    return null;
  }
  // Bare Ctrl+- would otherwise reach the shell as ^_; zoom takes it on purpose.
  const zoom = zoomStep(event);
  if (zoom) {
    return { type: "zoom", step: zoom };
  }
  if (key === "w" && event.shiftKey) {
    return { type: "close" };
  }
  if (event.key === "F4" && !event.shiftKey) {
    return { type: "close" };
  }
  if (event.shiftKey) {
    return null;
  }
  return jumpIndex(key);
}

/** `=` / `+` zoom in, `-` / `_` out, `0` resets, on the main row or the numpad. */
function zoomStep(event: ShortcutKey): ZoomStep | null {
  const code = event.code ?? "";
  if (code === "Equal" || code === "NumpadAdd" || event.key === "=" || event.key === "+") {
    return "in";
  }
  if (code === "Minus" || code === "NumpadSubtract" || event.key === "-" || event.key === "_") {
    return "out";
  }
  if (code === "Digit0" || code === "Numpad0" || event.key === "0") {
    return "reset";
  }
  return null;
}

function jumpIndex(key: string): ShortcutAction | null {
  if (key >= "1" && key <= "8") {
    return { type: "jump", index: Number(key) };
  }
  if (key === "9") {
    return { type: "jump", index: "last" };
  }
  return null;
}

export class TabStrip {
  readonly tabs: TabModel[] = [];
  activeKey: string = HOME_KEY;
  private seq = 0;
  private readonly closedIds = new Set<string>();

  constructor(private readonly deps: TabStripDeps) {
    this.tabs.push({
      key: HOME_KEY,
      kind: "home",
      status: "connected",
      title: HOME_TITLE,
    });
  }

  maxSessions(): number {
    return this.deps.limit ?? 8;
  }

  canOpen(): boolean {
    return this.sessionCount() < this.maxSessions();
  }

  sessionCount(): number {
    return this.tabs.filter((tab) => tab.kind === "session").length;
  }

  async openDevice(name: string, connectionName?: string): Promise<string> {
    const selected = connectionName ? connectionName : undefined;
    return this.begin(name, () => this.deps.openSession(name, selected), {
      track: "device",
      connectionName: selected,
    });
  }

  async openHost(
    name: string,
    hostId: string,
    open: () => Promise<{ id: string }>,
  ): Promise<string> {
    return this.begin(name, open, { track: "host", hostId });
  }

  activate(key: string): void {
    if (!this.tabs.some((tab) => tab.key === key) || this.activeKey === key) {
      return;
    }
    this.activeKey = key;
    this.emit();
  }

  close(key: string): boolean {
    const index = this.tabs.findIndex((tab) => tab.key === key);
    if (index < 0) {
      return false;
    }
    const tab = this.tabs[index];
    if (!tab || tab.kind !== "session") {
      return false;
    }
    this.tabs.splice(index, 1);
    this.requestCloseSession(tab.sessionId);
    this.refreshTitles();
    if (this.activeKey === key) {
      const next = this.tabs[index] ?? this.tabs[index - 1] ?? this.tabs[0];
      if (next) {
        this.activeKey = next.key;
      }
    }
    this.emit();
    return true;
  }

  closeActive(): boolean {
    return this.close(this.activeKey);
  }

  jump(index: number | "last"): void {
    const tab = index === "last" ? this.tabs[this.tabs.length - 1] : this.tabs[index - 1];
    if (!tab) {
      return;
    }
    this.activeKey = tab.key;
    this.emit();
  }

  onConnected(key: string, id: string): void {
    const tab = this.session(key);
    if (!tab) {
      return;
    }
    tab.status = "connected";
    tab.sessionId = id;
    tab.detail = undefined;
    this.emit();
  }

  onExit(key: string, code?: number | null): void {
    const tab = this.session(key);
    if (!tab) {
      return;
    }
    tab.status = "ended";
    tab.detail = code === undefined || code === null ? "退出码未知" : `退出码 ${code}`;
    this.emit();
  }

  onFailed(key: string, message: string): void {
    const tab = this.session(key);
    if (!tab) {
      return;
    }
    tab.status = "failed";
    tab.detail = message;
    this.emit();
  }

  prepareReconnect(key: string): boolean {
    const tab = this.session(key);
    if (!tab || tab.status !== "ended") {
      return false;
    }
    tab.status = "connecting";
    tab.detail = undefined;
    this.emit();
    return true;
  }

  private async begin(
    label: string,
    open: () => Promise<{ id: string }>,
    fields: { track: TabTrack; hostId?: string; connectionName?: string },
  ): Promise<string> {
    if (!this.canOpen()) {
      throw new Error(sessionLimitMessage(this.maxSessions()));
    }
    const key = `t${++this.seq}`;
    const tab: TabModel = {
      key,
      kind: "session",
      deviceName: label,
      connectionName: fields.connectionName,
      status: "connecting",
      title: deviceTabTitle(label, fields.connectionName),
      track: fields.track,
      hostId: fields.hostId,
    };
    this.tabs.push(tab);
    this.activeKey = key;
    this.refreshTitles();
    this.emit();
    try {
      const opened = await open();
      const current = this.tabs.find((item) => item.key === key);
      if (!current) {
        this.requestCloseSession(opened.id);
        return key;
      }
      current.sessionId = opened.id;
      return key;
    } catch (err) {
      const current = this.tabs.find((item) => item.key === key);
      if (current && current.status === "connecting") {
        current.status = "failed";
        current.detail = err instanceof Error ? err.message : String(err);
        this.emit();
      }
      throw err;
    }
  }

  private session(key: string): TabModel | undefined {
    const tab = this.tabs.find((item) => item.key === key);
    if (!tab || tab.kind !== "session") {
      return undefined;
    }
    return tab;
  }

  private requestCloseSession(id: string | undefined): void {
    if (!id || this.closedIds.has(id)) {
      return;
    }
    this.closedIds.add(id);
    void this.deps.closeSession(id);
  }

  private refreshTitles(): void {
    const seen = new Map<string, number>();
    for (const tab of this.tabs) {
      if (tab.kind === "home") {
        tab.title = HOME_TITLE;
        continue;
      }
      const name = deviceTabTitle(tab.deviceName ?? "", tab.connectionName);
      const count = (seen.get(name) ?? 0) + 1;
      seen.set(name, count);
      tab.title = count === 1 ? name : `${name} (${count})`;
    }
  }

  private emit(): void {
    this.deps.onChange?.();
  }
}
