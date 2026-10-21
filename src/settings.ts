import { authSetLock, authSettings, authSetup, type LockSettings } from "./auth.ts";
import { errorText } from "./hosts.ts";
import { mountCredentialPrompt } from "./login.ts";
import { planLockChange } from "./startup.ts";

/**
 * Gear button dialog with one switch: require login on the next launch.
 * The button itself is created by the shell so the top bar owns the slot.
 */
export function mountSettings(button: HTMLButtonElement): void {
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

  dialog.append(title, row, notice, hint, dialogError, promptHost, closeButton);
  slot.append(dialog);

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
}
