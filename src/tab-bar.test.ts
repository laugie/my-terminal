// #39 前端失败测试：「+」要紧跟最后一个标签。Chromium 里复现不出来；推测是 macOS WKWebView
// 按标签的 flex-basis（200px）计算标签栏宽度，而标签实际按内容宽度显示，于是「+」和标签之间
// 留出一段空白。标签宽度改为由内容决定、最多 200px，任何引擎算出的标签栏宽度都等于标签实际宽度。
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const css = readFileSync("src/styles.css", "utf8");

function rule(selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`(^|\\n)${escaped} \\{([^}]*)\\}`).exec(css);
  assert(match, `${selector} 规则存在`);
  return match[2] ?? "";
}

test("#39 a tab is as wide as its content, at most 200px", () => {
  const tab = rule(".tab");
  assert.match(tab, /\n\s*flex: 0 1 auto;/);
  assert.match(tab, /\n\s*max-width: 200px;/);
  assert.match(tab, /\n\s*min-width: 44px;/);
});

test("#39 the tab list is only as wide as its tabs", () => {
  const list = rule(".tabbar-list");
  assert.match(list, /\n\s*flex: 0 1 auto;/);
  assert.match(list, /\n\s*min-width: 0;/);
  assert.equal(/\n\s*(flex-grow|width):/.test(list), false, "标签列表不拉伸");
});
