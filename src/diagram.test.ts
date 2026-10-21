// #20 前端失败测试：src/diagram.test.ts；被测 src/diagram.ts（纯函数 + EventTarget，无 DOM 依赖，无新依赖）。
import assert from "node:assert/strict";
import { test } from "node:test";
import { bindDeviceActivation, buildDiagram, deviceKind, renderDiagramSvg } from "./diagram.js";
import type { TopologyView } from "./topology.js";

const dev = (name: string, extra: Record<string, unknown> = {}) => ({
  name, host: "192.0.2.10", port: 22, user: "me", mobile: false, presence: "none" as const, ...extra,
});
const VIEW = {
  configPath: "<config>/topology.json", state: "ok",
  regions: [
    { name: "示例办公室", devices: [dev("office-router"), dev("my-macbook", { mobile: true }), dev("打印机", { type: "toaster" })] },
    { name: "示例云", devices: [dev("bastion", { type: "cloud", host: "jump.example.com" }), dev("app-server", { jump: "bastion", host: "app.example.com" })] },
    { name: "示例家庭", devices: [dev("home-box", { jump: "bastion", type: "server" })] },
  ],
} as unknown as TopologyView;

test("explicit type wins (case-insensitive)", () => {
  assert.equal(deviceKind({ name: "office-router", type: "nas" }), "nas");
  assert.equal(deviceKind({ name: "x", type: "Windows" }), "windows");
});

test("guess by name keywords", () => {
  const cases: Array<[string, string]> = [
    ["office-router", "router"], ["GW-01", "router"], ["示例路由器", "router"], ["openwrt", "router"],
    ["home-nas", "nas"], ["synology-ds", "nas"], ["群晖", "nas"],
    ["my-macbook", "mac"], ["mac-mini", "mac"], ["imac", "mac"],
    ["win-desktop", "windows"], ["windows-pc", "windows"],
    ["vps-1", "cloud"], ["cloud-a", "cloud"], ["ec2-web", "cloud"], ["云主机", "cloud"],
    ["app-server", "server"], ["db-srv", "server"], ["服务器", "server"],
  ];
  for (const [name, kind] of cases) assert.equal(deviceKind({ name }), kind, name);
});

test("no false positives on substrings", () => {
  assert.equal(deviceKind({ name: "machine-01" }), "generic", "machine 不是 mac");
  assert.equal(deviceKind({ name: "window-cleaner" }), "generic");
  assert.equal(deviceKind({ name: "twin" }), "generic", "twin 不是 win");
});

test("unknown or missing → generic", () => {
  assert.equal(deviceKind({ name: "打印机", type: "toaster" }), "generic");
  assert.equal(deviceKind({ name: "thing" }), "generic");
  assert.equal(deviceKind({ name: "x", type: "" }), "generic");
});

test("model: one group per region, every device a node, jump edges incl. cross-region", () => {
  const m = buildDiagram(VIEW);
  assert.deepEqual(m.groups.map((g) => g.name), ["示例办公室", "示例云", "示例家庭"]);
  assert.equal(m.nodes.length, 6);
  const edges = m.edges.map((e) => `${e.from}->${e.to}:${e.crossRegion}`).sort();
  assert.deepEqual(edges, ["bastion->app-server:false", "bastion->home-box:true"]);
  for (const n of m.nodes) {
    const g = m.groups.find((x) => x.name === n.region)!;
    assert.ok(n.x >= g.x && n.y >= g.y && n.x <= g.x + g.width && n.y <= g.y + g.height, `${n.name} 在框内`);
  }
  const names = new Set(m.nodes.map((n) => `${Math.round(n.x)},${Math.round(n.y)}`));
  assert.equal(names.size, 6, "节点不重叠");
});

test("model: dangling jump does not crash or draw", () => {
  const v = { ...VIEW, regions: [{ name: "r", devices: [dev("a", { jump: "missing" })] }] } as unknown as TopologyView;
  assert.equal(buildDiagram(v).edges.length, 0);
});

test("svg: groups, icons, edges, mobile badge, visible name and address, a11y", () => {
  const svg = renderDiagramSvg(buildDiagram(VIEW));
  assert.ok(svg.startsWith("<svg"));
  assert.equal((svg.match(/class="region-frame"/g) ?? []).length, 3);
  assert.ok(svg.includes('data-kind="router"') && svg.includes('data-kind="cloud"') && svg.includes('data-kind="generic"'));
  assert.equal((svg.match(/class="jump-edge[^"]*"/g) ?? []).length, 2);
  assert.ok(/class="jump-edge cross-region"/.test(svg));
  assert.equal((svg.match(/class="mobile-badge"/g) ?? []).length, 1);
  assert.ok(svg.includes(">my-macbook<") && svg.includes("app.example.com"));
  assert.equal((svg.match(/role="button"/g) ?? []).length, 6);
  assert.equal((svg.match(/tabindex="0"/g) ?? []).length, 6);
});

test("svg: unconnectable device nodes are aria-disabled and the title includes the old hint", () => {
  const render = renderDiagramSvg as (
    model: ReturnType<typeof buildDiagram>,
    options?: { canOpen?: boolean },
  ) => string;
  const blocked = render(buildDiagram(VIEW), { canOpen: false });
  const devices = blocked.split('<g class="device').slice(1);
  assert.equal(devices.length, 6);
  for (const device of devices) {
    const openTag = device.slice(0, device.indexOf(">"));
    assert.ok(openTag.includes('aria-disabled="true"'), openTag);
    const title = device.match(/<title>([^<]*)<\/title>/)?.[1] ?? "";
    assert.ok(title.includes("最多 8 个标签"), title);
  }
  const mac = devices.find((device) => device.includes("my-macbook")) ?? "";
  const macTitle = mac.match(/<title>([^<]*)<\/title>/)?.[1] ?? "";
  assert.ok(macTitle.includes("移动设备") && macTitle.includes("最多 8 个标签"), macTitle);
  const open = render(buildDiagram(VIEW), { canOpen: true });
  assert.equal(open.includes('aria-disabled="true"'), false);
  assert.equal(open.includes("最多 8 个标签"), false);
});

test("svg escapes names", () => {
  const v = { ...VIEW, regions: [{ name: "<r>", devices: [dev('a"<b>&')] }] } as unknown as TopologyView;
  const svg = renderDiagramSvg(buildDiagram(v));
  assert.ok(!svg.includes("<b>") && svg.includes("&lt;b&gt;") && svg.includes("&amp;"));
  assert.ok(svg.includes("&quot;"), "设备名里的双引号要转义");
});

test("click, Enter and Space activate the same connect call as #16", () => {
  const calls: string[] = [];
  const t = new EventTarget();
  bindDeviceActivation(t, "app-server", (n) => calls.push(n));
  t.dispatchEvent(new Event("click"));
  for (const key of ["Enter", " ", "a", "Tab"]) {
    const e = Object.assign(new Event("keydown", { cancelable: true }), { key });
    t.dispatchEvent(e);
  }
  assert.deepEqual(calls, ["app-server", "app-server", "app-server"]);
});
