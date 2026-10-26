import { invoke } from "@tauri-apps/api/core";
import type { ShellMode, ShellReport, ShellTarget } from "./remote-shell.ts";

/** The backend plans the same route as the tab and runs a fixed script. */
export function runRemoteShell(target: ShellTarget, mode: ShellMode): Promise<ShellReport> {
  const fields =
    target.kind === "device"
      ? { deviceName: target.deviceName, connectionName: target.connectionName ?? null }
      : { host: target.host, user: target.user, port: target.port };
  return invoke<ShellReport>("remote_shell_run", { ...fields, mode });
}
