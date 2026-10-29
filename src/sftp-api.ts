import { Channel, invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ShellTarget } from "./remote-shell.ts";
import { cssPoint, type Direction, type FailReason } from "./sftp-model.ts";

export interface FileEntry {
  name: string;
  /** Full path, passed back as a source or destination. */
  path: string;
  dir: boolean;
  link: boolean;
  size: number;
  modified: number | null;
}

export interface Listing {
  path: string;
  /** null at the top. On Windows "" is the drive list. */
  parent: string | null;
  entries: FileEntry[];
  truncated: boolean;
}

export type TransferEvent =
  | { kind: "progress"; done: number; total: number }
  | { kind: "done"; done: number; total: number }
  | { kind: "failed"; reason: FailReason; message: string; done: number; total: number };

/** Same route as the tab: the backend plans the ssh arguments. */
export function sftpOpen(target: ShellTarget): Promise<{ id: string; home: string }> {
  const fields =
    target.kind === "device"
      ? { deviceName: target.deviceName, connectionName: target.connectionName ?? null }
      : { host: target.host, user: target.user, port: target.port };
  return invoke("sftp_open", fields);
}

export function sftpList(id: string, path: string): Promise<Listing> {
  return invoke("sftp_list", { id, path });
}

export function sftpClose(id: string): Promise<void> {
  return invoke("sftp_close", { id });
}

export function localDefaultDir(): Promise<string> {
  return invoke("local_default_dir");
}

export function localList(path: string): Promise<Listing> {
  return invoke("local_list", { path });
}

export function transferStart(
  id: string,
  direction: Direction,
  source: string,
  destDir: string,
  onEvent: (event: TransferEvent) => void,
): Promise<string> {
  const channel = new Channel<TransferEvent>();
  channel.onmessage = onEvent;
  return invoke("transfer_start", { id, direction, source, destDir, onEvent: channel });
}

export function transferCancel(id: string): Promise<void> {
  return invoke("transfer_cancel", { id });
}

export type NativeDrag =
  | { type: "enter"; paths: string[]; x: number; y: number }
  | { type: "over"; x: number; y: number }
  | { type: "drop"; paths: string[]; x: number; y: number }
  | { type: "leave" };

/**
 * Files dragged in from Finder or Explorer. Positions arrive in physical
 * pixels; they are turned into CSS pixels with the window's real ratio,
 * which already includes the UI zoom.
 */
export async function onNativeDrag(handler: (event: NativeDrag) => void): Promise<() => void> {
  let ratio = window.devicePixelRatio || 1;
  const measure = async () => {
    try {
      const size = await getCurrentWindow().innerSize();
      if (window.innerWidth > 0 && size.width > 0) {
        ratio = size.width / window.innerWidth;
      }
    } catch {
      ratio = window.devicePixelRatio || 1;
    }
  };
  void measure();
  window.addEventListener("resize", () => {
    void measure();
  });
  return getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;
    if (payload.type === "leave") {
      handler({ type: "leave" });
      return;
    }
    if (payload.type === "enter") {
      // The zoom may have changed since the last drag.
      void measure();
    }
    const { x, y } = cssPoint(payload.position, ratio);
    if (payload.type === "over") {
      handler({ type: "over", x, y });
    } else {
      handler({ type: payload.type, paths: payload.paths, x, y });
    }
  });
}
