// 终端里的复制、粘贴判定。不碰 DOM，剪贴板读写由调用方完成。

import type { ConfirmText } from "./close-confirm.ts";
import type { ShortcutKey } from "./tabs.ts";

/** `copy-clear` copies, then drops the selection. */
export type ClipboardAction = "copy" | "copy-clear" | "paste";

/** Lines shown in the paste confirmation. */
const PREVIEW_LINES = 5;
/** Characters kept from each previewed line. */
const PREVIEW_WIDTH = 80;

/**
 * The clipboard action for a key, or null to leave the key to the terminal.
 *
 * macOS: Cmd+C / Cmd+V. Ctrl+C always reaches the shell as ^C.
 * Windows: Ctrl+Shift+C / Ctrl+Shift+V. Ctrl+C copies only when there is a
 * selection; otherwise it is ^C.
 */
export function clipboardKey(
  event: ShortcutKey,
  platform: "mac" | "windows",
  hasSelection: boolean,
): ClipboardAction | null {
  if (event.isComposing || event.altKey) {
    return null;
  }
  const letter = latinLetter(event);
  if (letter !== "c" && letter !== "v") {
    return null;
  }
  if (platform === "mac") {
    if (!event.metaKey || event.ctrlKey || event.shiftKey) {
      return null;
    }
    return letter === "c" ? "copy" : "paste";
  }
  if (event.metaKey || !event.ctrlKey) {
    return null;
  }
  if (event.shiftKey) {
    return letter === "c" ? "copy" : "paste";
  }
  return letter === "c" && hasSelection ? "copy-clear" : null;
}

/** Windows right-click: copy a selection, otherwise paste. */
export function rightClickAction(hasSelection: boolean): "copy-clear" | "paste" {
  return hasSelection ? "copy-clear" : "paste";
}

/** Any line break asks first. A trailing one would run the command at once. */
export function pasteNeedsConfirm(text: string): boolean {
  return /[\r\n]/.test(text);
}

export function pasteLineCount(text: string): number {
  return splitLines(text).length;
}

/** Wording for the in-app confirmation before a multi-line paste. */
export function pasteConfirmText(text: string): ConfirmText {
  const lines = splitLines(text);
  const shown = lines
    .slice(0, PREVIEW_LINES)
    .map((line) => (line.length > PREVIEW_WIDTH ? `${line.slice(0, PREVIEW_WIDTH)}…` : line));
  if (lines.length > PREVIEW_LINES) {
    shown.push(`…另有 ${lines.length - PREVIEW_LINES} 行`);
  }
  return {
    message: `要粘贴 ${lines.length} 行内容吗？换行会让命令立即执行。`,
    confirmLabel: "粘贴",
    focusCancel: true,
    detail: shown.join("\n"),
  };
}

function splitLines(text: string): string[] {
  const lines = text.split(/\r\n|\r|\n/);
  if (lines.length > 1 && lines[lines.length - 1] === "") {
    lines.pop();
  }
  return lines;
}

/** The letter typed, or the physical key's letter on a non-Latin layout. */
function latinLetter(event: ShortcutKey): string {
  const key = event.key.toLowerCase();
  if (/^[a-z]$/.test(key)) {
    return key;
  }
  const match = /^Key([A-Z])$/.exec(event.code ?? "");
  return match ? match[1].toLowerCase() : "";
}
