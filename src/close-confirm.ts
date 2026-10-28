import type { TabStatus } from "./tabs.ts";

export const CLOSE_CONFIRM_MESSAGE = "结束该会话？";

let open = false;

/** True while the dialog is up. Tab shortcuts are ignored meanwhile. */
export function closeConfirmOpen(): boolean {
  return open;
}

/** Live sessions ask first. Ended and failed tabs close at once. */
export function needsCloseConfirm(status: TabStatus): boolean {
  return status === "connecting" || status === "connected";
}

/** Escape cancels. Enter and Space stay with the focused button. */
export function closeConfirmKey(key: string): "cancel" | null {
  return key === "Escape" ? "cancel" : null;
}

/** Wording for a confirmation other than ending one tab. */
export interface ConfirmText {
  message: string;
  confirmLabel: string;
  /** Start on Cancel, so a stray Enter keeps the app open. */
  focusCancel?: boolean;
  /** Preformatted text shown below the message, such as a paste preview. */
  detail?: string;
}

/** Markup for the in-app close confirmation. */
export function renderCloseConfirm(tabTitle: string, text?: ConfirmText): string {
  const message = escapeHtml(text?.message ?? CLOSE_CONFIRM_MESSAGE);
  const confirmLabel = escapeHtml(text?.confirmLabel ?? "结束会话");
  return (
    `<div class="close-confirm" role="alertdialog" aria-modal="true" aria-labelledby="close-confirm-message">` +
    `<p class="close-confirm-title">${escapeHtml(tabTitle)}</p>` +
    `<p class="close-confirm-message" id="close-confirm-message">${message}</p>` +
    (text?.detail ? `<pre class="close-confirm-detail">${escapeHtml(text.detail)}</pre>` : "") +
    `<div class="close-confirm-actions">` +
    `<button type="button" class="button" data-choice="cancel">取消</button>` +
    `<button type="button" class="button primary" data-choice="confirm">${confirmLabel}</button>` +
    `</div>` +
    `</div>`
  );
}

/**
 * Ask in the page. `window.confirm` returns false without showing anything in
 * the macOS webview, so it cannot be used here.
 */
export function askCloseConfirm(tabTitle: string, text?: ConfirmText): Promise<boolean> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "close-confirm-overlay";
    overlay.innerHTML = renderCloseConfirm(tabTitle, text);
    const buttons = [...overlay.querySelectorAll<HTMLButtonElement>("[data-choice]")];

    const finish = (confirmed: boolean) => {
      open = false;
      document.removeEventListener("keydown", onKey, true);
      overlay.remove();
      resolve(confirmed);
    };

    for (const button of buttons) {
      button.addEventListener("click", () => {
        finish(button.dataset.choice === "confirm");
      });
    }
    overlay.addEventListener("mousedown", (event) => {
      if (event.target === overlay) {
        event.preventDefault();
      }
    });

    function onKey(event: KeyboardEvent): void {
      if (closeConfirmKey(event.key) === "cancel") {
        event.preventDefault();
        event.stopPropagation();
        finish(false);
        return;
      }
      if (event.key === "Tab") {
        event.preventDefault();
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const step = event.shiftKey ? -1 : 1;
        buttons[(index + step + buttons.length) % buttons.length]?.focus();
        return;
      }
    }

    open = true;
    document.addEventListener("keydown", onKey, true);
    document.body.append(overlay);
    buttons[text?.focusCancel ? 0 : buttons.length - 1]?.focus();
  });
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
