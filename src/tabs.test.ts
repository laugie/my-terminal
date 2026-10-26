// #21 前端失败测试：src/tabs.test.ts；被测 src/tabs.ts（纯状态机 + 快捷键映射，无 DOM，无新依赖）。
// 期望接口：
//   new TabStrip({ openSession(name) => Promise<{id}>, closeSession(id) => Promise<void>, limit })
//   strip.tabs: {key, kind:"home"|"session", deviceName?, sessionId?, status, title}[]，strip.activeKey
//   openDevice(name) / activate(key) / close(key) / onConnected(key,id) / onExit(key) / onFailed(key,msg)
//   statusLabel(status) ；shortcutAction(ev, platform) -> {type:"close"} | {type:"jump", index} | null
import assert from "node:assert/strict";
import { test } from "node:test";
import { TabStrip, shortcutAction, statusLabel } from "./tabs.js";

function strip(limit = 8) {
  const opened: string[] = [], closed: string[] = [];
  let n = 0;
  const s = new TabStrip({
    openSession: async (name: string) => { opened.push(name); return { id: `s${++n}` }; },
    closeSession: async (id: string) => { closed.push(id); },
    limit,
  });
  return { s, opened, closed };
}
const tick = () => new Promise((r) => setTimeout(r, 0));

test("home is pinned first and cannot be closed", async () => {
  const { s } = strip();
  assert.equal(s.tabs[0].kind, "home");
  assert.equal(s.tabs[0].title, "拓扑");
  await s.openDevice("app-server");
  assert.equal(s.tabs[0].kind, "home", "新标签加在后面");
  assert.equal(s.close(s.tabs[0].key), false);
  assert.equal(s.tabs[0].kind, "home");
});

test("clicking an icon always opens a new tab and switches to it", async () => {
  const { s, opened } = strip();
  const a = await s.openDevice("app-server");
  assert.equal(s.activeKey, a);
  s.activate(s.tabs[0].key);
  const b = await s.openDevice("app-server");
  assert.ok(a !== b);
  assert.equal(s.activeKey, b);
  assert.deepEqual(opened, ["app-server", "app-server"]);
  assert.equal(s.tabs.length, 3);
});

test("same device: independent sessions, numbered titles", async () => {
  const { s } = strip();
  await s.openDevice("app-server"); await s.openDevice("app-server"); await s.openDevice("bastion");
  assert.deepEqual(s.tabs.map((t) => t.title), ["拓扑", "app-server", "app-server (2)", "bastion"]);
  assert.deepEqual(s.tabs.slice(1).map((t) => t.sessionId), ["s1", "s2", "s3"]);
});

test("status lifecycle and labels", async () => {
  const { s } = strip();
  const p = s.openDevice("app-server");
  assert.equal(s.tabs[1].status, "connecting");
  const key = await p;
  s.onConnected(key, "s1");
  assert.equal(s.tabs[1].status, "connected");
  s.onExit(key, 0);
  assert.equal(s.tabs[1].status, "ended");
  const k2 = await s.openDevice("bastion");
  s.onFailed(k2, "ssh 启动失败");
  assert.equal(s.tabs[2].status, "failed");
  assert.equal(statusLabel("connecting"), "连接中");
  assert.equal(statusLabel("connected"), "已连接");
  assert.equal(statusLabel("ended"), "已断开");
  assert.equal(statusLabel("failed"), "连接失败");
});

test("closing one tab calls close_session only for its session; others untouched", async () => {
  const { s, closed } = strip();
  const a = await s.openDevice("a"); const b = await s.openDevice("b"); await s.openDevice("c");
  s.onConnected(a, "s1"); s.onConnected(b, "s2");
  s.activate(b);
  assert.equal(s.close(b), true);
  await tick();
  assert.deepEqual(closed, ["s2"]);
  assert.deepEqual(s.tabs.map((t) => t.deviceName ?? "home"), ["home", "a", "c"]);
  assert.equal(s.tabs[1].status, "connected");
  assert.equal(s.activeKey, s.tabs[2].key, "关闭当前后激活右侧邻居");
});

test("closing the last session tab returns to home", async () => {
  const { s } = strip();
  const a = await s.openDevice("a");
  s.close(a);
  assert.equal(s.activeKey, s.tabs[0].key);
});

test("tab limit counts session tabs only and reports clearly", async () => {
  const { s, opened } = strip(2);
  await s.openDevice("a"); await s.openDevice("b");
  let msg = "";
  try { await s.openDevice("c"); } catch (e) { msg = String(e); }
  assert.ok(/最多 2 个标签/.test(msg), msg);
  assert.equal(opened.length, 2);
});

