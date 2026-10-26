// #35 前端失败测试：终端配色、字体列表，以及选择后按顺序逐个保存。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ANSI_KEYS,
  AppearanceController,
  DEFAULT_FONT,
  DEFAULT_THEME,
  FONTS,
  THEMES,
  contrastRatio,
  fontOf,
  themeOf,
} from "./appearance.js";

test("ids match the backend lists; the first entry is the default", () => {
  assert.deepEqual(
    THEMES.map((theme) => theme.id),
    ["midnight", "deep-sea", "one-dark", "dracula", "tokyo-night", "nord", "solarized-dark", "paper"],
  );
  assert.deepEqual(
    FONTS.map((font) => font.id),
    ["jetbrains-mono", "system", "sf-mono", "menlo", "cascadia-mono", "consolas", "fira-code", "meslo-nerd"],
  );
  assert.equal(DEFAULT_THEME, THEMES[0]?.id);
  assert.equal(DEFAULT_FONT, FONTS[0]?.id);
});

test("every theme defines all 16 ANSI colors as #rrggbb", () => {
  assert.equal(ANSI_KEYS.length, 16);
  for (const theme of THEMES) {
    for (const key of [...ANSI_KEYS, "background", "foreground", "cursor"] as const) {
      assert.match(theme.palette[key], /^#[0-9a-f]{6}$/, `${theme.id}.${key}`);
    }
  }
});

test("file-type colors are distinct and readable on the background", () => {
  // ls: directories blue, executables green, links cyan, archives red, images magenta.
  const kinds = ["red", "green", "yellow", "blue", "magenta", "cyan"] as const;
  for (const theme of THEMES) {
    const colors = kinds.map((key) => theme.palette[key]);
    assert.equal(new Set(colors).size, kinds.length, `${theme.id} 六种颜色互不相同`);
    assert.ok(contrastRatio(theme.palette.foreground, theme.palette.background) >= 4.5, `${theme.id} 正文`);
    if (theme.light) {
      continue;
    }
    for (const key of kinds) {
      const ratio = contrastRatio(theme.palette[key], theme.palette.background);
      assert.ok(ratio >= 3, `${theme.id}.${key} 对比度 ${ratio.toFixed(2)}`);
    }
  }
});

test("a light theme asks xterm to keep a minimum contrast", () => {
  for (const theme of THEMES) {
    if (theme.light) {
      assert.ok((theme.minimumContrastRatio ?? 1) >= 4.5, theme.id);
    }
  }
  assert.ok(THEMES.some((theme) => theme.light), "至少一套浅色");
});

test("unknown ids fall back to the defaults", () => {
  assert.equal(themeOf("neon").id, DEFAULT_THEME);
  assert.equal(themeOf(undefined).id, DEFAULT_THEME);
  assert.equal(themeOf("nord").id, "nord");
  assert.equal(fontOf(42).id, DEFAULT_FONT);
  assert.equal(fontOf("menlo").id, "menlo");
});

test("font stacks keep CJK and generic monospace fallbacks", () => {
  for (const font of FONTS) {
    assert.match(font.stack, /monospace$/, font.id);
    assert.match(font.stack, /"PingFang SC"/, font.id);
    assert.match(font.stack, /"Microsoft YaHei"/, font.id);
    assert.equal((font.stack.match(/"/g) ?? []).length % 2, 0, `${font.id} 引号成对`);
  }
  assert.deepEqual(
    FONTS.filter((font) => font.bundled).map((font) => font.id),
    ["jetbrains-mono"],
  );
  assert.match(fontOf("jetbrains-mono").stack, /^"JetBrains Mono", /);
});

test("contrast ratio follows WCAG", () => {
  assert.equal(Math.round(contrastRatio("#000000", "#ffffff")), 21);
  assert.equal(contrastRatio("#777777", "#777777"), 1);
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

test("changes apply at once and save one at a time, in order", async () => {
  const saved: Array<{ theme?: string; font?: string }> = [];
  const pending: Array<() => void> = [];
  let inFlight = 0;
  let maxInFlight = 0;
  const appearance = new AppearanceController({
    save(change) {
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      saved.push(change);
      const d = deferred<{ theme: string; font: string; readOnly: boolean }>();
      pending.push(() => {
        inFlight -= 1;
        d.resolve({ theme: "dracula", font: "jetbrains-mono", readOnly: false });
      });
      return d.promise;
    },
  });
  appearance.load({ theme: "midnight", font: "jetbrains-mono", readOnly: false });
  void appearance.set({ theme: "nord" });
  const last = appearance.set({ theme: "dracula", font: "menlo" });
  assert.equal(appearance.theme, "dracula");
  assert.equal(appearance.font, "menlo");
  for (let i = 0; i < 2; i += 1) {
    await new Promise((r) => setTimeout(r, 0));
    const next = pending.shift();
    if (!next) {
      throw new Error(`第 ${i + 1} 次保存没有发出`);
    }
    next();
  }
  await last;
  assert.deepEqual(saved, [{ theme: "nord" }, { theme: "dracula", font: "menlo" }]);
  assert.equal(maxInFlight, 1);
  assert.equal(appearance.font, "menlo", "旧的保存结果不能把选择改回去");
});

test("load normalizes unknown ids; a failed save keeps the choice and reports it", async () => {
  const appearance = new AppearanceController({
    save: async () => {
      throw new Error("无法保存配置");
    },
  });
  appearance.load({ theme: "neon", font: "comic", readOnly: false });
  assert.equal(appearance.theme, DEFAULT_THEME);
  assert.equal(appearance.font, DEFAULT_FONT);
  await appearance.set({ theme: "paper" });
  assert.equal(appearance.theme, "paper");
  assert.equal(appearance.message, "无法保存配置");
});

test("setting the same value does not save", async () => {
  let saves = 0;
  const appearance = new AppearanceController({
    save: async () => {
      saves += 1;
      return { theme: "midnight", font: "jetbrains-mono", readOnly: false };
    },
  });
  appearance.load({ theme: "midnight", font: "jetbrains-mono", readOnly: false });
  await appearance.set({ theme: "midnight" });
  await appearance.set({ theme: "unknown-id" });
  assert.equal(saves, 0);
});
