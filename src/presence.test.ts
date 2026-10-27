// #41 前端失败测试：拓扑在线状态。后端每轮返回每台设备的状态，前端按名字合并，
// 只改 presence；只有 presence 变化时 3D 场景原地更新，不重建。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  PRESENCE_INTERVAL_MS,
  applyPresence,
  connectedDevices,
  sameLayout,
} from "./presence.js";
import type { TabModel } from "./tabs.js";
import type { TopologyView } from "./topology.js";

const dev = (name: string, extra: Record<string, unknown> = {}) => ({
  name, host: "192.0.2.10", port: 22, user: "me", mobile: false, presence: "none" as const, ...extra,
});
const VIEW = {
  configPath: "<config>/topology.json",
  state: "ok",
  regions: [
    { name: "家", devices: [dev("nas", { presence: "online" }), dev("mac")] },
    { name: "云", devices: [dev("jump"), dev("app", { jump: "jump" })] },
  ],
} as unknown as TopologyView;

const presences = (view: TopologyView) =>
  Object.fromEntries(view.regions.flatMap((region) => region.devices.map((device) => [device.name, device.presence])));

test("#41 results replace presence by device name and nothing else", () => {
  const next = applyPresence(VIEW, [
    { name: "nas", presence: "offline" },
    { name: "mac", presence: "online" },
    { name: "app", presence: "unknown" },
    { name: "not-in-the-file", presence: "online" },
  ]);
  assert.deepEqual(presences(next), { nas: "offline", mac: "online", jump: "none", app: "unknown" });
  assert.deepEqual(presences(VIEW), { nas: "online", mac: "none", jump: "none", app: "none" }, "原视图不变");
  assert.equal(next.regions[1]!.devices[1]!.jump, "jump");
  assert.equal(next.configPath, VIEW.configPath);
  assert.ok(sameLayout(VIEW, next), "只改了在线状态");
});

test("#41 values the page does not know are ignored", () => {
  const next = applyPresence(VIEW, [{ name: "mac", presence: "sleeping" as never }]);
  assert.equal(presences(next).mac, "none");
});

test("#41 no change keeps the same view object", () => {
  assert.equal(applyPresence(VIEW, []), VIEW);
  assert.equal(applyPresence(VIEW, [{ name: "nas", presence: "online" }]), VIEW);
});

test("#41 a view that is not ok is left alone", () => {
  const missing = { configPath: "", state: "missing", regions: [] } as unknown as TopologyView;
  assert.equal(applyPresence(missing, [{ name: "nas", presence: "online" }]), missing);
});

test("#41 the layout changes when anything but presence changes", () => {
  const moved = {
    ...VIEW,
    regions: [VIEW.regions[0]!, { ...VIEW.regions[1]!, devices: [dev("jump")] }],
  } as TopologyView;
  assert.equal(sameLayout(VIEW, moved), false);
  const renamed = { ...VIEW, regions: [{ ...VIEW.regions[0]!, name: "家 2" }, VIEW.regions[1]!] } as TopologyView;
  assert.equal(sameLayout(VIEW, renamed), false);
  const rehosted = applyPresence(VIEW, []);
  assert.ok(sameLayout(VIEW, rehosted));
  const port = { ...VIEW, regions: [{ ...VIEW.regions[0]!, devices: [dev("nas", { port: 2222 }), dev("mac")] }, VIEW.regions[1]!] } as TopologyView;
  assert.equal(sameLayout(VIEW, port), false);
});

test("#41 only connected device tabs count, each name once", () => {
  const tab = (key: string, extra: Partial<TabModel>): TabModel =>
    ({ key, kind: "session", status: "connected", title: key, ...extra }) as TabModel;
  const tabs: TabModel[] = [
    { key: "home", kind: "home", status: "connected", title: "拓扑" },
    tab("a", { track: "device", deviceName: "nas" }),
    tab("b", { track: "device", deviceName: "nas" }),
    tab("c", { track: "device", deviceName: "app", status: "connecting" }),
    tab("d", { track: "device", deviceName: "mac", status: "ended" }),
    tab("e", { track: "host", hostId: "h1" }),
  ];
  assert.deepEqual(connectedDevices(tabs), ["nas"]);
});

test("#41 the page asks about every two and a half minutes", () => {
  assert.equal(PRESENCE_INTERVAL_MS, 150_000);
});
