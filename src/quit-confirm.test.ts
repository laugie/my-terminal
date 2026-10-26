// #37 前端失败测试：点窗口关闭按钮或 Cmd+Q 时，有会话就先用应用内确认框问一次。
import assert from "node:assert/strict";
import { test } from "node:test";
import { renderCloseConfirm } from "./close-confirm.js";
import { QuitPrompt, quitMessage } from "./quit-confirm.js";

test("the message counts the sessions that will end", () => {
  assert.equal(quitMessage(1), "退出 my-terminal？1 个会话会断开。");
  assert.equal(quitMessage(3), "退出 my-terminal？3 个会话会断开。");
});

test("the in-app dialog can carry the quit wording", () => {
  const html = renderCloseConfirm("退出 my-terminal", { message: quitMessage(2), confirmLabel: "退出" });
  assert.ok(html.includes("2 个会话会断开"));
  assert.ok(html.includes('data-choice="confirm">退出<'));
  assert.ok(html.includes('role="alertdialog"'));
  const plain = renderCloseConfirm("app-server");
  assert.ok(plain.includes("结束该会话？"), "关标签的默认文字不变");
  assert.ok(plain.includes('data-choice="confirm">结束会话<'));
});

function harness(answer: boolean) {
  const calls: string[] = [];
  let release!: (value: boolean) => void;
  const prompt = new QuitPrompt({
    shown: async () => {
      calls.push("shown");
    },
    ask: (message) => {
      calls.push(`ask:${message}`);
      return new Promise<boolean>((resolve) => {
        release = resolve;
      });
    },
    confirm: async () => {
      calls.push("confirm");
    },
  });
  return { calls, prompt, answer: () => release(answer) };
}

test("confirming quits; the backend hears the dialog is up first", async () => {
  const h = harness(true);
  const done = h.prompt.request(2);
  await new Promise((r) => setTimeout(r, 0));
  h.answer();
  await done;
  assert.deepEqual(h.calls, ["shown", `ask:${quitMessage(2)}`, "confirm"]);
});

test("cancelling keeps the app open", async () => {
  const h = harness(false);
  const done = h.prompt.request(1);
  await new Promise((r) => setTimeout(r, 0));
  h.answer();
  await done;
  assert.deepEqual(h.calls, ["shown", `ask:${quitMessage(1)}`]);
});

test("a second request while the dialog is up opens no second dialog", async () => {
  const h = harness(false);
  const first = h.prompt.request(1);
  const second = h.prompt.request(1);
  await new Promise((r) => setTimeout(r, 0));
  h.answer();
  await Promise.all([first, second]);
  assert.equal(h.calls.filter((call) => call.startsWith("ask:")).length, 1);
  assert.equal(h.calls.filter((call) => call === "shown").length, 2, "每次请求都回报，后端不会误判网页无响应");
});
