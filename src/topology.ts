import { Channel, invoke } from "@tauri-apps/api/core";
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

/** The backend looks the device up. This call does not send a host or jump. */
export function openTopologyDevice(
  deviceName: string,
  onEvent: (event: SessionEvent) => void,
): Promise<{ id: string }> {
  const onEventChannel = new Channel<SessionEvent>();
  onEventChannel.onmessage = onEvent;
  return invoke<{ id: string }>("open_session", {
    deviceName,
    onEvent: onEventChannel,
  });
}
