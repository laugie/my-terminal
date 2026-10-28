// 窗口在终端里拦截哪些键。不碰 DOM；没列在这里的键都交给 xterm，发给 shell。

import { clipboardKey, type ClipboardAction } from "./clipboard.js";
import { shortcutAction, type ShortcutAction, type ShortcutKey } from "./tabs.js";

export type TerminalKey =
  | { type: "shortcut"; action: ShortcutAction }
  | { type: "cycle"; step: 1 | -1 }
  | { type: "clipboard"; action: ClipboardAction }
  | { type: "new" };

/**
 * The window's action for a key, or null to send it to the shell.
 *
 * Every action needs Cmd (macOS) or Ctrl (Windows), so Shift alone, Option
 * and AltGr symbols always reach the shell.
 */
export function terminalKey(
  event: ShortcutKey,
  platform: "mac" | "windows",
  hasSelection: boolean,
): TerminalKey | null {
  if (event.isComposing || event.altKey) {
    return null;
  }
  if (event.ctrlKey && !event.metaKey && event.key === "Tab") {
    return { type: "cycle", step: event.shiftKey ? -1 : 1 };
  }
  const action = shortcutAction(event, platform);
  if (action) {
    return { type: "shortcut", action };
  }
  const clip = clipboardKey(event, platform, hasSelection);
  if (clip) {
    return { type: "clipboard", action: clip };
  }
  const command =
    platform === "mac"
      ? event.metaKey && !event.ctrlKey
      : event.ctrlKey && !event.metaKey && event.shiftKey;
  if (command && event.key.toUpperCase() === "T") {
    return { type: "new" };
  }
  return null;
}
