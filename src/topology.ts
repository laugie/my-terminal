import { Channel, invoke } from "@tauri-apps/api/core";
import { topologySessionFields } from "./connection-menu.js";
import type { DevicePresence } from "./presence.ts";
import type { SessionEvent } from "./session.ts";

export type Presence = "online" | "offline" | "unknown" | "none";

export interface TopologyDevice {
  name: string;
  host: string;
  port: number;
  user: string;
  mobile: boolean;
  presence: Presence;
  /** Config `type` when it was a string. Omitted when the file has no type. */
  type?: string;
  /** Jump target device name. Omitted when the device has no jump. */
  jump?: string;
  /** Command-relay names. Empty or omitted when the device only has the default method. */
  connections?: string[];
}

export interface TopologyRegion {
  name: string;
  devices: TopologyDevice[];
}

export interface TopologyView {
  configPath: string;
  state: "ok" | "missing" | "empty" | "error";
  message?: string;
  regions: TopologyRegion[];
}

export function loadTopology(): Promise<TopologyView> {
  return invoke<TopologyView>("load_topology");
}

/** The backend looks the device up. This call does not send a host, jump, or command. */
export function openTopologyDevice(
  deviceName: string,
  onEvent: (event: SessionEvent) => void,
  connectionName?: string | null,
): Promise<{ id: string }> {
  const onEventChannel = new Channel<SessionEvent>();
  onEventChannel.onmessage = onEvent;
  return invoke<{ id: string }>("open_session", {
    ...topologySessionFields(deviceName, connectionName),
    onEvent: onEventChannel,
  });
}

/**
 * One online-state round. The backend reads hosts from topology.json; the page
 * sends only the names of devices it already has connected tabs for.
 */
export function probePresence(connected: string[]): Promise<DevicePresence[]> {
  return invoke<DevicePresence[]>("probe_presence", { connected });
}
