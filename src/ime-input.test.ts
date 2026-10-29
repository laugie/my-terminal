// 1.0.6 回归测试：输入法直接上屏的中文标点、数字，不论事件先后，每个字符只发一次。
import assert from "node:assert/strict";
import { test } from "node:test";
import { ImeDirectInput } from "./ime-input.js";

type Step =
  | ["down", number, boolean?]
  | ["press", number]
  | ["up", number]
  | ["input", string, string?, boolean?]
  | ["compositionend"];

/** Text sent to the shell for a sequence of textarea events. */
function sent(steps: Step[]): string {
  const ime = new ImeDirectInput();
  let out = "";
  for (const step of steps) {
    switch (step[0]) {
      case "down":
        ime.key({ type: "keydown", keyCode: step[1], isComposing: step[2] ?? false });
        break;
      case "press":
        ime.key({ type: "keypress", keyCode: step[1], isComposing: false });
        break;
      case "up":
        ime.key({ type: "keyup", keyCode: step[1], isComposing: false });
        break;
      case "input":
        out += ime.input({ data: step[1], inputType: step[2] ?? "insertText", isComposing: step[3] ?? false }) ?? "";
        break;
      case "compositionend":
        ime.compositionEnd();
        break;
    }
  }
  return out;
}

/** One Pinyin punctuation key, as macOS WKWebView orders it: input before keydown 229. */
const inputFirst = (char: string, code: number): Step[] => [["input", char], ["down", 229], ["up", code]];
const keyFirst = (char: string, code: number): Step[] => [["down", 229], ["input", char], ["up", code]];
const lateCommit = (char: string, code: number): Step[] => [["down", 229], ["up", code], ["input", char]];

test("the xterm.js #6144 trace: Shift held, two ！ arrive", () => {
  assert.equal(sent([["down", 16], ...inputFirst("！", 49), ...inputFirst("！", 49), ["up", 16]]), "！！");
});

test("Shift+1 … Shift+0 under Pinyin type every full-width symbol once", () => {
  const symbols = ["！", "＠", "＃", "￥", "％", "……", "＆", "＊", "（", "）"];
  const codes = [49, 50, 51, 52, 53, 54, 55, 56, 57, 48];
  for (const order of [inputFirst, keyFirst, lateCommit]) {
    const steps: Step[] = [["down", 16]];
    symbols.forEach((symbol, i) => steps.push(...order(symbol, codes[i])));
    steps.push(["up", 16]);
    assert.equal(sent(steps), symbols.join(""), order.name);
  }
});

test("ten fast presses in mixed order send ten characters", () => {
  const orders = [inputFirst, keyFirst, lateCommit, inputFirst, lateCommit, keyFirst, inputFirst, inputFirst, keyFirst, lateCommit];
  const steps: Step[] = [["down", 16], ...orders.flatMap((order) => order("！", 49)), ["up", 16]];
  assert.equal(sent(steps), "！".repeat(10));
  // Rollover: the next key goes down before the last one comes up.
  const rollover: Step[] = [["down", 16], ["input", "！"], ["down", 229], ["input", "＠"], ["down", 229], ["up", 49], ["up", 50], ["up", 16]];
  assert.equal(sent(rollover), "！＠");
});

test("digits and plain punctuation committed by the input method are sent", () => {
  assert.equal(sent([...inputFirst("1", 49), ...keyFirst("，", 188), ...lateCommit("。", 190)]), "1，。");
});

test("xterm never sees keydown 229 outside a composition", () => {
  const ime = new ImeDirectInput();
  assert.equal(ime.key({ type: "keydown", keyCode: 229, isComposing: false }), true);
  assert.equal(ime.key({ type: "keydown", keyCode: 229, isComposing: true }), false, "composition keys stay with xterm");
  assert.equal(ime.key({ type: "keydown", keyCode: 65, isComposing: false }), false, "ordinary keys stay with xterm");
  assert.equal(ime.key({ type: "keyup", keyCode: 229, isComposing: false }), false);
});

test("composed Chinese is left to xterm, then punctuation works again", () => {
  const steps: Step[] = [
    ["down", 229],
    ["input", "n", "insertCompositionText", true],
    ["up", 78],
    ["down", 229, true],
    ["input", "ni", "insertCompositionText", true],
    ["up", 73],
    // Space picks 你好. Whatever input follows the commit is xterm's.
    ["down", 229, true],
    ["compositionend"],
    ["input", "你好", "insertText"],
    ["up", 32],
    ["down", 16],
    ...inputFirst("！", 49),
    ["up", 16],
  ];
  assert.equal(sent(steps), "！");
});

test("a key xterm typed from keypress is not sent again by its input event", () => {
  // Space and A–Z: xterm sends them from keypress, and the webview still fires input.
  assert.equal(sent([["down", 32], ["press", 32], ["input", " "], ["up", 32]]), "");
  assert.equal(sent([["down", 16], ["down", 65], ["press", 65], ["input", "A"], ["up", 65], ["up", 16]]), "");
  // A Pinyin key pressed before the space comes up is still sent.
  assert.equal(sent([["down", 32], ["press", 32], ["input", " "], ["down", 16], ...inputFirst("！", 49), ["up", 32]]), "！");
});

test("other input events are left to xterm", () => {
  const ime = new ImeDirectInput();
  for (const inputType of ["insertFromPaste", "insertCompositionText", "deleteContentBackward", "insertReplacementText"]) {
    assert.equal(ime.input({ data: "x", inputType, isComposing: false }), null, inputType);
  }
  assert.equal(ime.input({ data: "", inputType: "insertText", isComposing: false }), null);
  assert.equal(ime.input({ data: null, inputType: "insertText", isComposing: false }), null);
  assert.equal(ime.input({ data: "啊", inputType: "insertText", isComposing: true }), null);
});
