// #31 前端失败测试：关闭连接中/已连接的标签改用应用内确认，不再依赖 window.confirm（macOS WKWebView 上直接返回 false）。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CLOSE_CONFIRM_MESSAGE,
  closeConfirmKey,
  needsCloseConfirm,
  renderCloseConfirm,
} from "./close-confirm.js";

test("only connecting and connected tabs ask before closing", () => {
  assert.equal(needsCloseConfirm("connecting"), true);
  assert.equal(needsCloseConfirm("connected"), true);
  assert.equal(needsCloseConfirm("ended"), false);
  assert.equal(needsCloseConfirm("failed"), false);
});

test("Escape cancels; other keys are left to the focused button", () => {
  assert.equal(closeConfirmKey("Escape"), "cancel");
  assert.equal(closeConfirmKey("Enter"), null);
  assert.equal(closeConfirmKey(" "), null);
  assert.equal(closeConfirmKey("w"), null);
});

test("the dialog names the tab and offers cancel and end", () => {
  assert.equal(CLOSE_CONFIRM_MESSAGE, "结束该会话？");
  const html = renderCloseConfirm("DMIT <1>");
  assert.ok(html.includes('role="alertdialog"'));
  assert.ok(html.includes('aria-modal="true"'));
  assert.ok(html.includes(CLOSE_CONFIRM_MESSAGE));
  assert.ok(html.includes("DMIT &lt;1&gt;"), "标题要转义");
  assert.equal(html.includes("DMIT <1>"), false);
  assert.ok(html.includes('data-choice="cancel"'));
  assert.ok(html.includes('data-choice="confirm"'));
});
