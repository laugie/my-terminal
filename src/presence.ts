// 拓扑在线状态。后端在本机判断（会话、Tailscale、SSH 端口探测，不在设备上运行任何东西），
// 这里只负责按名字合并结果。纯函数，不碰 DOM。

import type { TabModel } from "./tabs.js";
import type { Presence, TopologyView } from "./topology.js";

/** Between rounds while the topology page is showing. */
export const PRESENCE_INTERVAL_MS = 150_000;

export interface DevicePresence {
  name: string;
  presence: Presence;
}

const KNOWN: ReadonlySet<string> = new Set(["online", "offline", "unknown", "none"]);

/** The view with presence replaced by device name. The same object when nothing changed. */
export function applyPresence(view: TopologyView, results: readonly DevicePresence[]): TopologyView {
  if (view.state !== "ok" || results.length === 0) {
    return view;
  }
  const byName = new Map<string, Presence>();
  for (const result of results) {
    if (KNOWN.has(result.presence)) {
      byName.set(result.name, result.presence);
    }
  }
  let changed = false;
  const regions = view.regions.map((region) => {
    let regionChanged = false;
    const devices = region.devices.map((device) => {
      const presence = byName.get(device.name);
      if (presence === undefined || presence === device.presence) {
        return device;
      }
      regionChanged = true;
      return { ...device, presence };
    });
    if (!regionChanged) {
      return region;
    }
    changed = true;
    return { ...region, devices };
  });
  return changed ? { ...view, regions } : view;
}

/** Device names with a connected session tab, each once. */
export function connectedDevices(tabs: readonly TabModel[]): string[] {
  const names = new Set<string>();
  for (const tab of tabs) {
    if (tab.kind === "session" && tab.track === "device" && tab.status === "connected" && tab.deviceName) {
      names.add(tab.deviceName);
    }
  }
  return [...names];
}

/** Whether two views differ at most in presence, so a drawing can be updated in place. */
export function sameLayout(a: TopologyView, b: TopologyView): boolean {
  if (a === b) {
    return true;
  }
  return layoutKey(a) === layoutKey(b);
}

function layoutKey(view: TopologyView): string {
  return JSON.stringify({
    ...view,
    regions: view.regions.map((region) => ({
      ...region,
      devices: region.devices.map(({ presence: _presence, ...device }) => device),
    })),
  });
}
