// #25 前端失败测试：首页在直连/ProxyJump 与中转命令之间选择，模板校验与后端白名单一致。
import assert from "node:assert/strict";
import { test } from "node:test";
import { buildDiagram, renderDiagramSvg } from "./diagram.js";
import {
  DEFAULT_CONNECTION_LABEL,
  connectionChoices,
  deviceTabTitle,
  menuAction,
  relayCommandError,
  renderConnectionMenu,
  topologySessionFields,
} from "./connection-menu.js";
import { TAB_LIMIT_MESSAGE, TabStrip } from "./tabs.js";
import type { TopologyView } from "./topology.js";

const dev = (name: string, extra: Record<string, unknown> = {}) => ({
  name,
  host: "192.0.2.10",
  port: 22,
  user: "me",
  mobile: false,
  presence: "none" as const,
  ...extra,
});

const VIEW = {
  configPath: "<config>/topology.json",
  state: "ok",
  regions: [
    {
      name: "示例",
      devices: [dev("app-server", { host: "app.example.com" })],
    },
  ],
} as unknown as TopologyView;

test("tab limit sentence is defined once and used by the diagram", () => {
  assert.equal(TAB_LIMIT_MESSAGE, "最多 8 个标签");
  const blocked = renderDiagramSvg(buildDiagram(VIEW), { canOpen: false });
  assert.equal(blocked.includes(TAB_LIMIT_MESSAGE), true);
  const open = renderDiagramSvg(buildDiagram(VIEW), { canOpen: true });
  assert.equal(open.includes(TAB_LIMIT_MESSAGE), false);
});

test("a device without connections opens the default method directly", () => {
  assert.equal(connectionChoices(undefined), null);
  assert.equal(connectionChoices([]), null);
});

test("connections add a default entry and one entry per name", () => {
  assert.equal(DEFAULT_CONNECTION_LABEL, "默认（直连/ProxyJump）");
  assert.deepEqual(connectionChoices(["经跳板命令", "nc 中转"]), [
    { connectionName: null, label: DEFAULT_CONNECTION_LABEL },
    { connectionName: "经跳板命令", label: "经跳板命令" },
    { connectionName: "nc 中转", label: "nc 中转" },
  ]);
});

test("tab title is the device name, or the device name and connection name", () => {
  assert.equal(deviceTabTitle("内网服务器"), "内网服务器");
  assert.equal(deviceTabTitle("内网服务器", null), "内网服务器");
  assert.equal(deviceTabTitle("内网服务器", ""), "内网服务器");
  assert.equal(deviceTabTitle("内网服务器", "经跳板命令"), "内网服务器 · 经跳板命令");
});

test("opening a relay names the tab and remembers the connection", async () => {
  const seen: Array<string | undefined> = [];
  const strip = new TabStrip({
    openSession: async (_name: string, connectionName?: string) => {
      seen.push(connectionName);
      return { id: "s1" };
    },
    closeSession: async () => {},
  });
  await strip.openDevice("app-server", "经跳板命令");
  await strip.openDevice("app-server");
  await strip.openDevice("app-server", "经跳板命令");
  assert.equal(strip.tabs[1]?.title, "app-server · 经跳板命令");
  assert.equal(strip.tabs[1]?.connectionName, "经跳板命令");
  assert.equal(strip.tabs[2]?.title, "app-server");
  assert.equal(strip.tabs[2]?.connectionName, undefined);
  assert.equal(strip.tabs[3]?.title, "app-server · 经跳板命令 (2)");
  assert.deepEqual(seen, ["经跳板命令", undefined, "经跳板命令"]);
});

test("session invoke sends connectionName only when a relay is chosen", () => {
  assert.deepEqual(topologySessionFields("app-server"), { deviceName: "app-server" });
  assert.deepEqual(topologySessionFields("app-server", null), { deviceName: "app-server" });
  assert.deepEqual(topologySessionFields("app-server", ""), { deviceName: "app-server" });
  assert.deepEqual(topologySessionFields("app-server", "经跳板命令"), {
    deviceName: "app-server",
    connectionName: "经跳板命令",
  });
});

test("connection menu lists both methods and refuses a jump-host secret prompt", () => {
  const html = renderConnectionMenu('a<b>&"', ["经跳板命令", "x<y"]);
  assert.equal(html.includes('role="menu"'), true);
  assert.equal(html.includes('role="menuitem"'), true);
  assert.equal(html.includes(DEFAULT_CONNECTION_LABEL), true);
  assert.equal(html.includes("经跳板命令"), true);
  assert.equal(html.includes('data-connection=""'), true);
  assert.equal(html.includes('data-connection="经跳板命令"'), true);
  assert.equal(html.includes('data-connection="x&lt;y"'), true);
  assert.equal(html.includes("<b>"), false);
  assert.equal(html.includes("&lt;b&gt;"), true);
  assert.equal(html.includes("&amp;"), true);
  assert.equal(html.includes("&quot;"), true);
  assert.equal(html.includes('class="relay-command-input"'), true);
  assert.equal(html.includes("不支持在跳板机上输入口令"), true);
});

test("menu keys move, choose, and close", () => {
  assert.deepEqual(menuAction("ArrowDown", 3, 0), { type: "move", index: 1 });
  assert.deepEqual(menuAction("ArrowDown", 3, 2), { type: "move", index: 0 });
  assert.deepEqual(menuAction("ArrowUp", 3, 0), { type: "move", index: 2 });
  assert.deepEqual(menuAction("ArrowDown", 1, 0), { type: "move", index: 0 });
  assert.deepEqual(menuAction("Enter", 3, 1), { type: "choose" });
  assert.deepEqual(menuAction(" ", 3, 1), { type: "choose" });
  assert.deepEqual(menuAction("Escape", 3, 1), { type: "close" });
  assert.deepEqual(menuAction("Tab", 3, 1), { type: "ignore" });
});

test("relay template allowlist matches the backend", () => {
  assert.equal(relayCommandError("tailscale nc %h %p"), null);
  assert.equal(relayCommandError("ab_c.d/e:f=g@h+i-j %h %p"), null);
  assert.equal(relayCommandError("nc %h %p %%"), null);
  assert.equal(relayCommandError("nc %h%p"), null);

  const empty = relayCommandError("");
  assert.ok(empty !== null && empty.includes("不能为空"), empty ?? "");
  const dashed = relayCommandError("-nc %h %p");
  assert.ok(dashed !== null && dashed.includes("不能以 - 开头"), dashed ?? "");

  const blocked = [
    ";",
    "|",
    "&",
    "$",
    "`",
    "'",
    '"',
    "<",
    ">",
    "(",
    ")",
    "\\",
    "*",
    "?",
    "!",
    "~",
    "#",
  ];
  for (const ch of blocked) {
    const err = relayCommandError(`nc %h %p${ch}`);
    assert.ok(err !== null, `应拒绝 ${ch}`);
  }
  for (const sample of ["nc %h %p\n", "nc %h %p\r", "nc %h %p\u0001", "nc %h %p\u007f"]) {
    assert.ok(relayCommandError(sample) !== null, sample);
  }
  const unknown = relayCommandError("nc %r %h %p");
  assert.ok(unknown !== null && unknown.includes("%r"), unknown ?? "");
  assert.ok(relayCommandError("nc %h %p %") !== null, "单独的 %");
  assert.ok(relayCommandError("nc %%h %p") !== null, "%%h 不是 %h");
  assert.ok(relayCommandError("nc %p") !== null, "缺 %h");
  assert.ok(relayCommandError("nc %h") !== null, "缺 %p");
  assert.ok(relayCommandError("%%") !== null, "只有 %%");
});
