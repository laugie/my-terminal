// 1.0.4 回归测试：Shift + 数字等可打印字符必须发给 shell，窗口只拦截定义好的快捷键。
import assert from "node:assert/strict";
import { test } from "node:test";
import type { ShortcutKey } from "./tabs.js";
import { terminalKey } from "./terminal-keys.js";

/** US layout: Shift + key code → the character it types. */
const SHIFTED: Array<[string, string]> = [
  ["Digit1", "!"],
  ["Digit2", "@"],
  ["Digit3", "#"],
  ["Digit4", "$"],
  ["Digit5", "%"],
  ["Digit6", "^"],
  ["Digit7", "&"],
  ["Digit8", "*"],
  ["Digit9", "("],
  ["Digit0", ")"],
  ["Minus", "_"],
  ["Equal", "+"],
  ["BracketLeft", "{"],
  ["BracketRight", "}"],
  ["Backslash", "|"],
  ["Semicolon", ":"],
  ["Quote", '"'],
  ["Comma", "<"],
  ["Period", ">"],
  ["Slash", "?"],
  ["Backquote", "~"],
];

const PLATFORMS = ["mac", "windows"] as const;

test("Shift + number and punctuation keys go to the shell on both platforms", () => {
  for (const platform of PLATFORMS) {
    for (const hasSelection of [false, true]) {
      for (const [code, key] of SHIFTED) {
        const event: ShortcutKey = { key, code, shiftKey: true };
        assert.equal(terminalKey(event, platform, hasSelection), null, `${platform} Shift+${code} (${key})`);
      }
    }
  }
});

test("Shift + letters and plain keys go to the shell on both platforms", () => {
  for (const platform of PLATFORMS) {
    for (const letter of "abcdefghijklmnopqrstuvwxyz") {
      const code = `Key${letter.toUpperCase()}`;
      assert.equal(terminalKey({ key: letter.toUpperCase(), code, shiftKey: true }, platform, true), null);
      assert.equal(terminalKey({ key: letter, code }, platform, true), null);
    }
    for (const digit of "0123456789") {
      assert.equal(terminalKey({ key: digit, code: `Digit${digit}` }, platform, false), null);
    }
  }
});

test("Option and AltGr symbols go to the shell", () => {
  assert.equal(terminalKey({ key: "€", code: "Digit2", altKey: true, shiftKey: true }, "mac", false), null);
  assert.equal(
    terminalKey({ key: "€", code: "KeyE", altKey: true, ctrlKey: true }, "windows", true),
    null,
  );
});

test("Windows shortcuts still work", () => {
  const w = (event: ShortcutKey, hasSelection = false) => terminalKey(event, "windows", hasSelection);
  assert.deepEqual(w({ key: "C", code: "KeyC", ctrlKey: true, shiftKey: true }, true), { type: "clipboard", action: "copy" });
  assert.deepEqual(w({ key: "V", code: "KeyV", ctrlKey: true, shiftKey: true }), { type: "clipboard", action: "paste" });
  assert.deepEqual(w({ key: "c", code: "KeyC", ctrlKey: true }, true), { type: "clipboard", action: "copy-clear" });
  assert.equal(w({ key: "c", code: "KeyC", ctrlKey: true }, false), null, "Ctrl+C without a selection is ^C");
  assert.deepEqual(w({ key: "T", code: "KeyT", ctrlKey: true, shiftKey: true }), { type: "new" });
  assert.deepEqual(w({ key: "W", code: "KeyW", ctrlKey: true, shiftKey: true }), { type: "shortcut", action: { type: "close" } });
  assert.deepEqual(w({ key: "F4", code: "F4", ctrlKey: true }), { type: "shortcut", action: { type: "close" } });
  assert.deepEqual(w({ key: "3", code: "Digit3", ctrlKey: true }), { type: "shortcut", action: { type: "jump", index: 3 } });
  assert.deepEqual(w({ key: "=", code: "Equal", ctrlKey: true }), { type: "shortcut", action: { type: "zoom", step: "in" } });
  assert.deepEqual(w({ key: "Tab", code: "Tab", ctrlKey: true }), { type: "cycle", step: 1 });
  assert.deepEqual(w({ key: "Tab", code: "Tab", ctrlKey: true, shiftKey: true }), { type: "cycle", step: -1 });
  assert.equal(w({ key: "w", code: "KeyW", ctrlKey: true }), null, "bare Ctrl+W stays with the shell");
  assert.equal(w({ key: "$", code: "Digit4", ctrlKey: true, shiftKey: true }), null, "Ctrl+Shift+4 is not a jump");
});

test("macOS shortcuts still work, and Ctrl+C stays an interrupt", () => {
  const m = (event: ShortcutKey, hasSelection = false) => terminalKey(event, "mac", hasSelection);
  assert.deepEqual(m({ key: "c", code: "KeyC", metaKey: true }, true), { type: "clipboard", action: "copy" });
  assert.deepEqual(m({ key: "v", code: "KeyV", metaKey: true }), { type: "clipboard", action: "paste" });
  assert.equal(m({ key: "c", code: "KeyC", ctrlKey: true }, true), null);
  assert.deepEqual(m({ key: "t", code: "KeyT", metaKey: true }), { type: "new" });
  assert.deepEqual(m({ key: "w", code: "KeyW", metaKey: true }), { type: "shortcut", action: { type: "close" } });
  assert.deepEqual(m({ key: "9", code: "Digit9", metaKey: true }), { type: "shortcut", action: { type: "jump", index: "last" } });
  assert.deepEqual(m({ key: "-", code: "Minus", metaKey: true }), { type: "shortcut", action: { type: "zoom", step: "out" } });
  assert.deepEqual(m({ key: "Tab", code: "Tab", ctrlKey: true }), { type: "cycle", step: 1 });
});

test("composition is never intercepted", () => {
  for (const platform of PLATFORMS) {
    assert.equal(terminalKey({ key: "c", code: "KeyC", metaKey: true, ctrlKey: true, isComposing: true }, platform, true), null);
    assert.equal(terminalKey({ key: "Process", code: "Digit4", shiftKey: true, isComposing: true }, platform, false), null);
  }
});