test("shortcuts: mac uses Cmd, Windows uses Ctrl; 9 = last", () => {
  const k = (key: string, m: Partial<{ metaKey: boolean; ctrlKey: boolean; shiftKey: boolean; altKey: boolean }>) =>
    ({ key, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, ...m });
  assert.deepEqual(shortcutAction(k("w", { metaKey: true }), "mac"), { type: "close" });
  assert.equal(shortcutAction(k("w", { ctrlKey: true }), "mac"), null, "mac 上 Ctrl+W 留给终端");
  assert.deepEqual(shortcutAction(k("3", { metaKey: true }), "mac"), { type: "jump", index: 3 });
  assert.deepEqual(shortcutAction(k("9", { metaKey: true }), "mac"), { type: "jump", index: "last" });
  assert.deepEqual(shortcutAction(k("1", { ctrlKey: true }), "windows"), { type: "jump", index: 1 });
  assert.deepEqual(shortcutAction(k("9", { ctrlKey: true }), "windows"), { type: "jump", index: "last" });
  assert.deepEqual(shortcutAction(k("W", { ctrlKey: true, shiftKey: true }), "windows"), { type: "close" });
  assert.equal(shortcutAction(k("1", { metaKey: true }), "windows"), null, "Windows 上 Win 键不处理");
  assert.equal(shortcutAction(k("1", { ctrlKey: true, altKey: true }), "windows"), null, "AltGr 不触发");
  assert.deepEqual(shortcutAction(k("0", { metaKey: true }), "mac"), { type: "zoom", step: "reset" }, "#33 Cmd+0 恢复 100%");
});

test("#33 zoom: Cmd/Ctrl + = / - / 0, with Shift and numpad variants", () => {
  type Mods = Partial<{ metaKey: boolean; ctrlKey: boolean; shiftKey: boolean; altKey: boolean; isComposing: boolean; code: string }>;
  const k = (key: string, m: Mods) =>
    ({ key, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, isComposing: false, ...m });
  const zin = { type: "zoom", step: "in" };
  const zout = { type: "zoom", step: "out" };
  const zreset = { type: "zoom", step: "reset" };
  for (const [platform, mod] of [["mac", { metaKey: true }], ["windows", { ctrlKey: true }]] as const) {
    assert.deepEqual(shortcutAction(k("=", { ...mod, code: "Equal" }), platform), zin, platform);
    assert.deepEqual(shortcutAction(k("+", { ...mod, shiftKey: true, code: "Equal" }), platform), zin, `${platform} Shift`);
    assert.deepEqual(shortcutAction(k("+", { ...mod, code: "NumpadAdd" }), platform), zin, `${platform} 小键盘`);
    assert.deepEqual(shortcutAction(k("-", { ...mod, code: "Minus" }), platform), zout, platform);
    assert.deepEqual(shortcutAction(k("_", { ...mod, shiftKey: true, code: "Minus" }), platform), zout, `${platform} Shift`);
    assert.deepEqual(shortcutAction(k("-", { ...mod, code: "NumpadSubtract" }), platform), zout, `${platform} 小键盘`);
    assert.deepEqual(shortcutAction(k("0", { ...mod, code: "Digit0" }), platform), zreset, platform);
    assert.deepEqual(shortcutAction(k("0", { ...mod, code: "Numpad0" }), platform), zreset, `${platform} 小键盘`);
    assert.equal(shortcutAction(k("=", { ...mod, altKey: true, code: "Equal" }), platform), null, `${platform} Alt 不触发`);
    assert.equal(shortcutAction(k("=", { ...mod, isComposing: true }), platform), null, `${platform} 组字不触发`);
  }
  assert.deepEqual(shortcutAction(k("ß", { metaKey: true, code: "Minus" }), "mac"), zout, "按物理键位");
  assert.equal(shortcutAction(k("=", { ctrlKey: true, code: "Equal" }), "mac"), null, "mac 上 Ctrl+= 留给终端");
  assert.equal(shortcutAction(k("=", { metaKey: true, code: "Equal" }), "windows"), null, "Windows 上 Win 键不处理");
  assert.equal(shortcutAction(k("=", { code: "Equal" }), "windows"), null, "不按修饰键不处理");
  assert.equal(shortcutAction(k("0", { ctrlKey: true, metaKey: true }), "mac"), null);
});

test("windows Ctrl+F4 closes; bare Ctrl+W and composing do not", () => {
  const k = (key: string, m: Partial<{ metaKey: boolean; ctrlKey: boolean; shiftKey: boolean; altKey: boolean; isComposing: boolean }>) =>
    ({ key, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, isComposing: false, ...m });
  assert.deepEqual(shortcutAction(k("F4", { ctrlKey: true }), "windows"), { type: "close" });
  assert.equal(shortcutAction(k("w", { ctrlKey: true }), "windows"), null);
  assert.equal(shortcutAction(k("w", { metaKey: true, isComposing: true }), "mac"), null);
});

test("Cmd/Ctrl+W on home does nothing; jump beyond count is ignored", async () => {
  const { s, closed } = strip();
  await s.openDevice("a");
  s.activate(s.tabs[0].key);
  assert.equal(s.closeActive(), false);
  assert.equal(s.tabs.length, 2);
  s.jump(5);
  assert.equal(s.activeKey, s.tabs[0].key);
  s.jump("last");
  assert.equal(s.activeKey, s.tabs[1].key);
  assert.deepEqual(closed, []);
});
