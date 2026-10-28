// #43 前端失败测试：终端里的复制、粘贴按键，Windows 右键，多行粘贴确认。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  clipboardKey,
  pasteConfirmText,
  pasteLineCount,
  pasteNeedsConfirm,
  rightClickAction,
} from "./clipboard.js";

test("mac: Cmd+C copies and Cmd+V pastes", () => {
  assert.equal(clipboardKey({ key: "c", metaKey: true }, "mac", true), "copy");
  assert.equal(clipboardKey({ key: "c", metaKey: true }, "mac", false), "copy");
  assert.equal(clipboardKey({ key: "v", metaKey: true }, "mac", false), "paste");
  assert.equal(clipboardKey({ key: "V", metaKey: true }, "mac", false), "paste");
});

test("mac: Ctrl+C stays an interrupt, even with a selection", () => {
  assert.equal(clipboardKey({ key: "c", ctrlKey: true }, "mac", true), null);
  assert.equal(clipboardKey({ key: "c", ctrlKey: true }, "mac", false), null);
  assert.equal(clipboardKey({ key: "v", ctrlKey: true }, "mac", false), null);
});

test("mac: other modifiers are not copy or paste", () => {
  assert.equal(clipboardKey({ key: "c", metaKey: true, altKey: true }, "mac", true), null);
  assert.equal(clipboardKey({ key: "c", metaKey: true, ctrlKey: true }, "mac", true), null);
  assert.equal(clipboardKey({ key: "C", metaKey: true, shiftKey: true }, "mac", true), null);
  assert.equal(clipboardKey({ key: "c" }, "mac", true), null);
});

test("windows: Ctrl+Shift+C copies and Ctrl+Shift+V pastes", () => {
  assert.equal(clipboardKey({ key: "C", ctrlKey: true, shiftKey: true }, "windows", true), "copy");
  assert.equal(clipboardKey({ key: "C", ctrlKey: true, shiftKey: true }, "windows", false), "copy");
  assert.equal(clipboardKey({ key: "V", ctrlKey: true, shiftKey: true }, "windows", false), "paste");
});

test("windows: Ctrl+C copies and clears a selection, otherwise interrupts", () => {
  assert.equal(clipboardKey({ key: "c", ctrlKey: true }, "windows", true), "copy-clear");
  assert.equal(clipboardKey({ key: "c", ctrlKey: true }, "windows", false), null);
});

test("windows: bare Ctrl+V and Cmd keys are left alone", () => {
  assert.equal(clipboardKey({ key: "v", ctrlKey: true }, "windows", false), null);
  assert.equal(clipboardKey({ key: "c", metaKey: true }, "windows", true), null);
  assert.equal(clipboardKey({ key: "c", ctrlKey: true, altKey: true }, "windows", true), null);
});

test("non-Latin layouts fall back to the physical key", () => {
  assert.equal(clipboardKey({ key: "с", code: "KeyC", metaKey: true }, "mac", true), "copy");
  assert.equal(
    clipboardKey({ key: "М", code: "KeyV", ctrlKey: true, shiftKey: true }, "windows", false),
    "paste",
  );
  // A Latin layout keeps the letter, wherever the key sits.
  assert.equal(clipboardKey({ key: "j", code: "KeyC", metaKey: true }, "mac", true), null);
});

test("composition is never copy or paste", () => {
  assert.equal(clipboardKey({ key: "c", metaKey: true, isComposing: true }, "mac", true), null);
  assert.equal(clipboardKey({ key: "c", ctrlKey: true, isComposing: true }, "windows", true), null);
});

test("right-click copies a selection, otherwise pastes", () => {
  assert.equal(rightClickAction(true), "copy-clear");
  assert.equal(rightClickAction(false), "paste");
});

test("any line break asks first; one line does not", () => {
  assert.equal(pasteNeedsConfirm("ls -la"), false);
  assert.equal(pasteNeedsConfirm(""), false);
  assert.equal(pasteNeedsConfirm("ls\npwd"), true);
  assert.equal(pasteNeedsConfirm("ls\r\npwd"), true);
  assert.equal(pasteNeedsConfirm("ls\rpwd"), true);
  // A trailing newline would run the command at once.
  assert.equal(pasteNeedsConfirm("rm -rf build\n"), true);
});

test("line count ignores one trailing newline", () => {
  assert.equal(pasteLineCount("a"), 1);
  assert.equal(pasteLineCount("a\n"), 1);
  assert.equal(pasteLineCount("a\nb"), 2);
  assert.equal(pasteLineCount("a\r\nb\r\nc\r\n"), 3);
  assert.equal(pasteLineCount("a\n\nb"), 3);
});

test("the confirmation names the line count and starts on Cancel", () => {
  const text = pasteConfirmText("echo 1\necho 2\necho 3\n");
  assert.ok(text.message.includes("3 行"), text.message);
  assert.equal(text.confirmLabel, "粘贴");
  assert.equal(text.focusCancel, true);
  assert.equal(text.detail, "echo 1\necho 2\necho 3");
});

test("the preview is short: five lines, long lines cut", () => {
  const lines = Array.from({ length: 8 }, (_, index) => `line ${index + 1}`);
  lines[1] = "x".repeat(200);
  const text = pasteConfirmText(lines.join("\n"));
  assert.ok(text.message.includes("8 行"), text.message);
  const shown = (text.detail ?? "").split("\n");
  assert.equal(shown.length, 6);
  assert.equal(shown[0], "line 1");
  assert.ok(shown[1].length <= 81 && shown[1].endsWith("…"), shown[1]);
  assert.equal(shown[4], "line 5");
  assert.equal(shown[5], "…另有 3 行");
});
