import { ANSI_KEYS, FONTS, THEMES, themeOf, type AppearanceController } from "./appearance.ts";
import { fontInstalled } from "./appearance-api.ts";
import { authSetLock, authSettings, authSetup, type LockSettings } from "./auth.ts";
import { errorText } from "./hosts.ts";
import { mountCredentialPrompt } from "./login.ts";
import { planLockChange } from "./startup.ts";
import { toolButton } from "./tab-bar.ts";
import { shortcutPlatform } from "./terminal.ts";
import { zoomLabel, type ZoomController } from "./zoom.ts";

export interface SettingsPanel {
  /** Show the current zoom level and which steps are possible. */
  renderZoom(): void;
  /** Show the current terminal theme and font. */
  renderAppearance(): void;
}

/**
 * Gear button dialog: UI zoom, terminal theme and font, and a switch that
 * requires login on the next launch. The button itself is created by the
 * shell so the top bar owns the slot.
 */
export function mountSettings(
  button: HTMLButtonElement,
  zoom: ZoomController,
  appearance: AppearanceController,
): SettingsPanel {
  const slot = button.parentElement;
  if (!(slot instanceof HTMLElement)) {
    throw new Error("missing settings slot");
  }

  const dialog = document.createElement("div");
  dialog.className = "settings-popover";
  dialog.hidden = true;
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-label", "设置");

  const title = document.createElement("h2");
  title.className = "section-title";
  title.id = "settings-title";
  title.textContent = "设置";
  dialog.setAttribute("aria-labelledby", title.id);

  const row = document.createElement("label");
  row.className = "settings-switch";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  const caption = document.createElement("span");
  caption.textContent = "启动时需要登录";
  row.append(checkbox, caption);

  const notice = document.createElement("p");
  notice.className = "settings-notice";

  const hint = document.createElement("p");
  hint.className = "login-hint";

  const dialogError = document.createElement("p");
  dialogError.className = "form-error";
  dialogError.setAttribute("role", "alert");

  const promptHost = document.createElement("div");

  const closeButton = document.createElement("button");
  closeButton.type = "button";
  closeButton.className = "button";
  closeButton.textContent = "关闭";

  const zoomRow = document.createElement("div");
  zoomRow.className = "settings-zoom";
  zoomRow.setAttribute("role", "group");
  const zoomCaption = document.createElement("span");
  zoomCaption.id = "settings-zoom-caption";
  zoomCaption.textContent = "界面缩放";
  zoomRow.setAttribute("aria-labelledby", zoomCaption.id);
  const zoomOut = toolButton("minus", "缩小", { iconOnly: true });
  const zoomValue = document.createElement("output");
  zoomValue.className = "settings-zoom-value";
  zoomValue.setAttribute("aria-live", "polite");
  const zoomIn = toolButton("plus", "放大", { iconOnly: true });
  const zoomReset = document.createElement("button");
  zoomReset.type = "button";
  zoomReset.className = "button";
  zoomReset.textContent = "重置";
  const zoomControls = document.createElement("div");
  zoomControls.className = "settings-zoom-controls";
  zoomControls.append(zoomOut, zoomValue, zoomIn, zoomReset);
  zoomRow.append(zoomCaption, zoomControls);
  const mod = shortcutPlatform() === "mac" ? "⌘" : "Ctrl+";
  const zoomHint = document.createElement("p");
  zoomHint.className = "login-hint";
  zoomHint.textContent = `快捷键：${mod}= 放大，${mod}- 缩小，${mod}0 恢复 100%`;
  const zoomNotice = document.createElement("p");
  zoomNotice.className = "settings-notice";
  zoomOut.addEventListener("click", () => {
    void zoom.step("out");
  });
  zoomIn.addEventListener("click", () => {
    void zoom.step("in");
  });
  zoomReset.addEventListener("click", () => {
    void zoom.step("reset");
  });

  const divider = document.createElement("hr");
  divider.className = "settings-divider";

  const themeField = selectField("配色", "settings-theme");
  for (const theme of THEMES) {
    themeField.select.append(new Option(theme.label, theme.id));
  }
  const swatches = document.createElement("div");
  swatches.className = "settings-swatches";
  swatches.setAttribute("aria-hidden", "true");
  const fontField = selectField("终端字体", "settings-font");
  const fontOptions = FONTS.map((choice) => {
    const installed = choice.bundled || fontInstalled(choice.family);
    const option = new Option(installed ? choice.label : `${choice.label}（未安装）`, choice.id);
    option.dataset.installed = installed ? "true" : "false";
    fontField.select.append(option);
    return option;
  });
  const appearanceNotice = document.createElement("p");
  appearanceNotice.className = "settings-notice";
  themeField.select.addEventListener("change", () => {
    void appearance.set({ theme: themeField.select.value });
  });
  fontField.select.addEventListener("change", () => {
    void appearance.set({ font: fontField.select.value });
  });

  const lookDivider = document.createElement("hr");
  lookDivider.className = "settings-divider";

  dialog.append(
    title,
    zoomRow,
    zoomHint,
    zoomNotice,
    lookDivider,
    themeField.row,
    swatches,
    fontField.row,
    appearanceNotice,
    divider,
    row,
    notice,
    hint,
    dialogError,
    promptHost,
    closeButton,
  );
  slot.append(dialog);
  renderZoom();
  renderAppearance();

  let savedEnabled = false;
  let hasSecret = false;
  let readOnly = false;
  let busy = false;
  let closePrompt: (() => void) | null = null;

  button.addEventListener("click", () => {
    if (dialog.hidden) {
      dialog.hidden = false;
      button.setAttribute("aria-expanded", "true");
      void refresh();
    } else {
      hide();
    }
  });
  closeButton.addEventListener("click", () => {
    hide();
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !dialog.hidden) {
      hide();
    }
  });
  checkbox.addEventListener("change", () => {
    const next = checkbox.checked;
    checkbox.checked = savedEnabled;
    if (readOnly || busy) {
      return;
    }
    void request(next);
  });

  function hide(): void {
    dialog.hidden = true;
    button.setAttribute("aria-expanded", "false");
    dismissPrompt();
    dialogError.textContent = "";
  }

  function dismissPrompt(): void {
    closePrompt?.();
    closePrompt = null;
  }

  async function refresh(): Promise<void> {
    dialogError.textContent = "";
    try {
      apply(await authSettings());
      hint.textContent = "";
    } catch (err) {
      dialogError.textContent = errorText(err);
    }
  }

  function apply(result: LockSettings): void {
    savedEnabled = result.loginLockEnabled;
    hasSecret = result.hasSecret;
    readOnly = result.readOnly;
    checkbox.checked = savedEnabled;
    checkbox.disabled = readOnly || busy;
    notice.textContent = result.message ?? "";
  }

  async function request(next: boolean): Promise<void> {
    dismissPrompt();
    dialogError.textContent = "";
    hint.textContent = "";
    const plan = planLockChange(savedEnabled, next, hasSecret);
    if (plan.step === "save") {
      try {
        await persist(next);
      } catch (err) {
        dialogError.textContent = errorText(err);
      }
      return;
    }
    const mode = plan.step === "setup" ? "setup" : "verify";
    busy = true;
    checkbox.disabled = true;
    const prompt = mountCredentialPrompt(
      promptHost,
      mode,
      async (username, secret, confirm) => {
        if (mode === "setup") {
          const snapshot = await authSetup(username, secret, confirm);
          if (snapshot.state !== "unlocked") {
            throw new Error(snapshot.message || "设置没有完成");
          }
        }
        await persist(next, username, secret);
        dismissPrompt();
      },
      () => {
        busy = false;
        checkbox.disabled = readOnly;
        closePrompt = null;
      },
    );
    closePrompt = () => {
      busy = false;
      checkbox.disabled = readOnly;
      prompt.close();
    };
  }

  async function persist(enabled: boolean, username?: string, secret?: string): Promise<void> {
    busy = true;
    checkbox.disabled = true;
    try {
      const result = await authSetLock(enabled, username, secret);
      apply(result);
      hint.textContent = enabled ? "下次启动时需要登录" : "已关闭，现在可以直接使用";
    } catch (err) {
      checkbox.checked = savedEnabled;
      try {
        apply(await authSettings());
      } catch {
        checkbox.disabled = readOnly;
      }
      throw err;
    } finally {
      busy = false;
      checkbox.disabled = readOnly || busy;
    }
  }

  function renderZoom(): void {
    zoomValue.textContent = zoomLabel(zoom.percent);
    zoomOut.disabled = !zoom.canStep("out");
    zoomIn.disabled = !zoom.canStep("in");
    zoomReset.disabled = !zoom.canStep("reset");
    zoomNotice.textContent = zoom.message;
  }

  function renderAppearance(): void {
    themeField.select.value = appearance.theme;
    fontField.select.value = appearance.font;
    for (const option of fontOptions) {
      // A missing font stays selectable while it is the saved choice.
      option.disabled = option.dataset.installed !== "true" && option.value !== appearance.font;
    }
    const palette = themeOf(appearance.theme).palette;
    swatches.style.background = palette.background;
    swatches.replaceChildren(
      ...ANSI_KEYS.map((key) => {
        const chip = document.createElement("span");
        chip.style.background = palette[key];
        return chip;
      }),
    );
    appearanceNotice.textContent = appearance.message;
  }

  return { renderZoom, renderAppearance };
}

function selectField(caption: string, id: string): { row: HTMLLabelElement; select: HTMLSelectElement } {
  const row = document.createElement("label");
  row.className = "settings-field";
  row.htmlFor = id;
  const text = document.createElement("span");
  text.textContent = caption;
  const select = document.createElement("select");
  select.id = id;
  select.className = "settings-select";
  row.append(text, select);
  return { row, select };
}
